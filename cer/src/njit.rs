// SPDX-License-Identifier: AGPL-3.0-or-later
// A baseline JIT for the C++ build (rgrc -l=cpp): a function's register
// code (regs.rs) as x86-64 or AArch64 machine code. The other half is the host's: a
// small C++ runtime (the cpp_polyfill of arch() below, written into the generated
// file) puts the bytes into executable memory and calls them.
//
// It works as wjit.rs does in WebAssembly: the registers stay in Vm.stack,
// a constant is read from Vm.kvals, and the frame is the interpreter's.
// Whenever an op is one this file does not write, or its operands are not
// numbers where the fast path wants numbers, the code returns that op's pc
// and the interpreter runs it. The code never calls, allocates or throws,
// so there is nothing to deoptimize.
//
//   f(sp, kv, pc, budget) -> pc to go on at in the interpreter
//
// sp is &stack[bp], kv &kvals[0], pc where to start (a loop head or 0;
// any other pc returns at once), budget the back edges it may take; when
// it runs out the answer is -4 - pc (rvm.rs jit_go reads the clock there).
//
// A value (value.rs, C++) is 16 bytes: the kind t (int) at 0, i (int) at
// 4, d (double) at 8; a number is t == 3.

use ranger::prelude::*;
use crate::ops::*;
use crate::regs::*;
use crate::value::*;
use crate::vm::*;
use crate::rvm::*;

/// value.rs (C++): a Val is 8 NaN-boxed bytes; its top 16 bits (the
/// "kind" the code compares) are at most TG_NUM for a number, else a tag
const VSZ: int = 8;
const TG_NUM: int = 0xFFF8;
const TG_UNDEF: int = 0xFFF9;
const TG_NULL: int = 0xFFFA;
const TG_BOOL: int = 0xFFFB;
const TG_STR: int = 0xFFFC;
const TG_OBJ: int = 0xFFFD;
const TG_BIG: int = 0xFFFE;

/// x86-64 registers: sp in rdi, kv in rsi, pc in edx, budget in rcx
/// (System V), kept in rbx
const RAX: int = 0;
const RBX: int = 3;
const RSI: int = 6;
const RDI: int = 7;
/// AArch64: sp in x0, kv in x1, pc in w2, budget in x3 (counted down in
/// place: the code calls nothing); x9..x11 scratch
const A_SP: int = 0;
const A_KV: int = 1;
const A_PC: int = 2;
const A_BUD: int = 3;

/// the operand bases, machine-neutral
const B_SP: int = 0;
const B_KV: int = 1;
/// The context block (the sixth argument): &this, the Elems table, the
/// frame's scope
const CTX_THIS: int = 0;
const CTX_EVECS: int = 8;
const CTX_ENV: int = 16;
const CTX_VM: int = 24;
const CTX_FN: int = 32;
const CTX_BP: int = 40;

/// conditions, machine-neutral (each machine maps them)
const C_EQ: int = 0;
const C_NE: int = 1;
/// unsigned / float: below-or-equal, above
const C_BE: int = 2;
const C_A: int = 3;
/// the float comparisons, NaN false (C_FNE: NaN true)
const C_FLT: int = 4;
const C_FLE: int = 5;
const C_FGT: int = 6;
const C_FGE: int = 7;
const C_FEQ: int = 8;
const C_FNE: int = 9;

/// arithmetic, machine-neutral
const F_ADD: int = 0;
const F_SUB: int = 1;
const F_MUL: int = 2;
const F_DIV: int = 3;

/// An assembler for one of two machines: the bytes, labels and the jumps
/// waiting for them.
pub struct Asm {
    pub arm: bool,
    pub b: Vec<int>,
    /// a label's offset, -1 while unbound
    pub labels: Vec<int>,
    /// branch fields to fill: where, which label, what kind (0 rel32,
    /// 1 AArch64 imm26, 2 AArch64 imm19)
    pub fix_at: Vec<int>,
    pub fix_label: Vec<int>,
    pub fix_kind: Vec<int>,
}

impl Asm {
    pub fn new(arm: bool, nlabels: int) -> Asm {
        let mut labels: Vec<int> = Vec::new();
        let mut i: int = 0;
        while i < nlabels {
            labels.push(-1);
            i += 1;
        }
        Asm { arm: arm, b: Vec::new(), labels: labels, fix_at: Vec::new(), fix_label: Vec::new(), fix_kind: Vec::new() }
    }
    fn u8(&mut self, x: int) {
        self.b.push(x & 255);
    }
    fn d32(&mut self, x: int) {
        self.b.push(x & 255);
        self.b.push((x >> 8) & 255);
        self.b.push((x >> 16) & 255);
        self.b.push((x >> 24) & 255);
    }
    /// an AArch64 instruction word (its high and low halves, so that the
    /// arithmetic stays inside a 32-bit int)
    fn w(&mut self, hi: int, lo: int) {
        self.b.push(lo & 255);
        self.b.push((lo >> 8) & 255);
        self.b.push(hi & 255);
        self.b.push((hi >> 8) & 255);
    }
    /// a new label, unbound
    fn label(&mut self) -> int {
        self.labels.push(-1);
        (self.labels.len() as int) - 1
    }
    fn here(&self) -> int {
        self.b.len() as int
    }
    fn bind(&mut self, l: int) {
        let h = self.here();
        self.labels[l as usize] = h;
    }
    fn fix(&mut self, l: int, kind: int) {
        let at = self.here();
        self.fix_at.push(at);
        self.fix_label.push(l);
        self.fix_kind.push(kind);
    }

    // ---------------------------------------------------------- x86-64

    fn x_rel32(&mut self, l: int) {
        self.fix(l, 0);
        self.d32(0);
    }
    /// ModRM with a 32-bit displacement from base (rdi or rsi: no SIB)
    fn x_mem(&mut self, reg: int, base: int, disp: int) {
        self.u8(0x80 + ((reg & 7) << 3) + (base & 7));
        self.d32(disp);
    }
    fn x_base(b: int) -> int {
        if b == B_SP { RDI } else { RSI }
    }
    fn x_sse_m(&mut self, prefix: int, op: int, r: int, base: int, disp: int) {
        self.u8(prefix);
        self.u8(0x0F);
        self.u8(op);
        self.x_mem(r, Asm::x_base(base), disp);
    }
    fn x_sse_rr(&mut self, prefix: int, op: int, r: int, m: int) {
        self.u8(prefix);
        self.u8(0x0F);
        self.u8(op);
        self.u8(0xC0 + ((r & 7) << 3) + (m & 7));
    }
    fn x_cc(c: int) -> int {
        // Jcc low nibbles: e 4, ne 5, be 6, a 7, ae 3
        if c == C_EQ || c == C_FEQ { 4 } else if c == C_NE || c == C_FNE { 5 } else if c == C_BE { 6 } else if c == C_A || c == C_FGT || c == C_FLT { 7 } else { 3 }
    }

    // ---------------------------------------------------------- AArch64

    /// the register and scaled offset for [base + disp] with an access of
    /// `scale` bytes; far ones go through x9
    fn a_ea(&mut self, base: int, disp: int, scale: int) -> int {
        let r = if base == B_SP { A_SP } else { A_KV };
        if disp % scale == 0 && disp / scale < 4096 {
            return r * 65536 + disp / scale;
        }
        self.a_mov32(9, disp);
        // add x9, base, x9
        self.w(0x8B09, (r << 5) + 9);
        9 * 65536
    }
    /// ldr/str with an unsigned scaled offset: op the high half of the
    /// word (0xB940 ldr w, 0xB900 str w, 0xFD40 ldr d, 0xFD00 str d,
    /// 0xF940 ldr x, 0xF900 str x)
    fn a_ldst(&mut self, op: int, rt: int, base: int, disp: int, scale: int) {
        let e = self.a_ea(base, disp, scale);
        let rn = e / 65536;
        let off = e % 65536;
        // imm12 at bit 10: bits 10..15 in lo, 16..21 in hi
        let lo = ((off & 63) << 10) + (rn << 5) + rt;
        let hi = op + (off >> 6);
        self.w(hi, lo);
    }
    /// mov w(r), imm32 (movz, then movk for the high half)
    fn a_mov32(&mut self, r: int, v: int) {
        let lo16 = v & 65535;
        let hi16 = (v >> 16) & 65535;
        // movz w, #lo16: 0x52800000 | imm16 << 5 | rd
        self.w(0x5280 + (lo16 >> 11), ((lo16 & 2047) << 5) + r);
        if hi16 != 0 {
            // movk w, #hi16, lsl 16: 0x72A00000
            self.w(0x72A0 + (hi16 >> 11), ((hi16 & 2047) << 5) + r);
        }
    }
    fn a_cc(c: int) -> int {
        // EQ 0, NE 1, HI 8, LS 9, MI 4, GT 12, GE 10
        if c == C_EQ || c == C_FEQ { 0 } else if c == C_NE || c == C_FNE { 1 } else if c == C_BE || c == C_FLE { 9 } else if c == C_A { 8 } else if c == C_FLT { 4 } else if c == C_FGT { 12 } else { 10 }
    }

    // ------------------------------------------------- machine-neutral

