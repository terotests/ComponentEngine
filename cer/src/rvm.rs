// SPDX-License-Identifier: AGPL-3.0-or-later
//! The register machine: runs the register code regs.rs makes, in place of
//! `Vm::run` when `Vm.regs` is set.
//!
//! A frame's slots stay on the stack from `bp` to `bp + fsize` while it
//! runs. An op without a register form runs its stack handler (`step`) on
//! the frame's window: the stack is cut at the window's top first and
//! padded back after. A call cuts the stack after its arguments, so the
//! callee's frame starts there, and its result lands in the slot of f.

use ranger::prelude::*;

use crate::ops::*;
use crate::regs::*;
use crate::value::*;
use crate::vm::*;

/// prototypes a chain cache entry follows, at most
pub const PIC_DEPTH: int = 4;
/// ints per prototype-chain cache entry (Vm.pic): the receiver's shape, the
/// shape epoch, the depth, the slot (-1: absent, the chain ends there), 1
/// for an accessor, then each prototype and its shape; a site's first
/// entry's last int is whose turn it is to be refilled
pub const PIC_W: int = 5 + 2 * PIC_DEPTH + 1;
/// entries per site, filled in turn
pub const PIC_WAYS: int = 4;

/// R_GET_METHOD's c at or below this: an mcache entry (MC_BASE - c)
pub const MC_BASE: int = -100000;

impl Vm {
    /// Register x of the frame at bp: a slot, or a constant when negative.
    #[inline(always)]
    pub fn rget(&self, bp: int, x: int) -> &Val {
        if x >= 0 {
            let i = (bp + x) as usize;
            // a register operand is below the frame's fsize and rrun keeps
            // the stack at least bp + fsize long (frame head, call_result,
            // r_win, r_call_slow pad it back)
            val_at(&self.stack, i)
        } else {
            let i = (-1 - x) as usize;
            // translate() makes constant operands from kvals indices
            val_at(&self.kvals, i)
        }
    }

    /// Object `o`, unchecked: callers pass ids read from live values.
    #[inline(always)]
    pub fn ob(&self, o: int) -> &JsObj {
        // object ids in values point at slots that stay allocated while
        // referenced (freed slots are only reused once unreferenced)
        obj_at(&self.objs, o as usize)
    }

    /// The scope `depth` levels out from frame fi's (fi is the running
    /// frame): Vm::get_env without looking the frame up again.
    #[inline(always)]
    pub fn env_of(&self, fi: usize, depth: int) -> int {
        let mut e = self.frames[fi].env;
        let mut d = depth;
        while d > 0 {
            e = self.ob(e).proto as int;
            d -= 1;
        }
        e
    }

    /// Vm::ic_slot for a global read (any data property), its lookups
    /// unchecked: the shape ids and key indexes come from the object and
    /// are checked against the shape's key count.
    #[inline(always)]
    pub fn global_ic(&self, g: int, atom: int, c: int) -> int {
        let sh = self.ob(g).shape as int;
        if c >= 65536 {
            return if (c >> 16) == sh { c & 65535 } else { -1 };
        }
        let sd = shape_at(&self.shapes, sh as usize);
        if c >= 0 && c < sd.nkeys {
            let s = shape_at(&self.shapes, sd.kref as usize);
            if *int_at(&s.keys, c as usize) == atom && (*int_at(&s.attrs, c as usize) & P_ACCESSOR) == 0 {
                return c;
            }
        }
        -1
    }

    /// Slot `k` of scope `e`, unchecked: the compiler resolved it against
    /// the scope the closure was made in.
    #[inline(always)]
    pub fn env_slot(&self, e: int, k: int) -> Val {
        let ev = self.ob(e).elems as usize;
        // a scope gets its slots when it is made (renter, PUSH_ENV) and
        // keeps them while a closure or frame refers to it
        *val_at(&evec_at(&self.evecs, ev).v, k as usize)
    }

    /// Slot `k` of scope `e` takes `v` (Vm::put_elem unchecked, as
    /// env_slot): counts change only when a value on either side is on
    /// the heap.
    #[inline(always)]
    pub fn env_put(&mut self, e: int, k: int, v: Val) {
        let ev = self.ob(e).elems as usize;
        let old = *val_at(&evec_at(&self.evecs, ev).v, k as usize);
        if is_heap(&v) || is_heap(&old) {
            self.put_elem(e, k, v);
            return;
        }
        elem_put_in(evec_mut(&mut self.evecs, ev), k as usize, v);
    }

    /// Register slot `i` (absolute) takes `v`, unchecked like rget.
    #[inline(always)]
    pub fn rset(&mut self, i: usize, v: Val) {
        // as rget
        val_put(&mut self.stack, i, v);
    }

    /// The arithmetic op at pc of frame fi took its slow path: the JIT
    /// stops speculating that it sees numbers there (wjit.rs Cg.spec).
    #[inline(never)]
    pub fn note_nonnum(&mut self, fi: usize, pc: int) {
        let pi = self.frames[fi].proto as usize;
        let ri = self.protos[pi].rbase + pc;
        jit_note_nonnum(self, pi, ri);
    }

    /// A back edge to pc with the JIT on: once the function is hot its
    /// compiled code (wjit.rs) runs from pc; the pc the interpreter goes on
    /// at, or -1 when the time slice ended there (the frame is stopped).
    pub fn jit_run(&mut self, pi: usize, rb: int, bp: int, fi: usize, pc: int) -> int {
        let ri = (rb + pc) as usize;
        while self.jit_miss.len() <= ri {
            self.jit_miss.push(0);
        }
        if self.jit_miss[ri] > JIT_MISS_MAX {
            return pc;
        }
        while self.jit_hot.len() <= pi {
            self.jit_hot.push(0);
        }
        if self.jit_hot[pi] < self.jit_min {
            self.jit_hot[pi] += 1;
            return pc;
        }
        self.jit_go(pi, bp, fi, pc, true)
    }

    /// Runs the compiled code of function pi from pc: the pc to go on at,
    /// -1 the time slice ended (the frame is stopped), -2 an op threw (the
    /// frame's pc is set), -3 frames changed (go on with the top one).
    pub fn jit_go(&mut self, pi: usize, bp: int, fi: usize, pc: int, head: bool) -> int {
        let budget = if self.slicing { SLICE_TICKS } else { 1073741824 };
        let r = jit_enter(self, pi, bp, pc, budget, head);
        if r >= 0 || r == -2 || r == -3 {
            return r;
        }
        // the budget ran out: read the clock at this back edge
        let at = -4 - r;
        if self.slicing {
            self.ticks = 1;
            if self.brk(fi, at) {
                return -1;
            }
        }
        at
    }

    /// The atom of the property op at pc (its stack op's operand).
    pub fn ratom(&self, pi: usize, pc: int) -> int {
        let op = self.protos[pi].code[pc as usize];
        if (op.code as int) == OP_GET_LOCAL_PROP {
            (op.b as int)
        } else {
            (op.a as int)
        }
    }

    /// Stack ops the register code runs on the window that `step` does
    /// not do (run() does them); true when the frame changed.
    pub fn win_op(&mut self, op: Op, pi: usize, bp: int, fi: usize, pc: &mut int) -> bool {
        match (op.code as int) {
            OP_SET_PROP => {
                let val = self.pop();
                let target = self.pop();
                self.set(&target, (op.a as int), val.clone());
                self.stack.push(val);
            }
            OP_SET_ELEM => {
                let val = self.pop();
                let k = self.pop();
                let target = self.pop();
                self.set_elem(&target, &k, val.clone());
                self.stack.push(val);
            }
            OP_GET_METHOD_ELEM => {
                let k = self.pop();
                let v = self.pop();
                let f = self.get_elem(&v, &k);
                self.stack.push(f);
                self.stack.push(v);
            }
            OP_ADD_ENV_POP => {
                self.add_env_pop(op);
            }
            OP_ADD_PROP_POP => {
                self.add_prop_pop(op);
            }
            _ => {
                return self.step(op, pi, bp, fi, pc);
            }
        }
        false
    }

    /// `x += v` on a closure variable (R_ADD_ENV): w1 the old value, w2
    /// what is added, w3 the env index, the depth in w0's sub byte.
    #[inline(never)]
    pub fn r_add_env(&mut self, bp: int, fi: usize, pc: int, w0: int, w1: int, w2: int, w3: int) {
        let depth = (w0 >> 8) & 255;
        if let (Val::Num(x), Val::Num(y)) = (self.rget(bp, w1), self.rget(bp, w2)) {
            let r = *x + *y;
            let e = self.get_env(depth);
            self.put_elem(e, w3, Val::Num(r));
            return;
        }
        self.frames[fi].pc = pc + 1;
        let a = self.rget(bp, w1).clone();
        let b = self.rget(bp, w2).clone();
        self.add_env(depth, w3, a, b);
    }

    /// `o[k]` as a method (R_GET_METHOD_ELEM): f to dst, o to dst + 1.
    #[inline(never)]
    pub fn r_get_method_elem(&mut self, bp: int, fi: usize, pc: int, w1: int, w2: int, w3: int) {
        self.frames[fi].pc = pc + 1;
        let v = self.rget(bp, w2).clone();
        let k = self.rget(bp, w3).clone();
        let mut f = Val::Undef;
        let mut done = false;
        if let (Val::Obj(o), Val::Num(n)) = (&v, &k) {
            let ob = &self.objs[*o as usize];
            if ob.class == C_ARRAY {
                let i = *n as int;
                if i >= 0 && (i as usize) < self.evecs[ob.elems as usize].v.len() && (i as double) == *n {
                    f = self.evecs[ob.elems as usize].v[i as usize].clone();
                    done = true;
                }
            }
        }
        if !done {
            f = self.get_elem(&v, &k);
        }
        if !self.throwing {
            self.stack[(bp + w1) as usize] = f;
            self.stack[(bp + w1 + 1) as usize] = v;
        }
    }

    /// R_TONUM / R_INC on a value that is not a number: kind 0 ToNumeric,
    /// 1 minus, 2 + 1, 3 - 1, 4 ToNumber (unary +).
    #[inline(never)]
    pub fn r_unary_slow(&mut self, bp: int, fi: usize, pc: int, kind: int, w1: int, w2: int) {
        self.frames[fi].pc = pc + 1;
        let v = self.rget(bp, w2).clone();
        let r = if kind == 0 || kind == 4 {
            if kind == 4 && self.class_of(&v) == C_BIGINT {
                self.throw_type("Cannot convert a BigInt value to a number");
                return;
            }
            self.to_numeric(v)
        } else if kind == 1 {
            self.neg_val(v)
        } else {
            self.inc_val(v, if kind == 2 { 1.0 } else { -1.0 })
        };
        if !self.throwing {
            self.stack[(bp + w1) as usize] = r;
        }
    }

    /// R_CHECK's error: a let/const read too early (kind 0, `name` its
    /// constant) or destructuring null / undefined.
    #[inline(never)]
    pub fn r_check_fail(&mut self, pi: usize, kind: int, name: int) {
        if kind == 0 {
            let n = self.protos[pi].consts[name as usize].clone();
            let ns = self.to_string(&n);
            self.throw_ref(format!("Cannot access '{}' before initialization", ns).as_str());
        } else {
            self.throw_type("Cannot destructure 'undefined' or 'null'");
        }
    }

    /// Before suspended frames go on (Engine::resume, maybe in an engine
    /// restored from a snapshot, which keeps no register code): every
    /// running frame's function in register code, as a return goes
    /// straight into its caller's.
    pub fn translate_frames(&mut self) -> bool {
        let mut i = 0;
        while i < self.frames.len() {
            let pi = self.frames[i].proto as usize;
            if self.protos[pi].rstate == 0 {
                unpack_code(&mut self.protos[pi]);
                let e = translate(&mut self.protos[pi], &mut self.rcode, &mut self.kvals, &mut self.rtab);
                if !e.is_empty() {
                    self.throw_type(e.as_str());
                    return false;
                }
            }
            i += 1;
        }
        true
    }

