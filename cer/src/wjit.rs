// SPDX-License-Identifier: AGPL-3.0-or-later
// A baseline JIT that writes a function's register code (regs.rs) as a
// WebAssembly module. In a browser (or any WebAssembly host) the host
// compiles the module with its own engine, puts the function into this
// module's function table, and the register machine calls it at a loop's
// back edge. Rust only, behind the `wjit` feature on wasm32.
//
// The compiled function works on the frame as the interpreter does: the
// registers stay in Vm.stack, a constant is read from Vm.kvals, and the
// frame's layout is unchanged. That is why there is no deoptimization:
// whenever an op is one this file does not write, or its operands are not
// of the kind the fast path handles (a string in an addition, an object in
// a comparison), the function returns the op's pc and the interpreter runs
// it from there. The function never calls, allocates or throws.
//
//   f(sp, kv, pc, budget) -> pc to go on at in the interpreter
//
// sp is the address of stack[bp], kv of kvals[0], pc where to start (any
// pc that begins a block; any other pc returns at once). budget counts the
// back edges it may take before it returns (time slices, rvm.rs); a return
// because the budget ran out comes back as -1 - pc.
//
// The code is a dispatch loop over the function's blocks (a block starts at
// pc 0 and at every jump target): a br_table picks the block, a block falls
// through into the next, a jump sets the key and goes round the loop. A
// value's layout in memory (its tag byte and where the payloads lie) is
// found once by looking at real values (probe); when it is not the one this
// file expects the JIT stays off.

use ranger::prelude::*;
use crate::ops::*;
use crate::regs::*;
use crate::value::*;

// ---------------------------------------------------------------- layout

/// Where the parts of a Val lie in memory.
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub ok: bool,
    pub size: u32,
    pub t_undef: u8,
    pub t_null: u8,
    pub t_bool: u8,
    pub t_num: u8,
    pub t_str: u8,
    pub t_obj: u8,
    /// offsets of the payloads
    pub off_bool: u32,
    pub off_num: u32,
    pub off_int: u32,
}

fn bytes_of(v: Val) -> [u8; 16] {
    let mut buf = [0u8; 16];
    let mut out = [0u8; 16];
    if std::mem::size_of::<Val>() != 16 {
        return out;
    }
    // SAFETY: buf is 16 bytes, zeroed, and Val (Copy, 16 bytes) is written
    // into it unaligned; the bytes are then read one at a time with volatile
    // loads, so a padding byte the write left alone reads as its zero and
    // the compiler cannot reason the reads away
    unsafe {
        std::ptr::write_unaligned(buf.as_mut_ptr() as *mut Val, v);
        for i in 0..16 {
            out[i] = std::ptr::read_volatile(buf.as_ptr().add(i));
        }
    }
    out
}

/// Looks at real values to find the layout; `ok` false when it is not a tag
/// byte at offset 0 followed by the payloads, all 16 bytes.
pub fn probe() -> Layout {
    let mut l = Layout {
        ok: false,
        size: std::mem::size_of::<Val>() as u32,
        t_undef: 0,
        t_null: 0,
        t_bool: 0,
        t_num: 0,
        t_str: 0,
        t_obj: 0,
        off_bool: 0,
        off_num: 0,
        off_int: 0,
    };
    if l.size != 16 || std::mem::align_of::<Val>() != 8 {
        return l;
    }
    let u = bytes_of(Val::Undef);
    let n = bytes_of(Val::Null);
    let bt = bytes_of(Val::Bool(true));
    let bf = bytes_of(Val::Bool(false));
    let x = 1.2345678901234567e-300f64;
    let nm = bytes_of(Val::Num(x));
    let s = bytes_of(Val::Str(0x5a3c_1e2d));
    let o = bytes_of(Val::Obj(0x1b2c_3d4e));
    l.t_undef = u[0];
    l.t_null = n[0];
    l.t_bool = bt[0];
    l.t_num = nm[0];
    l.t_str = s[0];
    l.t_obj = o[0];
    let tags = [l.t_undef, l.t_null, l.t_bool, l.t_num, l.t_str, l.t_obj];
    for i in 0..tags.len() {
        for j in (i + 1)..tags.len() {
            if tags[i] == tags[j] {
                return l;
            }
        }
    }
    if bf[0] != bt[0] {
        return l;
    }
    // the bool: the one byte true and false differ in
    let mut ob: i32 = -1;
    for i in 1..16 {
        if bt[i] != bf[i] {
            if ob >= 0 || bt[i] != 1 || bf[i] != 0 {
                return l;
            }
            ob = i as i32;
        }
    }
    let xb = x.to_le_bytes();
    let mut on: i32 = -1;
    for off in [8usize, 4, 1] {
        if off + 8 <= 16 && nm[off..off + 8] == xb {
            on = off as i32;
            break;
        }
    }
    let sb = 0x5a3c_1e2di32.to_le_bytes();
    let obb = 0x1b2c_3d4ei32.to_le_bytes();
    let mut oi: i32 = -1;
    for off in [4usize, 8, 1] {
        if s[off..off + 4] == sb && o[off..off + 4] == obb {
            oi = off as i32;
            break;
        }
    }
    if ob < 0 || on < 0 || oi < 0 {
        return l;
    }
    l.off_bool = ob as u32;
    l.off_num = on as u32;
    l.off_int = oi as u32;
    l.ok = true;
    l
}

// --------------------------------------------------------------- encoder

/// Bytes of a module being written.
pub struct WBuf {
    pub b: Vec<u8>,
}