    fn jmp(&mut self, l: int) {
        if self.arm {
            self.fix(l, 1);
            self.w(0x1400, 0);
        } else {
            self.u8(0xE9);
            self.x_rel32(l);
        }
    }
    fn jcc(&mut self, c: int, l: int) {
        if self.arm {
            self.fix(l, 2);
            self.w(0x5400, Asm::a_cc(c));
        } else {
            self.u8(0x0F);
            self.u8(0x80 + Asm::x_cc(c));
            self.x_rel32(l);
        }
    }
    /// x86: after ucomisd, NaN shows as parity
    fn jnan(&mut self, l: int) {
        if self.arm {
            // VS: unordered
            self.fix(l, 2);
            self.w(0x5400, 6);
        } else {
            self.u8(0x0F);
            self.u8(0x8A);
            self.x_rel32(l);
        }
    }
    /// compare the kind (top 16 bits) of [base+disp] with k (scratch
    /// overwritten)
    fn cmp_kind(&mut self, base: int, disp: int, k: int) {
        self.load_kind(base, disp);
        self.cmp_scratch(k);
    }
    /// to l when [base+disp] is not a number
    fn not_num(&mut self, base: int, disp: int, l: int) {
        self.cmp_kind(base, disp, TG_NUM);
        self.jcc(C_A, l);
    }
    /// the kind (top 16 bits) of [base+disp] into the scratch register
    /// (rax / x9), then cmp_scratch
    fn load_kind(&mut self, base: int, disp: int) {
        if self.arm {
            self.a_ldst(0xF940, 9, base, disp, 8);
            // lsr x9, x9, #48
            self.w(0xD370, 0xFD29);
        } else {
            // mov rax, [mem]; shr rax, 48
            self.u8(0x48);
            self.u8(0x8B);
            self.x_mem(RAX, Asm::x_base(base), disp);
            self.u8(0x48);
            self.u8(0xC1);
            self.u8(0xE8);
            self.u8(48);
        }
    }
    fn cmp_scratch(&mut self, k: int) {
        if self.arm {
            self.a_mov32(11, k);
            // cmp w9, w11
            self.w(0x6B0B, (9 << 5) + 31);
        } else {
            // cmp eax, imm32
            self.u8(0x3D);
            self.d32(k);
        }
    }
    fn load_d(&mut self, f: int, base: int, disp: int) {
        if self.arm {
            self.a_ldst(0xFD40, f, base, disp, 8);
        } else {
            self.x_sse_m(0xF2, 0x10, f, base, disp);
        }
    }
    fn store_d(&mut self, f: int, base: int, disp: int) {
        if self.arm {
            self.a_ldst(0xFD00, f, base, disp, 8);
        } else {
            self.x_sse_m(0xF2, 0x11, f, base, disp);
        }
    }
    /// f(d) = f(d) op f(s)
    fn arith(&mut self, op: int, d: int, s: int) {
        if self.arm {
            // fadd 0x1E602800, fsub 0x1E603800, fmul 0x1E600800, fdiv 0x1E601800
            let lo = if op == F_ADD { 0x2800 } else if op == F_SUB { 0x3800 } else if op == F_MUL { 0x0800 } else { 0x1800 };
            self.w(0x1E60 + s, lo + (d << 5) + d);
        } else {
            let o = if op == F_ADD { 0x58 } else if op == F_SUB { 0x5C } else if op == F_MUL { 0x59 } else { 0x5E };
            self.x_sse_rr(0xF2, o, d, s);
        }
    }
    /// compare f(x) with f(y) (then a float condition)
    fn fcmp(&mut self, x: int, y: int) {
        if self.arm {
            self.w(0x1E60 + y, 0x2000 + (x << 5));
        } else {
            self.x_sse_rr(0x66, 0x2E, x, y);
        }
    }
    /// to l when f(x) cond f(y) holds; NaN: only C_FNE holds
    fn jump_fcmp(&mut self, c: int, x: int, y: int, l: int) {
        if self.arm {
            self.fcmp(x, y);
            self.jcc(c, l);
            return;
        }
        // x86: ucomisd sets CF/ZF like an unsigned compare, NaN all three
        if c == C_FLT {
            self.fcmp(y, x);
            self.jcc(C_A, l);
        } else if c == C_FLE {
            self.fcmp(y, x);
            self.jcc(C_FGE, l);
        } else if c == C_FGT {
            self.fcmp(x, y);
            self.jcc(C_A, l);
        } else if c == C_FGE {
            self.fcmp(x, y);
            self.jcc(C_FGE, l);
        } else if c == C_FEQ {
            let no = self.label();
            self.fcmp(x, y);
            self.jnan(no);
            self.jcc(C_EQ, l);
            self.bind(no);
        } else {
            self.fcmp(x, y);
            self.jnan(l);
            self.jcc(C_NE, l);
        }
    }
    /// f(x) = 0.0
    fn zero_d(&mut self, f: int) {
        if self.arm {
            // movi d, #0: 0x2F00E400
            self.w(0x2F00, 0xE400 + f);
        } else {
            self.x_sse_rr(0x66, 0x57, f, f);
        }
    }
    /// f(x) = (double) v
    fn dbl_imm(&mut self, f: int, v: int) {
        if self.arm {
            self.a_mov32(9, v);
            // scvtf d, w9: 0x1E620000
            self.w(0x1E62, (9 << 5) + f);
        } else {
            self.u8(0xB8);
            self.d32(v);
            self.u8(0xF2);
            self.u8(0x0F);
            self.u8(0x2A);
            self.u8(0xC0 + ((f & 7) << 3) + RAX);
        }
    }
    /// a tagged Val (tag << 48 | low, low a small int) to [base+disp]
    fn store_tag(&mut self, base: int, disp: int, tag: int, low: int) {
        if self.arm {
            // movz x10, #low; movk x10, #tag, lsl 48
            self.w(0xD280 + (low >> 11), ((low & 2047) << 5) + 10);
            self.w(0xF2E0 + (tag >> 11), ((tag & 2047) << 5) + 10);
            self.a_ldst(0xF900, 10, base, disp, 8);
        } else {
            // mov rax, imm64; mov [mem], rax
            self.u8(0x48);
            self.u8(0xB8);
            self.d32(low);
            self.u8(0);
            self.u8(0);
            self.u8(tag & 255);
            self.u8((tag >> 8) & 255);
            self.u8(0x48);
            self.u8(0x89);
            self.x_mem(RAX, Asm::x_base(base), disp);
        }
    }
    /// a whole Val from [sb+sd] to [db+dd]
    fn copy_val(&mut self, db: int, dd: int, sb: int, sd: int) {
        if self.arm {
            self.a_ldst(0xF940, 10, sb, sd, 8);
            self.a_ldst(0xF900, 10, db, dd, 8);
        } else {
            self.x_sse_m(0xF2, 0x10, 7, sb, sd);
            self.x_sse_m(0xF2, 0x11, 7, db, dd);
        }
    }
    /// the budget less one; to l when it runs out
    fn spend(&mut self, l: int) {
        if self.arm {
            // subs x3, x3, #1
            self.w(0xF100, 0x0400 + (A_BUD << 5) + A_BUD);
        } else {
            // dec rbx
            self.u8(0x48);
            self.u8(0xFF);
            self.u8(0xCB);
        }
        self.jcc(C_EQ, l);
    }
    // ------------------------------------------- objects (shape guards)
    // The object table (Vm.objs: one std::shared_ptr<JsObj> per object,
    // its pointer first) comes in r8 / x4, the context block (CTX_*) in r9 / x5. The
    // object being read is p: rax / x12.

