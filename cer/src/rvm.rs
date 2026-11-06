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

    /// Slot `k` of scope `e`, unchecked: the compiler resolved it against
    /// the scope the closure was made in.
    #[inline(always)]
    pub fn env_slot(&self, e: int, k: int) -> Val {
        let ev = self.ob(e).elems as usize;
        // a scope gets its slots when it is made (renter, PUSH_ENV) and
        // keeps them while a closure or frame refers to it
        *val_at(&evec_at(&self.evecs, ev).v, k as usize)
    }

    /// Register slot `i` (absolute) takes `v`, unchecked like rget.
    #[inline(always)]
    pub fn rset(&mut self, i: usize, v: Val) {
        // as rget
        val_put(&mut self.stack, i, v);
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
    fn r_add_env(&mut self, bp: int, fi: usize, pc: int, w0: int, w1: int, w2: int, w3: int) {
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
    fn r_get_method_elem(&mut self, bp: int, fi: usize, pc: int, w1: int, w2: int, w3: int) {
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
    /// 1 minus, 2 + 1, 3 - 1.
    #[inline(never)]
    fn r_unary_slow(&mut self, bp: int, fi: usize, pc: int, kind: int, w1: int, w2: int) {
        self.frames[fi].pc = pc + 1;
        let v = self.rget(bp, w2).clone();
        let r = if kind == 0 {
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
    fn r_check_fail(&mut self, pi: usize, kind: int, name: int) {
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
                let w1 = ins.a;
                let w2 = ins.b;
                let w3 = ins.c;
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
                            // bit operations on numbers that are int32 already
                            } else if code >= OP_BAND && code <= OP_SHR && a >= -2147483648.0 && a <= 2147483647.0 && b >= -2147483648.0 && b <= 2147483647.0 {
                                let (p, q) = (a as int, b as int);
                                r = Val::Num(match code {
                                    OP_BAND => (p & q) as double,
                                    OP_BOR => (p | q) as double,
                                    OP_BXOR => (p ^ q) as double,
                                    OP_SHL => wrap32(p << (q & 31)) as double,
                                    _ => (p >> (q & 31)) as double,
                                });
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
                        let t = if r { w0 >> 16 } else { w3 };
                        if (t as int) <= pc && self.gc_due() {
                            self.frames[fi].pc = (t as int);
                            self.rgc(want);
                        }
                        pc = (t as int);
                        if self.slicing && self.brk(fi, pc) {
                            return;
                        }
                        continue;
                    }
                    R_JUMP => {
                        if (w1 as int) <= pc && self.gc_due() {
                            self.frames[fi].pc = (w1 as int);
                            self.rgc(want);
                        }
                        pc = (w1 as int);
                        if self.slicing && self.brk(fi, pc) {
                            return;
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
                        if truthy(self.rget(bp, (w1 as int))) {
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
                        if truthy(self.rget(bp, (w1 as int))) {
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
                        if truthy(self.rget(bp, (w1 as int))) {
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
                        if truthy(self.rget(bp, (w1 as int))) {
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
                                t = self.rtab[(w2 + 1 + ((k as int) as i32)) as usize];
                            }
                        }
                        if t < 0 {
                            t = self.rtab[(w2 + 1 + (self.rtab[w2 as usize] as i32)) as usize];
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
                                if i >= 0 && (i as usize) < evec_at(&self.evecs, ev).v.len() && (i as double) == *n {
                                    r = val_at(&evec_at(&self.evecs, ev).v, i as usize).clone();
                                    done = true;
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
                        if o >= 0 && num {
                            self.inc(&val);
                            let ob = &mut self.objs[o as usize];
                            let mut old: int = -1;
                            let mut old_s: int = 0;
                            if ob.class == C_ARRAY {
                                let i = n as int;
                                let ev = ob.elems as usize;
                                if i >= 0 && (i as usize) < evec_at(&self.evecs, ev).v.len() && (i as double) == n && ob.pos != 2 {
                                    old = obj_of(val_at(&evec_at(&self.evecs, ev).v, i as usize));
                                    old_s = str_of(val_at(&evec_at(&self.evecs, ev).v, i as usize));
                                    val_put(&mut evec_mut(&mut self.evecs, ev).v, i as usize, val.clone());
                                    let oc = ob.cold as usize;
                                    if oc > 0 && (i as usize) < self.colds[oc].elems2.len() {
                                        self.colds[oc].elems2[i as usize] = Val::Bool(false);
                                    }
                                    done = true;
                                } else if i >= 0 && (i as usize) == self.evecs[ob.elems as usize].v.len() && (i as double) == n && ob.extensible {
                                    if ob.elems == 0 {
                                        if !self.efree.is_empty() {
                                            ob.elems = self.efree.pop().unwrap();
                                        } else {
                                            grow_evecs(&mut self.evecs);
                                            self.evecs.push(Elems { v: Vec::new() });
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
                        self.frames[fi].pc = (pc + 1 + ((w0 >> 16) as int));
                        if self.gc_due() {
                            self.rgc(want);
                        }
                        let fpos = bp + (w1 as int);
                        if ((w0 >> 8) & 255) == 1 {
                            self.stack[(fpos + 1) as usize] = Val::Undef;
                        }
                        let fo = obj_of(&self.stack[fpos as usize]);
                        if fo >= 0 && self.ob(fo).class == C_FUNCTION {
                            let fp = self.objs[fo as usize].func as usize;
                            let plain = {
                                let p = &self.protos[fp];
                                !p.uses_args && p.rest < 0 && !p.class_ctor
                            };
                            // the result goes to the slot of f, or to w3 - 1
                            let dst = if w3 > 0 { bp + (w3 as int) - 1 } else { fpos };
                            if plain {
                                let co = self.protos[fp].generator || self.protos[fp].is_async;
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
                                pc = self.frames[fi].pc;
                                want = (bp + self.protos[pi].fsize) as usize;
                                rb = self.protos[pi].rbase;
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
                            plain = !p.uses_args && p.rest < 0 && !p.class_ctor && p.constructible();
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
                            if !self.renter(fo, fpos, argc, fpos) {
                                continue 'frames;
                            }
                            fi = self.frames.len() - 1;
                            self.frames[fi].construct = true;
                            self.frames[fi].new_target = f;
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
                        if self.frames[fi].construct && !is_obj(&v) {
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
                            pi = self.frames[fi].proto as usize;
                            bp = self.frames[fi].bp;
                            want = (bp + self.protos[pi].fsize) as usize;
                            rb = self.protos[pi].rbase;
                            pc = self.frames[fi].pc;
                            let to = if fr.ret >= 0 { fr.ret as usize } else { fpos };
                            self.stack[to] = v;
                            self.grow_stack(want);
                            if self.slicing && self.frame_brk() {
                                return;
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
                        let e = self.get_env((w2 as int));
                        let v = self.env_slot(e, (w3 as int));
                        self.rset((bp + (w1 as int)) as usize, v);
                        if ((w0 >> 8) & 255) == 1 {
                            self.rset((bp + (w1 as int) + 1) as usize, Val::Undef);
                        }
                        pc = (pc + 1 + ((w0 >> 16) as int));
                        continue;
                    }
                    R_SET_ENV => {
                        let e = self.get_env((w2 as int));
                        let v = self.rget(bp, (w1 as int)).clone();
                        self.put_elem(e, (w3 as int), v);
                        pc = (pc + 1 + ((w0 >> 16) as int));
                        continue;
                    }
                    R_GET_GLOBAL => {
                        let g = self.global;
                        let c = self.protos[pi].code[pc as usize].c;
                        let slot = self.ic_slot(g, (w3 as int), (c as int), false);
                        if slot >= 0 {
                            let v = self.objs[g as usize].get_slot(slot as usize).clone();
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
                            self.r_unary_slow(bp, fi, pc, if neg { 1 } else { 0 }, (w1 as int), (w2 as int));
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
                        let r = !truthy(self.rget(bp, (w2 as int)));
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

    #[inline(never)]
    fn r_add_slow(&mut self, bp: int, fi: usize, pc: int, w1: int, w2: int, w3: int) {
        let a = self.rget(bp, w2).clone();
        let b = self.rget(bp, w3).clone();
        self.frames[fi].pc = pc + 1;
        let r = self.add_vals(&a, &b);
        if !self.throwing {
            self.stack[(bp + w1) as usize] = r;
        }
    }

    #[inline(never)]
    fn r_arith_slow(&mut self, code: int, bp: int, fi: usize, pc: int, w1: int, w2: int, w3: int) {
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
    fn r_get_prop_slow(&mut self, o: int, pi: usize, rb: int, bp: int, fi: usize, pc: int, w0: int, w1: int, w2: int) -> bool {
        let atom = self.ratom(pi, pc);
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
            let c = self.rcode[(rb + pc) as usize].c;
            let slot = self.ic_get(o, atom, (c as int), pi, pc + 1);
            self.rcode[(rb + pc) as usize].c = self.protos[pi].code[pc as usize].c;
            if slot >= 0 {
                let r = self.objs[o as usize].get_slot(slot as usize).clone();
                self.stack[(bp + w1) as usize] = r;
                return false;
            }
            if atom == A_LENGTH && self.objs[o as usize].class == C_ARRAY && self.find(o, atom) < 0 {
                let r = Val::Num(self.evecs[self.objs[o as usize].elems as usize].v.len() as double);
                self.stack[(bp + w1) as usize] = r;
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
                if go >= 0 && self.objs[go as usize].class == C_FUNCTION {
                    let gp = self.objs[go as usize].func as usize;
                    let plain = {
                        let p = &self.protos[gp];
                        !p.uses_args && p.rest < 0 && !p.class_ctor && !p.generator && !p.is_async
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
        let r = self.get(&v, atom);
        if !self.throwing {
            self.stack[(bp + w1) as usize] = r;
        }
        false
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
        let mut w: int = 0;
        while e0 >= 0 && w < PIC_WAYS {
            let b = ((e0 + w as i32) * PIC_W as i32) as usize;
            w += 1;
            if self.pic[b + 1] != self.shape_epoch as i32 || self.objs[o as usize].shape != self.pic[b] as i32 {
                continue;
            }
            let depth = self.pic[b + 2] as usize;
            let mut cur = o;
            let mut k: usize = 0;
            let mut ok = true;
            while k < depth {
                let pk = self.pic[b + 5 + 2 * k];
                if self.objs[cur as usize].proto != pk as i32 || self.objs[pk as usize].shape != self.pic[b + 6 + 2 * k] as i32 {
                    ok = false;
                    break;
                }
                cur = pk as int;
                k += 1;
            }
            if !ok {
                continue;
            }
            if self.pic[b + 3] >= 0 {
                self.pic_way = b as int;
                return cur;
            }
            if self.objs[cur as usize].proto < 0 {
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
        if !self.pic_plain(o, atom) || self.find(o, atom) >= 0 {
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
        c != C_PROXY && c != C_STRING && atom != A_LENGTH && atom != A_NAME && atom != A_PROTOTYPE
    }

    /// R_SET_PROP past its inline cache, or one that keeps its value.
    #[inline(never)]
    fn r_set_prop_slow(&mut self, o: int, pi: usize, rb: int, bp: int, fi: usize, pc: int, w0: int, w1: int, w2: int, w3: int) {
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
                let atom = self.ratom(pi, pc);
                let s = self.ic_set(o, atom, w3, pi, pc + 1);
                self.rcode[(rb + pc) as usize].c = self.protos[pi].code[pc as usize].c;
                s
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
    fn r_set_elem_slow(&mut self, bp: int, fi: usize, pc: int, w1: int, w2: int, val: Val) {
        let target = self.rget(bp, w1).clone();
        let kv = self.rget(bp, w2).clone();
        self.frames[fi].pc = pc + 1;
        self.set_elem(&target, &kv, val);
    }

    /// R_GET_METHOD: the method and its receiver into w1, w1 + 1; the
    /// chain cache answers here, the rest goes to r_get_method_slow.
    #[inline(never)]
    fn r_get_method(&mut self, bp: int, rb: int, fi: usize, pc: int, w0: int, w1: int, w2: int, w3: int) {
        let v = *self.rget(bp, w2);
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
            if (site as usize) < self.rpic.len() && self.rpic[site as usize] >= 0 {
                // the site's first way, a data property one prototype up
                // (a method on its class's prototype): checked here first
                let b = (self.rpic[site as usize] * (PIC_W as i32)) as usize;
                if b + 6 < self.pic.len() && self.pic[b] == self.ob(o).shape && self.pic[b + 1] == (self.shape_epoch as i32) && self.pic[b + 2] == 1 && self.pic[b + 3] >= 0 && self.pic[b + 4] == 0 {
                    let p = self.pic[b + 5];
                    if self.ob(o).proto == p && self.ob(p as int).shape == self.pic[b + 6] {
                        let f = slot_at(self.ob(p as int), self.pic[b + 3] as usize);
                        let fun = if let Val::Obj(fo) = f { self.ob(fo).class == C_FUNCTION } else { false };
                        if fun || ((w0 >> 8) & 255) != 0 || self.is_callable(&f) {
                            self.rset((bp + w1) as usize, f);
                            self.rset((bp + w1 + 1) as usize, v);
                            return;
                        }
                    }
                }
                // a way that fits o's shape was learnt on that shape
                // without the property of its own
                let h = self.pic_probe(site, o);
                if h != -1 && (h < 0 || self.pic[(self.pic_way + 4) as usize] == 0) {
                    let f = if h >= 0 { self.objs[h as usize].get_slot(self.pic[(self.pic_way + 3) as usize] as usize) } else { Val::Undef };
                    if ((w0 >> 8) & 255) != 0 || self.is_callable(&f) {
                        self.rset((bp + w1) as usize, f);
                        self.rset((bp + w1 + 1) as usize, v);
                        return;
                    }
                }
            }
        }
        self.r_get_method_slow(bp, rb, fi, pc, w0, w1, v, w3);
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
                }
            }
        }
        if !done {
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
    fn r_call_slow(&mut self, fo: int, fpos: int, argc: int, bp: int, w3: int, want: usize) -> bool {
        let dst = if w3 > 0 { bp + w3 - 1 } else { fpos };
        let nf = self.frames.len();
        self.stack.truncate((fpos + argc + 2) as usize);
        let mut fo = fo;
        let mut fpos = fpos;
        let mut argc = argc;
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
        if fo >= 0 && self.ob(fo).class == C_FUNCTION && fpos != dst {
            let tp = self.ob(fo).func as usize;
            let plain = {
                let p = &self.protos[tp];
                !p.uses_args && p.rest < 0 && !p.class_ctor && !p.generator && !p.is_async
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
    fn r_new_array(&mut self, bp: int, w1: int, n: int) {
        let p = self.array_proto;
        let a = self.alloc(C_ARRAY, p);
        let e = self.elems_id(a);
        reserve_vals(&mut self.evecs[e].v, n as usize);
        let from = bp + w1;
        let mut i = from;
        while i < from + n {
            let v = self.stack[i as usize];
            self.push_elem(a, v);
            i += 1;
        }
        self.rset(from as usize, Val::Obj(a));
    }

    #[inline(never)]
    fn r_step_push(&mut self, pi: usize, bp: int, fi: usize, pc: int, w1: int) {
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
    fn r_step_k(&mut self, pi: usize, bp: int, fi: usize, pc: int, w0: int, w1: int, w2: int, w3: int) {
        let k = (w0 >> 8) & 255;
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
    fn r_iter_next(&mut self, bp: int, fi: usize, pc: int, w1: int, w3: int) -> bool {
        let it = self.stack[(bp + w1) as usize].clone();
        let o = obj_of(&it);
        let src_obj = if o >= 0 { self.colds[self.objs[o as usize].cold as usize].home } else { -1 };
        self.frames[fi].pc = pc + 1;
        loop {
            match self.iter_next(&it) {
                Some(v) => {
                    if src_obj >= 0 && self.objs[o as usize].class == C_ITER {
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
    fn r_win(&mut self, pi: usize, bp: int, fi: usize, pc: int, w1: int, want: usize) -> int {
        self.stack.truncate((bp + w1) as usize);
        let op = self.protos[pi].code[pc as usize];
        let mut p2 = pc + 1;
        self.frames[fi].pc = p2;
        let changed = self.win_op(op, pi, bp, fi, &mut p2);
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
        if self.protos[pi].rstate == 0 && !self.renter_translate(pi) {
            return false;
        }
        let (nparams, nslots, fsize, env_size, arrow, strict, entry) = {
            let p = &self.protos[pi];
            (p.nparams, p.nslots, p.fsize, p.env_size, p.arrow, p.strict, p.rentry)
        };
        let bp = fpos + 2;
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
            let t = self.stack[(bp - 1) as usize];
            if strict || is_obj(&t) { t } else { self.sloppy_this(t) }
        };
        let mut env = self.ob(fo).env as int;
        if env_size > 0 {
            env = self.renter_env(env, env_size);
        }
        unpack_code(&mut self.protos[pi]);
        self.frames.push(Frame {
            proto: pi as int,
            fobj: fo,
            pc: entry,
            bp: bp,
            env: env,
            this_val: this_val,
            new_target: nt,
            args_obj: -1,
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