impl WBuf {
    pub fn new() -> WBuf {
        WBuf { b: Vec::new() }
    }
    pub fn byte(&mut self, x: u8) {
        self.b.push(x);
    }
    pub fn u(&mut self, mut x: u64) {
        loop {
            let mut c = (x & 0x7f) as u8;
            x >>= 7;
            if x != 0 {
                c |= 0x80;
            }
            self.b.push(c);
            if x == 0 {
                break;
            }
        }
    }
    pub fn s(&mut self, mut x: i64) {
        loop {
            let c = (x & 0x7f) as u8;
            x >>= 7;
            let done = (x == 0 && c & 0x40 == 0) || (x == -1 && c & 0x40 != 0);
            self.b.push(if done { c } else { c | 0x80 });
            if done {
                break;
            }
        }
    }
    pub fn name(&mut self, s: &str) {
        self.u(s.len() as u64);
        self.b.extend_from_slice(s.as_bytes());
    }
    pub fn section(&mut self, id: u8, body: &WBuf) {
        self.byte(id);
        self.u(body.b.len() as u64);
        self.b.extend_from_slice(&body.b);
    }
}

// wasm opcodes this file writes
const UNREACHABLE: u8 = 0x00;
const BLOCK: u8 = 0x02;
const LOOP: u8 = 0x03;
const IF: u8 = 0x04;
const ELSE: u8 = 0x05;
const END: u8 = 0x0b;
const BR: u8 = 0x0c;
const BR_TABLE: u8 = 0x0e;
const RETURN: u8 = 0x0f;
const SELECT: u8 = 0x1b;
const LOCAL_GET: u8 = 0x20;
const LOCAL_SET: u8 = 0x21;
const LOCAL_TEE: u8 = 0x22;
const I32_LOAD: u8 = 0x28;
const I64_LOAD: u8 = 0x29;
const F64_LOAD: u8 = 0x2b;
const I32_LOAD8_U: u8 = 0x2d;
const I64_STORE: u8 = 0x37;
const F64_STORE: u8 = 0x39;
const I32_STORE8: u8 = 0x3a;
const I32_CONST: u8 = 0x41;
const F64_CONST: u8 = 0x44;
const I32_EQZ: u8 = 0x45;
const I32_EQ: u8 = 0x46;
const I32_NE: u8 = 0x47;
const I32_LT_S: u8 = 0x48;
const I32_LE_S: u8 = 0x4c;
const F64_EQ: u8 = 0x61;
const F64_NE: u8 = 0x62;
const F64_LT: u8 = 0x63;
const F64_GT: u8 = 0x64;
const F64_LE: u8 = 0x65;
const F64_GE: u8 = 0x66;
const I32_SUB: u8 = 0x6b;
const I32_AND: u8 = 0x71;
const I32_OR: u8 = 0x72;
const I32_XOR: u8 = 0x73;
const I32_SHL: u8 = 0x74;
const I32_SHR_S: u8 = 0x75;
const F64_NEG: u8 = 0x9a;
const F64_ADD: u8 = 0xa0;
const F64_SUB: u8 = 0xa1;
const F64_MUL: u8 = 0xa2;
const F64_DIV: u8 = 0xa3;
const I32_TRUNC_F64_S: u8 = 0xaa;
const F64_TRUNC: u8 = 0x9d;
const F64_CONVERT_I32_S: u8 = 0xb7;

const T_I32: u8 = 0x7f;
const T_I64: u8 = 0x7e;
const T_F64: u8 = 0x7c;
const T_VOID: u8 = 0x40;

// params and locals of the compiled function
const L_SP: u32 = 0;
const L_KV: u32 = 1;
const L_KEY: u32 = 2;
const L_BUDGET: u32 = 3;
const L_VM: u32 = 4;
const L_BP: u32 = 5;
const L_OUT: u32 = 6;
const L_TAG: u32 = 7;
const L_LOOPED: u32 = 8;
const L_A: u32 = 9; // f64
const L_B: u32 = 10; // f64
const L_Q0: u32 = 11; // i64 x4 (R_LOAD2)
const L_Q2: u32 = 13;
const NPARAMS: u32 = 6;

// the helpers the module imports (function indices; f comes after them)
const H_GET_ELEM: u32 = 0;
const H_SET_ELEM: u32 = 1;
const H_GET_PROP: u32 = 2;
const H_SET_PROP: u32 = 3;
const NHELPERS: u32 = 4;
const HELPERS: [&str; 4] = ["get_elem", "set_elem", "get_prop", "set_prop"];
const CALL: u8 = 0x10;
const I32_REM_S: u8 = 0x6f;

// --------------------------------------------------------------- compiler

/// What an operand register is at compile time.
#[derive(Clone, Copy)]
enum Opnd {
    /// a frame slot
    Reg(u32),
    /// a constant number
    Num(f64),
    /// another constant: kvals[i]
    K(u32),
}

struct Cg<'a> {
    w: WBuf,
    lay: Layout,
    kvals: &'a [Val],
    /// enclosing labels above the current block's code: depth to the exit
    /// block is `exit_d + extra`, to the dispatch loop one more
    exit_d: u32,
    extra: u32,
    /// back edges compiled (a function without one is not worth it)
    backs: u32,
    /// ops written as code rather than as an exit
    done: u32,
    /// exits by op kind
    exits: [u32; 64],
    /// the function's proto and where its code is in rcode
    pi: u32,
    rb: u32,
}