    /// AArch64 ldr/str from a register base (rn), far offsets through x9
    fn a_ldst_r(&mut self, op: int, rt: int, rn: int, disp: int, scale: int) {
        let mut b = rn;
        let mut off = disp / scale;
        if disp % scale != 0 || off >= 4096 {
            self.a_mov32(9, disp);
            // add x9, rn, x9
            self.w(0x8B09, (rn << 5) + 9);
            b = 9;
            off = 0;
        }
        let lo = ((off & 63) << 10) + (b << 5) + rt;
        self.w(op + (off >> 6), lo);
    }
    /// p = the JsObj of the object index in p (16-byte table entries)
    fn obj_from_index(&mut self) {
        if self.arm {
            // lsl x12, x12, #4; ldr x12, [x4, x12]
            self.w(0xD37C, 0xED8C);
            self.w(0xF86C, 0x688C);
        } else {
            // shl rax, 4; mov rax, [r8 + rax]
            self.u8(0x48);
            self.u8(0xC1);
            self.u8(0xE0);
            self.u8(4);
            self.u8(0x49);
            self.u8(0x8B);
            self.u8(0x04);
            self.u8(0x00);
        }
    }
    /// p = the JsObj of the object Val at [base+disp] (its low 32 bits)
    fn obj_ptr(&mut self, base: int, disp: int) {
        if self.arm {
            // ldr w12, [base, #disp]
            self.a_ldst(0xB940, 12, base, disp, 4);
        } else {
            // mov eax, [base + disp]
            self.u8(0x8B);
            self.x_mem(RAX, Asm::x_base(base), disp);
        }
        self.obj_from_index();
    }
    /// q (r10 / x14) = the pointer at [ctx + disp]
    fn ctx_ptr(&mut self, disp: int) {
        if self.arm {
            self.a_ldst_r(0xF940, 14, 5, disp, 8);
        } else {
            // mov r10, [r9 + disp8]
            self.u8(0x4D);
            self.u8(0x8B);
            self.u8(0x51);
            self.u8(disp);
        }
    }
    /// compare `this`'s kind with k (q then holds &this)
    fn cmp_this_kind(&mut self, k: int) {
        self.ctx_ptr(CTX_THIS);
        if self.arm {
            self.a_ldst_r(0xF940, 9, 14, 0, 8);
            self.w(0xD370, 0xFD29);
        } else {
            // mov rax, [r10]; shr rax, 48
            self.u8(0x49);
            self.u8(0x8B);
            self.u8(0x02);
            self.u8(0x48);
            self.u8(0xC1);
            self.u8(0xE8);
            self.u8(48);
        }
        self.cmp_scratch(k);
    }
    /// p = the JsObj of `this` (after cmp_this_kind: q holds &this)
    fn this_obj_ptr(&mut self) {
        if self.arm {
            self.a_ldst_r(0xB940, 12, 14, 0, 4);
        } else {
            // mov eax, [r10]
            self.u8(0x41);
            self.u8(0x8B);
            self.u8(0x02);
        }
        self.obj_from_index();
    }
    /// p = the frame's scope (an object index; negative: none)
    fn env_index(&mut self) {
        if self.arm {
            self.a_ldst_r(0xB980, 12, 5, CTX_ENV, 4);
        } else {
            // movsxd rax, [r9 + disp8]
            self.u8(0x49);
            self.u8(0x63);
            self.u8(0x41);
            self.u8(CTX_ENV);
        }
    }
    /// p = the int at [p+disp], sign-extended (p a pointer)
    fn p_load_int(&mut self, disp: int) {
        if self.arm {
            self.a_ldst_r(0xB980, 12, 12, disp, 4);
        } else {
            // movsxd rax, [rax + disp32]
            self.u8(0x48);
            self.u8(0x63);
            self.u8(0x80);
            self.d32(disp);
        }
    }
    /// to l when the index in p is negative
    fn p_neg(&mut self, l: int) {
        if self.arm {
            // cmp w12, #0; b.mi l (C_FLT is MI on AArch64)
            self.w(0x7100, (12 << 5) + 31);
            self.jcc(C_FLT, l);
        } else {
            // test eax, eax; js l
            self.u8(0x85);
            self.u8(0xC0);
            self.u8(0x0F);
            self.u8(0x88);
            self.x_rel32(l);
        }
    }
    /// p = the element vector's buffer of the Elems index in p, to l when
    /// it holds no more than `need` bytes (q is overwritten)
    fn elems_buf(&mut self, ev: int, need: int, l: int) {
        self.ctx_ptr(CTX_EVECS);
        if self.arm {
            // lsl x12, x12, #4; ldr x12, [x14, x12]
            self.w(0xD37C, 0xED8C);
            self.w(0xF86C, 0x69CC);
            self.a_ldst_r(0xF940, 14, 12, ev + 8, 8);
            self.a_ldst_r(0xF940, 12, 12, ev, 8);
            // sub x14, x14, x12; cmp x14, x9
            self.w(0xCB0C, (14 << 5) + 14);
            self.a_mov32(9, need);
            self.w(0xEB09, (14 << 5) + 31);
        } else {
            // shl rax, 4; mov rax, [r10 + rax]
            self.u8(0x48);
            self.u8(0xC1);
            self.u8(0xE0);
            self.u8(4);
            self.u8(0x49);
            self.u8(0x8B);
            self.u8(0x04);
            self.u8(0x02);
            // mov r10, [rax + ev + 8]; mov rax, [rax + ev]
            self.u8(0x4C);
            self.u8(0x8B);
            self.u8(0x90);
            self.d32(ev + 8);
            self.u8(0x48);
            self.u8(0x8B);
            self.u8(0x80);
            self.d32(ev);
            // sub r10, rax; cmp r10, need
            self.u8(0x49);
            self.u8(0x29);
            self.u8(0xC2);
            self.u8(0x49);
            self.u8(0x81);
            self.u8(0xFA);
            self.d32(need);
        }
        self.jcc(C_BE, l);
    }
    /// slow(vm, pi, pc, bp) (the context block's function): on when it
    /// did the op, to l when it answers 0. The frame's registers are saved
    /// around the call; no value lives in a float register across ops.
    fn slow_call(&mut self, pi: int, pc: int, l: int) {
        if self.arm {
            // stp x29, x30, [sp, #-64]!; stp x0, x1, [sp, #16];
            // stp x3, x4, [sp, #32]; str x5, [sp, #48]
            self.w(0xA9BC, 0x7BFD);
            self.w(0xA901, 0x07E0);
            self.w(0xA902, 0x13E3);
            self.w(0xF900, 0x1BE5);
            self.a_ldst_r(0xB940, 3, 5, CTX_BP, 4);
            self.a_mov32(1, pi);
            self.a_mov32(2, pc);
            self.a_ldst_r(0xF940, 0, 5, CTX_VM, 8);
            self.a_ldst_r(0xF940, 9, 5, CTX_FN, 8);
            // blr x9; mov w9, w0
            self.w(0xD63F, 0x0120);
            self.w(0x2A00, 0x03E9);
            // ldp x0, x1, [sp, #16]; ldp x3, x4, [sp, #32];
            // ldr x5, [sp, #48]; ldp x29, x30, [sp], #64
            self.w(0xA941, 0x07E0);
            self.w(0xA942, 0x13E3);
            self.w(0xF940, 0x1BE5);
            self.w(0xA8C4, 0x7BFD);
            // cbz w9, l
            self.fix(l, 2);
            self.w(0x3400, 9);
        } else {
            // push rdi; push rsi; push r8; push r9 (rsp stays 16-aligned:
            // the return address and rbx are on it already)
            self.u8(0x57);
            self.u8(0x56);
            self.u8(0x41);
            self.u8(0x50);
            self.u8(0x41);
            self.u8(0x51);
            // mov rax, [r9 + CTX_FN]; mov esi, pi; mov edx, pc;
            // mov ecx, [r9 + CTX_BP]; mov rdi, [r9 + CTX_VM]; call rax
            self.u8(0x49);
            self.u8(0x8B);
            self.u8(0x41);
            self.u8(CTX_FN);
            self.u8(0xBE);
            self.d32(pi);
            self.u8(0xBA);
            self.d32(pc);
            self.u8(0x41);
            self.u8(0x8B);
            self.u8(0x49);
            self.u8(CTX_BP);
            self.u8(0x49);
            self.u8(0x8B);
            self.u8(0x79);
            self.u8(CTX_VM);
            self.u8(0xFF);
            self.u8(0xD0);
            // pop r9; pop r8; pop rsi; pop rdi; test eax, eax; jz l
            self.u8(0x41);
            self.u8(0x59);
            self.u8(0x41);
            self.u8(0x58);
            self.u8(0x5E);
            self.u8(0x5F);
            self.u8(0x85);
            self.u8(0xC0);
            self.jcc(C_EQ, l);
        }
    }
    /// to l when the 8 bytes at x and y are the same
    fn same_bits(&mut self, xb: int, xd: int, yb: int, yd: int, l: int) {
        if self.arm {
            // y first: a far x reuses x9
            self.a_ldst(0xF940, 13, yb, yd, 8);
            self.a_ldst(0xF940, 9, xb, xd, 8);
            // cmp x9, x13
            self.w(0xEB0D, (9 << 5) + 31);
        } else {
            // mov rax, [x]; cmp rax, [y]
            self.u8(0x48);
            self.u8(0x8B);
            self.x_mem(RAX, Asm::x_base(xb), xd);
            self.u8(0x48);
            self.u8(0x3B);
            self.x_mem(RAX, Asm::x_base(yb), yd);
        }
        self.jcc(C_EQ, l);
    }
    /// compare the low 32 bits of [base+disp] with 0
    fn low_zero(&mut self, base: int, disp: int) {
        if self.arm {
            self.a_ldst(0xB940, 9, base, disp, 4);
            // cmp w9, #0
            self.w(0x7100, (9 << 5) + 31);
        } else {
            // cmp dword [mem], 0
            self.u8(0x83);
            self.x_mem(7, Asm::x_base(base), disp);
            self.u8(0);
        }
    }
    /// compare the int at [p+disp] with v (then jcc)
    fn p_cmp32(&mut self, disp: int, v: int) {
        if self.arm {
            self.a_ldst_r(0xB940, 13, 12, disp, 4);
            self.a_mov32(9, v);
            // cmp w13, w9
            self.w(0x6B09, (13 << 5) + 31);
        } else {
            // cmp dword [rax + disp], imm32
            self.u8(0x81);
            self.u8(0xB8);
            self.d32(disp);
            self.d32(v);
        }
    }
    /// keep the scratch kind (in r11 / w13)
    fn p_save_kind(&mut self) {
        if self.arm {
            // mov w13, w9
            self.w(0x2A09, 0x03ED);
        } else {
            // mov r11d, eax
            self.u8(0x41);
            self.u8(0x89);
            self.u8(0xC3);
        }
    }
    /// compare the scratch kind with the kept one
    fn cmp_saved_kind(&mut self) {
        if self.arm {
            // cmp w9, w13
            self.w(0x6B0D, (9 << 5) + 31);
        } else {
            // cmp eax, r11d
            self.u8(0x44);
            self.u8(0x39);
            self.u8(0xD8);
        }
    }
    /// compare the kind of the Val at [p+disp] with k (p kept)
    fn p_cmp_kind(&mut self, disp: int, k: int) {
        if self.arm {
            self.a_ldst_r(0xF940, 13, 12, disp, 8);
            // lsr x13, x13, #48
            self.w(0xD370, 0xFDAD);
            self.a_mov32(11, k);
            // cmp w13, w11
            self.w(0x6B0B, (13 << 5) + 31);
        } else {
            // mov r11, [rax + disp]; shr r11, 48; cmp r11d, imm32
            self.u8(0x4C);
            self.u8(0x8B);
            self.u8(0x98);
            self.d32(disp);
            self.u8(0x49);
            self.u8(0xC1);
            self.u8(0xEB);
            self.u8(48);
            self.u8(0x41);
            self.u8(0x81);
            self.u8(0xFB);
            self.d32(k);
        }
    }
    /// p = the pointer at [p+disp]
    fn p_load_ptr(&mut self, disp: int) {
        if self.arm {
            self.a_ldst_r(0xF940, 12, 12, disp, 8);
        } else {
            // mov rax, [rax + disp]
            self.u8(0x48);
            self.u8(0x8B);
            self.u8(0x80);
            self.d32(disp);
        }
    }
    /// the Val at [p+pd] to [db+dd]
    fn val_from_p(&mut self, db: int, dd: int, pd: int) {
        if self.arm {
            self.a_ldst_r(0xF940, 10, 12, pd, 8);
            self.a_ldst(0xF900, 10, db, dd, 8);
        } else {
            // movsd xmm7, [rax + pd]
            self.u8(0xF2);
            self.u8(0x0F);
            self.u8(0x10);
            self.u8(0xB8);
            self.d32(pd);
            self.x_sse_m(0xF2, 0x11, 7, db, dd);
        }
    }
    /// the Val at [sb+sd] to [p+pd]
    fn val_to_p(&mut self, pd: int, sb: int, sd: int) {
        if self.arm {
            self.a_ldst(0xF940, 10, sb, sd, 8);
            self.a_ldst_r(0xF900, 10, 12, pd, 8);
        } else {
            self.x_sse_m(0xF2, 0x10, 7, sb, sd);
            // movsd [rax + pd], xmm7
            self.u8(0xF2);
            self.u8(0x0F);
            self.u8(0x11);
            self.u8(0xB8);
            self.d32(pd);
        }
    }
    fn prologue(&mut self) {
        if !self.arm {
            // push rbx; mov rbx, rcx
            self.u8(0x53);
            self.u8(0x48);
            self.u8(0x89);
            self.u8(0xCB);
        }
    }
    /// to l when the entry pc is pc
    fn entry(&mut self, pc: int, l: int) {
        if self.arm {
            self.a_mov32(9, pc);
            // cmp w2, w9
            self.w(0x6B09, (A_PC << 5) + 31);
        } else {
            // cmp edx, imm32
            self.u8(0x81);
            self.u8(0xFA);
            self.d32(pc);
        }
        self.jcc(C_EQ, l);
    }
    /// the answer is the entry pc
    fn ret_entry(&mut self) {
        if self.arm {
            // sxtw x0, w2
            self.w(0x9340, 0x7C00 + (A_PC << 5));
        } else {
            // movsxd rax, edx
            self.u8(0x48);
            self.u8(0x63);
            self.u8(0xC2);
        }
    }
    fn ret_value(&mut self, v: int) {
        if self.arm {
            self.a_mov32(0, v);
            // sxtw x0, w0
            self.w(0x9340, 0x7C00);
        } else {
            self.u8(0x48);
            self.u8(0xC7);
            self.u8(0xC0);
            self.d32(v);
        }
    }
    fn epilogue(&mut self) {
        if self.arm {
            self.w(0xD65F, 0x03C0);
        } else {
            // pop rbx; ret
            self.u8(0x5B);
            self.u8(0xC3);
        }
    }
    /// Fills every branch; false when a label was never bound or a branch
    /// is out of reach.
    fn link(&mut self) -> bool {
        let mut k: usize = 0;
        while k < self.fix_at.len() {
            let at = self.fix_at[k];
            let target = self.labels[self.fix_label[k] as usize];
            if target < 0 {
                return false;
            }
            let kind = self.fix_kind[k];
            if kind == 0 {
                let rel = target - (at + 4);
                self.b[at as usize] = rel & 255;
                self.b[(at + 1) as usize] = (rel >> 8) & 255;
                self.b[(at + 2) as usize] = (rel >> 16) & 255;
                self.b[(at + 3) as usize] = (rel >> 24) & 255;
            } else {
                // in instructions
                let rel = (target - at) / 4;
                let a0 = at as usize;
                if kind == 1 {
                    // imm26 in bits 0..25
                    let v = rel & 0x3FFFFFF;
                    self.b[a0] = v & 255;
                    self.b[a0 + 1] = (v >> 8) & 255;
                    self.b[a0 + 2] = (v >> 16) & 255;
                    self.b[a0 + 3] = (self.b[a0 + 3] & 0xFC) + ((v >> 24) & 3);
                } else {
                    // imm19 in bits 5..23
                    if rel >= 262144 || rel < -262144 {
                        return false;
                    }
                    let v = rel & 0x7FFFF;
                    let lo = (self.b[a0] & 31) + ((v & 7) << 5);
                    self.b[a0] = lo;
                    self.b[a0 + 1] = (v >> 3) & 255;
                    self.b[a0 + 2] = (v >> 11) & 255;
                }
            }
            k += 1;
        }
        true
    }
}

