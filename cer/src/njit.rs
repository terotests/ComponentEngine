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

/// value.rs (C++): the size of a Val and where its parts lie
const VSZ: int = 16;
const OFF_I: int = 4;
const OFF_D: int = 8;
const T_NUM: int = 3;

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
    /// compare the kind (t) of [base+disp] with k
    fn cmp_kind(&mut self, base: int, disp: int, k: int) {
        if self.arm {
            self.a_ldst(0xB940, 9, base, disp, 4);
            // cmp w9, #k
            self.w(0x7100 + (k >> 6), ((k & 63) << 10) + (9 << 5) + 31);
        } else {
            self.u8(0x83);
            self.x_mem(7, Asm::x_base(base), disp);
            self.u8(k);
        }
    }
    /// load the kind into the scratch register (then cmp_scratch)
    fn load_kind(&mut self, base: int, disp: int) {
        if self.arm {
            self.a_ldst(0xB940, 9, base, disp, 4);
        } else {
            self.u8(0x8B);
            self.x_mem(RAX, Asm::x_base(base), disp);
        }
    }
    fn cmp_scratch(&mut self, k: int) {
        if self.arm {
            self.w(0x7100 + (k >> 6), ((k & 63) << 10) + (9 << 5) + 31);
        } else {
            self.u8(0x83);
            self.u8(0xF8);
            self.u8(k);
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
    /// a Val's t and i
    fn store_tag(&mut self, base: int, disp: int, t: int, i: int) {
        if self.arm {
            self.a_mov32(10, t);
            self.a_ldst(0xB900, 10, base, disp, 4);
            if i == 0 {
                self.a_ldst(0xB900, 31, base, disp + OFF_I, 4);
            } else {
                self.a_mov32(10, i);
                self.a_ldst(0xB900, 10, base, disp + OFF_I, 4);
            }
        } else {
            self.u8(0xC7);
            self.x_mem(0, Asm::x_base(base), disp);
            self.d32(t);
            self.u8(0xC7);
            self.x_mem(0, Asm::x_base(base), disp + OFF_I);
            self.d32(i);
        }
    }
    /// a whole Val from [sb+sd] to [db+dd]
    fn copy_val(&mut self, db: int, dd: int, sb: int, sd: int) {
        if self.arm {
            self.a_ldst(0xF940, 10, sb, sd, 8);
            self.a_ldst(0xF940, 11, sb, sd + 8, 8);
            self.a_ldst(0xF900, 10, db, dd, 8);
            self.a_ldst(0xF900, 11, db, dd + 8, 8);
        } else {
            self.u8(0x0F);
            self.u8(0x10);
            self.x_mem(7, Asm::x_base(sb), sd);
            self.u8(0x0F);
            self.u8(0x11);
            self.x_mem(7, Asm::x_base(db), dd);
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

/// Whether the op kind is one this file writes.
fn handled(kind: int, sub: int) -> bool {
    if kind == R_NOP || kind == R_MOVE || kind == R_LOAD2 || kind == R_ADD || kind == R_SUB || kind == R_MUL {
        return true;
    }
    if kind == R_ARITH {
        return sub == OP_DIV;
    }
    if kind == R_CMP_JF || kind == R_CMP_JT || kind == R_LOOP || kind == R_CMP {
        return is_num_cmp(sub);
    }
    kind == R_JUMP || kind == R_JF || kind == R_JT || kind == R_INC_LOCAL || kind == R_INC || kind == R_POSTINC || kind == R_ADD_LOCAL_POP
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
        self.a.cmp_kind(opnd_base(x), opnd_disp(x), T_NUM);
        self.a.jcc(C_NE, l);
    }
    fn load(&mut self, f: int, x: int) {
        self.a.load_d(f, opnd_base(x), opnd_disp(x) + OFF_D);
    }
    /// f into register slot r as a number
    fn store_num(&mut self, f: int, r: int) {
        self.a.store_tag(B_SP, r * VSZ, T_NUM, 0);
        self.a.store_d(f, B_SP, r * VSZ + OFF_D);
    }
    /// a jump from pc to t: a back edge spends one of the budget
    fn jump_to(&mut self, pc: int, t: int) {
        if t <= pc {
            let l = self.budget(t);
            self.a.spend(l);
        }
        self.a.jmp(t);
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
        self.a.cmp_scratch(T_NUM);
        self.a.jcc(C_EQ, num);
        self.a.cmp_scratch(5);
        self.a.jcc(C_EQ, tru);
        self.a.cmp_scratch(1);
        self.a.jcc(C_BE, fal);
        self.a.cmp_scratch(4);
        self.a.jcc(C_A, ex);
        // a boolean or a string: i != 0
        self.a.cmp_kind(b, d + OFF_I, 0);
        self.a.jcc(C_EQ, fal);
        self.a.jmp(tru);
        self.a.bind(num);
        self.a.zero_d(1);
        self.a.load_d(0, b, d + OFF_D);
        // 0 and NaN are false
        self.a.jump_fcmp(C_FNE, 0, 1, tru);
        self.a.jmp(fal);
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
        } else if kind == R_JF || kind == R_JT {
            to = (o.b as int);
        } else if kind == R_CMP_JF || kind == R_CMP_JT {
            to = (o.c as int);
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

/// The machine code of function rcode[rb .. rb + n] for AArch64 (arm) or
/// x86-64, or an empty Vec when it has no loop the code handles.
pub fn compile(ops: &Vec<Op>, rb: int, n: int, arm: bool) -> Vec<int> {
    let tg = targets(ops, rb, n);
    let mut worth = false;
    let mut pc: int = 0;
    while pc < n {
        let o = ops[(rb + pc) as usize];
        let w0 = (o.code as int);
        let kind = w0 & 255;
        if handled(kind, (w0 >> 8) & 255) {
            if kind == R_LOOP || (kind == R_JUMP && (o.a as int) <= pc) {
                worth = true;
            }
        }
        pc += 1;
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
        let w0 = (ops[(rb + pc) as usize].code as int);
        if (pc == 0 || tg[pc as usize]) && handled(w0 & 255, (w0 >> 8) & 255) {
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
        if !handled(kind, sub) {
            let l = cg.exit(pc);
            cg.a.jmp(l);
            falls = false;
        } else if kind == R_NOP {
            // nothing
        } else if kind == R_MOVE {
            cg.a.copy_val(B_SP, w1 * VSZ, opnd_base(w2), opnd_disp(w2));
        } else if kind == R_LOAD2 {
            cg.a.copy_val(B_SP, w1 * VSZ, opnd_base(w2), opnd_disp(w2));
            cg.a.copy_val(B_SP, (w1 + 1) * VSZ, opnd_base(w3), opnd_disp(w3));
        } else if kind == R_ADD || kind == R_SUB || kind == R_MUL || kind == R_ARITH {
            cg.guard_num(w2, pc);
            cg.guard_num(w3, pc);
            cg.load(0, w2);
            cg.load(1, w3);
            let f = if kind == R_ADD { F_ADD } else if kind == R_SUB { F_SUB } else if kind == R_MUL { F_MUL } else { F_DIV };
            cg.a.arith(f, 0, 1);
            cg.store_num(0, w1);
        } else if kind == R_CMP_JF || kind == R_CMP_JT || kind == R_LOOP || kind == R_CMP {
            let x = if kind == R_CMP { w2 } else { w1 };
            let y = if kind == R_CMP { w3 } else { w2 };
            cg.guard_num(x, pc);
            cg.guard_num(y, pc);
            cg.load(0, x);
            cg.load(1, y);
            let yes = cg.a.label();
            let after = cg.a.label();
            cg.a.jump_fcmp(fcond(sub), 0, 1, yes);
            // does not hold
            if kind == R_CMP {
                cg.a.store_tag(B_SP, w1 * VSZ, 2, 0);
                cg.a.jmp(after);
                cg.a.bind(yes);
                cg.a.store_tag(B_SP, w1 * VSZ, 2, 1);
                cg.a.bind(after);
            } else if kind == R_CMP_JF {
                cg.jump_to(pc, w3);
                cg.a.bind(yes);
                cg.a.bind(after);
            } else if kind == R_CMP_JT {
                cg.a.jmp(after);
                cg.a.bind(yes);
                cg.jump_to(pc, w3);
                cg.a.bind(after);
            } else {
                // R_LOOP: holds -> the body (skip), else the exit (w3)
                cg.jump_to(pc, w3);
                cg.a.bind(yes);
                cg.jump_to(pc, skip);
                cg.a.bind(after);
                falls = false;
            }
        } else if kind == R_JUMP {
            cg.jump_to(pc, w1);
            falls = false;
        } else if kind == R_JF || kind == R_JT {
            let yes = cg.a.label();
            let after = cg.a.label();
            cg.jump_truth(w1, pc, kind == R_JT, yes);
            cg.a.jmp(after);
            cg.a.bind(yes);
            cg.jump_to(pc, w2);
            cg.a.bind(after);
        } else if kind == R_INC_LOCAL {
            cg.guard_num(w1, pc);
            cg.load(0, w1);
            cg.a.dbl_imm(1, w2);
            cg.a.arith(F_ADD, 0, 1);
            cg.a.store_d(0, B_SP, w1 * VSZ + OFF_D);
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
            cg.a.store_d(1, B_SP, w2 * VSZ + OFF_D);
            cg.store_num(if sub == 0 { 0 } else { 1 }, w1);
        } else if kind == R_ADD_LOCAL_POP {
            cg.guard_num(w1, pc);
            cg.guard_num(w2, pc);
            cg.load(0, w1);
            cg.load(1, w2);
            cg.a.arith(F_ADD, 0, 1);
            cg.a.store_d(0, B_SP, w1 * VSZ + OFF_D);
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
            let code = if m > 0 { compile(&vm.rcode, rb, n, m == 2) } else { Vec::new() };
            if code.len() > 0 {
                h = install(&code);
            }
        }
        vm.jit_fn[pi] = h;
    }
    if h < 0 {
        return pc;
    }
    let r = call(h, &vm.stack, bp, &vm.kvals, pc, budget);
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
        cpp_polyfill: r#"#include <cstdint>
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
typedef long long (*Fn)(void* sp, void* kv, long long pc, long long budget);
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

inline int njit_call(int h, void* sp, void* kv, int pc, int budget) {
  njit_rt::Fn f = (njit_rt::Fn)njit_rt::codes()[(size_t)h].p;
  return (int)f(sp, kv, pc, budget);
}
"#
    );
    m
}

fn call(h: int, stack: &Vec<Val>, bp: int, kv: &Vec<Val>, pc: int, budget: int) -> int {
    let r: int = ranger::native!(rust: pc, cpp: "njit_call({h}, (void*)({stack}.data() + {bp}), (void*)({kv}.data()), {pc}, {budget})");
    r
}