impl<'a> Cg<'a> {
    fn op(&mut self, x: u8) {
        self.w.byte(x);
    }
    fn i32c(&mut self, x: i64) {
        self.w.byte(I32_CONST);
        self.w.s(x);
    }
    fn get(&mut self, l: u32) {
        self.w.byte(LOCAL_GET);
        self.w.u(l as u64);
    }
    fn set(&mut self, l: u32) {
        self.w.byte(LOCAL_SET);
        self.w.u(l as u64);
    }
    fn mem(&mut self, op: u8, align: u32, off: u32) {
        self.w.byte(op);
        self.w.u(align as u64);
        self.w.u(off as u64);
    }
    fn opnd(&self, x: i32) -> Opnd {
        if x >= 0 {
            return Opnd::Reg(x as u32);
        }
        let k = (-1 - x) as usize;
        match self.kvals.get(k) {
            Some(Val::Num(n)) => Opnd::Num(*n),
            _ => Opnd::K(k as u32),
        }
    }
    /// base local and byte offset of an operand's value in memory
    fn place(&self, o: Opnd) -> (u32, u32) {
        match o {
            Opnd::Reg(r) => (L_SP, r * 16),
            Opnd::K(k) => (L_KV, k * 16),
            Opnd::Num(_) => (L_SP, 0),
        }
    }
    fn exit_depth(&self) -> u32 {
        self.exit_d + self.extra
    }
    /// return pc to the interpreter
    fn exit_at(&mut self, pc: i64) {
        self.i32c(pc);
        self.set(L_OUT);
        let d = self.exit_depth();
        self.op(BR);
        self.w.u(d as u64);
    }
    /// the condition on the wasm stack holds: return pc
    fn exit_if(&mut self, pc: i64) {
        self.op(IF);
        self.op(T_VOID);
        self.extra += 1;
        self.exit_at(pc);
        self.extra -= 1;
        self.op(END);
    }
    /// go on at block `to` (through the dispatch loop)
    fn jump(&mut self, to: i64, back: bool) {
        if back {
            self.backs += 1;
            // budget -= 1; when it is gone return -1 - to
            self.get(L_BUDGET);
            self.i32c(1);
            self.op(I32_SUB);
            self.w.byte(LOCAL_TEE);
            self.w.u(L_BUDGET as u64);
            self.i32c(0);
            self.op(I32_LE_S);
            self.exit_if(-1 - to);
            self.i32c(1 << 29);
            self.set(L_LOOPED);
        }
        self.i32c(to);
        self.set(L_KEY);
        let d = self.exit_depth() + 1;
        self.op(BR);
        self.w.u(d as u64);
    }
    /// the operand's tag on the wasm stack
    fn tag(&mut self, o: Opnd) {
        match o {
            Opnd::Num(_) => {
                let t = self.lay.t_num as i64;
                self.i32c(t);
            }
            _ => {
                let (b, off) = self.place(o);
                self.get(b);
                self.mem(I32_LOAD8_U, 0, off);
            }
        }
    }
    /// return pc unless the operand is a number
    fn need_num(&mut self, o: Opnd, pc: i64) {
        match o {
            Opnd::Num(_) => {}
            Opnd::K(_) => {
                // a constant that is not a number never is
                self.exit_at(pc);
            }
            Opnd::Reg(_) => {
                self.tag(o);
                let t = self.lay.t_num as i64;
                self.i32c(t);
                self.op(I32_NE);
                self.exit_if(pc);
            }
        }
    }
    /// the operand's number on the wasm stack (after need_num)
    fn num(&mut self, o: Opnd) {
        match o {
            Opnd::Num(n) => {
                self.op(F64_CONST);
                self.w.b.extend_from_slice(&n.to_le_bytes());
            }
            _ => {
                let (b, off) = self.place(o);
                self.get(b);
                let on = self.lay.off_num;
                self.mem(F64_LOAD, 3, off + on);
            }
        }
    }
    /// slot r = Num(local l)
    fn store_num(&mut self, r: u32, l: u32) {
        self.get(L_SP);
        let t = self.lay.t_num as i64;
        self.i32c(t);
        self.mem(I32_STORE8, 0, r * 16);
        self.get(L_SP);
        self.get(l);
        let on = self.lay.off_num;
        self.mem(F64_STORE, 3, r * 16 + on);
    }
    /// slot r = Bool(the i32 on the wasm stack); it goes through L_TAG
    fn store_bool(&mut self, r: u32) {
        self.set(L_TAG);
        self.get(L_SP);
        let t = self.lay.t_bool as i64;
        self.i32c(t);
        self.mem(I32_STORE8, 0, r * 16);
        self.get(L_SP);
        self.get(L_TAG);
        let ob = self.lay.off_bool;
        self.mem(I32_STORE8, 0, r * 16 + ob);
    }
    /// the 16 bytes of an operand as two i64 into locals q, q + 1
    fn load_val(&mut self, o: Opnd, q: u32) -> bool {
        match o {
            Opnd::Num(_) | Opnd::K(_) => {
                let (b, off) = match o {
                    Opnd::Num(_) => {
                        // a number constant: its kvals entry
                        return false;
                    }
                    _ => self.place(o),
                };
                self.get(b);
                self.mem(I64_LOAD, 3, off);
                self.set(q);
                self.get(b);
                self.mem(I64_LOAD, 3, off + 8);
                self.set(q + 1);
                true
            }
            Opnd::Reg(_) => {
                let (b, off) = self.place(o);
                self.get(b);
                self.mem(I64_LOAD, 3, off);
                self.set(q);
                self.get(b);
                self.mem(I64_LOAD, 3, off + 8);
                self.set(q + 1);
                true
            }
        }
    }
    fn store_val(&mut self, r: u32, q: u32) {
        self.get(L_SP);
        self.get(q);
        self.mem(I64_STORE, 3, r * 16);
        self.get(L_SP);
        self.get(q + 1);
        self.mem(I64_STORE, 3, r * 16 + 8);
    }
    /// the raw operand (a kvals index for constants, numbers included)
    fn raw(&self, x: i32) -> Opnd {
        if x >= 0 {
            Opnd::Reg(x as u32)
        } else {
            Opnd::K((-1 - x) as u32)
        }
    }
    /// truthy(operand) as an i32 on the wasm stack, or return pc for a
    /// string that is not a constant
    fn truthy(&mut self, x: i32, pc: i64) {
        if x < 0 {
            let k = (-1 - x) as usize;
            let t = match self.kvals.get(k) {
                Some(v) => crate::vm::truthy(v),
                None => false,
            };
            self.i32c(if t { 1 } else { 0 });
            return;
        }
        let o = Opnd::Reg(x as u32);
        let (b, off) = self.place(o);
        self.tag(o);
        self.set(L_TAG);
        let l = self.lay;
        // a string: return pc (its truth needs its length)
        self.get(L_TAG);
        self.i32c(l.t_str as i64);
        self.op(I32_EQ);
        self.exit_if(pc);
        // number: n != 0 && n == n
        self.get(b);
        self.mem(F64_LOAD, 3, off + l.off_num);
        self.set(L_A);
        self.get(L_A);
        self.op(F64_CONST);
        self.w.b.extend_from_slice(&0.0f64.to_le_bytes());
        self.op(F64_NE);
        self.get(L_A);
        self.get(L_A);
        self.op(F64_EQ);
        self.op(I32_AND);
        // bool: its byte; otherwise object
        self.get(b);
        self.mem(I32_LOAD8_U, 0, off + l.off_bool);
        self.get(L_TAG);
        self.i32c(l.t_obj as i64);
        self.op(I32_EQ);
        self.get(L_TAG);
        self.i32c(l.t_bool as i64);
        self.op(I32_EQ);
        self.op(SELECT);
        self.get(L_TAG);
        self.i32c(l.t_num as i64);
        self.op(I32_EQ);
        self.op(SELECT);
    }
    /// x <cmp> y for two numbers as an i32, or return pc
    fn cmp(&mut self, cmp: i32, x: i32, y: i32, pc: i64) -> bool {
        let f = match cmp as int {
            OP_LT => F64_LT,
            OP_GT => F64_GT,
            OP_LE => F64_LE,
            OP_GE => F64_GE,
            OP_SEQ | OP_EQ => F64_EQ,
            OP_SNE | OP_NE => F64_NE,
            _ => return false,
        };
        let a = self.opnd(x);
        let b = self.opnd(y);
        self.need_num(a, pc);
        self.need_num(b, pc);
        self.num(a);
        self.num(b);
        self.op(f);
        true
    }
    /// a binary number op into dst, or false when this file does not do it
    fn arith(&mut self, code: int, dst: i32, x: i32, y: i32, pc: i64) -> bool {
        let fop = match code {
            OP_ADD => F64_ADD,
            OP_SUB => F64_SUB,
            OP_MUL => F64_MUL,
            OP_DIV => F64_DIV,
            OP_BAND | OP_BOR | OP_BXOR | OP_SHL | OP_SHR => 0,
            _ => return false,
        };
        if dst < 0 {
            return false;
        }
        let a = self.opnd(x);
        let b = self.opnd(y);
        self.need_num(a, pc);
        self.need_num(b, pc);
        if fop != 0 {
            self.num(a);
            self.num(b);
            self.op(fop);
            self.set(L_A);
            self.store_num(dst as u32, L_A);
            return true;
        }
        // int32 operands only (the interpreter's fast path); others return
        self.num(a);
        self.set(L_A);
        self.num(b);
        self.set(L_B);
        for l in [L_A, L_B] {
            // !(v >= -2^31 && v <= 2^31 - 1)
            self.get(l);
            self.op(F64_CONST);
            self.w.b.extend_from_slice(&(-2147483648.0f64).to_le_bytes());
            self.op(F64_GE);
            self.get(l);
            self.op(F64_CONST);
            self.w.b.extend_from_slice(&2147483647.0f64.to_le_bytes());
            self.op(F64_LE);
            self.op(I32_AND);
            self.op(I32_EQZ);
            self.exit_if(pc);
        }
        self.get(L_A);
        self.op(I32_TRUNC_F64_S);
        self.get(L_B);
        self.op(I32_TRUNC_F64_S);
        self.op(match code {
            OP_BAND => I32_AND,
            OP_BOR => I32_OR,
            OP_BXOR => I32_XOR,
            OP_SHL => I32_SHL,
            _ => I32_SHR_S,
        });
        self.op(F64_CONVERT_I32_S);
        self.set(L_A);
        self.store_num(dst as u32, L_A);
        true
    }