/// Where operand x lies: a frame slot (x >= 0) from sp, a constant from kv.
fn opnd_base(x: int) -> int {
    if x >= 0 { B_SP } else { B_KV }
}
fn opnd_disp(x: int) -> int {
    if x >= 0 { x * VSZ } else { (-1 - x) * VSZ }
}

/// Where the object's parts lie in the C++ build (layout()): ok (1 when
/// the table holds 16-byte std::shared_ptr entries), then the offsets of
/// JsObj.shape, inl0, inl1, vals, Slots.v, JsObj.proto, JsObj.elems and
/// Elems.v.
const L_OK: usize = 0;
const L_SHAPE: usize = 1;
const L_INL0: usize = 2;
const L_INL1: usize = 3;
const L_VALS: usize = 4;
const L_V: usize = 5;
const L_PROTO: usize = 6;
const L_ELEMS: usize = 7;
const L_EV: usize = 8;

/// A property op whose site's cache (w3: shape * 65536 + slot) saw one
/// shape: read or written in place behind a check of that shape.
fn prop_site(kind: int, sub: int, w3: int, lay: &Vec<int>) -> bool {
    if lay[L_OK] != 1 || w3 < 65536 {
        return false;
    }
    // a far slot past what one AArch64 load reaches
    if ((w3 & 65535) - 2) * VSZ + 8 >= 32760 {
        return false;
    }
    kind == R_GET_PROP || kind == R_GET_THIS_PROP || (kind == R_SET_PROP && sub == 0)
}

/// Whether the op is one this file writes (pc its place, tab Vm.rtab).
fn handles(o: Op, pc: int, lay: &Vec<int>, tab: &Vec<int>) -> bool {
    let w0 = (o.code as int);
    let kind = w0 & 255;
    let sub = (w0 >> 8) & 255;
    if is_slow(kind, sub) {
        return true;
    }
    if kind == R_GET_ENV {
        // depth and slot within reach of one immediate
        return lay[L_OK] == 1 && (o.b as int) >= 0 && (o.b as int) < 64 && (o.c as int) >= 0 && (o.c as int) < 1000000;
    }
    if kind == R_SWITCH {
        return switch_ok(o, pc, tab);
    }
    handled(kind, sub)
}

/// An op the code hands to slow() (properties outside one shape's fast
/// path, globals, arithmetic past + - * /): what the interpreter does
/// without calls into JS, else back to the interpreter.
fn is_slow(kind: int, sub: int) -> bool {
    if kind == R_GET_PROP || kind == R_GET_THIS_PROP || kind == R_GET_GLOBAL || kind == R_SET_GLOBAL || kind == R_SET_ENV || kind == R_TYPEOF || kind == R_GET_ELEM || kind == R_TONUM {
        return true;
    }
    if kind == R_SET_PROP {
        return sub == 0;
    }
    kind == R_ARITH && sub != OP_DIV
}

/// A numeric switch with a small table that only jumps forward.
fn switch_ok(o: Op, pc: int, tab: &Vec<int>) -> bool {
    let t = (o.b as int);
    if t < 0 || (t as usize) >= tab.len() {
        return false;
    }
    let size = tab[t as usize];
    if size < 0 || size > 64 || ((t + 1 + size) as usize) >= tab.len() {
        return false;
    }
    let lo = (o.c as int);
    if lo < -1000000 || lo > 1000000 {
        return false;
    }
    let mut i: int = 0;
    while i <= size {
        if tab[(t + 1 + i) as usize] <= pc {
            return false;
        }
        i += 1;
    }
    true
}

/// Whether the op kind is one this file writes.
fn handled(kind: int, sub: int) -> bool {
    if kind == R_NOP || kind == R_MOVE || kind == R_LOAD2 || kind == R_ADD || kind == R_SUB || kind == R_MUL {
        return true;
    }
    if kind == R_ARITH {
        return sub == OP_DIV;
    }
    if kind == R_CMP_JF || kind == R_CMP_JT || kind == R_LOOP || kind == R_CMP || kind == R_CMP_JFK || kind == R_CMP_JTK {
        return is_num_cmp(sub);
    }
    kind == R_NOT || kind == R_JUMP || kind == R_JF || kind == R_JT || kind == R_JF_KEEP || kind == R_JT_KEEP || kind == R_LOCAL_NULL_JF || kind == R_INC_LOCAL || kind == R_INC || kind == R_POSTINC || kind == R_ADD_LOCAL_POP
}

fn is_num_cmp(c: int) -> bool {
    c == OP_LT || c == OP_GT || c == OP_LE || c == OP_GE || c == OP_SEQ || c == OP_EQ || c == OP_SNE || c == OP_NE
}