    /// Runs frames until the frame count falls back to `base`.
    pub fn rrun(&mut self, base: int) {
        'frames: loop {
            if self.throwing {
                if !self.unwind(base) {
                    return;
                }
            }
            if (self.frames.len() as int) <= base {
                return;
            }
            if self.slicing && self.frame_brk() {
                return;
            }
            let mut fi = self.frames.len() - 1;
            let mut pi = self.frames[fi].proto as usize;
            let mut bp = self.frames[fi].bp;
            let mut pc = self.frames[fi].pc;
            if self.protos[pi].rstate == 0 {
                unpack_code(&mut self.protos[pi]);
                let e = translate(&mut self.protos[pi], &mut self.rcode, &mut self.kvals, &mut self.rtab);
                if !e.is_empty() {
                    self.throw_type(e.as_str());
                    continue 'frames;
                }
            }
            let mut want = (bp + self.protos[pi].fsize) as usize;
            let mut rb = self.protos[pi].rbase;
            while self.stack.len() < want {
                self.stack.push(Val::Undef);
            }
            if self.frames[fi].mv_to >= 0 {
                let v = self.stack[self.frames[fi].mv_from as usize].clone();
                self.stack[self.frames[fi].mv_to as usize] = v;
                self.frames[fi].mv_to = -1;
            }
            loop {
                let ins = {
                    // pc stays inside the function's translated code
                    op_at(&self.rcode, (rb + pc) as usize)
                };
                let w0 = ins.code;
                // the operands widened once here: the arms index with them
                let w1 = ins.a as int;
                let w2 = ins.b as int;
                let w3 = ins.c as int;
                let mut npc: int = -1;
                match ((w0 & 255) as int) {
                    R_NOP => {
                        pc = (pc + 1 + ((w0 >> 16) as int));
                        continue;
                    }
                    R_MOVE => {
                        let v = self.rget(bp, (w2 as int)).clone();
                        self.rset((bp + (w1 as int)) as usize, v);
                        pc = (pc + 1 + ((w0 >> 16) as int));
                        continue;
                    }
                    R_LOAD2 => {
                        let v = self.rget(bp, (w2 as int)).clone();
                        let u = self.rget(bp, (w3 as int)).clone();
                        self.rset((bp + (w1 as int)) as usize, v);
                        self.rset((bp + (w1 as int) + 1) as usize, u);
                        pc = (pc + 1 + ((w0 >> 16) as int));
                        continue;
                    }
                    R_ADD => {
                        let mut r = Val::Undef;
                        let mut done = false;
                        if let (Val::Num(x), Val::Num(y)) = (self.rget(bp, (w2 as int)), self.rget(bp, (w3 as int))) {
                            r = Val::Num(*x + *y);
                            done = true;
                        }
                        if done {
                            self.rset((bp + (w1 as int)) as usize, r);
                            pc = (pc + 1 + ((w0 >> 16) as int));
                            continue;
                        }
                        self.r_add_slow(bp, fi, pc, (w1 as int), (w2 as int), (w3 as int));
                    }
                    R_SUB | R_MUL | R_ARITH => {
                        let code = if ((w0 & 255) as int) == R_SUB {
                            OP_SUB
                        } else if ((w0 & 255) as int) == R_MUL {
                            OP_MUL
                        } else {
                            (((w0 >> 8) & 255) as int)
                        };
                        let mut r = Val::Undef;
                        let mut done = false;
                        if let (Val::Num(x), Val::Num(y)) = (self.rget(bp, (w2 as int)), self.rget(bp, (w3 as int))) {
                            let (a, b) = (*x, *y);
                            if code == OP_SUB {
                                r = Val::Num(a - b);
                            } else if code == OP_MUL {
                                r = Val::Num(a * b);
                            } else if code == OP_MOD && a >= 0.0 && b > 0.0 && a <= 2147483647.0 && b <= 2147483647.0 && a == ((a as int) as double) && b == ((b as int) as double) {
                                // whole non-negative int32s: an int remainder
                                let p = a as int;
                                let q = b as int;
                                r = Val::Num((p % q) as double);
                            // bit operations on numbers in int32 range: ToInt32 is
                            // the truncation
                            } else if code >= OP_BAND && code <= OP_SHR && a >= -2147483648.0 && a <= 2147483647.0 && b >= -2147483648.0 && b <= 2147483647.0 {
                                let (p, q) = (trunc_i32(a), trunc_i32(b));
                                r = Val::Num(match code {
                                    OP_BAND => (p & q) as double,
                                    OP_BOR => (p | q) as double,
                                    OP_BXOR => (p ^ q) as double,
                                    OP_SHL => wrap32(p << (q & 31)) as double,
                                    _ => (p >> (q & 31)) as double,
                                });
                            } else if code >= OP_BAND && code <= OP_USHR {
                                r = Val::Num(bit_num(code, a, b));
                            } else {
                                r = Val::Num(arith_num(code, a, b));
                            }
                            done = true;
                        }
                        if done {
                            self.rset((bp + (w1 as int)) as usize, r);
                            pc = (pc + 1 + ((w0 >> 16) as int));
                            continue;
                        }
                        self.r_arith_slow(code, bp, fi, pc, (w1 as int), (w2 as int), (w3 as int));
                    }
                    R_CMP => {
                        let code = (w0 >> 8) & 255;
                        let r = self.rcmp((code as int), pi, bp, (w2 as int), (w3 as int), fi, pc);
                        if !self.throwing {
                            self.rset((bp + (w1 as int)) as usize, Val::Bool(r));
                        }
                    }
                    R_CMP_JF | R_CMP_JT => {
                        let code = (w0 >> 8) & 255;
                        let mut num = false;
                        let mut r = false;
                        if let (Val::Num(a), Val::Num(b)) = (self.rget(bp, (w1 as int)), self.rget(bp, (w2 as int))) {
                            num = true;
                            r = match (code as int) {
                                OP_LT => *a < *b,
                                OP_GT => *a > *b,
                                OP_LE => *a <= *b,
                                OP_GE => *a >= *b,
                                OP_SEQ | OP_EQ => *a == *b,
                                _ => *a != *b,
                            };
                        }
                        if !num {
                            r = self.rcmp((code as int), pi, bp, (w1 as int), (w2 as int), fi, pc);
                        }
                        if r == (((w0 & 255) as int) == R_CMP_JT) && !self.throwing {
                            if (w3 as int) <= pc {
                                self.frames[fi].pc = (w3 as int);
                                if self.gc_due() {
                                    self.rgc(want);
                                }
                            }
                            npc = (w3 as int);
                            if self.slicing && self.brk(fi, npc) {
                                return;
                            }
                        }
                    }
                    R_LOOP => {
                        let code = (w0 >> 8) & 255;
                        let mut num = false;
                        let mut r = false;
                        if let (Val::Num(a), Val::Num(b)) = (self.rget(bp, (w1 as int)), self.rget(bp, (w2 as int))) {
                            num = true;
                            r = match (code as int) {
                                OP_LT => *a < *b,
                                OP_GT => *a > *b,
                                OP_LE => *a <= *b,
                                OP_GE => *a >= *b,
                                OP_SEQ | OP_EQ => *a == *b,
                                _ => *a != *b,
                            };
                        }
                        if !num {
                            r = self.rcmp((code as int), pi, bp, (w1 as int), (w2 as int), fi, pc);
                            if self.throwing {
                                self.frames[fi].pc = pc + 1;
                                if !self.unwind(base) {
                                    return;
                                }
                                continue 'frames;
                            }
                        }
                        let t = if r { (w0 >> 16) as int } else { w3 };
                        if (t as int) <= pc && self.gc_due() {
                            self.frames[fi].pc = (t as int);
                            self.rgc(want);
                        }
                        let back = (t as int) <= pc;
                        pc = (t as int);
                        if self.slicing && self.brk(fi, pc) {
                            return;
                        }
                        if back && self.jit_on {
                            pc = self.jit_run(pi, rb, bp, fi, pc);
                            if pc < 0 {
                                if pc == -1 || (pc == -2 && !self.unwind(base)) {
                                    return;
                                }
                                continue 'frames;
                            }
                        }
                        continue;
                    }
                    R_JUMP => {
                        let back = (w1 as int) <= pc;
                        if back && self.gc_due() {
                            self.frames[fi].pc = (w1 as int);
                            self.rgc(want);
                        }
                        pc = (w1 as int);
                        if self.slicing && self.brk(fi, pc) {
                            return;
                        }
                        if back && self.jit_on {
                            pc = self.jit_run(pi, rb, bp, fi, pc);
                            if pc < 0 {
                                if pc == -1 || (pc == -2 && !self.unwind(base)) {
                                    return;
                                }
                                continue 'frames;
                            }
                        }
                        continue;
                    }
                    R_JNU => {
                        if matches!(self.rget(bp, (w1 as int)), Val::Undef) {
                            pc = (pc + 1 + ((w0 >> 16) as int));
                        } else {
                            pc = (w2 as int);
                            if self.slicing && self.brk(fi, pc) {
                                return;
                            }
                        }
                        continue;
                    }
                    R_JF => {
                        if self.truthy_v(self.rget(bp, (w1 as int))) {
                            pc = (pc + 1 + ((w0 >> 16) as int));
                        } else {
                            pc = (w2 as int);
                            if self.slicing && self.brk(fi, pc) {
                                return;
                            }
                        }
                        continue;
                    }
                    R_JT => {
                        if self.truthy_v(self.rget(bp, (w1 as int))) {
                            pc = (w2 as int);
                            if self.slicing && self.brk(fi, pc) {
                                return;
                            }
                        } else {
                            pc = (pc + 1 + ((w0 >> 16) as int));
                        }
                        continue;
                    }
                    R_CMP_JFK | R_CMP_JTK => {
                        // the value is written either way: it may be a local
                        // (var b = x === y) that is read after the jump
                        let code = (w0 >> 8) & 255;
                        let mut num = false;
                        let mut r = false;
                        if let (Val::Num(a), Val::Num(b)) = (self.rget(bp, (w2 as int)), self.rget(bp, (w3 as int))) {
                            num = true;
                            r = match (code as int) {
                                OP_LT => *a < *b,
                                OP_GT => *a > *b,
                                OP_LE => *a <= *b,
                                OP_GE => *a >= *b,
                                OP_SEQ | OP_EQ => *a == *b,
                                _ => *a != *b,
                            };
                        }
                        if !num {
                            r = self.rcmp((code as int), pi, bp, (w2 as int), (w3 as int), fi, pc);
                        }
                        if !self.throwing {
                            self.stack[(bp + ((w1 & 65535) as int)) as usize] = Val::Bool(r);
                            if r == (((w0 & 255) as int) == R_CMP_JTK) {
                                npc = ((w1 >> 16 as int) as int);
                                if self.slicing && self.brk(fi, npc) {
                                    return;
                                }
                            }
                        }
                    }
                    R_JF_KEEP => {
                        if self.truthy_v(self.rget(bp, (w1 as int))) {
                            pc = (pc + 1 + ((w0 >> 16) as int));
                        } else {
                            pc = (w2 as int);
                            if self.slicing && self.brk(fi, pc) {
                                return;
                            }
                        }
                        continue;
                    }
                    R_JT_KEEP => {
                        if self.truthy_v(self.rget(bp, (w1 as int))) {
                            pc = (w2 as int);
                            if self.slicing && self.brk(fi, pc) {
                                return;
                            }
                        } else {
                            pc = (pc + 1 + ((w0 >> 16) as int));
                        }
                        continue;
                    }
                    R_JNN_KEEP => {
                        let nullish = match self.rget(bp, (w1 as int)) {
                            Val::Undef | Val::Null => true,
                            _ => false,
                        };
                        if (w0 & 65280) != 0 {
                            // sub 1, `x?.`: the jump when nullish (r_jnull)
                            let to = self.r_jnull(bp, (w1 as int), (w2 as int));
                            pc = if to >= 0 { to } else { pc + 1 + ((w0 >> 16) as int) };
                            continue;
                        }
                        if nullish {
                            pc = (pc + 1 + ((w0 >> 16) as int));
                        } else {
                            pc = (w2 as int);
                            if self.slicing && self.brk(fi, pc) {
                                return;
                            }
                        }
                        continue;
                    }
                    R_LOCAL_NULL_JF => {
                        let cmp = w2;
                        let r = match self.stack[(bp + (w1 as int)) as usize] {
                            Val::Null => (cmp as int) == OP_SEQ || (cmp as int) == OP_EQ,
                            Val::Undef => (cmp as int) == OP_SNE || (cmp as int) == OP_EQ,
                            _ => (cmp as int) == OP_SNE || (cmp as int) == OP_NE,
                        };
                        if !r {
                            if (w3 as int) <= pc {
                                self.frames[fi].pc = (w3 as int);
                                if self.gc_due() {
                                    self.rgc(want);
                                }
                            }
                            pc = (w3 as int);
                            if self.slicing && self.brk(fi, pc) {
                                return;
                            }
                        } else {
                            pc = (pc + 1 + ((w0 >> 16) as int));
                        }
                        continue;
                    }
                    R_CASE_INT_JT => {
                        let mut hit = false;
                        if let Val::Num(x) = self.rget(bp, (w1 as int)) {
                            hit = *x == (w2 as double);
                        }
                        if hit {
                            pc = (w3 as int);
                            if self.slicing && self.brk(fi, pc) {
                                return;
                            }
                        } else {
                            pc = (pc + 1 + ((w0 >> 16) as int));
                        }
                        continue;
                    }
                    R_SWITCH => {
                        let mut t = -1;
                        if let Val::Num(x) = self.rget(bp, (w1 as int)) {
                            let k = *x - (w3 as double);
                            let size = self.rtab[w2 as usize];
                            if k >= 0.0 && k < (size as double) && k == ((k as int) as double) {
                                t = self.rtab[(w2 + 1 + (k as int)) as usize];
                            }
                        }
                        if t < 0 {
                            t = self.rtab[(w2 + 1 + (self.rtab[w2 as usize] as int)) as usize];
                        }
                        pc = t;
                        if self.slicing && self.brk(fi, pc) {
                            return;
                        }
                        continue;
                    }
                    R_CASE_JT => {
                        let r = self.rcmp(OP_SEQ, pi, bp, (w1 as int), (w2 as int), fi, pc);
                        if r && !self.throwing {
                            npc = (w3 as int);
                            if self.slicing && self.brk(fi, npc) {
                                return;
                            }
                        }
                    }
                    R_GET_PROP | R_GET_THIS_PROP => {
                        // w3 is the inline cache; the atom is the stack op's
                        let c = w3;
                        let mut o: int = -1;
                        if ((w0 & 255) as int) == R_GET_PROP {
                            if let Val::Obj(x) = self.rget(bp, (w2 as int)) {
                                o = *x;
                            }
                        } else if let Val::Obj(x) = &self.frames[fi].this_val {
                            o = *x;
                        }
                        if o >= 0 && c >= 65536 && ((c >> 16) as int) == self.ob(o).shape as int {
                            // the cache's shape: the slot without a call
                            // the shape matched, so its slot exists
                            let r = slot_at(self.ob(o), (c & 65535) as usize);
                            self.rset((bp + (w1 as int)) as usize, r);
                            pc = (pc + 1 + ((w0 >> 16) as int));
                            continue;
                        }
                        if o >= 0 && c >= 65536 && ((rb + pc) as usize) < self.ic2.len() {
                            // the site's second shape (ic2): the two swap
                            let site = (rb + pc) as usize;
                            let c2 = self.ic2[site];
                            if c2 >= 65536 && ((c2 >> 16) as int) == self.ob(o).shape as int {
                                self.rcode[site].c = c2;
                                self.ic2[site] = c as i32;
                                let r = slot_at(self.ob(o), (c2 & 65535) as usize);
                                self.rset((bp + (w1 as int)) as usize, r);
                                pc = (pc + 1 + ((w0 >> 16) as int));
                                continue;
                            }
                        }
                        if c == -4 && o >= 0 && (self.ob(o).class == C_ARRAY || self.ob(o).class == C_ARGUMENTS) {
                            // a site r_get_prop_slow saw reading an array's length (or
                            // an unmodified arguments object's: a written or deleted
                            // length makes it C_SARGS)
                            let n = evec_at(&self.evecs, self.ob(o).elems as usize).v.len() as double;
                            self.rset((bp + (w1 as int)) as usize, Val::Num(n));
                            pc = (pc + 1 + ((w0 >> 16) as int));
                            continue;
                        }
                        if self.r_get_prop_slow(o, pi, rb, bp, fi, pc, (w0 as int), (w1 as int), (w2 as int)) {
                            continue 'frames;
                        }
                    }
                    R_SET_PROP => {
                        let o = obj_of(self.rget(bp, (w1 as int)));
                        if o >= 0 && w3 >= 65536 && ((w3 >> 16) as int) == self.ob(o).shape as int && ((w0 >> 8) & 255) == 0 {
                            let val = self.rget(bp, (w2 as int)).clone();
                            self.put_slot(o, ((w3 & 65535) as int), val);
                            pc = (pc + 1 + ((w0 >> 16) as int));
                            continue;
                        }
                        if o >= 0 && w3 >= 65536 && ((w0 >> 8) & 255) == 0 && ((rb + pc) as usize) < self.ic2.len() {
                            let site = (rb + pc) as usize;
                            let c2 = self.ic2[site];
                            if c2 >= 65536 && ((c2 >> 16) as int) == self.ob(o).shape as int {
                                self.rcode[site].c = c2;
                                self.ic2[site] = w3 as i32;
                                let val = self.rget(bp, (w2 as int)).clone();
                                self.put_slot(o, ((c2 & 65535) as int), val);
                                pc = (pc + 1 + ((w0 >> 16) as int));
                                continue;
                            }
                        }
                        self.r_set_prop_slow(o, pi, rb, bp, fi, pc, (w0 as int), (w1 as int), (w2 as int), (w3 as int));
                    }
                    R_GET_ELEM => {
                        let mut r = Val::Undef;
                        let mut done = false;
                        if let (Val::Obj(o), Val::Num(n)) = (self.rget(bp, (w2 as int)), self.rget(bp, (w3 as int))) {
                            let ob = obj_at(&self.objs, *o as usize);
                            if ob.class == C_ARRAY {
                                let i = *n as int;
                                let ev = ob.elems as usize;
                                if i >= 0 && (i as usize) < evec_at(&self.evecs, ev).v.len() && (i as double) == *n && ob.pos != 3 && !self.idx_protos {
                                    r = val_at(&evec_at(&self.evecs, ev).v, i as usize).clone();
                                    done = true;
                                }
                            } else if ob.class == C_TYPED {
                                // a typed array's element (ta_slot: no detached, resizable or converted buffer)
                                let i = *n as int;
                                if (i as double) == *n {
                                    let s = self.ta_slot(*o, i);
                                    if s >= 0 {
                                        r = Val::Num(cold_at(&self.colds, obj_at(&self.objs, ob.env as usize).cold as usize).nums[s as usize]);
                                        done = true;
                                    }
                                }
                            }
                        }
                        if done {
                            self.rset((bp + (w1 as int)) as usize, r);
                            pc = (pc + 1 + ((w0 >> 16) as int));
                            continue;
                        }
                        let v = self.rget(bp, (w2 as int)).clone();
                        let kv = self.rget(bp, (w3 as int)).clone();
                        self.frames[fi].pc = pc + 1;
                        if let (Val::Obj(po), Val::Str(kh)) = (v, kv) {
                            if self.objs[po as usize].class == C_PROXY && self.strs[kh as usize].atom >= 0 {
                                let a = self.strs[kh as usize].atom as int;
                                let fpos = bp + self.protos[pi].fsize;
                                if self.r_proxy_get(po, a, v, fpos, bp + (w1 as int)) {
                                    continue 'frames;
                                }
                            }
                        }
                        let r = self.get_elem(&v, &kv);
                        if !self.throwing {
                            self.rset((bp + (w1 as int)) as usize, r);
                        }
                    }
                    R_SET_ELEM => {
                        let val = self.rget(bp, (w3 as int)).clone();
                        let o = obj_of(self.rget(bp, (w1 as int)));
                        let mut n: double = 0.0;
                        let mut num = false;
                        if let Val::Num(x) = self.rget(bp, (w2 as int)) {
                            n = *x;
                            num = true;
                        }
                        let mut done = false;
                        if o >= 0 && num && self.objs[o as usize].class == C_TYPED {
                            if let Val::Num(x) = val {
                                let i = n as int;
                                if (i as double) == n && self.ta_put_num(o, i, x) {
                                    done = true;
                                }
                            }
                        }
                        if !done && o >= 0 && num {
                            self.inc(&val);
                            let ob = &mut self.objs[o as usize];
                            let mut old: int = -1;
                            let mut old_s: int = 0;
                            if ob.class == C_ARRAY {
                                let i = n as int;
                                let ev = ob.elems as usize;
                                if i >= 0 && (i as usize) < evec_at(&self.evecs, ev).v.len() && (i as double) == n && ob.pos < 2 {
                                    old = obj_of(val_at(&evec_at(&self.evecs, ev).v, i as usize));
                                    old_s = str_of(val_at(&evec_at(&self.evecs, ev).v, i as usize));
                                    elem_put_in(evec_mut(&mut self.evecs, ev), i as usize, val.clone());
                                    let oc = ob.cold as usize;
                                    if oc > 0 && (i as usize) < self.colds[oc].elems2.len() {
                                        self.colds[oc].elems2[i as usize] = Val::Bool(false);
                                    }
                                    done = true;
                                } else if i >= 0 && (i as usize) == self.evecs[ob.elems as usize].v.len() && (i as double) == n && ob.extensible && ob.pos != 3 {
                                    if ob.elems == 0 {
                                        if !self.efree.is_empty() {
                                            ob.elems = self.efree.pop().unwrap();
                                        } else {
                                            grow_evecs(&mut self.evecs);
                                            self.evecs.push(elems_of(Vec::new()));
                                            ob.elems = (self.evecs.len() - 1) as i32;
                                        }
                                    }
                                    self.evecs[ob.elems as usize].v.push(val.clone());
                                    done = true;
                                }
                            }
                            if done {
                                self.dec_o(old);
                                self.sdec(old_s);
                            } else {
                                self.dec(&val);
                            }
                        }
                        if !done {
                            self.r_set_elem_slow(bp, fi, pc, (w1 as int), (w2 as int), val);
                        }
                        if ((w0 >> 8) & 255) == 1 {
                            // the assignment's value, where the object was
                            self.rset((bp + (w1 as int)) as usize, val);
                        }
                    }
                    R_GET_METHOD => {
                        self.r_get_method(bp, rb, fi, pc, (w0 as int), (w1 as int), (w2 as int), (w3 as int));
                    }
                    R_CALL => {
                        let argc = w2;
                        frame_mut(&mut self.frames, fi).pc = (pc + 1 + ((w0 >> 16) as int));
                        if self.gc_due() {
                            self.rgc(want);
                        }
                        let fpos = bp + (w1 as int);
                        if ((w0 >> 8) & 255) == 1 {
                            self.stack[(fpos + 1) as usize] = Val::Undef;
                        }
                        let fo = obj_of(val_at(&self.stack, fpos as usize));
                        if fo >= 0 && self.ob(fo).class == C_FUNCTION {
                            let fp = self.ob(fo).func as usize;
                            let plain = {
                                let p = proto_at(&self.protos, fp);
                                p.rest < 0 && !p.class_ctor
                            };
                            // the result goes to the slot of f, or to w3 - 1
                            let dst = if w3 > 0 { bp + (w3 as int) - 1 } else { fpos };
                            if plain {
                                let co = proto_at(&self.protos, fp).generator || proto_at(&self.protos, fp).is_async;
                                if co && dst != fpos {
                                    self.frames[fi].mv_from = fpos;
                                    self.frames[fi].mv_to = dst;
                                }
                                if !self.renter(fo, fpos, (argc as int), if co { -1 } else { dst }) {
                                    continue 'frames;
                                }
                                // the callee runs in this loop
                                fi = self.frames.len() - 1;
                                pi = fp;
                                bp = fpos + 2;
                                pc = frame_at(&self.frames, fi).pc;
                                want = (bp + proto_at(&self.protos, pi).fsize) as usize;
                                rb = proto_at(&self.protos, pi).rbase;
                                if self.slicing && self.frame_brk() {
                                    return;
                                }
                                continue;
                            }
                        }
                        if self.r_call_slow(fo, fpos, (argc as int), bp, (w3 as int), want) {
                            continue 'frames;
                        }
                        if self.throwing {
                            if !self.unwind(base) {
                                return;
                            }
                            continue 'frames;
                        }
                        pc = (pc + 1 + ((w0 >> 16) as int));
                        continue;
                    }
                    R_NEW_ARRAY => {
                        self.r_new_array(bp, (w1 as int), (w2 as int));
                    }
                    R_NEW => {
                        let argc = w2 as int;
                        let fpos = bp + (w1 as int);
                        let fo = obj_of(&self.stack[fpos as usize]);
                        let mut plain = false;
                        if fo >= 0 && self.ob(fo).class == C_FUNCTION {
                            let p = &self.protos[self.ob(fo).func as usize];
                            plain = p.rest < 0 && !p.class_ctor && p.constructible();
                        }
                        if plain {
                            self.frames[fi].pc = (pc + 1 + ((w0 >> 16) as int));
                            if self.gc_due() {
                                self.rgc(want);
                            }
                            let top = (fpos + argc + 2) as usize;
                            if self.stack.len() < top {
                                grow_stack(&mut self.stack, top);
                            }
                            let mut k = argc;
                            while k > 0 {
                                self.stack[(fpos + 1 + k) as usize] = self.stack[(fpos + k) as usize];
                                k -= 1;
                            }
                            let f = self.stack[fpos as usize];
                            let this = self.make_this(&f);
                            self.stack[(fpos + 1) as usize] = this;
                            // a forwarder's m in its place
                            let m = if self.ob(fo).func >= 0 && self.protos[self.ob(fo).func as usize].fwd_atom >= 0 { self.fwd_target(self.ob(fo).func as usize, &this) } else { -1 };
                            if m >= 0 {
                                self.stack[fpos as usize] = Val::Obj(m);
                            }
                            if !self.renter(if m >= 0 { m } else { fo }, fpos, argc, fpos) {
                                continue 'frames;
                            }
                            fi = self.frames.len() - 1;
                            self.frames[fi].construct = true;
                            self.frames[fi].new_target = if m >= 0 { Val::Undef } else { f };
                            pi = self.frames[fi].proto as usize;
                            bp = fpos + 2;
                            pc = self.frames[fi].pc;
                            want = (bp + self.protos[pi].fsize) as usize;
                            rb = self.protos[pi].rbase;
                            if self.slicing && self.frame_brk() {
                                return;
                            }
                            continue;
                        }
                        let p2 = self.r_win(pi, bp, fi, pc, (w3 as int), want);
                        if self.throwing {
                            if !self.unwind(base) {
                                return;
                            }
                            continue 'frames;
                        }
                        if p2 < 0 {
                            continue 'frames;
                        }
                        npc = p2;
                    }
                    R_RET | R_RET_UNDEF => {
                        let mut v = if ((w0 & 255) as int) == R_RET { self.rget(bp, (w1 as int)).clone() } else { Val::Undef };
                        if frame_at(&self.frames, fi).construct && !is_obj(&v) {
                            if self.r_ret_construct(&mut v, fi, pc) {
                                if !self.unwind(base) {
                                    return;
                                }
                                continue 'frames;
                            }
                        }
                        let fr = self.frames.pop().unwrap();
                        let depth = self.frames.len() as int;
                        while !self.handlers.is_empty() && self.handlers[self.handlers.len() - 1].frame >= depth {
                            self.handlers.pop();
                        }
                        let fpos = (fr.bp - 2) as usize;
                        if (self.frames.len() as int) > base {
                            // into the caller's slot of f (or the one the
                            // call named); the stack ends at the caller's
                            // frame again and the caller goes on here
                            fi = self.frames.len() - 1;
                            let fc = frame_at(&self.frames, fi);
                            pi = fc.proto as usize;
                            bp = fc.bp;
                            pc = fc.pc;
                            want = (bp + proto_at(&self.protos, pi).fsize) as usize;
                            rb = proto_at(&self.protos, pi).rbase;
                            let to = if fr.ret >= 0 { fr.ret as usize } else { fpos };
                            // the caller's slot of f or a register of its frame
                            val_put(&mut self.stack, to, v);
                            self.grow_stack(want);
                            if self.slicing && self.frame_brk() {
                                return;
                            }
                            if self.jit_on && ((rb + pc) as usize) < self.jit_res.len() && self.jit_res[(rb + pc) as usize] {
                                // after a call out of compiled code: back in
                                pc = self.jit_go(pi, bp, fi, pc, false);
                                if pc < 0 {
                                    if pc == -1 || (pc == -2 && !self.unwind(base)) {
                                        return;
                                    }
                                    continue 'frames;
                                }
                            }
                            continue;
                        } else {
                            self.stack.truncate(fpos);
                            self.stack.push(v);
                        }
                        continue 'frames;
                    }
                    R_THIS => {
                        let t = self.frames[fi].this_val.clone();
                        self.rset((bp + (w1 as int)) as usize, t);
                        pc = (pc + 1 + ((w0 >> 16) as int));
                        continue;
                    }
                    R_GET_ENV => {
                        let e = self.env_of(fi, (w2 as int));
                        let v = self.env_slot(e, (w3 as int));
                        self.rset((bp + (w1 as int)) as usize, v);
                        if ((w0 >> 8) & 255) == 1 {
                            self.rset((bp + (w1 as int) + 1) as usize, Val::Undef);
                        }
                        pc = (pc + 1 + ((w0 >> 16) as int));
                        continue;
                    }
                    R_SET_ENV => {
                        let e = self.env_of(fi, (w2 as int));
                        let v = self.rget(bp, (w1 as int)).clone();
                        self.env_put(e, (w3 as int), v);
                        pc = (pc + 1 + ((w0 >> 16) as int));
                        continue;
                    }
                    R_GET_GLOBAL => {
                        let g = self.global;
                        let c = op_at(&proto_at(&self.protos, pi).code, pc as usize).c;
                        let slot = self.global_ic(g, (w3 as int), (c as int));
                        if slot >= 0 {
                            let v = slot_at(self.ob(g), slot as usize);
                            self.rset((bp + (w1 as int)) as usize, v);
                            pc = (pc + 1 + ((w0 >> 16) as int));
                            continue;
                        }
                        self.r_step_push(pi, bp, fi, pc, (w1 as int));
                    }
                    R_ITER_NEXT => {
                        if !self.r_iter_next(bp, fi, pc, (w1 as int), (w3 as int)) && !self.throwing {
                            npc = (w2 as int);
                            if self.slicing && self.brk(fi, npc) {
                                return;
                            }
                        }
                    }
                    R_TYPEOF => {
                        let v = self.rget(bp, (w2 as int)).clone();
                        let t = self.type_val(&v);
                        self.rset((bp + (w1 as int)) as usize, t);
                        pc = (pc + 1 + ((w0 >> 16) as int));
                        continue;
                    }
                    R_ADD_ENV => {
                        if let (Val::Num(x), Val::Num(y)) = (self.rget(bp, (w1 as int)), self.rget(bp, (w2 as int))) {
                            let r = Val::Num(*x + *y);
                            let e = self.env_of(fi, ((w0 >> 8) & 255) as int);
                            self.env_put(e, (w3 as int), r);
                            pc = (pc + 1 + ((w0 >> 16) as int));
                            continue;
                        }
                        self.r_add_env(bp, fi, pc, (w0 as int), (w1 as int), (w2 as int), (w3 as int));
                    }
                    R_GET_METHOD_ELEM => {
                        self.r_get_method_elem(bp, fi, pc, (w1 as int), (w2 as int), (w3 as int));
                    }
                    R_CHECK => {
                        let v = *self.rget(bp, (w1 as int));
                        let bad = if ((w0 >> 8) & 255) == 0 {
                            if let Val::Obj(o) = v { o == self.tdz_obj } else { false }
                        } else {
                            matches!(v, Val::Undef) || matches!(v, Val::Null)
                        };
                        if bad {
                            self.frames[fi].pc = pc + 1;
                            self.r_check_fail(pi, ((w0 >> 8) & 255) as int, (w2 as int));
                        }
                    }
                    R_TONUM => {
                        let neg = ((w0 >> 8) & 255) == 1;
                        if let Val::Num(x) = self.rget(bp, (w2 as int)) {
                            let r = if neg { -*x } else { *x };
                            self.rset((bp + (w1 as int)) as usize, Val::Num(r));
                        } else {
                            self.r_unary_slow(bp, fi, pc, if neg { 1 } else if ((w0 >> 8) & 255) == 2 { 4 } else { 0 }, (w1 as int), (w2 as int));
                        }
                    }
                    R_INC => {
                        let dec = ((w0 >> 8) & 255) == 1;
                        if let Val::Num(x) = self.rget(bp, (w2 as int)) {
                            let r = if dec { *x - 1.0 } else { *x + 1.0 };
                            self.rset((bp + (w1 as int)) as usize, Val::Num(r));
                        } else {
                            self.r_unary_slow(bp, fi, pc, if dec { 3 } else { 2 }, (w1 as int), (w2 as int));
                        }
                    }
                    R_SET_GLOBAL => {
                        let g = self.global;
                        let c = self.protos[pi].code[pc as usize].c;
                        let v = self.rget(bp, (w1 as int)).clone();
                        let slot = self.ic_slot(g, (w3 as int), (c as int), true);
                        if slot >= 0 {
                            self.put_slot(g, slot, v);
                        } else {
                            self.frames[fi].pc = pc + 1;
                            self.set_global((w3 as int), v, ((w0 >> 8) & 255) as int, pi, pc + 1);
                        }
                    }
                    R_NOT => {
                        let r = !self.truthy_v(self.rget(bp, (w2 as int)));
                        self.rset((bp + (w1 as int)) as usize, Val::Bool(r));
                        pc = (pc + 1 + ((w0 >> 16) as int));
                        continue;
                    }
                    R_NEW_OBJECT => {
                        let o = self.new_object();
                        self.rset((bp + (w1 as int)) as usize, Val::Obj(o));
                        pc = (pc + 1 + ((w0 >> 16) as int));
                        continue;
                    }
                    R_INIT_PROP => {
                        let v = self.rget(bp, (w2 as int)).clone();
                        let o = obj_of(self.rget(bp, (w1 as int)));
                        // c caches the transition: shape before * 65536 + after
                        let sh = self.objs[o as usize].shape as int;
                        let c = self.protos[pi].code[pc as usize].c;
                        if c >= 0 && ((c >> 16) as int) == sh {
                            self.objs[o as usize].shape = c & 65535;
                            let most = self.shapes[self.holder((c & 65535) as int)].keys.len();
                            slot_room(&mut self.objs[o as usize], most);
                            self.push_val(o, v);
                        } else {
                            self.frames[fi].pc = pc + 1;
                            self.define(o, (w3 as int), v, 0);
                            let sh2 = self.objs[o as usize].shape as int;
                            if sh2 != sh && sh < 32768 && sh2 < 65536 && !self.shapes[sh as usize].owned && !self.shapes[sh2 as usize].owned {
                                self.protos[pi].code[pc as usize].c = (sh * 65536 + sh2) as i32;
                            }
                        }
                    }
                    R_CLOSURE => {
                        let f = self.closure((w2 as int));
                        self.rset((bp + (w1 as int)) as usize, Val::Obj(f));
                        pc = (pc + 1 + ((w0 >> 16) as int));
                        continue;
                    }
                    R_INC_LOCAL => {
                        let i = (bp + (w1 as int)) as usize;
                        if let Val::Num(n) = self.stack[i] {
                            self.stack[i] = Val::Num(n + (w2 as double));
                            pc = (pc + 1 + ((w0 >> 16) as int));
                            continue;
                        }
                        let v = self.stack[i].clone();
                        self.frames[fi].pc = pc + 1;
                        let (_, nv) = self.numeric_step(&v, (w2 as int));
                        if !self.throwing {
                            self.stack[i] = nv;
                        }
                    }
                    R_POSTINC => {
                        let i = (bp + (w2 as int)) as usize;
                        let pre = ((w0 >> 8) & 255) == 1;
                        if let Val::Num(old) = self.stack[i] {
                            let nv = old + (w3 as double);
                            self.stack[i] = Val::Num(nv);
                            self.rset((bp + (w1 as int)) as usize, Val::Num(if pre { nv } else { old }));
                            pc = (pc + 1 + ((w0 >> 16) as int));
                            continue;
                        }
                        let v = self.stack[i].clone();
                        self.frames[fi].pc = pc + 1;
                        let (old, nv) = self.numeric_step(&v, (w3 as int));
                        if !self.throwing {
                            self.stack[i] = nv.clone();
                            self.rset((bp + (w1 as int)) as usize, if pre { nv } else { old });
                        }
                    }
                    R_ADD_LOCAL_POP => {
                        let b = *self.rget(bp, (w2 as int));
                        let i = (bp + (w1 as int)) as usize;
                        // numbers first: the string append below is the rarer case
                        if let (Val::Num(x), Val::Num(y)) = (val_at(&self.stack, i), &b) {
                            let r = Val::Num(*x + *y);
                            self.rset(i, r);
                            pc = (pc + 1 + ((w0 >> 16) as int));
                            continue;
                        }
                        let h = self.append_val(self.stack[i], b);
                        if h >= 0 {
                            self.stack[i] = Val::Str(h);
                        } else {
                            let a = self.stack[i].clone();
                            let mut r = Val::Undef;
                            let mut quick = false;
                            if let (Val::Num(x), Val::Num(y)) = (&a, &b) {
                                r = Val::Num(*x + *y);
                                quick = true;
                            }
                            if !quick {
                                self.frames[fi].pc = pc + 1;
                                r = self.add_vals(&a, &b);
                            }
                            if !self.throwing {
                                self.stack[i] = r;
                            }
                        }
                    }
                    R_STEP0 => {
                        let op = self.protos[pi].code[pc as usize];
                        let mut p2 = pc + 1;
                        self.frames[fi].pc = p2;
                        self.step(op, pi, bp, fi, &mut p2);
                    }
                    R_STEP_PUSH => {
                        self.r_step_push(pi, bp, fi, pc, (w1 as int));
                    }
                    R_STEP_K => {
                        self.r_step_k(pi, bp, fi, pc, (w0 as int), (w1 as int), (w2 as int), (w3 as int));
                    }
                    R_WIN => {
                        let p2 = self.r_win(pi, bp, fi, pc, (w1 as int), want);
                        if self.throwing {
                            if !self.unwind(base) {
                                return;
                            }
                            continue 'frames;
                        }
                        if p2 < 0 {
                            continue 'frames;
                        }
                        npc = p2;
                        if npc != pc + 1 + ((w0 >> 16) as int) && self.slicing && self.brk(fi, npc) {
                            return;
                        }
                    }
                    _ => {
                        // translate() writes only the R_ ops above
                        unknown_op();
                        self.throw_type("register code: unknown instruction");
                    }
                }
                if self.throwing {
                    self.frames[fi].pc = pc + 1;
                    if !self.unwind(base) {
                        return;
                    }
                    continue 'frames;
                }
                pc = if npc >= 0 { npc } else { pc + 1 + ((op_at(&self.rcode, (rb + pc) as usize).code >> 16) as int) };
            }
        }
    }

    /// R_JNN_KEEP sub 1 (`x?.`): slot w1 undefined or null becomes undefined, and the
    /// jump's target w2; -1 to fall through. Kept out of rrun's loop.
    #[inline(never)]
    pub fn r_jnull(&mut self, bp: int, w1: int, w2: int) -> int {
        let i = (bp + w1) as usize;
        if matches!(self.stack[i], Val::Undef | Val::Null) {
            self.stack[i] = Val::Undef;
            return w2;
        }
        -1
    }

    #[inline(never)]
    pub fn r_add_slow(&mut self, bp: int, fi: usize, pc: int, w1: int, w2: int, w3: int) {
        if self.jit_on {
            self.note_nonnum(fi, pc);
        }
        let a = self.rget(bp, w2).clone();
        let b = self.rget(bp, w3).clone();
        // a longer string whose buffer nothing else holds grows in place
        // (str_append: a keeps its text as a prefix), as `s += x` does
        if let (Val::Str(h), Val::Str(_)) = (&a, &b) {
            if self.strs[*h as usize].s.as_bytes().len() >= 32 {
                let r = self.append_val(a, b);
                if r >= 0 {
                    self.stack[(bp + w1) as usize] = Val::Str(r);
                    return;
                }
            }
        }
        self.frames[fi].pc = pc + 1;
        let r = self.add_vals(&a, &b);
        if !self.throwing {
            self.stack[(bp + w1) as usize] = r;
        }
    }

    #[inline(never)]
    pub fn r_arith_slow(&mut self, code: int, bp: int, fi: usize, pc: int, w1: int, w2: int, w3: int) {
        if self.jit_on {
            self.note_nonnum(fi, pc);
        }
        let a = self.rget(bp, w2).clone();
        let b = self.rget(bp, w3).clone();
        self.frames[fi].pc = pc + 1;
        let r = self.arith(code, &a, &b);
        if !self.throwing {
            self.stack[(bp + w1) as usize] = r;
        }
    }

    /// R_GET_PROP / R_GET_THIS_PROP past its inline cache.
    #[inline(never)]
    pub fn r_get_prop_slow(&mut self, o: int, pi: usize, rb: int, bp: int, fi: usize, pc: int, w0: int, w1: int, w2: int) -> bool {
        let atom = self.ratom(pi, pc);
        if o < 0 && atom == A_LENGTH && (w0 & 255) == R_GET_PROP {
            if let Val::Str(h) = *self.rget(bp, w2) {
                let n = self.hlen(h) as double;
                self.stack[(bp + w1) as usize] = Val::Num(n);
                return false;
            }
        }
        let mut h0: int = -1;
        if o >= 0 && self.rcode[(rb + pc) as usize].c == -3 {
            // the site last read through the chain: its cache first (a hit
            // means o's shape has no such own property)
            h0 = self.pic_probe(rb + pc, o);
            if h0 != -1 {
                let way = self.pic_way as usize;
                if h0 < 0 || self.pic[way + 4] == 0 {
                    let r = if h0 >= 0 { self.objs[h0 as usize].get_slot(self.pic[way + 3] as usize) } else { Val::Undef };
                    self.stack[(bp + w1) as usize] = r;
                    return false;
                }
                // a getter on the chain: on to its call
            }
        }
        if o >= 0 && h0 == -1 {
            let site = (rb + pc) as usize;
            let c = self.rcode[site].c;
            let c2 = self.ic2_hit(site, o);
            if c2 >= 0 {
                self.rcode[site].c = c2;
                self.ic2[site] = c;
                let r = self.objs[o as usize].get_slot((c2 & 65535) as usize);
                self.stack[(bp + w1) as usize] = r;
                return false;
            }
            let slot = self.ic_get(o, atom, (c as int), pi, pc + 1);
            self.rcode[site].c = self.protos[pi].code[pc as usize].c;
            if slot >= 0 && c >= 65536 {
                self.ic2_keep(site, c);
            }
            if slot >= 0 {
                let r = self.objs[o as usize].get_slot(slot as usize).clone();
                self.stack[(bp + w1) as usize] = r;
                return false;
            }
            if atom == A_LENGTH && (self.objs[o as usize].class == C_ARRAY || self.objs[o as usize].class == C_ARGUMENTS) && self.find(o, atom) < 0 {
                let r = Val::Num(self.evecs[self.objs[o as usize].elems as usize].v.len() as double);
                self.stack[(bp + w1) as usize] = r;
                // the next read here takes rrun's array-length path
                self.rcode[(rb + pc) as usize].c = -4;
                return false;
            }
        }
        if o >= 0 {
            let h = if h0 != -1 { h0 } else { self.pic_get(rb + pc, o, atom) };
            if h != -1 {
                let way = self.pic_way as usize;
                let r = if h >= 0 { self.objs[h as usize].get_slot(self.pic[way + 3] as usize) } else { Val::Undef };
                if h < 0 || self.pic[way + 4] == 0 {
                    self.stack[(bp + w1) as usize] = r;
                    self.rcode[(rb + pc) as usize].c = -3;
                    return false;
                }
                // a getter: a plain function runs as a call in this loop
                self.rcode[(rb + pc) as usize].c = -3;
                let g = self.evecs[self.objs[obj_of(&r) as usize].elems as usize].v[0];
                let go = obj_of(&g);
                if go >= 0 && self.objs[go as usize].class == C_NATIVE && (self.objs[go as usize].func as int) == crate::typed::NF_TA_LEN {
                    // TypedArray.prototype.length's own getter on a typed array
                    let recv = if (w0 & 255) == R_GET_PROP { *self.rget(bp, w2) } else { self.frames[fi].this_val };
                    let ro = obj_of(&recv);
                    if ro >= 0 && self.objs[ro as usize].class == C_TYPED {
                        self.stack[(bp + w1) as usize] = Val::Num(self.ta_length(ro) as double);
                        return false;
                    }
                }
                if go >= 0 && self.objs[go as usize].class == C_FUNCTION {
                    let gp = self.objs[go as usize].func as usize;
                    let plain = {
                        let p = &self.protos[gp];
                        p.rest < 0 && !p.class_ctor && !p.generator && !p.is_async
                    };
                    if plain {
                        let recv = if (w0 & 255) == R_GET_PROP { *self.rget(bp, w2) } else { self.frames[fi].this_val };
                        self.frames[fi].pc = pc + 1;
                        let fpos = bp + self.protos[pi].fsize;
                        while (self.stack.len() as int) < fpos + 2 {
                            self.stack.push(Val::Undef);
                        }
                        self.stack[fpos as usize] = g;
                        self.stack[(fpos + 1) as usize] = recv;
                        return self.renter(go, fpos, 0, bp + w1);
                    }
                }
            }
        }
        let v = if (w0 & 255) == R_GET_PROP { self.rget(bp, w2).clone() } else { self.frames[fi].this_val.clone() };
        self.frames[fi].pc = pc + 1;
        if o >= 0 && self.objs[o as usize].class == C_PROXY {
            let fpos = bp + self.protos[pi].fsize;
            if self.r_proxy_get(o, atom, v, fpos, bp + w1) {
                return true;
            }
        }
        let r = self.get(&v, atom);
        if !self.throwing {
            self.stack[(bp + w1) as usize] = r;
        }
        false
    }

    /// `p[atom]` (receiver `recv`) for a proxy of a plain object whose
    /// handler has its own `get`, a plain JS function: the trap runs as a
    /// call in this loop, not in a nested run, and its result goes to
    /// `dst`. False when that does not apply: then nothing was done and
    /// the caller takes the ordinary way (proxy_get_atom). The invariant
    /// for a non-configurable own property of the target is checked
    /// before the call (a trap that makes one on its own target as it runs
    /// is not checked against it).
    pub fn r_proxy_get(&mut self, p: int, atom: int, recv: Val, fpos: int, dst: int) -> bool {
        let target = self.objs[p as usize].env as int;
        let handler = self.colds[self.objs[p as usize].cold as usize].home;
        if handler < 0 || target < 0 || self.proxy_hooks < 0 || self.objs[target as usize].class != C_OBJECT || self.objs[handler as usize].class != C_OBJECT {
            return false;
        }
        let hs = self.find(handler, A_GET);
        if hs < 0 || (self.attr_at(handler, hs) & P_ACCESSOR) != 0 {
            return false;
        }
        let trap = self.objs[handler as usize].get_slot(hs as usize);
        let go = obj_of(&trap);
        if go < 0 || self.objs[go as usize].class != C_FUNCTION {
            return false;
        }
        let plain = {
            let q = &self.protos[self.objs[go as usize].func as usize];
            q.rest < 0 && !q.class_ctor && !q.generator && !q.is_async
        };
        if !plain {
            return false;
        }
        let ts = self.find(target, atom);
        if ts >= 0 && (self.attr_at(target, ts) & P_FIXED) != 0 {
            return false;
        }
        let k = self.key_val(atom);
        self.grow_stack((fpos + 5) as usize);
        self.stack[fpos as usize] = trap;
        self.stack[(fpos + 1) as usize] = Val::Obj(handler);
        self.stack[(fpos + 2) as usize] = Val::Obj(target);
        self.stack[(fpos + 3) as usize] = k;
        self.stack[(fpos + 4) as usize] = recv;
        self.renter(go, fpos, 3, dst)
    }

    /// The stack at least `want` long, undefined past its end.
    #[inline]
    pub fn grow_stack(&mut self, want: usize) {
        while self.stack.len() < want {
            self.stack.push(Val::Undef);
        }
    }

    /// Where frame fi's register code starts in rcode.
    #[inline]
    fn rbase_of(&self, fi: usize) -> int {
        self.protos[self.frames[fi].proto as usize].rbase
    }

    /// A read of `atom`, which `o` does not have itself, from o's
    /// prototype or the one after it, through the cache of rcode site
    /// `site`: the object holding it (its slot is the entry's 5th int), or
    /// -1 when the cache does not know and could not learn it.
    fn pic_get(&mut self, site: int, o: int, atom: int) -> int {
        let si = site as usize;
        if si >= self.rpic.len() {
            while self.rpic.len() < self.rcode.len() {
                self.rpic.push(-1);
            }
        }
        let h = self.pic_probe(site, o);
        if h != -1 {
            return h;
        }
        self.pic_learn(si, o, atom)
    }

    /// pic_get's lookup without learning: the holder, -2 for a chain
    /// known to end without the property, -1 for no way that fits.
    #[inline(always)]
    fn pic_probe(&mut self, site: int, o: int) -> int {
        let e0 = self.rpic[site as usize];
        if e0 < 0 {
            return -1;
        }
        let epoch = self.shape_epoch as i32;
        let osh = self.ob(o).shape;
        let mut w: int = 0;
        while w < PIC_WAYS {
            let b = ((e0 + w as i32) * PIC_W as i32) as usize;
            w += 1;
            if i32_at(&self.pic, b) != osh || i32_at(&self.pic, b + 1) != epoch {
                continue;
            }
            let depth = i32_at(&self.pic, b + 2) as usize;
            let mut cur = o;
            let mut k: usize = 0;
            let mut ok = true;
            while k < depth {
                let pk = i32_at(&self.pic, b + 5 + 2 * k);
                // pk is alive when cur's prototype is pk
                if self.ob(cur).proto != pk || self.ob(pk as int).shape != i32_at(&self.pic, b + 6 + 2 * k) {
                    ok = false;
                    break;
                }
                cur = pk as int;
                k += 1;
            }
            if !ok {
                continue;
            }
            if i32_at(&self.pic, b + 3) >= 0 {
                self.pic_way = b as int;
                return cur;
            }
            if self.ob(cur).proto < 0 {
                return -2;
            }
        }
        -1
    }

    /// pic_get's miss: the chain read and kept in a way of site si.
    #[inline(never)]
    fn pic_learn(&mut self, si: usize, o: int, atom: int) -> int {
        let e0 = self.rpic[si];
        // learn it: plain objects on the way, then a property or the end of
        // the chain
        // a dictionary receiver gains keys without a shape change or an
        // epoch (add_own): not cached
        if !self.pic_plain(o, atom) || self.shapes[self.objs[o as usize].shape as usize].owned || self.find(o, atom) >= 0 {
            return -1;
        }
        let mut chain: Vec<int> = Vec::new();
        let mut cur = o;
        let mut slot: int = -1;
        loop {
            let nx = self.objs[cur as usize].proto as int;
            if nx < 0 {
                break;
            }
            if (chain.len() as int) >= PIC_DEPTH || self.objs[nx as usize].class == C_PROXY {
                return -1;
            }
            chain.push(nx);
            cur = nx;
            slot = self.find(cur, atom);
            if slot >= 0 {
                break;
            }
            if !self.pic_plain(cur, atom) {
                return -1;
            }
        }
        if chain.is_empty() {
            return -1;
        }
        let acc = slot >= 0 && (self.attr_at(cur, slot) & P_ACCESSOR) != 0;
        let base = if e0 >= 0 {
            e0
        } else {
            let n = self.pic.len() as int / PIC_W;
            let mut k: int = 0;
            while k < PIC_W * PIC_WAYS {
                self.pic.push(-1);
                k += 1;
            }
            self.pic[(n * PIC_W + PIC_W - 1) as usize] = 0;
            self.rpic[si] = n as i32;
            n as i32
        };
        // the next way in turn (the first entry's last int counts)
        let ti = (base * PIC_W as i32 + PIC_W as i32 - 1) as usize;
        let turn = self.pic[ti];
        self.pic[ti] = (turn + 1) % PIC_WAYS as i32;
        let b = ((base + turn) * PIC_W as i32) as usize;
        self.pic[b] = self.objs[o as usize].shape as i32;
        self.pic[b + 1] = self.shape_epoch as i32;
        self.pic[b + 2] = chain.len() as i32;
        self.pic[b + 3] = slot as i32;
        self.pic[b + 4] = if acc { 1 } else { 0 };
        let mut k: usize = 0;
        while k < chain.len() {
            let x = chain[k];
            self.pic[b + 5 + 2 * k] = x as i32;
            self.pic[b + 6 + 2 * k] = self.objs[x as usize].shape as i32;
            k += 1;
        }
        self.pic_way = b as int;
        if slot < 0 {
            -2
        } else {
            cur
        }
    }

    /// Whether a miss of `atom` on `x` just goes on to its prototype
    /// (get_obj): no proxy, no property made on demand.
    #[inline]
    fn pic_plain(&self, x: int, atom: int) -> bool {
        let c = self.objs[x as usize].class;
        if c == C_OBJECT {
            return true;
        }
        if c == C_TYPED {
            // a typed array's length is TypedArray.prototype's getter
            return atom != A_NAME && atom != A_PROTOTYPE;
        }
        c != C_PROXY && c != C_STRING && atom != A_LENGTH && atom != A_NAME && atom != A_PROTOTYPE
    }

    /// R_SET_PROP past its inline cache, or one that keeps its value.
    #[inline(never)]
    pub fn r_set_prop_slow(&mut self, o: int, pi: usize, rb: int, bp: int, fi: usize, pc: int, w0: int, w1: int, w2: int, w3: int) {
        let val = self.rget(bp, w2).clone();
        let mut done = false;
        if o >= 0 && w3 <= -2 {
            // the cache's transition: a plain object of the shape before
            // takes the property as its next slot
            let t = -2 - w3;
            let sh = self.objs[o as usize].shape as int;
            if (t >> 16) == sh && self.add_cached(o, pi, pc) {
                let after = (t & 65535) as int;
                self.objs[o as usize].shape = after as i32;
                let most = self.shapes[self.holder(after)].keys.len();
                slot_room(&mut self.objs[o as usize], most);
                self.push_val(o, val);
                if ((w0 >> 8) & 255) == 1 {
                    self.stack[(bp + w1) as usize] = val;
                }
                return;
            }
        }
        if o >= 0 {
            let slot = if w3 >= 65536 && (w3 >> 16) == self.objs[o as usize].shape as int {
                w3 & 65535
            } else {
                let site = (rb + pc) as usize;
                let c2 = self.ic2_hit(site, o);
                if c2 >= 0 {
                    self.rcode[site].c = c2;
                    self.ic2[site] = w3 as i32;
                    (c2 & 65535) as int
                } else {
                    let atom = self.ratom(pi, pc);
                    let s = self.ic_set(o, atom, w3, pi, pc + 1);
                    self.rcode[site].c = self.protos[pi].code[pc as usize].c;
                    if s >= 0 && w3 >= 65536 {
                        self.ic2_keep(site, w3 as i32);
                    }
                    s
                }
            };
            if slot >= 0 {
                self.put_slot(o, slot, val.clone());
                done = true;
            }
        }
        if !done {
            let target = self.rget(bp, w1).clone();
            self.frames[fi].pc = pc + 1;
            let atom = self.ratom(pi, pc);
            let sh = if o >= 0 { self.objs[o as usize].shape as int } else { -1 };
            self.set(&target, atom, val.clone());
            if sh >= 0 && sh < 32768 && !self.throwing && self.add_cached(o, pi, pc) {
                // an add that went to the next slot: the transition is cached
                let sh2 = self.objs[o as usize].shape as int;
                if sh2 != sh && sh2 < 65536 && !self.shapes[sh as usize].owned && !self.shapes[sh2 as usize].owned
                    && self.shapes[sh2 as usize].nkeys == self.shapes[sh as usize].nkeys + 1 {
                    let k = self.shapes[sh as usize].nkeys as usize;
                    let h = self.holder(sh2);
                    if self.shapes[h].keys[k] == atom && self.shapes[h].attrs[k] == 0 {
                        let c = (-2 - (sh * 65536 + sh2)) as i32;
                        self.protos[pi].code[pc as usize].c = c;
                        self.rcode[(rb + pc) as usize].c = c;
                    }
                }
            }
        }
        if ((w0 >> 8) & 255) == 1 && !self.throwing {
            self.stack[(bp + w1) as usize] = val;
        }
    }

    /// The second cache of register site `site` when it holds `o`'s shape
    /// (shape * 65536 + slot), else -1.
    #[inline]
    fn ic2_hit(&self, site: usize, o: int) -> i32 {
        if site < self.ic2.len() {
            let c2 = self.ic2[site];
            if c2 >= 65536 && (c2 >> 16) == self.objs[o as usize].shape {
                return c2;
            }
        }
        -1
    }

    /// Keeps `c` (a cache the site's c held before a miss) as its second.
    fn ic2_keep(&mut self, site: usize, c: i32) {
        if site >= self.ic2.len() {
            let n = if self.rcode.len() > site { self.rcode.len() } else { site + 1 };
            while self.ic2.len() < n {
                self.ic2.push(-1);
            }
        }
        self.ic2[site] = c;
    }

    /// Whether an add of the property of the set op at pc to `o` may skip
    /// set_obj: a plain extensible object, no setter or read-only property
    /// of that name anywhere, no proxy that could be on its chain.
    fn add_cached(&self, o: int, pi: usize, pc: int) -> bool {
        let ob = &self.objs[o as usize];
        if ob.class != C_OBJECT || !ob.extensible || (self.proxy_made && self.proxy_on_chain(o)) {
            return false;
        }
        let atom = self.ratom(pi, pc);
        atom != A_PROTO && !self.setter_atom(atom)
    }

    #[inline(never)]
    pub fn r_set_elem_slow(&mut self, bp: int, fi: usize, pc: int, w1: int, w2: int, val: Val) {
        let target = self.rget(bp, w1).clone();
        let kv = self.rget(bp, w2).clone();
        self.frames[fi].pc = pc + 1;
        self.set_elem(&target, &kv, val);
    }

    /// R_GET_METHOD: the method and its receiver into w1, w1 + 1; the
    /// chain cache answers here, the rest goes to r_get_method_slow.
    #[inline(never)]
    pub fn r_get_method(&mut self, bp: int, rb: int, fi: usize, pc: int, w0: int, w1: int, w2: int, w3: int) {
        let v = *self.rget(bp, w2);
        if w3 <= MC_BASE {
            if let Val::Obj(o) = v {
                // a method one prototype up: the site's entry in mcache
                let k = ((MC_BASE - w3) * 5) as usize;
                let (osh, p) = {
                    let ob = self.ob(o);
                    (ob.shape, ob.proto)
                };
                if k + 4 < self.mcache.len() && i32_at(&self.mcache, k) == osh && i32_at(&self.mcache, k + 1) == (self.shape_epoch as i32) && p == i32_at(&self.mcache, k + 2) {
                    let po = self.ob(p as int);
                    if po.shape == i32_at(&self.mcache, k + 3) {
                        let f = slot_at(po, i32_at(&self.mcache, k + 4) as usize);
                        if let Val::Obj(fo) = f {
                            let fc = self.ob(fo).class;
                            if fc == C_FUNCTION || fc == C_NATIVE {
                                self.rset((bp + w1) as usize, f);
                                self.rset((bp + w1 + 1) as usize, v);
                                return;
                            }
                        }
                    }
                }
            }
        }
        self.r_get_method_more(bp, rb, fi, pc, w0, w1, v, w3);
    }

    /// r_get_method past the site's mcache entry: a primitive's method,
    /// the receiver's own property, the site's ways (pic).
    #[inline(never)]
    fn r_get_method_more(&mut self, bp: int, rb: int, fi: usize, pc: int, w0: int, w1: int, v: Val, w3: int) {
        if w3 > MC_BASE && w3 <= -16 {
            if matches!(v, Val::Str(_)) || matches!(v, Val::Num(_)) {
                // a string's (number's) method: String.prototype's
                // (Number.prototype's) slot -16 - w3, its key checked as an
                // inline cache's
                let sp = if matches!(v, Val::Num(_)) { self.number_proto } else { self.string_proto };
                let atom = self.protos[self.frames[fi].proto as usize].code[pc as usize].a as int;
                let slot = self.ic_slot(sp, atom, -16 - w3, false);
                if slot >= 0 {
                    let f = slot_at(self.ob(sp), slot as usize);
                    self.rset((bp + w1) as usize, f);
                    self.rset((bp + w1 + 1) as usize, v);
                    return;
                }
            }
        }
        if let Val::Obj(o) = v {
            if w3 >= 65536 && ((w3 >> 16) as int) == self.ob(o).shape as int {
                // the cache's shape: o's own data property
                let f = slot_at(self.ob(o), (w3 & 65535) as usize);
                if ((w0 >> 8) & 255) != 0 || self.is_callable(&f) {
                    self.rset((bp + w1) as usize, f);
                    self.rset((bp + w1 + 1) as usize, v);
                    return;
                }
            }
            let site = rb + pc;
            if (site as usize) < self.rpic.len() {
                // a way that fits o's shape was learnt on that shape
                // without the property of its own
                let h = self.pic_probe(site, o);
                if h != -1 && (h < 0 || i32_at(&self.pic, (self.pic_way + 4) as usize) == 0) {
                    let f = if h >= 0 { slot_at(self.ob(h), i32_at(&self.pic, (self.pic_way + 3) as usize) as usize) } else { Val::Undef };
                    let fun = if let Val::Obj(fo) = f { self.ob(fo).class == C_FUNCTION } else { false };
                    if fun || ((w0 >> 8) & 255) != 0 || self.is_callable(&f) {
                        self.rset((bp + w1) as usize, f);
                        self.rset((bp + w1 + 1) as usize, v);
                        return;
                    }
                }
            }
        }
        self.r_get_method_slow(bp, rb, fi, pc, w0, w1, v, w3);
    }

    /// Keeps a method found one prototype up (holder h, slot) in the
    /// site's mcache entry, taking a new entry the first time.
    fn mc_learn(&mut self, site: int, o: int, h: int, slot: i32, w3: int, f: &Val) {
        let fun = if let Val::Obj(fo) = f { self.ob(*fo).class == C_FUNCTION || self.ob(*fo).class == C_NATIVE } else { false };
        if !fun || slot < 0 {
            return;
        }
        let k = if w3 <= MC_BASE { (MC_BASE - w3) as usize } else { self.mcache.len() / 5 };
        if k >= 100000000 {
            return;
        }
        while self.mcache.len() < k * 5 + 5 {
            self.mcache.push(-1);
        }
        self.mcache[k * 5] = self.ob(o).shape;
        self.mcache[k * 5 + 1] = self.shape_epoch as i32;
        self.mcache[k * 5 + 2] = h as i32;
        self.mcache[k * 5 + 3] = self.ob(h).shape;
        self.mcache[k * 5 + 4] = slot;
        self.rcode[site as usize].c = (MC_BASE - k as int) as i32;
    }

    #[inline(never)]
    fn r_get_method_slow(&mut self, bp: int, rb: int, fi: usize, pc: int, w0: int, w1: int, v: Val, w3: int) {
        self.frames[fi].pc = pc + 1;
        let pi = self.frames[fi].proto as usize;
        let atom = self.protos[pi].code[pc as usize].a as int;
        let mut f = Val::Undef;
        let mut done = false;
        if let Val::Obj(o) = &v {
            let o = *o;
            let slot = self.find(o, atom);
            if slot >= 0 && (self.attr_at(o, slot) & P_ACCESSOR) == 0 {
                f = self.objs[o as usize].get_slot(slot as usize).clone();
                done = true;
                let nc = self.ic_of(o, slot);
                if nc >= 65536 {
                    self.rcode[(rb + pc) as usize].c = nc as i32;
                }
            } else if slot < 0 {
                let h = self.pic_get(rb + pc, o, atom);
                if h != -1 && (h < 0 || self.pic[(self.pic_way + 4) as usize] == 0) {
                    f = if h >= 0 { self.objs[h as usize].get_slot(self.pic[(self.pic_way + 3) as usize] as usize) } else { Val::Undef };
                    done = true;
                    if h >= 0 && (w3 == 0 || w3 <= MC_BASE) && self.ob(o).proto == h as i32 {
                        self.mc_learn(rb + pc, o, h, self.pic[(self.pic_way + 3) as usize], w3, &f);
                    }
                }
            }
        }
        if !done {
            if matches!(v, Val::Str(_)) || (matches!(v, Val::Num(_)) && self.number_proto >= 0) {
                if atom != A_LENGTH {
                    // String.prototype's (Number.prototype's) own method:
                    // r_get_method's primitive path next time
                    let sp = if matches!(v, Val::Num(_)) { self.number_proto } else { self.string_proto };
                    let slot = self.find(sp, atom);
                    if slot >= 0 && slot < 65536 && (self.attr_at(sp, slot) & P_ACCESSOR) == 0 && self.is_callable(&self.objs[sp as usize].get_slot(slot as usize)) {
                        self.rcode[(rb + pc) as usize].c = (-16 - slot) as i32;
                    }
                }
            }
            f = self.get(&v, atom);
        }
        if !self.throwing && ((w0 >> 8) & 255) == 0 && !self.is_callable(&f) {
            let name = self.atom_str(atom);
            self.throw_type(format!("{} is not a function", name).as_str());
        }
        self.stack[(bp + w1) as usize] = f;
        self.stack[(bp + w1 + 1) as usize] = v;
    }

    /// R_CALL of what is not a plain JS function: true when the frames
    /// changed (the caller goes on through 'frames); else the result is in
    /// its slot and the stack is the frame's again.
    #[inline(never)]
    pub fn r_call_slow(&mut self, fo: int, fpos: int, argc: int, bp: int, w3: int, want: usize) -> bool {
        let dst = if w3 > 0 { bp + w3 - 1 } else { fpos };
        // a hot native (Math, hasOwnProperty, push, Map get...) in place:
        // its result to dst, its operands' slots emptied as the
        // truncation below would
        if fo >= 0 && self.ob(fo).class == C_NATIVE {
            let id = self.ob(fo).func as int;
            if id != crate::builtins::NF_CALL && id != crate::builtins::NF_APPLY && self.native_fast(id, fpos, argc) {
                if self.throwing {
                    return false;
                }
                let v = self.stack[fpos as usize];
                let mut i = fpos;
                while i < fpos + argc + 2 {
                    self.stack[i as usize] = Val::Undef;
                    i += 1;
                }
                self.stack[dst as usize] = v;
                return false;
            }
            if id != crate::builtins::NF_CALL && id != crate::builtins::NF_APPLY && id != crate::typed::NF_FAST_WRAP && !self.throwing {
                // the native's full call, as call_op makes it (native_fast
                // not tried a second time); its result to dst
                self.stack.truncate((fpos + argc + 2) as usize);
                let mut args: Vec<Val> = take_args(&mut self.arg_pool);
                let mut i = 0;
                while i < argc {
                    args.push(self.stack[(fpos + 2 + i) as usize]);
                    i += 1;
                }
                let this = self.stack[(fpos + 1) as usize];
                self.stack.truncate(fpos as usize);
                self.native_depth += 1;
                // a String.prototype / Number.prototype method of primitive
                // operands: call_native's checks (array, proxy, typed...)
                // are not its, call_native2 answers it
                let mut direct = (id >= crate::builtins::NF_SP_CHARAT && id <= crate::builtins::NF_SP_ITERATOR) || (id >= crate::builtins::NF_NP_TOSTRING && id <= crate::builtins::NF_NP_TOEXPONENTIAL);
                let mut k = 0;
                while direct && k < args.len() {
                    if obj_of(&args[k]) >= 0 {
                        direct = false;
                    }
                    k += 1;
                }
                let r = if direct { self.call_native2(id, fo, this, &args, false, Val::Undef) } else { self.call_native(id, fo, this, &args, false, Val::Undef) };
                self.native_depth -= 1;
                give_args(&mut self.arg_pool, args);
                self.stack.push(r);
                if self.throwing {
                    return false;
                }
                if self.stack.len() < want {
                    grow_stack(&mut self.stack, want);
                }
                if dst != fpos {
                    self.stack[dst as usize] = r;
                }
                return false;
            }
        }
        let nf = self.frames.len();
        self.stack.truncate((fpos + argc + 2) as usize);
        let mut fo = fo;
        let mut fpos = fpos;
        let mut argc = argc;
        if fo >= 0 && self.ob(fo).class == C_NATIVE && (self.ob(fo).func as int) == crate::typed::NF_FAST_WRAP && (self.ob(fo).env as int) >= 0 {
            // a __fastWrap method's other cases: its prelude function, entered here
            fo = self.ob(fo).env as int;
            self.stack[fpos as usize] = Val::Obj(fo);
        }
        while fo >= 0 && self.ob(fo).class == C_NATIVE && (self.ob(fo).func as int) == crate::builtins::NF_CALL {
            // f.call(this, a…): [call, f, this, a…] is already [f, this, a…]
            // one slot on, so f is called there (its result still goes to dst)
            if argc == 0 {
                self.stack.push(Val::Undef);
                argc = 1;
            }
            fo = obj_of(&self.stack[(fpos + 1) as usize]);
            fpos += 1;
            argc -= 1;
        }
        if fo >= 0 && self.ob(fo).class == C_NATIVE && (self.ob(fo).func as int) == crate::builtins::NF_APPLY {
            // f.apply(this, arr) of a plain JS f: [apply, f, this, arr] as
            // [f, this, a…] one slot on, so f is entered in place below
            let g = obj_of(&self.stack[(fpos + 1) as usize]);
            if g >= 0 && self.ob(g).class == C_FUNCTION && fpos + 1 != dst {
                let arr = if argc >= 2 { self.stack[(fpos + 3) as usize] } else { Val::Undef };
                if argc < 1 {
                    self.stack.push(Val::Undef);
                }
                let items = self.array_like_to_vec(&arr);
                if self.throwing {
                    return false;
                }
                self.stack.truncate((fpos + 3) as usize);
                let n = items.len() as int;
                for x in items {
                    self.stack.push(x);
                }
                fo = g;
                fpos += 1;
                argc = n;
            }
        }
        if fo >= 0 && self.ob(fo).class == C_FUNCTION && fpos != dst {
            let tp = self.ob(fo).func as usize;
            let plain = {
                let p = &self.protos[tp];
                p.rest < 0 && !p.class_ctor && !p.generator && !p.is_async
            };
            if plain {
                self.renter(fo, fpos, argc, dst);
                return true;
            }
        }
        if fo >= 0 && self.ob(fo).class == C_FUNCTION {
            self.enter_function(fo, argc, false, Val::Undef);
            self.call_result(nf, fpos, dst, want);
            return true;
        }
        if self.call_op(argc) {
            self.call_result(nf, fpos, dst, want);
            return true;
        }
        if self.throwing {
            return false;
        }
        if self.stack.len() < want {
            grow_stack(&mut self.stack, want);
        }
        if dst != fpos {
            let v = self.stack[fpos as usize];
            self.stack[dst as usize] = v;
        }
        false
    }

    /// A constructor's return of a non-object: `this`, or true when it
    /// threw (a derived constructor's).
    #[inline(never)]
    fn r_ret_construct(&mut self, v: &mut Val, fi: usize, pc: int) -> bool {
        if !matches!(*v, Val::Undef) && self.protos[self.frames[fi].proto as usize].derived {
            self.frames[fi].pc = pc + 1;
            self.throw_type("Derived constructors may only return object or undefined");
            return true;
        }
        *v = self.frames[fi].this_val.clone();
        false
    }

    /// R_NEW_ARRAY: the n items from slot w1 on into a new array, which
    /// goes to slot w1.
    #[inline(never)]
    pub fn r_new_array(&mut self, bp: int, w1: int, n: int) {
        let p = self.array_proto;
        let a = self.alloc(C_ARRAY, p);
        let e = self.elems_id(a);
        reserve_vals(&mut self.evecs[e].v, n as usize);
        let from = bp + w1;
        let mut i = from;
        while i < from + n {
            let v = self.stack[i as usize];
            // push_elem without looking up the vector each time
            self.inc(&v);
            self.evecs[e].v.push(v);
            i += 1;
        }
        self.rset(from as usize, Val::Obj(a));
    }

    #[inline(never)]
    pub fn r_step_push(&mut self, pi: usize, bp: int, fi: usize, pc: int, w1: int) {
        let op = self.protos[pi].code[pc as usize];
        let mut p2 = pc + 1;
        self.frames[fi].pc = p2;
        self.step(op, pi, bp, fi, &mut p2);
        if !self.throwing {
            let v = self.stack.pop().unwrap();
            self.stack[(bp + w1) as usize] = v;
        }
    }

    #[inline(never)]
    pub fn r_step_k(&mut self, pi: usize, bp: int, fi: usize, pc: int, w0: int, w1: int, w2: int, w3: int) {
        let k = (w0 >> 8) & 255;
        if k == 2 && (self.protos[pi].code[pc as usize].code as int) == OP_INSTANCEOF {
            let a = *self.rget(bp, w2);
            let b = *self.rget(bp, w3);
            let r = self.instanceof_fast(&a, &b);
            if r >= 0 {
                self.stack[(bp + w1) as usize] = Val::Bool(r == 1);
                return;
            }
        }
        if k == 2 && (self.protos[pi].code[pc as usize].code as int) == OP_IN {
            // `i in arr` for an element of a plain array: no stack trip
            let ov = self.rget(bp, w3).clone();
            let nv = self.rget(bp, w2).clone();
            if let Val::Obj(o) = ov {
                if let Val::Num(n) = nv {
                    let i = n as int;
                    if (i as double) == n && i >= 0 && self.objs[o as usize].class == C_ARRAY && i < self.evecs[self.objs[o as usize].elems as usize].v.len() as int && !self.is_hole(o, i) {
                        self.stack[(bp + w1) as usize] = Val::Bool(true);
                        return;
                    }
                }
            }
        }
        if k >= 1 {
            let x = self.rget(bp, w2).clone();
            self.stack.push(x);
        }
        if k >= 2 {
            let y = self.rget(bp, w3).clone();
            self.stack.push(y);
        }
        let op = self.protos[pi].code[pc as usize];
        let mut p2 = pc + 1;
        self.frames[fi].pc = p2;
        self.step(op, pi, bp, fi, &mut p2);
        if !self.throwing {
            let v = self.stack.pop().unwrap();
            self.stack[(bp + w1) as usize] = v;
        }
    }

    /// The next value of the iterator in slot w1 into slot w3; false when
    /// it is done (or threw).
    #[inline(never)]
    pub fn r_iter_next(&mut self, bp: int, fi: usize, pc: int, w1: int, w3: int) -> bool {
        let it = self.stack[(bp + w1) as usize].clone();
        let o = obj_of(&it);
        let src_obj = if o >= 0 { self.colds[self.objs[o as usize].cold as usize].home } else { -1 };
        self.frames[fi].pc = pc + 1;
        loop {
            match self.iter_next(&it) {
                Some(v) => {
                    if src_obj >= 0 && self.objs[o as usize].class == C_ITER && self.for_in_recheck(o) {
                        // a key deleted since the loop began is skipped
                        if !self.has_property(src_obj, &v) {
                            continue;
                        }
                    }
                    self.stack[(bp + w3) as usize] = v;
                    return true;
                }
                None => {
                    return false;
                }
            }
        }
    }

    /// A stack op on the frame's window: where to go on, -1 when the
    /// frames changed.
    #[inline(never)]
    pub fn r_win(&mut self, pi: usize, bp: int, fi: usize, pc: int, w1: int, want: usize) -> int {
        let op = self.protos[pi].code[pc as usize];
        let code = op.code as int;
        if code == OP_TRY || code == OP_END_TRY {
            // the handler's stack height is the window's top (what the
            // truncation below would leave); the registers stay as they are
            if code == OP_TRY {
                let env = self.frames[fi].env;
                self.handlers.push(Handler { frame: fi as int, catch_pc: (op.a as int), sp: bp + w1, env: env });
            } else {
                self.handlers.pop();
            }
            self.frames[fi].pc = pc + 1;
            return pc + 1;
        }
        self.stack.truncate((bp + w1) as usize);
        let mut p2 = pc + 1;
        self.frames[fi].pc = p2;
        let mut changed = false;
        // a small BigInt literal: its text (on the window's top) read in place
        let mut done = false;
        if code == OP_GET_PRIVATE && op.b != -2 && w1 > 0 {
            // obj.#x as an own data property (Vm::priv_get's first case)
            let top = (bp + w1 - 1) as usize;
            if let Val::Obj(o) = self.stack[top] {
                let s = self.find(o, op.a as int);
                if s >= 0 && (self.attr_at(o, s) & P_ACCESSOR) == 0 {
                    self.stack[top] = self.objs[o as usize].get_slot(s as usize);
                    done = true;
                }
            }
        } else if code == OP_SET_PRIVATE && op.c != 1 && w1 > 1 {
            // obj.#x = v for an own data property (Vm::priv_set's case)
            let top = (bp + w1 - 1) as usize;
            if let Val::Obj(o) = self.stack[top - 1] {
                let s = self.find(o, op.a as int);
                if s >= 0 && (self.attr_at(o, s) & P_ACCESSOR) == 0 {
                    let val = self.stack[top];
                    self.put_slot(o, s, val);
                    self.stack.truncate(top);
                    self.stack[top - 1] = val;
                    done = true;
                }
            }
        }
        if done {
        } else if code == OP_TOSTR && w1 > 0 && !matches!(self.stack[(bp + w1 - 1) as usize], Val::Obj(_)) {
            // a template's primitive substitution: as is (Vm::quick_op)
            done = true;
        } else if code == OP_BIGINT && w1 > 0 {
            let top = (bp + w1 - 1) as usize;
            if let Val::Str(h) = self.stack[top] {
                let n = crate::bigint::small_big_of(self.sref(h));
                if n < crate::bigint::small_big_limit() {
                    self.stack[top] = self.bigint_small(n);
                    done = true;
                }
            }
        }
        if !done {
            changed = self.win_op(op, pi, bp, fi, &mut p2);
        }
        if self.throwing {
            return 0;
        }
        if changed {
            return -1;
        }
        while self.stack.len() < want {
            self.stack.push(Val::Undef);
        }
        if self.stack.len() > want {
            self.stack.truncate(want);
        }
        p2
    }

    /// A call of a JS function without `arguments`, a rest parameter or a
    /// class constructor's checks, from register code: [f, this, args…] at
    /// fpos. The callee's frame starts after `this` and the stack ends
    /// where the frame does.
    pub fn renter(&mut self, fo: int, fpos: int, argc: int, ret: int) -> bool {
        let pi = self.ob(fo).func as usize;
        if proto_at(&self.protos, pi).rstate == 0 && !self.renter_translate(pi) {
            return false;
        }
        let (nparams, nslots, fsize, env_size, arrow, strict, entry, uses_args) = {
            let p = proto_at(&self.protos, pi);
            (p.nparams, p.nslots, p.fsize, p.env_size, p.arrow, p.strict, p.rentry, p.uses_args)
        };
        let bp = fpos + 2;
        // `arguments`: made from the arguments as they came, before the
        // locals past the parameters are cleared
        let args_obj = if uses_args { self.make_args(fo, bp, argc, strict) } else { -1 };
        let end = (bp + fsize) as usize;
        // the parameters that came stay and the other locals start
        // undefined; the slots after them are written before they are read.
        // What lies past the frame's end is left (a frame's return does not
        // shorten the stack); a collection cuts it first (rgc)
        let keep = bp + if argc < nparams { argc } else { nparams };
        let mut i = keep;
        let have = self.stack.len() as int;
        let upto = if bp + nslots < have { bp + nslots } else { have };
        while i < upto {
            self.rset(i as usize, Val::Undef);
            i += 1;
        }
        if self.stack.len() < end {
            grow_stack(&mut self.stack, end);
        }
        let mut nt = Val::Undef;
        let this_val = if arrow {
            let c = self.ob(fo).cold as usize;
            if !self.colds[c].elems2.is_empty() {
                nt = self.colds[c].elems2[0];
            }
            self.colds[c].prim
        } else {
            let t = *val_at(&self.stack, (bp - 1) as usize);
            if strict || is_obj(&t) {
                t
            } else if matches!(t, Val::Undef) {
                // a plain call f(): the global object, no call out
                Val::Obj(self.global)
            } else {
                self.sloppy_this(t)
            }
        };
        let mut env = self.ob(fo).env as int;
        if env_size > 0 {
            env = self.renter_env(env, env_size);
        }
        // translated code is unpacked already (renter_translate): only a
        // new function's code is packed (pack_code), before it first runs
        self.frames.push(Frame {
            proto: pi as int,
            fobj: fo,
            pc: entry,
            bp: bp,
            env: env,
            this_val: this_val,
            new_target: nt,
            args_obj: args_obj,
            construct: false,
            gen: -1,
            ret: ret,
            mv_from: -1,
            mv_to: -1,
        });
        true
    }

    #[inline(never)]
    fn renter_translate(&mut self, pi: usize) -> bool {
        unpack_code(&mut self.protos[pi]);
        let e = translate(&mut self.protos[pi], &mut self.rcode, &mut self.kvals, &mut self.rtab);
        if !e.is_empty() {
            self.throw_type(e.as_str());
            return false;
        }
        true
    }

    /// A sloppy function's `this`: the global object for null and
    /// undefined, a wrapper for a primitive.
    #[inline(never)]
    fn sloppy_this(&mut self, t: Val) -> Val {
        if matches!(t, Val::Undef) || matches!(t, Val::Null) {
            return Val::Obj(self.global);
        }
        let bo = self.to_object(&t);
        Val::Obj(bo)
    }

    /// A new scope of `n` undefined slots inside `outer`.
    #[inline(never)]
    fn renter_env(&mut self, outer: int, n: int) -> int {
        let e = self.alloc(C_ENV, outer);
        let mut k = 0;
        while k < n {
            self.push_elem(e, Val::Undef);
            k += 1;
        }
        e
    }

    /// After a call that may have pushed the callee's frame (nf is the
    /// frame count before it): its result goes to dst, by the frame's
    /// return or here when it is in the slot of f already.
    pub fn call_result(&mut self, nf: usize, fpos: int, dst: int, want: usize) {
        if self.throwing || dst == fpos {
            return;
        }
        if self.frames.len() > nf {
            let p = &self.protos[self.frames[nf].proto as usize];
            if self.frames[nf].gen < 0 && !p.generator && !p.is_async {
                self.frames[nf].ret = dst;
                return;
            }
            // a coroutine's frame leaves its result in the slot of f
            // without a return: it moves when the caller goes on
            self.frames[nf - 1].mv_from = fpos;
            self.frames[nf - 1].mv_to = dst;
            return;
        }
        while self.stack.len() < want {
            self.stack.push(Val::Undef);
        }
        let v = self.stack[fpos as usize].clone();
        self.stack[dst as usize] = v;
    }

    /// A collection at a safe point of register code: the stack past the
    /// running frame's end holds only what returned frames left, so it goes
    /// first and keeps nothing alive.
    pub fn rgc(&mut self, end: usize) {
        if self.stack.len() > end {
            self.stack.truncate(end);
        }
        self.gc();
    }

    /// A comparison decided by the values' kinds alone: 1 true, 0 false, -1 when
    /// it needs rcompare. Strict equality always decides; loose equality when
    /// one side is null or undefined.
    #[inline(always)]
    fn fast_cmp(&self, code: int, a: &Val, b: &Val) -> int {
        let eq: int = if code == OP_SEQ || code == OP_SNE {
            match (a, b) {
                (Val::Num(x), Val::Num(y)) => if *x == *y { 1 } else { 0 },
                (Val::Obj(x), Val::Obj(y)) => if *x == *y { 1 } else { 0 },
                (Val::Big(x), Val::Big(y)) => if *x == *y { 1 } else { 0 },
                (Val::Undef, Val::Undef) | (Val::Null, Val::Null) => 1,
                (Val::Bool(x), Val::Bool(y)) => if *x == *y { 1 } else { 0 },
                (Val::Str(x), Val::Str(y)) => if *x == *y || self.sref(*x) == self.sref(*y) { 1 } else { 0 },
                _ => 0,
            }
        } else if code == OP_EQ || code == OP_NE {
            match (a, b) {
                (Val::Undef, Val::Undef) | (Val::Null, Val::Null) | (Val::Undef, Val::Null) | (Val::Null, Val::Undef) => 1,
                (Val::Undef, Val::Obj(_)) | (Val::Null, Val::Obj(_)) | (Val::Obj(_), Val::Undef) | (Val::Obj(_), Val::Null) => 0,
                (Val::Undef, Val::Num(_)) | (Val::Null, Val::Num(_)) | (Val::Num(_), Val::Undef) | (Val::Num(_), Val::Null) => 0,
                (Val::Undef, Val::Str(_)) | (Val::Null, Val::Str(_)) | (Val::Str(_), Val::Undef) | (Val::Str(_), Val::Null) => 0,
                (Val::Undef, Val::Bool(_)) | (Val::Null, Val::Bool(_)) | (Val::Bool(_), Val::Undef) | (Val::Bool(_), Val::Null) => 0,
                (Val::Str(x), Val::Str(y)) => if *x == *y || self.sref(*x) == self.sref(*y) { 1 } else { 0 },
                _ => -1,
            }
        } else {
            -1
        };
        if eq < 0 {
            return -1;
        }
        if code == OP_SEQ || code == OP_EQ {
            eq
        } else {
            1 - eq
        }
    }

    /// A comparison of two registers: strict and null equality here, the
    /// rest by rcompare.
    #[inline(always)]
    pub fn rcmp(&mut self, code: int, pi: usize, bp: int, x: int, y: int, fi: usize, pc: int) -> bool {
        let q = self.fast_cmp(code, self.rget(bp, x), self.rget(bp, y));
        if q >= 0 {
            return q == 1;
        }
        self.rcompare(code, pi, bp, x, y, fi, pc)
    }

    /// A comparison of two registers (OP_LT … OP_SNE).
    #[inline]
    pub fn rcompare(&mut self, code: int, _pi: usize, bp: int, x: int, y: int, fi: usize, pc: int) -> bool {
        let mut num = false;
        let mut r = false;
        if let (Val::Num(a), Val::Num(b)) = (self.rget(bp, x), self.rget(bp, y)) {
            num = true;
            r = match code {
                OP_LT => *a < *b,
                OP_GT => *a > *b,
                OP_LE => *a <= *b,
                OP_GE => *a >= *b,
                OP_SEQ | OP_EQ => *a == *b,
                _ => *a != *b,
            };
        }
        if num {
            return r;
        }
        let strict = code == OP_SEQ || code == OP_SNE;
        if strict {
            let (oa, ob) = (is_obj(self.rget(bp, x)), is_obj(self.rget(bp, y)));
            if !oa || !ob {
                let same = !oa && !ob && self.strict_equals(self.rget(bp, x), self.rget(bp, y));
                return same == (code == OP_SEQ);
            }
        }
        let q = if strict || code == OP_EQ || code == OP_NE { quick_eq(self.rget(bp, x), self.rget(bp, y), strict) } else { -1 };
        if q >= 0 {
            return (q == 1) == (code == OP_SEQ || code == OP_EQ);
        }
        let a = self.rget(bp, x).clone();
        let b = self.rget(bp, y).clone();
        self.frames[fi].pc = pc + 1;
        self.compare(code, &a, &b)
    }
}