    /// A helper for the op at pc: 0 it did it, 1 it did not (return pc),
    /// 2 it did and the interpreter goes on after it (return next).
    fn helper(&mut self, h: u32, pc: i64, next: i64) {
        self.get(L_VM);
        self.get(L_BP);
        let (pi, rb) = (self.pi as i64, self.rb as i64);
        self.i32c(pi);
        self.i32c(pc);
        self.i32c(rb + pc);
        self.op(CALL);
        self.w.u(h as u64);
        self.w.byte(LOCAL_TEE);
        self.w.u(L_TAG as u64);
        self.i32c(1);
        self.op(I32_EQ);
        self.exit_if(pc);
        self.get(L_TAG);
        self.i32c(2);
        self.op(I32_EQ);
        self.exit_if(next);
    }
    /// x % y for whole non-negative int32s (the interpreter's fast path)
    fn modulo(&mut self, dst: i32, x: i32, y: i32, pc: i64) -> bool {
        if dst < 0 {
            return false;
        }
        let a = self.opnd(x);
        let b = self.opnd(y);
        self.need_num(a, pc);
        self.need_num(b, pc);
        self.num(a);
        self.set(L_A);
        self.num(b);
        self.set(L_B);
        // !(a >= 0 && b > 0 && a <= 2^31-1 && b <= 2^31-1 && whole)
        self.get(L_A);
        self.op(F64_CONST);
        self.w.b.extend_from_slice(&0.0f64.to_le_bytes());
        self.op(F64_GE);
        self.get(L_B);
        self.op(F64_CONST);
        self.w.b.extend_from_slice(&0.0f64.to_le_bytes());
        self.op(F64_GT);
        self.op(I32_AND);
        for l in [L_A, L_B] {
            self.get(l);
            self.op(F64_CONST);
            self.w.b.extend_from_slice(&2147483647.0f64.to_le_bytes());
            self.op(F64_LE);
            self.op(I32_AND);
            self.get(l);
            self.op(F64_TRUNC);
            self.get(l);
            self.op(F64_EQ);
            self.op(I32_AND);
        }
        self.op(I32_EQZ);
        self.exit_if(pc);
        self.get(L_A);
        self.op(I32_TRUNC_F64_S);
        self.get(L_B);
        self.op(I32_TRUNC_F64_S);
        self.op(I32_REM_S);
        self.op(F64_CONVERT_I32_S);
        self.set(L_A);
        self.store_num(dst as u32, L_A);
        true
    }