/// The function being written: the assembler, and the stubs that go back
/// to the interpreter (each pc's at most once).
struct Cg {
    a: Asm,
    n: int,
    /// label of the stub that answers pc (-1: none yet)
    exits: Vec<int>,
    /// label of the stub that answers -4 - pc (the budget ran out on the
    /// way to pc)
    budgets: Vec<int>,
    epi: int,
}

impl Cg {
    fn exit(&mut self, pc: int) -> int {
        if self.exits[pc as usize] < 0 {
            let l = self.a.label();
            self.exits[pc as usize] = l;
        }
        self.exits[pc as usize]
    }
    fn budget(&mut self, pc: int) -> int {
        if self.budgets[pc as usize] < 0 {
            let l = self.a.label();
            self.budgets[pc as usize] = l;
        }
        self.budgets[pc as usize]
    }
    /// operand x is a number, else back to the interpreter at pc
    fn guard_num(&mut self, x: int, pc: int) {
        let l = self.exit(pc);
        self.a.not_num(opnd_base(x), opnd_disp(x), l);
    }
    fn load(&mut self, f: int, x: int) {
        self.a.load_d(f, opnd_base(x), opnd_disp(x));
    }
    /// f into register slot r as a number (a NaN keeps the hardware's
    /// bits, below the tags)
    fn store_num(&mut self, f: int, r: int) {
        self.a.store_d(f, B_SP, r * VSZ);
    }
    /// a jump from pc to t: a back edge spends one of the budget
    fn jump_to(&mut self, pc: int, t: int) {
        if t <= pc {
            let l = self.budget(t);
            self.a.spend(l);
        }
        self.a.jmp(t);
    }
    /// p = the object's slot holder, and the slot's offset from it (the
    /// shape checked: else to ex)
    fn slot_of(&mut self, slot: int, shape: int, ex: int, lay: &Vec<int>) -> int {
        self.a.p_cmp32(lay[L_SHAPE], shape);
        self.a.jcc(C_NE, ex);
        if slot == 0 {
            return lay[L_INL0];
        }
        if slot == 1 {
            return lay[L_INL1];
        }
        // the Slots object, then its vector's buffer
        self.a.p_load_ptr(lay[L_VALS]);
        self.a.p_load_ptr(lay[L_V]);
        (slot - 2) * VSZ
    }
    /// dst = obj.prop (w2 the object; `this` for R_GET_THIS_PROP)
    fn get_prop(&mut self, kind: int, dst: int, w2: int, c: int, ex: int, lay: &Vec<int>) {
        if kind == R_GET_THIS_PROP {
            self.a.cmp_this_kind(TG_OBJ);
            self.a.jcc(C_NE, ex);
            self.a.this_obj_ptr();
        } else {
            self.a.cmp_kind(opnd_base(w2), opnd_disp(w2), TG_OBJ);
            self.a.jcc(C_NE, ex);
            self.a.obj_ptr(opnd_base(w2), opnd_disp(w2));
        }
        let pd = self.slot_of(c & 65535, c >> 16, ex, lay);
        self.a.val_from_p(B_SP, dst * VSZ, pd);
    }
    /// obj.prop = v, when neither v nor the old value is counted (both
    /// undefined, null, a boolean or a number): no references to change
    fn set_prop(&mut self, w1: int, w2: int, c: int, ex: int, lay: &Vec<int>) {
        self.a.load_kind(opnd_base(w2), opnd_disp(w2));
        self.a.cmp_scratch(TG_BOOL);
        self.a.jcc(C_A, ex);
        self.a.cmp_kind(opnd_base(w1), opnd_disp(w1), TG_OBJ);
        self.a.jcc(C_NE, ex);
        self.a.obj_ptr(opnd_base(w1), opnd_disp(w1));
        let pd = self.slot_of(c & 65535, c >> 16, ex, lay);
        self.a.p_cmp_kind(pd, TG_BOOL);
        self.a.jcc(C_A, ex);
        self.a.val_to_p(pd, opnd_base(w2), opnd_disp(w2));
    }
    /// dst = slot k of the scope `depth` levels out (dst + 1 = undefined
    /// for sub 1); a missing scope or slot goes back to the interpreter
    fn get_env(&mut self, dst: int, depth: int, k: int, sub: int, pc: int, lay: &Vec<int>) {
        let ex = self.exit(pc);
        self.a.env_index();
        let mut d: int = 0;
        while d < depth {
            self.a.p_neg(ex);
            self.a.obj_from_index();
            self.a.p_load_int(lay[L_PROTO]);
            d += 1;
        }
        self.a.p_neg(ex);
        self.a.obj_from_index();
        self.a.p_load_int(lay[L_ELEMS]);
        self.a.p_neg(ex);
        self.a.elems_buf(lay[L_EV], k * VSZ + VSZ - 1, ex);
        self.a.val_from_p(B_SP, dst * VSZ, k * VSZ);
        if sub == 1 {
            self.a.store_tag(B_SP, (dst + 1) * VSZ, TG_UNDEF, 0);
        }
    }
    /// to `yes` when x op y holds (op a comparison code), else on; what
    /// the code cannot decide goes back to the interpreter at pc
    fn cond(&mut self, code: int, x: int, y: int, pc: int, yes: int) {
        if code == OP_SEQ || code == OP_SNE || code == OP_EQ || code == OP_NE {
            let no = self.a.label();
            let eq_l = if code == OP_SEQ || code == OP_EQ { yes } else { no };
            let ne_l = if code == OP_SEQ || code == OP_EQ { no } else { yes };
            self.equal(x, y, code == OP_EQ || code == OP_NE, pc, eq_l, ne_l);
            self.a.bind(no);
            return;
        }
        self.guard_num(x, pc);
        self.guard_num(y, pc);
        self.load(0, x);
        self.load(1, y);
        self.a.jump_fcmp(fcond(code), 0, 1, yes);
    }
    /// to eq when x === y (x == y when loose), else to ne
    fn equal(&mut self, x: int, y: int, loose: bool, pc: int, eq_l: int, ne_l: int) {
        let xb = opnd_base(x);
        let xd = opnd_disp(x);
        let yb = opnd_base(y);
        let yd = opnd_disp(y);
        let ex = self.exit(pc);
        let num = self.a.label();
        self.a.cmp_kind(xb, xd, TG_NUM);
        self.a.jcc(C_BE, num);
        // x is no number: the same bits are equal (undefined, null, the
        // same boolean, object, string handle or small BigInt)
        self.a.same_bits(xb, xd, yb, yd, eq_l);
        if loose {
            // undefined == null; another kind converts (interpreter)
            let nx = self.a.label();
            self.a.cmp_kind(xb, xd, TG_NULL);
            self.a.jcc(C_A, nx);
            self.a.cmp_kind(yb, yd, TG_UNDEF);
            self.a.jcc(C_EQ, eq_l);
            self.a.cmp_scratch(TG_NULL);
            self.a.jcc(C_EQ, eq_l);
            self.a.jmp(ne_l);
            self.a.bind(nx);
            // y nullish: unequal
            self.a.cmp_kind(yb, yd, TG_UNDEF);
            self.a.jcc(C_EQ, ne_l);
            self.a.cmp_scratch(TG_NULL);
            self.a.jcc(C_EQ, ne_l);
        }
        // two strings with other handles: their text decides
        let s = self.a.label();
        self.a.cmp_kind(xb, xd, TG_STR);
        self.a.jcc(C_NE, s);
        self.a.cmp_kind(yb, yd, TG_STR);
        self.a.jcc(C_EQ, ex);
        self.a.bind(s);
        if loose {
            // other kinds differing: conversions (interpreter); the same
            // kind with other bits: unequal
            self.a.load_kind(xb, xd);
            self.a.p_save_kind();
            self.a.load_kind(yb, yd);
            self.a.cmp_saved_kind();
            self.a.jcc(C_NE, ex);
        }
        self.a.jmp(ne_l);
        self.a.bind(num);
        self.a.not_num(yb, yd, if loose { ex } else { ne_l });
        self.load(0, x);
        self.load(1, y);
        self.a.jump_fcmp(C_FEQ, 0, 1, eq_l);
        self.a.jmp(ne_l);
    }
    /// to label `yes` when x is truthy (want true) or falsy (want false);
    /// a BigInt goes back to the interpreter
    fn jump_truth(&mut self, x: int, pc: int, want: bool, yes: int) {
        let b = opnd_base(x);
        let d = opnd_disp(x);
        let tru = self.a.label();
        let fal = self.a.label();
        let num = self.a.label();
        let ex = self.exit(pc);
        self.a.load_kind(b, d);
        self.a.cmp_scratch(TG_NUM);
        self.a.jcc(C_BE, num);
        self.a.cmp_scratch(TG_OBJ);
        self.a.jcc(C_EQ, tru);
        self.a.cmp_scratch(TG_NULL);
        self.a.jcc(C_BE, fal);
        self.a.cmp_scratch(TG_STR);
        self.a.jcc(C_A, ex);
        // a boolean or a string: low bits != 0 (the empty string is handle 0)
        self.a.low_zero(b, d);
        self.a.jcc(C_EQ, fal);
        self.a.jmp(tru);
        self.a.bind(num);
        self.a.zero_d(1);
        self.a.load_d(0, b, d);
        // 0 and NaN are false
        self.a.jump_fcmp(C_FEQ, 0, 1, fal);
        self.a.fcmp(0, 0);
        self.a.jnan(fal);
        self.a.jmp(tru);
        let go = self.a.label();
        let on = self.a.label();
        self.a.bind(tru);
        if want { self.a.jmp(go); } else { self.a.jmp(on); }
        self.a.bind(fal);
        if want { self.a.jmp(on); } else { self.a.jmp(go); }
        self.a.bind(go);
        self.a.jmp(yes);
        self.a.bind(on);
    }
}