// ---- the VM core's unchecked access (Rust): the callers keep indices in
// range, debug builds check them. The other targets index as usual.

/// `v[i]`.
/// Property slot `i` of `o` (an inline cache's slot: in range).
#[ranger::target(rust)]
#[inline(always)]
pub fn slot_at(o: &JsObj, i: usize) -> Val {
    if i == 0 {
        o.inl0
    } else if i == 1 {
        o.inl1
    } else {
        *val_at(&o.vals, i - 2)
    }
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn slot_at(o: &JsObj, i: usize) -> Val {
    o.get_slot(i)
}

#[ranger::target(rust)]
#[inline(always)]
pub fn val_at(v: &Vec<Val>, i: usize) -> &Val {
    debug_assert!(i < v.len(), "value index past its vector");
    // SAFETY: the callers' invariants (rget, env_slot, inline caches)
    unsafe { v.get_unchecked(i) }
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn val_at(v: &Vec<Val>, i: usize) -> &Val {
    &v[i]
}

/// `v[i] = x`.
#[ranger::target(rust)]
#[inline(always)]
pub fn val_put(v: &mut Vec<Val>, i: usize, x: Val) {
    debug_assert!(i < v.len(), "value index past its vector");
    // SAFETY: as val_at; Val is Copy, so the old value needs no drop
    unsafe {
        *v.get_unchecked_mut(i) = x;
    }
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn val_put(v: &mut Vec<Val>, i: usize, x: Val) {
    v[i] = x;
}

/// `v[i]` of the register code.
#[ranger::target(rust)]
#[inline(always)]
pub fn op_at(v: &Vec<Op>, i: usize) -> Op {
    debug_assert!(i < v.len(), "pc past the register code");
    // SAFETY: pc stays inside the running function's translated code
    unsafe { *v.get_unchecked(i) }
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn op_at(v: &Vec<Op>, i: usize) -> Op {
    v[i]
}

/// `v[i]` of the frames.
#[ranger::target(rust)]
#[inline(always)]
pub fn frame_at(v: &Vec<Frame>, i: usize) -> &Frame {
    debug_assert!(i < v.len(), "frame index past frames");
    // SAFETY: rrun's fi is frames.len() - 1, refreshed whenever a frame
    // comes or goes
    unsafe { v.get_unchecked(i) }
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn frame_at(v: &Vec<Frame>, i: usize) -> &Frame {
    &v[i]
}

/// `v[i]` of the frames, to write.
#[ranger::target(rust)]
#[inline(always)]
pub fn frame_mut(v: &mut Vec<Frame>, i: usize) -> &mut Frame {
    debug_assert!(i < v.len(), "frame index past frames");
    // SAFETY: as frame_at
    unsafe { v.get_unchecked_mut(i) }
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn frame_mut(v: &mut Vec<Frame>, i: usize) -> &mut Frame {
    &mut v[i]
}

/// `v[i]` of the functions.
#[ranger::target(rust)]
#[inline(always)]
pub fn proto_at(v: &Vec<Proto>, i: usize) -> &Proto {
    debug_assert!(i < v.len(), "function index past protos");
    // SAFETY: a frame's proto and a function object's func name compiled
    // functions, which are never removed
    unsafe { v.get_unchecked(i) }
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn proto_at(v: &Vec<Proto>, i: usize) -> &Proto {
    &v[i]
}

/// `v[i]` of the shapes.
#[ranger::target(rust)]
#[inline(always)]
pub fn shape_at(v: &Vec<Shape>, i: usize) -> &Shape {
    debug_assert!(i < v.len(), "shape id past shapes");
    // SAFETY: an object's shape and a shape's kref name live shapes
    unsafe { v.get_unchecked(i) }
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn shape_at(v: &Vec<Shape>, i: usize) -> &Shape {
    &v[i]
}

/// `v[i]` of an int vector (a shape's keys or attributes).
#[ranger::target(rust)]
#[inline(always)]
pub fn int_at(v: &Vec<int>, i: usize) -> &int {
    debug_assert!(i < v.len(), "index past an int vector");
    // SAFETY: callers check i against the shape's key count, which the
    // holder's keys and attrs cover
    unsafe { v.get_unchecked(i) }
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn int_at(v: &Vec<int>, i: usize) -> &int {
    &v[i]
}

/// `v[i]` of a cache table (pic, rpic, mcache).
#[ranger::target(rust)]
#[inline(always)]
pub fn i32_at(v: &Vec<i32>, i: usize) -> i32 {
    debug_assert!(i < v.len(), "index past a cache table");
    // SAFETY: a site names whole entries only: pic_learn and mc_learn
    // push every int of an entry before the site (rpic, the op's c) gets
    // its index, and the tables never shrink while register code lives
    unsafe { *v.get_unchecked(i) }
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn i32_at(v: &Vec<i32>, i: usize) -> i32 {
    v[i]
}

/// `v[i]` of the objects.
#[ranger::target(rust)]
#[inline(always)]
pub fn obj_at(v: &Vec<JsObj>, i: usize) -> &JsObj {
    debug_assert!(i < v.len(), "object id past objs");
    // SAFETY: ids in live values name allocated slots
    unsafe { v.get_unchecked(i) }
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn obj_at(v: &Vec<JsObj>, i: usize) -> &JsObj {
    &v[i]
}

/// `v[i]` of the objects, to write.
#[ranger::target(rust)]
#[inline(always)]
pub fn obj_mut(v: &mut Vec<JsObj>, i: usize) -> &mut JsObj {
    debug_assert!(i < v.len(), "object id past objs");
    // SAFETY: as obj_at
    unsafe { v.get_unchecked_mut(i) }
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn obj_mut(v: &mut Vec<JsObj>, i: usize) -> &mut JsObj {
    &mut v[i]
}

/// `v[i]` of the Colds.
#[ranger::target(rust)]
#[inline(always)]
pub fn cold_at(v: &Vec<Cold>, i: usize) -> &Cold {
    debug_assert!(i < v.len(), "cold past colds");
    // SAFETY: an object's cold index names an allocated Cold (0: the empty one)
    unsafe { v.get_unchecked(i) }
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn cold_at(v: &Vec<Cold>, i: usize) -> &Cold {
    &v[i]
}

/// `v[i]` of the element vectors.
#[ranger::target(rust)]
#[inline(always)]
pub fn evec_at(v: &Vec<Elems>, i: usize) -> &Elems {
    debug_assert!(i < v.len(), "element vector past evecs");
    // SAFETY: an object's elems index names a live element vector
    unsafe { v.get_unchecked(i) }
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn evec_at(v: &Vec<Elems>, i: usize) -> &Elems {
    &v[i]
}

/// `v[i]` of the element vectors, to write.
#[ranger::target(rust)]
#[inline(always)]
pub fn evec_mut(v: &mut Vec<Elems>, i: usize) -> &mut Elems {
    debug_assert!(i < v.len(), "element vector past evecs");
    // SAFETY: as evec_at
    unsafe { v.get_unchecked_mut(i) }
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn evec_mut(v: &mut Vec<Elems>, i: usize) -> &mut Elems {
    &mut v[i]
}

/// Whether a value vector has no room left (Rust: grow_vals sizes it).
#[ranger::target(rust)]
#[inline(always)]
pub fn vals_full(v: &Vec<Val>) -> bool {
    v.len() == v.capacity()
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn vals_full(_v: &Vec<Val>) -> bool {
    false
}

/// rrun met an op translate() does not write: never in Rust (debug builds
/// stop); the other targets throw after it.
#[ranger::target(rust)]
#[inline(always)]
pub fn unknown_op() {
    debug_assert!(false, "register code: unknown instruction");
    // SAFETY: translate() writes only the ops rrun matches
    unsafe { std::hint::unreachable_unchecked() }
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn unknown_op() {}

/// Runs the function's compiled code from pc (wjit.rs); the pc to go on at,
/// or -1 - pc when its budget ran out. Only Rust compiles; elsewhere the
/// interpreter goes on at pc.
#[ranger::target(rust)]
pub fn jit_enter(vm: &mut Vm, pi: usize, bp: int, pc: int, budget: int, head: bool) -> int {
    crate::wjit::enter(vm, pi, bp, pc, budget as i32, head)
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn jit_enter(_vm: &mut Vm, _pi: usize, _bp: int, pc: int, _budget: int, _head: bool) -> int {
    pc
}

/// Vm::note_nonnum's mark, kept by the JIT (wjit.rs note_nonnum, which
/// also sends code that speculated on the site back). Only Rust compiles.
#[ranger::target(rust)]
pub fn jit_note_nonnum(vm: &mut Vm, pi: usize, ri: int) {
    crate::wjit::note_nonnum(vm, pi, ri as usize);
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn jit_note_nonnum(_vm: &mut Vm, _pi: usize, _ri: int) {}

/// The machine has run to its end (no frames left): functions compiled
/// and waiting for their batch go to the host now (wjit.rs). Only Rust
/// compiles.
#[ranger::target(rust)]
pub fn jit_flush(vm: &mut Vm) {
    crate::wjit::flush_pending(vm);
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn jit_flush(_vm: &mut Vm) {}

/// A loop head whose compiled code went back to the interpreter before a
/// back edge this often (4 per such entry, 1 back per good one) is no
/// longer entered.
pub const JIT_MISS_MAX: int = 1024;

/// `a` (in int32 range, checked by the caller) truncated toward zero.
#[ranger::target(rust)]
#[inline(always)]
pub fn trunc_i32(a: double) -> int {
    debug_assert!(a >= -2147483648.0 && a <= 2147483647.0);
    // SAFETY: the caller checks that a is in int32 range (NaN fails it)
    unsafe { a.to_int_unchecked::<i32>() as int }
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn trunc_i32(a: double) -> int {
    a as int
}

/// A bit operation on two numbers outside rrun's int32 fast path (`>>>`,
/// uint32s): below 2^53 ToInt32 truncates and keeps the low 32 bits.
#[inline(never)]
pub fn bit_num(code: int, a: double, b: double) -> double {
    if !(a > -9007199254740992.0 && a < 9007199254740992.0 && b > -9007199254740992.0 && b < 9007199254740992.0) {
        return arith_num(code, a, b);
    }
    let (p, q) = (wrap32(a as int), wrap32(b as int));
    match code {
        OP_BAND => (p & q) as double,
        OP_BOR => (p | q) as double,
        OP_BXOR => (p ^ q) as double,
        OP_SHL => wrap32(p << (q & 31)) as double,
        OP_SHR => (p >> (q & 31)) as double,
        _ => ((p & 4294967295) >> (q & 31)) as double,
    }
}