    /// One op; false when it is written as an exit.
    fn one(&mut self, pc: usize, ins: Op) -> bool {
        let w0 = ins.code;
        let (w1, w2, w3) = (ins.a, ins.b, ins.c);
        let kind = (w0 & 255) as int;
        let sub = ((w0 >> 8) & 255) as int;
        let p = pc as i64;
        match kind {
            R_NOP => true,
            R_MOVE => {
                if w1 < 0 {
                    return false;
                }
                let o = self.raw(w2);
                self.load_val(o, L_Q0);
                self.store_val(w1 as u32, L_Q0);
                true
            }
            R_LOAD2 => {
                if w1 < 0 {
                    return false;
                }
                let a = self.raw(w2);
                let b = self.raw(w3);
                self.load_val(a, L_Q0);
                self.load_val(b, L_Q2);
                self.store_val(w1 as u32, L_Q0);
                self.store_val(w1 as u32 + 1, L_Q2);
                true
            }
            R_ADD => self.arith(OP_ADD, w1, w2, w3, p),
            R_SUB => self.arith(OP_SUB, w1, w2, w3, p),
            R_MUL => self.arith(OP_MUL, w1, w2, w3, p),
            R_ARITH => {
                if sub == OP_MOD {
                    self.modulo(w1, w2, w3, p)
                } else {
                    self.arith(sub, w1, w2, w3, p)
                }
            }
            R_GET_ELEM | R_SET_ELEM | R_GET_PROP | R_SET_PROP => {
                let next = p + 1 + ((w0 >> 16) as i64);
                let h = match kind {
                    R_GET_ELEM => H_GET_ELEM,
                    R_SET_ELEM => H_SET_ELEM,
                    R_GET_PROP => H_GET_PROP,
                    _ => H_SET_PROP,
                };
                self.helper(h, p, next);
                true
            }
            R_CMP => {
                if w1 < 0 {
                    return false;
                }
                if !self.cmp(sub as i32, w2, w3, p) {
                    return false;
                }
                self.store_bool(w1 as u32);
                true
            }
            R_CMP_JF | R_CMP_JT => {
                if !self.cmp(sub as i32, w1, w2, p) {
                    return false;
                }
                if kind == R_CMP_JF {
                    self.op(I32_EQZ);
                }
                self.op(IF);
                self.op(T_VOID);
                self.extra += 1;
                self.jump(w3 as i64, (w3 as i64) <= p);
                self.extra -= 1;
                self.op(END);
                true
            }
            R_LOOP => {
                let body = (w0 >> 16) as i64;
                if !self.cmp(sub as i32, w1, w2, p) {
                    return false;
                }
                self.op(IF);
                self.op(T_VOID);
                self.extra += 1;
                self.jump(body, body <= p);
                self.op(ELSE);
                self.jump(w3 as i64, (w3 as i64) <= p);
                self.extra -= 1;
                self.op(END);
                // both arms jump
                self.op(UNREACHABLE);
                true
            }
            R_JUMP => {
                self.jump(w1 as i64, (w1 as i64) <= p);
                true
            }
            R_JF | R_JT => {
                self.truthy(w1, p);
                if kind == R_JF {
                    self.op(I32_EQZ);
                }
                self.op(IF);
                self.op(T_VOID);
                self.extra += 1;
                self.jump(w2 as i64, (w2 as i64) <= p);
                self.extra -= 1;
                self.op(END);
                true
            }
            R_NOT => {
                if w1 < 0 {
                    return false;
                }
                self.truthy(w2, p);
                self.op(I32_EQZ);
                self.store_bool(w1 as u32);
                true
            }
            R_INC_LOCAL => {
                if w1 < 0 {
                    return false;
                }
                let o = Opnd::Reg(w1 as u32);
                self.need_num(o, p);
                self.num(o);
                self.op(F64_CONST);
                self.w.b.extend_from_slice(&(w2 as f64).to_le_bytes());
                self.op(F64_ADD);
                self.set(L_A);
                self.store_num(w1 as u32, L_A);
                true
            }
            R_POSTINC => {
                if w1 < 0 || w2 < 0 {
                    return false;
                }
                let o = Opnd::Reg(w2 as u32);
                self.need_num(o, p);
                self.num(o);
                self.set(L_B);
                self.get(L_B);
                self.op(F64_CONST);
                self.w.b.extend_from_slice(&(w3 as f64).to_le_bytes());
                self.op(F64_ADD);
                self.set(L_A);
                self.store_num(w2 as u32, L_A);
                self.store_num(w1 as u32, if sub == 1 { L_A } else { L_B });
                true
            }
            R_INC => {
                if w1 < 0 {
                    return false;
                }
                let o = self.opnd(w2);
                self.need_num(o, p);
                self.num(o);
                self.op(F64_CONST);
                self.w.b.extend_from_slice(&1.0f64.to_le_bytes());
                self.op(if sub == 1 { F64_SUB } else { F64_ADD });
                self.set(L_A);
                self.store_num(w1 as u32, L_A);
                true
            }
            R_TONUM => {
                if w1 < 0 {
                    return false;
                }
                let o = self.opnd(w2);
                self.need_num(o, p);
                self.num(o);
                if sub == 1 {
                    self.op(F64_NEG);
                }
                self.set(L_A);
                self.store_num(w1 as u32, L_A);
                true
            }
            R_ADD_LOCAL_POP => {
                if w1 < 0 {
                    return false;
                }
                let a = Opnd::Reg(w1 as u32);
                let b = self.opnd(w2);
                self.need_num(a, p);
                self.need_num(b, p);
                self.num(a);
                self.num(b);
                self.op(F64_ADD);
                self.set(L_A);
                self.store_num(w1 as u32, L_A);
                true
            }
            _ => false,
        }
    }
}