fn fcond(code: int) -> int {
    if code == OP_LT { C_FLT } else if code == OP_LE { C_FLE } else if code == OP_GT { C_FGT } else if code == OP_GE { C_FGE } else if code == OP_SEQ || code == OP_EQ { C_FEQ } else { C_FNE }
}

/// The jump targets of the function (where the code may be entered).
fn targets(ops: &Vec<Op>, rb: int, n: int) -> Vec<bool> {
    let mut t: Vec<bool> = Vec::new();
    let mut i: int = 0;
    while i < n {
        t.push(false);
        i += 1;
    }
    let mut pc: int = 0;
    while pc < n {
        let o = ops[(rb + pc) as usize];
        let w0 = (o.code as int);
        let kind = w0 & 255;
        let mut to: int = -1;
        let mut to2: int = -1;
        if kind == R_JUMP {
            to = (o.a as int);
        } else if kind == R_JF || kind == R_JT || kind == R_JF_KEEP || kind == R_JT_KEEP {
            to = (o.b as int);
        } else if kind == R_CMP_JF || kind == R_CMP_JT || kind == R_LOCAL_NULL_JF {
            to = (o.c as int);
        } else if kind == R_CMP_JFK || kind == R_CMP_JTK {
            to = (o.a as int) >> 16;
        } else if kind == R_LOOP {
            to = w0 >> 16;
            to2 = (o.c as int);
        }
        if to >= 0 && to < n {
            t[to as usize] = true;
        }
        if to2 >= 0 && to2 < n {
            t[to2 as usize] = true;
        }
        pc += 1;
    }
    t
}

/// Where the interpreter comes back into the code: the op after each call
/// (rvm.rs jit_res), so a function that calls goes on in compiled code.
fn resumes(ops: &Vec<Op>, rb: int, n: int) -> Vec<bool> {
    let mut t: Vec<bool> = Vec::new();
    let mut i: int = 0;
    while i < n {
        t.push(false);
        i += 1;
    }
    let mut pc: int = 0;
    while pc < n {
        let w0 = (ops[(rb + pc) as usize].code as int);
        if (w0 & 255) == R_CALL {
            let at = pc + 1 + (w0 >> 16);
            if at < n {
                t[at as usize] = true;
            }
        }
        pc += 1;
    }
    t
}