/// The jump targets of an op this file writes.
fn targets(ins: Op, pc: usize, out: &mut Vec<usize>) {
    let w0 = ins.code;
    let kind = (w0 & 255) as int;
    match kind {
        R_CMP_JF | R_CMP_JT => out.push(ins.c as usize),
        R_LOOP => {
            out.push((w0 >> 16) as usize);
            out.push(ins.c as usize);
        }
        R_JUMP => out.push(ins.a as usize),
        R_JF | R_JT => out.push(ins.b as usize),
        _ => {}
    }
    let _ = pc;
}

/// What compile made.
pub struct Compiled {
    pub bytes: Vec<u8>,
    /// ops written as code, and all ops
    pub done: u32,
    pub ops: u32,
    pub exits: [u32; 64],
}

/// The module for one function's register code `code` (rcode from its
/// rbase), or None when it has no back edge this file can write.
pub fn compile(code: &[Op], kvals: &[Val], lay: Layout, pi: u32, rb: u32) -> Option<Compiled> {
    if !lay.ok || code.is_empty() {
        return None;
    }
    let n = code.len();
    let mut starts = vec![false; n];
    starts[0] = true;
    let mut ts = Vec::new();
    for (i, op) in code.iter().enumerate() {
        targets(*op, i, &mut ts);
    }
    for t in ts {
        if t < n {
            starts[t] = true;
        }
    }
    let blocks: Vec<usize> = (0..n).filter(|i| starts[*i]).collect();
    let nb = blocks.len() as u32;
    let mut cg = Cg {
        w: WBuf::new(),
        lay,
        kvals,
        exit_d: 0,
        extra: 0,
        backs: 0,
        done: 0,
        exits: [0; 64],
        pi,
        rb,
    };
    // locals: out, tag, looped (i32); a, b (f64); q0..q3 (i64)
    cg.w.u(3);
    cg.w.u(3);
    cg.w.byte(T_I32);
    cg.w.u(2);
    cg.w.byte(T_F64);
    cg.w.u(4);
    cg.w.byte(T_I64);
    // loop $L / block $exit / block b_{nb-1} .. b_0
    cg.op(LOOP);
    cg.op(T_VOID);
    cg.op(BLOCK);
    cg.op(T_VOID);
    for _ in 0..nb {
        cg.op(BLOCK);
        cg.op(T_VOID);
    }
    cg.get(L_KEY);
    cg.set(L_OUT);
    cg.get(L_KEY);
    cg.op(BR_TABLE);
    cg.w.u(n as u64);
    let mut bi: u32 = 0;
    for i in 0..n {
        if starts[i] {
            cg.w.u(bi as u64);
            bi += 1;
        } else {
            cg.w.u(nb as u64);
        }
    }
    cg.w.u(nb as u64);
    for (k, &start) in blocks.iter().enumerate() {
        cg.op(END);
        cg.exit_d = nb - 1 - k as u32;
        let stop = if k + 1 < blocks.len() { blocks[k + 1] } else { n };
        let mut pc = start;
        let mut open = true;
        while pc < stop {
            let ins = code[pc];
            if cg.one(pc, ins) {
                cg.done += 1;
                let kind = (ins.code & 255) as int;
                if kind == R_JUMP || kind == R_LOOP {
                    // nothing after it runs in this block
                    open = false;
                    break;
                }
                pc += 1;
            } else {
                let kind = (ins.code & 255) as usize;
                if kind < 64 {
                    cg.exits[kind] += 1;
                }
                cg.exit_at(pc as i64);
                // what follows is unreachable here (wasm allows it after a
                // br); it is still written so the block's back edge counts
                pc += 1;
            }
        }
        if open && k + 1 == blocks.len() {
            // past the end: return the last pc
            cg.exit_at((n - 1) as i64);
        }
    }
    cg.op(END); // $exit
    // a pc that went on past a back edge carries 2^29 (enter() counts the
    // entries that did not); a budget exit is negative already
    cg.get(L_OUT);
    cg.i32c(0);
    cg.get(L_LOOPED);
    cg.get(L_OUT);
    cg.i32c(0);
    cg.op(I32_LT_S);
    cg.op(SELECT);
    cg.op(I32_OR);
    cg.op(RETURN);
    cg.op(END); // $L
    cg.op(UNREACHABLE);
    cg.op(END); // function
    if cg.backs == 0 {
        return None;
    }
    let done = cg.done;
    Some(Compiled { bytes: module(&cg.w.b), done, ops: n as u32, exits: cg.exits })
}

/// A module of one function `f` (sp, kv, pc, budget, vm, bp) -> i32 over
/// the imported memory env.memory and the helpers env.<name> (vm, bp, pi,
/// pc, rcode index) -> i32; its body `body` (locals and code).
pub fn module(body: &[u8]) -> Vec<u8> {
    let mut m = WBuf::new();
    m.b.extend_from_slice(b"\0asm");
    m.b.extend_from_slice(&[1, 0, 0, 0]);
    // types: 0 f, 1 a helper
    let mut s = WBuf::new();
    s.u(2);
    for np in [NPARAMS, 5] {
        s.byte(0x60);
        s.u(np as u64);
        for _ in 0..np {
            s.byte(T_I32);
        }
        s.u(1);
        s.byte(T_I32);
    }
    m.section(1, &s);
    let mut s = WBuf::new();
    s.u(1 + NHELPERS as u64);
    for h in HELPERS.iter() {
        s.name("env");
        s.name(h);
        s.byte(0x00);
        s.u(1);
    }
    s.name("env");
    s.name("memory");
    s.byte(0x02);
    s.byte(0x00);
    s.u(1);
    m.section(2, &s);
    let mut s = WBuf::new();
    s.u(1);
    s.u(0);
    m.section(3, &s);
    let mut s = WBuf::new();
    s.u(1);
    s.name("f");
    s.byte(0x00);
    s.u(NHELPERS as u64);
    m.section(7, &s);
    let mut s = WBuf::new();
    s.u(1);
    s.u(body.len() as u64);
    s.b.extend_from_slice(body);
    m.section(10, &s);
    m.b
}

// ------------------------------------------------------------------ host

/// The host's side: compile a module and give its `f` a place in this
/// module's function table.
#[cfg(all(target_arch = "wasm32", feature = "wjit"))]
mod host {
    #[link(wasm_import_module = "cerxes_jit")]
    extern "C" {
        /// compiles the module (ptr, len), puts its export `f` into the
        /// function table; the index, or -1
        fn install(ptr: *const u8, len: usize) -> i32;
    }

    pub fn put(bytes: &[u8]) -> i32 {
        // SAFETY: the host reads len bytes at ptr and returns
        unsafe { install(bytes.as_ptr(), bytes.len()) }
    }

    pub fn call(idx: i32, sp: usize, kv: usize, pc: i32, budget: i32, vm: usize, bp: i32) -> i32 {
        // SAFETY: idx is a table index install returned for a function of
        // this type; on wasm32 a function pointer is its table index
        let f: extern "C" fn(i32, i32, i32, i32, i32, i32) -> i32 = unsafe { std::mem::transmute(idx as usize) };
        f(sp as i32, kv as i32, pc, budget, vm as i32, bp)
    }
}

#[cfg(not(all(target_arch = "wasm32", feature = "wjit")))]
mod host {
    pub fn put(_bytes: &[u8]) -> i32 {
        -1
    }
    pub fn call(_idx: i32, _sp: usize, _kv: usize, pc: i32, _budget: i32, _vm: usize, _bp: i32) -> i32 {
        pc
    }
}

/// Whether this build can run compiled code.
pub fn available() -> bool {
    cfg!(all(target_arch = "wasm32", feature = "wjit")) && probe().ok
}

thread_local! {
    static LAYOUT: Layout = probe();
}

/// Counters for a report.
#[derive(Clone, Copy, Debug)]
pub struct Stats {
    pub compiled: u32,
    pub refused: u32,
    /// loop heads no longer entered (they left at once too often)
    pub dropped: u32,
    pub bytes: u64,
    pub entries: u64,
    pub ops_done: u64,
    pub ops_all: u64,
    /// by register op kind: how many compiled functions have it as an exit
    pub exits: [u32; 64],
}

impl Default for Stats {
    fn default() -> Stats {
        Stats { compiled: 0, refused: 0, dropped: 0, bytes: 0, entries: 0, ops_done: 0, ops_all: 0, exits: [0; 64] }
    }
}

thread_local! {
    pub static STATS: std::cell::Cell<Stats> = std::cell::Cell::new(Stats::default());
}

pub fn stats() -> Stats {
    STATS.with(|s| s.get())
}

/// Compiles the function pi (when it has none yet) and runs it from pc;
/// the pc to go on at, or -1 - pc when its budget ran out.
pub fn enter(vm: &mut crate::vm::Vm, pi: usize, bp: i64, pc: i64, budget: i32) -> i64 {
    if vm.jit_fn.len() <= pi {
        vm.jit_fn.resize(pi + 1, -1);
    }
    let mut f = vm.jit_fn[pi];
    if f == -1 {
        let p = &vm.protos[pi];
        let rb = p.rbase as usize;
        let n = p.code.len();
        let lay = LAYOUT.with(|l| *l);
        f = -2;
        if rb + n <= vm.rcode.len() {
            if let Some(c) = compile(&vm.rcode[rb..rb + n], &vm.kvals, lay, pi as u32, rb as u32) {
                let idx = host::put(&c.bytes);
                STATS.with(|s| {
                    let mut t = s.get();
                    if idx >= 0 {
                        t.compiled += 1;
                        t.bytes += c.bytes.len() as u64;
                        t.ops_done += c.done as u64;
                        t.ops_all += c.ops as u64;
                        for k in 0..64 {
                            t.exits[k] += c.exits[k];
                        }
                    } else {
                        t.refused += 1;
                    }
                    s.set(t);
                });
                if idx >= 0 {
                    f = idx as i64;
                }
            } else {
                STATS.with(|s| {
                    let mut t = s.get();
                    t.refused += 1;
                    s.set(t);
                });
            }
        }
        vm.jit_fn[pi] = f;
    }
    if f < 0 {
        return pc;
    }
    STATS.with(|s| {
        let mut t = s.get();
        t.entries += 1;
        s.set(t);
    });
    let sp = vm.stack.as_ptr() as usize + (bp as usize) * 16;
    let kv = vm.kvals.as_ptr() as usize;
    let vp = vm as *mut crate::vm::Vm as usize;
    let r = host::call(f as i32, sp, kv, pc as i32, budget, vp, bp as i32);
    if r < 0 {
        return r as i64;
    }
    // jit_miss counts, by loop head, the entries that went back to the
    // interpreter before a back edge: code that leaves at once on every
    // pass only costs, and after enough of those that head is not entered
    // again (another loop of the function still is)
    let ri = (vm.protos[pi].rbase + pc) as usize;
    if r & (1 << 29) != 0 {
        if vm.jit_miss[ri] > 0 {
            vm.jit_miss[ri] -= 1;
        }
    } else {
        vm.jit_miss[ri] += 4;
        if vm.jit_miss[ri] > crate::rvm::JIT_MISS_MAX {
            STATS.with(|s| {
                let mut t = s.get();
                t.dropped += 1;
                s.set(t);
            });
        }
    }
    (r & !(1 << 29)) as i64
}