/// The machine code of function rcode[rb .. rb + n] for AArch64 (arm) or
/// x86-64, or an empty Vec when it has no loop the code handles.
pub fn compile(ops: &Vec<Op>, pi: int, rb: int, n: int, arm: bool, lay: &Vec<int>, tab: &Vec<int>) -> Vec<int> {
    let tg = targets(ops, rb, n);
    let rs = resumes(ops, rb, n);
    let mut worth = false;
    let mut count: int = 0;
    let mut pc: int = 0;
    while pc < n {
        let o = ops[(rb + pc) as usize];
        let w0 = (o.code as int);
        let kind = w0 & 255;
        if handles(o, pc, lay, tab) {
            if kind == R_LOOP || (kind == R_JUMP && (o.a as int) <= pc) {
                worth = true;
            }
            if kind != R_NOP {
                count += 1;
            }
        }
        pc += 1;
    }
    // a function without a loop is entered at its start and after each
    // call: worth it when it does some work here
    if count >= 4 {
        worth = true;
    }
    if !worth {
        return Vec::new();
    }
    // labels 0..n are the pcs
    let mut cg = Cg { a: Asm::new(arm, n), n: n, exits: Vec::new(), budgets: Vec::new(), epi: 0 };
    let mut i: int = 0;
    while i < n {
        cg.exits.push(-1);
        cg.budgets.push(-1);
        i += 1;
    }
    cg.epi = cg.a.label();
    let epi = cg.epi;
    cg.a.prologue();
    pc = 0;
    while pc < n {
        if (pc == 0 || tg[pc as usize] || rs[pc as usize]) && handles(ops[(rb + pc) as usize], pc, lay, tab) {
            cg.a.entry(pc, pc);
        }
        pc += 1;
    }
    // any other pc: back at once
    cg.a.ret_entry();
    cg.a.jmp(epi);
    pc = 0;
    while pc < n {
        cg.a.bind(pc);
        let o = ops[(rb + pc) as usize];
        let w0 = (o.code as int);
        let kind = w0 & 255;
        let sub = (w0 >> 8) & 255;
        let skip = w0 >> 16;
        let w1 = (o.a as int);
        let w2 = (o.b as int);
        let w3 = (o.c as int);
        let next = pc + 1 + skip;
        let mut falls = true;
        if !handles(o, pc, lay, tab) {
            let l = cg.exit(pc);
            cg.a.jmp(l);
            falls = false;
        } else if (kind == R_GET_PROP || kind == R_GET_THIS_PROP || kind == R_SET_PROP) && prop_site(kind, sub, w3, lay) {
            // one shape in place; anything else through slow()
            let slow = cg.a.label();
            let done = cg.a.label();
            if kind == R_SET_PROP {
                cg.set_prop(w1, w2, w3, slow, lay);
            } else {
                cg.get_prop(kind, w1, w2, w3, slow, lay);
            }
            cg.a.jmp(done);
            cg.a.bind(slow);
            let ex = cg.exit(pc);
            cg.a.slow_call(pi, pc, ex);
            cg.a.bind(done);
        } else if kind == R_ARITH && sub == OP_DIV {
            cg.guard_num(w2, pc);
            cg.guard_num(w3, pc);
            cg.load(0, w2);
            cg.load(1, w3);
            cg.a.arith(F_DIV, 0, 1);
            cg.store_num(0, w1);
        } else if kind == R_TONUM && sub != 1 {
            // a number stays itself; the rest converts in slow()
            let slow = cg.a.label();
            let done = cg.a.label();
            cg.a.not_num(opnd_base(w2), opnd_disp(w2), slow);
            cg.a.copy_val(B_SP, w1 * VSZ, opnd_base(w2), opnd_disp(w2));
            cg.a.jmp(done);
            cg.a.bind(slow);
            let ex = cg.exit(pc);
            cg.a.slow_call(pi, pc, ex);
            cg.a.bind(done);
        } else if kind == R_NOT {
            let tru = cg.a.label();
            let after = cg.a.label();
            cg.jump_truth(w2, pc, true, tru);
            cg.a.store_tag(B_SP, w1 * VSZ, TG_BOOL, 1);
            cg.a.jmp(after);
            cg.a.bind(tru);
            cg.a.store_tag(B_SP, w1 * VSZ, TG_BOOL, 0);
            cg.a.bind(after);
        } else if is_slow(kind, sub) {
            let ex = cg.exit(pc);
            cg.a.slow_call(pi, pc, ex);
        } else if kind == R_NOP {
            // nothing
        } else if kind == R_MOVE {
            cg.a.copy_val(B_SP, w1 * VSZ, opnd_base(w2), opnd_disp(w2));
        } else if kind == R_LOAD2 {
            cg.a.copy_val(B_SP, w1 * VSZ, opnd_base(w2), opnd_disp(w2));
            cg.a.copy_val(B_SP, (w1 + 1) * VSZ, opnd_base(w3), opnd_disp(w3));
        } else if kind == R_ADD || kind == R_SUB || kind == R_MUL {
            cg.guard_num(w2, pc);
            cg.guard_num(w3, pc);
            cg.load(0, w2);
            cg.load(1, w3);
            let f = if kind == R_ADD { F_ADD } else if kind == R_SUB { F_SUB } else { F_MUL };
            cg.a.arith(f, 0, 1);
            cg.store_num(0, w1);
        } else if kind == R_GET_ENV {
            cg.get_env(w1, w2, w3, sub, pc, lay);
        } else if kind == R_SWITCH {
            // a number in the table's range to its case, anything else to
            // the default
            let size = tab[w2 as usize];
            let dflt = tab[(w2 + 1 + size) as usize];
            cg.a.load_kind(B_SP, w1 * VSZ);
            cg.a.cmp_scratch(TG_NUM);
            cg.a.jcc(C_A, dflt);
            cg.load(0, w1);
            let mut i: int = 0;
            while i < size {
                cg.a.dbl_imm(1, w3 + i);
                cg.a.jump_fcmp(C_FEQ, 0, 1, tab[(w2 + 1 + i) as usize]);
                i += 1;
            }
            cg.a.jmp(dflt);
            falls = false;
        } else if kind == R_LOCAL_NULL_JF {
            // on when (x null, cmp === or ==), (undefined, !== or ==),
            // (else, !== or !=); else to w3
            let on_null = w2 == OP_SEQ || w2 == OP_EQ;
            let on_undef = w2 == OP_SNE || w2 == OP_EQ;
            let on_else = w2 == OP_SNE || w2 == OP_NE;
            let go = cg.a.label();
            let on = cg.a.label();
            cg.a.load_kind(B_SP, w1 * VSZ);
            cg.a.cmp_scratch(TG_NULL);
            cg.a.jcc(C_EQ, if on_null { on } else { go });
            cg.a.cmp_scratch(TG_UNDEF);
            cg.a.jcc(C_EQ, if on_undef { on } else { go });
            if on_else { cg.a.jmp(on); } else { cg.a.jmp(go); }
            cg.a.bind(go);
            cg.jump_to(pc, w3);
            cg.a.bind(on);
        } else if kind == R_CMP_JFK || kind == R_CMP_JTK {
            // the result goes to its slot either way, then the jump
            let dst = w1 & 65535;
            let to = w1 >> 16;
            let yes = cg.a.label();
            let after = cg.a.label();
            cg.cond(sub, w2, w3, pc, yes);
            cg.a.store_tag(B_SP, dst * VSZ, TG_BOOL, 0);
            if kind == R_CMP_JFK {
                cg.jump_to(pc, to);
                cg.a.bind(yes);
                cg.a.store_tag(B_SP, dst * VSZ, TG_BOOL, 1);
            } else {
                cg.a.jmp(after);
                cg.a.bind(yes);
                cg.a.store_tag(B_SP, dst * VSZ, TG_BOOL, 1);
                cg.jump_to(pc, to);
            }
            cg.a.bind(after);
        } else if kind == R_CMP_JF || kind == R_CMP_JT || kind == R_CMP {
            let x = if kind == R_CMP { w2 } else { w1 };
            let y = if kind == R_CMP { w3 } else { w2 };
            let yes = cg.a.label();
            let after = cg.a.label();
            cg.cond(sub, x, y, pc, yes);
            if kind == R_CMP {
                cg.a.store_tag(B_SP, w1 * VSZ, TG_BOOL, 0);
                cg.a.jmp(after);
                cg.a.bind(yes);
                cg.a.store_tag(B_SP, w1 * VSZ, TG_BOOL, 1);
                cg.a.bind(after);
            } else if kind == R_CMP_JF {
                cg.jump_to(pc, w3);
                cg.a.bind(yes);
                cg.a.bind(after);
            } else {
                cg.a.jmp(after);
                cg.a.bind(yes);
                cg.jump_to(pc, w3);
                cg.a.bind(after);
            }
        } else if kind == R_LOOP {
            let x = w1;
            let y = w2;
            cg.guard_num(x, pc);
            cg.guard_num(y, pc);
            cg.load(0, x);
            cg.load(1, y);
            let yes = cg.a.label();
            let after = cg.a.label();
            cg.a.jump_fcmp(fcond(sub), 0, 1, yes);
            // holds -> the body (skip), else the exit (w3)
            cg.jump_to(pc, w3);
            cg.a.bind(yes);
            cg.jump_to(pc, skip);
            cg.a.bind(after);
            falls = false;
        } else if kind == R_JUMP {
            cg.jump_to(pc, w1);
            falls = false;
        } else if kind == R_JF || kind == R_JT || kind == R_JF_KEEP || kind == R_JT_KEEP {
            let yes = cg.a.label();
            let after = cg.a.label();
            cg.jump_truth(w1, pc, kind == R_JT || kind == R_JT_KEEP, yes);
            cg.a.jmp(after);
            cg.a.bind(yes);
            cg.jump_to(pc, w2);
            cg.a.bind(after);
        } else if kind == R_INC_LOCAL {
            cg.guard_num(w1, pc);
            cg.load(0, w1);
            cg.a.dbl_imm(1, w2);
            cg.a.arith(F_ADD, 0, 1);
            cg.a.store_d(0, B_SP, w1 * VSZ);
        } else if kind == R_INC {
            cg.guard_num(w2, pc);
            cg.load(0, w2);
            cg.a.dbl_imm(1, 1);
            cg.a.arith(if sub == 0 { F_ADD } else { F_SUB }, 0, 1);
            cg.store_num(0, w1);
        } else if kind == R_POSTINC {
            // dst = the old value (sub 0) or the new (sub 1); local += delta
            cg.guard_num(w2, pc);
            cg.load(0, w2);
            cg.a.dbl_imm(1, w3);
            cg.a.arith(F_ADD, 1, 0);
            cg.a.store_d(1, B_SP, w2 * VSZ);
            cg.store_num(if sub == 0 { 0 } else { 1 }, w1);
        } else if kind == R_ADD_LOCAL_POP {
            cg.guard_num(w1, pc);
            cg.guard_num(w2, pc);
            cg.load(0, w1);
            cg.load(1, w2);
            cg.a.arith(F_ADD, 0, 1);
            cg.a.store_d(0, B_SP, w1 * VSZ);
        }
        if falls && next != pc + 1 {
            cg.a.jmp(next);
        }
        pc += 1;
    }
    // past the last op
    cg.a.ret_value(n);
    cg.a.jmp(epi);
    pc = 0;
    while pc < n {
        if cg.exits[pc as usize] >= 0 {
            let l = cg.exits[pc as usize];
            cg.a.bind(l);
            cg.a.ret_value(pc);
            cg.a.jmp(epi);
        }
        if cg.budgets[pc as usize] >= 0 {
            let l = cg.budgets[pc as usize];
            cg.a.bind(l);
            cg.a.ret_value(-4 - pc);
            cg.a.jmp(epi);
        }
        pc += 1;
    }
    cg.a.bind(epi);
    cg.a.epilogue();
    if !cg.a.link() {
        return Vec::new();
    }
    cg.a.b
}
/// Runs function pi's code from pc (compiled the first time): the pc the
/// interpreter goes on at, or -4 - pc when the budget ran out on the way
/// to pc. vm.jit_fn[pi]: -1 not tried, -2 nothing to compile, else the
/// host's handle.
pub fn enter(vm: &mut crate::vm::Vm, pi: usize, bp: int, pc: int, budget: int) -> int {
    while vm.jit_fn.len() <= pi {
        vm.jit_fn.push(-1);
    }
    let mut h = vm.jit_fn[pi];
    if h == -1 {
        h = -2;
        let rb = vm.protos[pi].rbase;
        let n = vm.protos[pi].code.len() as int;
        if rb >= 0 && ((rb + n) as usize) <= vm.rcode.len() {
            let m = arch();
            let lay = layout();
            let code = if m > 0 { compile(&vm.rcode, pi as int, rb, n, m == 2, &lay, &vm.rtab) } else { Vec::new() };
            if code.len() > 0 {
                h = install(&code);
            }
            if h >= 0 {
                // the interpreter comes back in after each call
                let rs = resumes(&vm.rcode, rb, n);
                let mut i: int = 0;
                while i < n {
                    if rs[i as usize] {
                        let at = (rb + i) as usize;
                        while vm.jit_res.len() <= at {
                            vm.jit_res.push(false);
                        }
                        vm.jit_res[at] = true;
                    }
                    i += 1;
                }
            }
        }
        vm.jit_fn[pi] = h;
    }
    if h < 0 {
        // nothing compiled: rvm.rs jit_run no longer comes here from pc
        let ri = (vm.protos[pi].rbase + pc) as usize;
        while vm.jit_miss.len() <= ri {
            vm.jit_miss.push(0);
        }
        vm.jit_miss[ri] = 1 << 30;
        return pc;
    }
    let fi = vm.frames.len() - 1;
    // the context block (CTX_*): &this, the Elems table, the frame's
    // scope, the machine, slow() and the frame base
    let r: int = ranger::native!(rust: pc, cpp: "njit_call({h}, (void*)({vm}->stack.data() + {bp}), (void*)({vm}->kvals.data()), {pc}, {budget}, (void*)({vm}->objs.data()), (void*)(&{vm}->frames[{fi}].this_val), (void*)({vm}->evecs.data()), {vm}->frames[{fi}].env, (void*)(&{vm}), (void*)(&Njit::slow), {bp})");
    if r == pc {
        // back at once: the loop head starts with an op the code does not
        // do; after JIT_MISS_MAX of these the head is no longer entered
        let ri = (vm.protos[pi].rbase + pc) as usize;
        while vm.jit_miss.len() <= ri {
            vm.jit_miss.push(0);
        }
        vm.jit_miss[ri] += 4;
    }
    r
}

/// The op at pc of function pi for compiled code (frame base bp): 1 when
/// it is done (the interpreter's paths that call no JS and throw nothing),
/// 0 to go back to the interpreter at pc. Compiled code calls it through
/// the context block (CTX_FN): it allocates nothing, so the stack, object
/// and element tables stay where the code has them.
pub fn slow(vm: &mut crate::vm::Vm, pi: int, pc: int, bp: int) -> int {
    let rb = vm.protos[pi as usize].rbase;
    let site = (rb + pc) as usize;
    let o = vm.rcode[site];
    let w0 = (o.code as int);
    let kind = w0 & 255;
    let sub = (w0 >> 8) & 255;
    let w1 = (o.a as int);
    let w2 = (o.b as int);
    let w3 = (o.c as int);
    if kind == R_GET_GLOBAL || kind == R_SET_GLOBAL {
        let g = vm.global;
        let c = vm.protos[pi as usize].code[pc as usize].c as int;
        if kind == R_GET_GLOBAL {
            let slot = vm.global_ic(g, w3, c);
            if slot < 0 {
                return 0;
            }
            let v = slot_at(vm.ob(g), slot as usize);
            vm.rset((bp + w1) as usize, v);
            return 1;
        }
        let slot = vm.ic_slot(g, w3, c, true);
        if slot < 0 {
            return 0;
        }
        let v = vm.rget(bp, w1).clone();
        vm.put_slot(g, slot, v);
        return 1;
    }
    if kind == R_TONUM {
        if let Some(x) = (vm.rget(bp, w2)).as_num() {
            let r = if sub == 1 { -x } else { x };
            vm.rset((bp + w1) as usize, Val::NumC(r));
            return 1;
        }
        return 0;
    }
    if kind == R_SET_ENV {
        let fi = vm.frames.len() - 1;
        let e = vm.env_of(fi, w2);
        let v = vm.rget(bp, w1).clone();
        vm.env_put(e, w3, v);
        return 1;
    }
    if kind == R_TYPEOF {
        if vm.type_vals.is_empty() {
            return 0;
        }
        let v = vm.rget(bp, w2).clone();
        let t = vm.type_val(&v);
        vm.rset((bp + w1) as usize, t);
        return 1;
    }
    if kind == R_GET_ELEM {
        let mut r = Val::Undef();
        let mut done = false;
        if let Some(o) = (vm.rget(bp, w2)).as_obj() {
            if let Some(n) = (vm.rget(bp, w3)).as_num() {
                let ob = obj_at(&vm.objs, o as usize);
                if ob.class == C_ARRAY {
                    let i = n as int;
                    let ev = ob.elems as usize;
                    if i >= 0 && i < (evec_at(&vm.evecs, ev).v.len() as int) && (i as double) == n && ob.pos != 3 && !vm.idx_protos {
                        r = val_at(&evec_at(&vm.evecs, ev).v, i as usize).clone();
                        done = true;
                    }
                }
            }
        }
        if !done {
            return 0;
        }
        vm.rset((bp + w1) as usize, r);
        return 1;
    }
    if kind == R_ARITH {
        let va = *vm.rget(bp, w2);
        let vb = *vm.rget(bp, w3);
        if !(va.is_num() && vb.is_num()) {
            return 0;
        }
        let a = va.num();
        let b = vb.num();
        let r = if sub >= OP_BAND && sub <= OP_USHR {
            bit_fast(sub, a, b)
        } else if sub == OP_MOD && a >= 1.0 && b > 0.0 && a <= 2147483647.0 && b <= 2147483647.0 && a == ((a as int) as double) && b == ((b as int) as double) {
            ((a as int) % (b as int)) as double
        } else {
            arith_num(sub, a, b)
        };
        vm.rset((bp + w1) as usize, Val::NumC(r));
        return 1;
    }
    if kind == R_SET_PROP {
        let ob = obj_of(vm.rget(bp, w1));
        if ob < 0 || w3 < 65536 || sub != 0 {
            return 0;
        }
        let mut c = w3;
        if ((c >> 16) as int) != vm.ob(ob).shape as int {
            if site >= vm.ic2.len() {
                return 0;
            }
            let c2 = vm.ic2[site] as int;
            if c2 < 65536 || (c2 >> 16) != vm.ob(ob).shape as int {
                return 0;
            }
            c = c2;
        }
        let v = vm.rget(bp, w2).clone();
        vm.put_slot(ob, c & 65535, v);
        return 1;
    }
    if kind == R_GET_PROP || kind == R_GET_THIS_PROP {
        let mut ob: int = -1;
        if kind == R_GET_PROP {
            if let Some(x) = (vm.rget(bp, w2)).as_obj() {
                ob = x;
            }
        } else {
            let fi = vm.frames.len() - 1;
            if let Some(x) = (&vm.frames[fi].this_val).as_obj() {
                ob = x;
            }
        }
        if ob < 0 {
            return 0;
        }
        let c = w3;
        if c >= 65536 {
            let mut cc = c;
            if (c >> 16) != vm.ob(ob).shape as int {
                if site >= vm.ic2.len() {
                    return 0;
                }
                cc = vm.ic2[site] as int;
                if cc < 65536 || (cc >> 16) != vm.ob(ob).shape as int {
                    return 0;
                }
            }
            let r = slot_at(vm.ob(ob), (cc & 65535) as usize);
            vm.rset((bp + w1) as usize, r);
            return 1;
        }
        if c == -4 && (vm.ob(ob).class == C_ARRAY || vm.ob(ob).class == C_ARGUMENTS) {
            let n = evec_at(&vm.evecs, vm.ob(ob).elems as usize).v.len() as double;
            vm.rset((bp + w1) as usize, Val::NumC(n));
            return 1;
        }
        return 0;
    }
    0
}

/// The bytes into executable memory, a handle back (-2 when it cannot).
/// njit_install is in the runtime arch() writes.
fn install(code: &Vec<int>) -> int {
    let h: int = ranger::native!(rust: -2, cpp: "njit_install({code})");
    h
}

/// The machine: 1 x86-64, 2 AArch64, 0 one the JIT does not write (and
/// every target but C++: the `rust` arms, which rustc never sees, as the
/// module is not in cargo's build). Its
/// cpp_polyfill is the C++ runtime of the JIT (executable memory, the call
/// into it), written once at the top of the generated file, so a C++ host
/// needs nothing of its own. One mapping per function, written while it is
/// read-write and then made read-execute (W^X); Apple silicon: MAP_JIT with
/// the thread's write protection toggled around the copy. CER_NJIT=0 in the
/// environment turns the JIT off.
fn arch() -> int {
    let m: int = ranger::native!(
        rust: 0,
        cpp: "njit_arch()",
        cpp_polyfill: r#"#include <cstddef>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <vector>
#include <sys/mman.h>
#include <unistd.h>
#if defined(__APPLE__)
#include <pthread.h>
#include <libkern/OSCacheControl.h>
#endif

namespace njit_rt {
typedef long long (*Fn)(void* sp, void* kv, long long pc, long long budget, void* objs, void* ctx);
struct Code {
  void* p;
  size_t n;
};
inline std::vector<Code>& codes() {
  static std::vector<Code> v;
  return v;
}
inline bool& disabled() {
  static bool off = false;
  return off;
}
}  // namespace njit_rt

/// The machine njit.rs writes for: 1 x86-64, 2 AArch64, 0 none (the JIT
/// stays off). CER_NJIT=0 in the environment turns it off too.
inline int njit_arch() {
  const char* e = getenv("CER_NJIT");
  if (e && e[0] == '0') return 0;
#if defined(__x86_64__)
  return 1;
#elif defined(__aarch64__)
  return 2;
#else
  return 0;
#endif
}

/// The bytes (each 0..255) as an executable function; -2 when the system
/// refuses executable memory (the JIT then stays off for that function).
inline int njit_install(const std::vector<int>& bytes) {
  if (njit_rt::disabled() || bytes.empty()) return -2;
#if !(defined(__x86_64__) || defined(__aarch64__))
  return -2;
#else
  size_t page = (size_t)sysconf(_SC_PAGESIZE);
  size_t n = (bytes.size() + page - 1) / page * page;
#if defined(__APPLE__)
  void* p = mmap(nullptr, n, PROT_READ | PROT_WRITE | PROT_EXEC, MAP_PRIVATE | MAP_ANON | MAP_JIT, -1, 0);
#else
  void* p = mmap(nullptr, n, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
#endif
  if (p == MAP_FAILED) {
    njit_rt::disabled() = true;
    return -2;
  }
#if defined(__APPLE__)
  pthread_jit_write_protect_np(0);
#endif
  unsigned char* d = (unsigned char*)p;
  for (size_t i = 0; i < bytes.size(); i++) d[i] = (unsigned char)bytes[i];
#if defined(__APPLE__)
  pthread_jit_write_protect_np(1);
  sys_icache_invalidate(p, bytes.size());
#else
  if (mprotect(p, n, PROT_READ | PROT_EXEC) != 0) {
    munmap(p, n);
    njit_rt::disabled() = true;
    return -2;
  }
#if defined(__aarch64__)
  __builtin___clear_cache((char*)p, (char*)p + bytes.size());
#endif
#endif
  njit_rt::codes().push_back({p, n});
  return (int)njit_rt::codes().size() - 1;
#endif
}

/// ctx: &this, the Elems table, the frame's scope (njit.rs CTX_*)
inline int njit_call(int h, void* sp, void* kv, int pc, int budget, void* objs, void* self, void* evecs, int env, void* vm, void* slow, int bp) {
  njit_rt::Fn f = (njit_rt::Fn)njit_rt::codes()[(size_t)h].p;
  long long ctx[6] = {(long long)(intptr_t)self, (long long)(intptr_t)evecs, (long long)env, (long long)(intptr_t)vm, (long long)(intptr_t)slow, (long long)bp};
  return (int)f(sp, kv, pc, budget, objs, (void*)ctx);
}
"#
    );
    m
}

/// The object layout of the C++ build (L_OK..L_V); all 0 elsewhere.
fn layout() -> Vec<int> {
    let ok: int = ranger::native!(rust: 0, cpp: "((int)sizeof(std::shared_ptr<JsObj>) == 16 && (int)sizeof(std::shared_ptr<Elems>) == 16 && (int)sizeof(Val) == 8 ? 1 : 0)");
    let mut v: Vec<int> = Vec::new();
    v.push(ok);
    let a: int = ranger::native!(rust: 0, cpp: "(int)offsetof(JsObj, shape)");
    v.push(a);
    let b: int = ranger::native!(rust: 0, cpp: "(int)offsetof(JsObj, inl0)");
    v.push(b);
    let c: int = ranger::native!(rust: 0, cpp: "(int)offsetof(JsObj, inl1)");
    v.push(c);
    let d: int = ranger::native!(rust: 0, cpp: "(int)offsetof(JsObj, vals)");
    v.push(d);
    let e: int = ranger::native!(rust: 0, cpp: "(int)offsetof(Slots, v)");
    v.push(e);
    let f: int = ranger::native!(rust: 0, cpp: "(int)offsetof(JsObj, proto)");
    v.push(f);
    let g: int = ranger::native!(rust: 0, cpp: "(int)offsetof(JsObj, elems)");
    v.push(g);
    let h: int = ranger::native!(rust: 0, cpp: "(int)offsetof(Elems, v)");
    v.push(h);
    v
}