// ---------------------------------------------------------------- helpers
//
// The ops the compiled code does not write itself but asks the VM for:
// each does what the interpreter's fast path does (rvm.rs) and nothing
// that can run JavaScript, push a frame or move Vm.stack. 0 done, 1 not
// done (the interpreter runs the op), 2 done but go back to the
// interpreter after it (a collection is due).

/// The VM a helper was called with.
///
/// SAFETY: vm is the address enter() passed, of the Vm whose rrun called
/// the compiled code; rrun does not touch it until the call returns.
#[inline(always)]
unsafe fn vm_of<'a>(vm: i32) -> &'a mut crate::vm::Vm {
    &mut *(vm as usize as *mut crate::vm::Vm)
}

fn after(vm: &crate::vm::Vm) -> i32 {
    if vm.gc_due() {
        2
    } else {
        0
    }
}

/// R_GET_ELEM: an array element by a whole number in range.
#[no_mangle]
pub extern "C" fn cerxes_jit_get_elem(vm: i32, bp: i32, _pi: i32, _pc: i32, ri: i32) -> i32 {
    let vm = unsafe { vm_of(vm) };
    let ins = vm.rcode[ri as usize];
    let bp = bp as int;
    let mut r = Val::Undef;
    let mut done = false;
    if let (Val::Obj(o), Val::Num(n)) = (vm.rget(bp, ins.b as int), vm.rget(bp, ins.c as int)) {
        let ob = &vm.objs[*o as usize];
        if ob.class == C_ARRAY {
            let i = *n as int;
            let ev = &vm.evecs[ob.elems as usize].v;
            if i >= 0 && (i as usize) < ev.len() && (i as double) == *n {
                r = ev[i as usize];
                done = true;
            }
        }
    }
    if !done {
        return 1;
    }
    vm.rset((bp + ins.a as int) as usize, r);
    0
}

/// R_SET_ELEM: an array element by a whole number in range, or one past
/// the end of an extensible array.
#[no_mangle]
pub extern "C" fn cerxes_jit_set_elem(vm: i32, bp: i32, _pi: i32, _pc: i32, ri: i32) -> i32 {
    let vm = unsafe { vm_of(vm) };
    let ins = vm.rcode[ri as usize];
    let bp = bp as int;
    let val = *vm.rget(bp, ins.c as int);
    let o = crate::vm::obj_of(vm.rget(bp, ins.a as int));
    let n = match vm.rget(bp, ins.b as int) {
        Val::Num(x) => *x,
        _ => return 1,
    };
    if o < 0 || vm.objs[o as usize].class != C_ARRAY || vm.objs[o as usize].pos == 2 {
        return 1;
    }
    let i = n as int;
    if i < 0 || (i as double) != n {
        return 1;
    }
    let e = vm.objs[o as usize].elems as usize;
    let len = vm.evecs[e].v.len();
    if (i as usize) < len {
        vm.inc(&val);
        let old = crate::vm::obj_of(&vm.evecs[e].v[i as usize]);
        let old_s = crate::vm::str_of(&vm.evecs[e].v[i as usize]);
        vm.evecs[e].v[i as usize] = val;
        let oc = vm.objs[o as usize].cold as usize;
        if oc > 0 && (i as usize) < vm.colds[oc].elems2.len() {
            vm.colds[oc].elems2[i as usize] = Val::Bool(false);
        }
        vm.dec_o(old);
        vm.sdec(old_s);
        return after(vm);
    }
    if (i as usize) == len && e != 0 && vm.objs[o as usize].extensible {
        vm.inc(&val);
        vm.evecs[e].v.push(val);
        return after(vm);
    }
    1
}

/// R_GET_PROP: an own data property its inline cache knows, or an array's
/// length.
#[no_mangle]
pub extern "C" fn cerxes_jit_get_prop(vm: i32, bp: i32, pi: i32, pc: i32, ri: i32) -> i32 {
    let vm = unsafe { vm_of(vm) };
    let ins = vm.rcode[ri as usize];
    let bp = bp as int;
    let o = crate::vm::obj_of(vm.rget(bp, ins.b as int));
    if o < 0 {
        return 1;
    }
    let c = ins.c;
    let ob = &vm.objs[o as usize];
    if c >= 65536 && (c >> 16) == ob.shape {
        let r = ob.vals[(c & 65535) as usize];
        vm.rset((bp + ins.a as int) as usize, r);
        return 0;
    }
    if ob.class == C_ARRAY && vm.ratom(pi as usize, pc as int) == crate::vm::A_LENGTH {
        let n = vm.evecs[ob.elems as usize].v.len() as double;
        vm.rset((bp + ins.a as int) as usize, Val::Num(n));
        return 0;
    }
    1
}

/// R_SET_PROP: an own data property its inline cache knows.
#[no_mangle]
pub extern "C" fn cerxes_jit_set_prop(vm: i32, bp: i32, _pi: i32, _pc: i32, ri: i32) -> i32 {
    let vm = unsafe { vm_of(vm) };
    let ins = vm.rcode[ri as usize];
    let bp = bp as int;
    let o = crate::vm::obj_of(vm.rget(bp, ins.a as int));
    let w3 = ins.c;
    if o >= 0 && w3 >= 65536 && (w3 >> 16) == vm.objs[o as usize].shape && ((ins.code >> 8) & 255) == 0 {
        let val = *vm.rget(bp, ins.b as int);
        vm.put_slot(o, (w3 & 65535) as int, val);
        return after(vm);
    }
    1
}
