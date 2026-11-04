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
    #[inline]
    pub fn rget(&self, bp: int, x: int) -> &Val {
        if x >= 0 {
            &self.stack[(bp + x) as usize]
        } else {
            &self.kvals[(-1 - x) as usize]
        }
    }

    /// The atom of the property op at pc (its stack op's operand).
    pub fn ratom(&self, pi: usize, pc: int) -> int {
        let op = self.protos[pi].code[pc as usize];
        if op.code == OP_GET_LOCAL_PROP {
            op.b
        } else {
            op.a
        }
    }

    /// Stack ops the register code runs on the window that `step` does
    /// not do (run() does them); true when the frame changed.
    pub fn win_op(&mut self, op: Op, pi: usize, bp: int, fi: usize, pc: &mut int) -> bool {
        match op.code {
            OP_SET_PROP => {
                let val = self.pop();
                let target = self.pop();
                self.set(&target, op.a, val.clone());
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
                let ins = self.rcode[(rb + pc) as usize];
                let w0 = ins.code;
                let w1 = ins.a;
                let w2 = ins.b;
                let w3 = ins.c;
                let next = pc + 1 + (w0 >> 16);
                let mut npc = next;
                match w0 & 255 {
                    R_NOP => {
                        pc = next;
                        continue;
                    }
                    R_MOVE => {
                        let v = self.rget(bp, w2).clone();
                        self.stack[(bp + w1) as usize] = v;
                        pc = next;
                        continue;
                    }
                    R_LOAD2 => {
                        let v = self.rget(bp, w2).clone();
                        let u = self.rget(bp, w3).clone();
                        self.stack[(bp + w1) as usize] = v;
                        self.stack[(bp + w1 + 1) as usize] = u;
                        pc = next;
                        continue;
                    }
                    R_ADD => {
                        let mut r = Val::Undef;
                        let mut done = false;
                        if let (Val::Num(x), Val::Num(y)) = (self.rget(bp, w2), self.rget(bp, w3)) {
                            r = Val::Num(*x + *y);
                            done = true;
                        }
                        if done {
                            self.stack[(bp + w1) as usize] = r;
                            pc = next;
                            continue;
                        }
                        self.r_add_slow(bp, fi, pc, w1, w2, w3);
                    }
                    R_SUB | R_MUL | R_ARITH => {
                        let code = if (w0 & 255) == R_SUB {
                            OP_SUB
                        } else if (w0 & 255) == R_MUL {
                            OP_MUL
                        } else {
                            (w0 >> 8) & 255
                        };
                        let mut r = Val::Undef;
                        let mut done = false;
                        if let (Val::Num(x), Val::Num(y)) = (self.rget(bp, w2), self.rget(bp, w3)) {
                            let (a, b) = (*x, *y);
                            // bit operations on numbers that are int32 already
                            if code >= OP_BAND && code <= OP_SHR && a >= -2147483648.0 && a <= 2147483647.0 && b >= -2147483648.0 && b <= 2147483647.0 {
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
                            self.stack[(bp + w1) as usize] = r;
                            pc = next;
                            continue;
                        }
                        self.r_arith_slow(code, bp, fi, pc, w1, w2, w3);
                    }
                    R_CMP => {
                        let code = (w0 >> 8) & 255;
                        let r = self.rcmp(code, pi, bp, w2, w3, fi, pc);
                        if !self.throwing {
                            self.stack[(bp + w1) as usize] = Val::Bool(r);
                        }
                    }
                    R_CMP_JF | R_CMP_JT => {
                        let code = (w0 >> 8) & 255;
                        let mut num = false;
                        let mut r = false;
                        if let (Val::Num(a), Val::Num(b)) = (self.rget(bp, w1), self.rget(bp, w2)) {
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
                        if !num {
                            r = self.rcmp(code, pi, bp, w1, w2, fi, pc);
                        }
                        if r == ((w0 & 255) == R_CMP_JT) && !self.throwing {
                            if w3 <= pc {
                                self.frames[fi].pc = w3;
                                if self.gc_due() {
                                    self.rgc(want);
                                }
                            }
                            npc = w3;
                        }
                    }
                    R_LOOP => {
                        let code = (w0 >> 8) & 255;
                        let mut num = false;
                        let mut r = false;
                        if let (Val::Num(a), Val::Num(b)) = (self.rget(bp, w1), self.rget(bp, w2)) {
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
                        if !num {
                            r = self.rcmp(code, pi, bp, w1, w2, fi, pc);
                            if self.throwing {
                                self.frames[fi].pc = pc + 1;
                                if !self.unwind(base) {
                                    return;
                                }
                                continue 'frames;
                            }
                        }
                        let t = if r { w0 >> 16 } else { w3 };
                        if t <= pc && self.gc_due() {
                            self.frames[fi].pc = t;
                            self.rgc(want);
                        }
                        pc = t;
                        continue;
                    }
                    R_JUMP => {
                        if w1 <= pc && self.gc_due() {
                            self.frames[fi].pc = w1;
                            self.rgc(want);
                        }
                        pc = w1;
                        continue;
                    }
                    R_JF => {
                        pc = if truthy(self.rget(bp, w1)) { next } else { w2 };
                        continue;
                    }
                    R_JT => {
                        pc = if truthy(self.rget(bp, w1)) { w2 } else { next };
                        continue;
                    }
                    R_CMP_JFK | R_CMP_JTK => {
                        // the value is written either way: it may be a local
                        // (var b = x === y) that is read after the jump
                        let code = (w0 >> 8) & 255;
                        let mut num = false;
                        let mut r = false;
                        if let (Val::Num(a), Val::Num(b)) = (self.rget(bp, w2), self.rget(bp, w3)) {
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
                        if !num {
                            r = self.rcmp(code, pi, bp, w2, w3, fi, pc);
                        }
                        if !self.throwing {
                            self.stack[(bp + (w1 & 65535)) as usize] = Val::Bool(r);
                            if r == ((w0 & 255) == R_CMP_JTK) {
                                npc = w1 >> 16;
                            }
                        }
                    }
                    R_JF_KEEP => {
                        pc = if truthy(self.rget(bp, w1)) { next } else { w2 };
                        continue;
                    }
                    R_JT_KEEP => {
                        pc = if truthy(self.rget(bp, w1)) { w2 } else { next };
                        continue;
                    }
                    R_JNN_KEEP => {
                        let nullish = match self.rget(bp, w1) {
                            Val::Undef | Val::Null => true,
                            _ => false,
                        };
                        pc = if nullish { next } else { w2 };
                        continue;
                    }
                    R_LOCAL_NULL_JF => {
                        let cmp = w2;
                        let r = match self.stack[(bp + w1) as usize] {
                            Val::Null => cmp == OP_SEQ || cmp == OP_EQ,
                            Val::Undef => cmp == OP_SNE || cmp == OP_EQ,
                            _ => cmp == OP_SNE || cmp == OP_NE,
                        };
                        if !r {
                            if w3 <= pc {
                                self.frames[fi].pc = w3;
                                if self.gc_due() {
                                    self.rgc(want);
                                }
                            }
                            pc = w3;
                        } else {
                            pc = next;
                        }
                        continue;
                    }
                    R_CASE_INT_JT => {
                        let mut hit = false;
                        if let Val::Num(x) = self.rget(bp, w1) {
                            hit = *x == (w2 as double);
                        }
                        pc = if hit { w3 } else { next };
                        continue;
                    }
                    R_SWITCH => {
                        let mut t = -1;
                        if let Val::Num(x) = self.rget(bp, w1) {
                            let k = *x - (w3 as double);
                            let size = self.rtab[w2 as usize];
                            if k >= 0.0 && k < (size as double) && k == ((k as int) as double) {
                                t = self.rtab[(w2 + 1 + (k as int)) as usize];
                            }
                        }
                        if t < 0 {
                            t = self.rtab[(w2 + 1 + self.rtab[w2 as usize]) as usize];
                        }
                        pc = t;
                        continue;
                    }
                    R_CASE_JT => {
                        let r = self.rcmp(OP_SEQ, pi, bp, w1, w2, fi, pc);
                        if r && !self.throwing {
                            npc = w3;
                        }
                    }
                    R_GET_PROP | R_GET_THIS_PROP => {
                        // w3 is the inline cache; the atom is the stack op's
                        let c = w3;
                        let mut o: int = -1;
                        if (w0 & 255) == R_GET_PROP {
                            if let Val::Obj(x) = self.rget(bp, w2) {
                                o = *x;
                            }
                        } else if let Val::Obj(x) = &self.frames[fi].this_val {
                            o = *x;
                        }
                        if o >= 0 && c >= 65536 && (c >> 16) == self.objs[o as usize].shape as int {
                            // the cache's shape: the slot without a call
                            let r = self.objs[o as usize].vals[(c & 65535) as usize].clone();
                            self.stack[(bp + w1) as usize] = r;
                            pc = next;
                            continue;
                        }
                        if self.r_get_prop_slow(o, pi, rb, bp, fi, pc, w0, w1, w2) {
                            continue 'frames;
                        }
                    }
                    R_SET_PROP => {
                        let o = obj_of(self.rget(bp, w1));
                        if o >= 0 && w3 >= 65536 && (w3 >> 16) == self.objs[o as usize].shape as int && ((w0 >> 8) & 255) == 0 {
                            let val = self.rget(bp, w2).clone();
                            self.put_slot(o, w3 & 65535, val);
                            pc = next;
                            continue;
                        }
                        self.r_set_prop_slow(o, pi, rb, bp, fi, pc, w0, w1, w2, w3);
                    }
                    R_GET_ELEM => {
                        let mut r = Val::Undef;
                        let mut done = false;
                        if let (Val::Obj(o), Val::Num(n)) = (self.rget(bp, w2), self.rget(bp, w3)) {
                            let ob = &self.objs[*o as usize];
                            if ob.class == C_ARRAY {
                                let i = *n as int;
                                if i >= 0 && (i as usize) < ob.elems.len() && (i as double) == *n {
                                    r = ob.elems[i as usize].clone();
                                    done = true;
                                }
                            }
                        }
                        if done {
                            self.stack[(bp + w1) as usize] = r;
                            pc = next;
                            continue;
                        }
                        let v = self.rget(bp, w2).clone();
                        let kv = self.rget(bp, w3).clone();
                        self.frames[fi].pc = pc + 1;
                        let r = self.get_elem(&v, &kv);
                        if !self.throwing {
                            self.stack[(bp + w1) as usize] = r;
                        }
                    }
                    R_SET_ELEM => {
                        let val = self.rget(bp, w3).clone();
                        let o = obj_of(self.rget(bp, w1));
                        let mut n: double = 0.0;
                        let mut num = false;
                        if let Val::Num(x) = self.rget(bp, w2) {
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
                                if i >= 0 && (i as usize) < ob.elems.len() && (i as double) == n && ob.pos != 2 {
                                    old = obj_of(&ob.elems[i as usize]);
                                    old_s = str_of(&ob.elems[i as usize]);
                                    ob.elems[i as usize] = val.clone();
                                    let oc = ob.cold as usize;
                                    if oc > 0 && (i as usize) < self.colds[oc].elems2.len() {
                                        self.colds[oc].elems2[i as usize] = Val::Bool(false);
                                    }
                                    done = true;
                                } else if i >= 0 && (i as usize) == ob.elems.len() && (i as double) == n && ob.extensible {
                                    ob.elems.push(val.clone());
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
                            self.r_set_elem_slow(bp, fi, pc, w1, w2, val);
                        }
                    }
                    R_GET_METHOD => {
                        self.r_get_method(bp, fi, pc, w0, w1, w2, w3);
                    }
                    R_CALL => {
                        let argc = w2;
                        self.frames[fi].pc = next;
                        if self.gc_due() {
                            self.rgc(want);
                        }
                        let fpos = bp + w1;
                        if ((w0 >> 8) & 255) == 1 {
                            self.stack[(fpos + 1) as usize] = Val::Undef;
                        }
                        let fo = obj_of(&self.stack[fpos as usize]);
                        if fo >= 0 && self.objs[fo as usize].class == C_FUNCTION {
                            let fp = self.objs[fo as usize].func as usize;
                            let plain = {
                                let p = &self.protos[fp];
                                !p.uses_args && p.rest < 0 && !p.class_ctor
                            };
                            // the result goes to the slot of f, or to w3 - 1
                            let dst = if w3 > 0 { bp + w3 - 1 } else { fpos };
                            if plain {
                                let co = self.protos[fp].generator || self.protos[fp].is_async;
                                if co && dst != fpos {
                                    self.frames[fi].mv_from = fpos;
                                    self.frames[fi].mv_to = dst;
                                }
                                if !self.renter(fo, fpos, argc, if co { -1 } else { dst }) {
                                    continue 'frames;
                                }
                                // the callee runs in this loop
                                fi = self.frames.len() - 1;
                                pi = fp;
                                bp = fpos + 2;
                                pc = self.frames[fi].pc;
                                want = (bp + self.protos[pi].fsize) as usize;
                                rb = self.protos[pi].rbase;
                                continue;
                            }
                        }
                        if self.r_call_slow(fo, fpos, argc, bp, w3, want) {
                            continue 'frames;
                        }
                        if self.throwing {
                            if !self.unwind(base) {
                                return;
                            }
                            continue 'frames;
                        }
                        pc = next;
                        continue;
                    }
                    R_RET | R_RET_UNDEF => {
                        let mut v = if (w0 & 255) == R_RET { self.rget(bp, w1).clone() } else { Val::Undef };
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
                            continue;
                        } else {
                            self.stack.truncate(fpos);
                            self.stack.push(v);
                        }
                        continue 'frames;
                    }
                    R_THIS => {
                        let t = self.frames[fi].this_val.clone();
                        self.stack[(bp + w1) as usize] = t;
                        pc = next;
                        continue;
                    }
                    R_GET_ENV => {
                        let e = self.get_env(w2);
                        let v = self.objs[e as usize].elems[w3 as usize].clone();
                        self.stack[(bp + w1) as usize] = v;
                        if ((w0 >> 8) & 255) == 1 {
                            self.stack[(bp + w1 + 1) as usize] = Val::Undef;
                        }
                        pc = next;
                        continue;
                    }
                    R_SET_ENV => {
                        let e = self.get_env(w2);
                        let v = self.rget(bp, w1).clone();
                        self.put_elem(e, w3, v);
                        pc = next;
                        continue;
                    }
                    R_GET_GLOBAL => {
                        let g = self.global;
                        let c = self.protos[pi].code[pc as usize].c;
                        let slot = self.ic_slot(g, w3, c, false);
                        if slot >= 0 {
                            let v = self.objs[g as usize].vals[slot as usize].clone();
                            self.stack[(bp + w1) as usize] = v;
                            pc = next;
                            continue;
                        }
                        self.r_step_push(pi, bp, fi, pc, w1);
                    }
                    R_ITER_NEXT => {
                        if !self.r_iter_next(bp, fi, pc, w1, w3) && !self.throwing {
                            npc = w2;
                        }
                    }
                    R_TYPEOF => {
                        let v = self.rget(bp, w2).clone();
                        let t = self.type_val(&v);
                        self.stack[(bp + w1) as usize] = t;
                        pc = next;
                        continue;
                    }
                    R_NOT => {
                        let r = !truthy(self.rget(bp, w2));
                        self.stack[(bp + w1) as usize] = Val::Bool(r);
                        pc = next;
                        continue;
                    }
                    R_NEW_OBJECT => {
                        let o = self.new_object();
                        self.stack[(bp + w1) as usize] = Val::Obj(o);
                        pc = next;
                        continue;
                    }
                    R_INIT_PROP => {
                        let v = self.rget(bp, w2).clone();
                        let o = obj_of(self.rget(bp, w1));
                        // c caches the transition: shape before * 2^24 + after
                        let sh = self.objs[o as usize].shape as int;
                        let c = self.protos[pi].code[pc as usize].c;
                        if c >= 0 && (c >> 24) == sh {
                            self.objs[o as usize].shape = (c & 16777215) as i32;
                            self.push_val(o, v);
                        } else {
                            self.frames[fi].pc = pc + 1;
                            self.define(o, w3, v, 0);
                            let sh2 = self.objs[o as usize].shape as int;
                            if sh2 != sh && sh2 < 16777216 && !self.shapes[sh as usize].owned && !self.shapes[sh2 as usize].owned {
                                self.protos[pi].code[pc as usize].c = sh * 16777216 + sh2;
                            }
                        }
                    }
                    R_CLOSURE => {
                        let f = self.closure(w2);
                        self.stack[(bp + w1) as usize] = Val::Obj(f);
                        pc = next;
                        continue;
                    }
                    R_INC_LOCAL => {
                        let i = (bp + w1) as usize;
                        if let Val::Num(n) = self.stack[i] {
                            self.stack[i] = Val::Num(n + (w2 as double));
                            pc = next;
                            continue;
                        }
                        let v = self.stack[i].clone();
                        self.frames[fi].pc = pc + 1;
                        let (_, nv) = self.numeric_step(&v, w2);
                        if !self.throwing {
                            self.stack[i] = nv;
                        }
                    }
                    R_POSTINC => {
                        let i = (bp + w2) as usize;
                        let pre = ((w0 >> 8) & 255) == 1;
                        if let Val::Num(old) = self.stack[i] {
                            let nv = old + (w3 as double);
                            self.stack[i] = Val::Num(nv);
                            self.stack[(bp + w1) as usize] = Val::Num(if pre { nv } else { old });
                            pc = next;
                            continue;
                        }
                        let v = self.stack[i].clone();
                        self.frames[fi].pc = pc + 1;
                        let (old, nv) = self.numeric_step(&v, w3);
                        if !self.throwing {
                            self.stack[i] = nv.clone();
                            self.stack[(bp + w1) as usize] = if pre { nv } else { old };
                        }
                    }
                    R_ADD_LOCAL_POP => {
                        let b = *self.rget(bp, w2);
                        let i = (bp + w1) as usize;
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
                        self.r_step_push(pi, bp, fi, pc, w1);
                    }
                    R_WIN => {
                        let p2 = self.r_win(pi, bp, fi, pc, w1, want);
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
                    _ => {
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
                pc = npc;
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
        if o >= 0 {
            let c = self.rcode[(rb + pc) as usize].c;
            let slot = self.ic_get(o, atom, c, pi, pc + 1);
            self.rcode[(rb + pc) as usize].c = self.protos[pi].code[pc as usize].c;
            if slot >= 0 {
                let r = self.objs[o as usize].vals[slot as usize].clone();
                self.stack[(bp + w1) as usize] = r;
                return false;
            }
            if atom == A_LENGTH && self.objs[o as usize].class == C_ARRAY && self.find(o, atom) < 0 {
                let r = Val::Num(self.objs[o as usize].elems.len() as double);
                self.stack[(bp + w1) as usize] = r;
                return false;
            }
        }
        if o >= 0 {
            let h = self.pic_get(rb + pc, o, atom);
            if h != -1 {
                let way = self.pic_way as usize;
                let r = if h >= 0 { self.objs[h as usize].vals[self.pic[way + 3] as usize] } else { Val::Undef };
                if h < 0 || self.pic[way + 4] == 0 {
                    self.stack[(bp + w1) as usize] = r;
                    return false;
                }
                // a getter: a plain function runs as a call in this loop
                let g = self.objs[obj_of(&r) as usize].elems[0];
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
        let e0 = self.rpic[si];
        let mut w: int = 0;
        while e0 >= 0 && w < PIC_WAYS {
            let b = ((e0 + w) * PIC_W) as usize;
            w += 1;
            if self.pic[b + 1] != self.shape_epoch || self.objs[o as usize].shape != self.pic[b] as i32 {
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
                cur = pk;
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
            self.rpic[si] = n;
            n
        };
        // the next way in turn (the first entry's last int counts)
        let ti = (base * PIC_W + PIC_W - 1) as usize;
        let turn = self.pic[ti];
        self.pic[ti] = (turn + 1) % PIC_WAYS;
        let b = ((base + turn) * PIC_W) as usize;
        self.pic[b] = self.objs[o as usize].shape as int;
        self.pic[b + 1] = self.shape_epoch;
        self.pic[b + 2] = chain.len() as int;
        self.pic[b + 3] = slot;
        self.pic[b + 4] = if acc { 1 } else { 0 };
        let mut k: usize = 0;
        while k < chain.len() {
            let x = chain[k];
            self.pic[b + 5 + 2 * k] = x;
            self.pic[b + 6 + 2 * k] = self.objs[x as usize].shape as int;
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
            self.set(&target, atom, val.clone());
        }
        if ((w0 >> 8) & 255) == 1 && !self.throwing {
            self.stack[(bp + w1) as usize] = val;
        }
    }

    #[inline(never)]
    fn r_set_elem_slow(&mut self, bp: int, fi: usize, pc: int, w1: int, w2: int, val: Val) {
        let target = self.rget(bp, w1).clone();
        let kv = self.rget(bp, w2).clone();
        self.frames[fi].pc = pc + 1;
        self.set_elem(&target, &kv, val);
    }

    #[inline(never)]
    fn r_get_method(&mut self, bp: int, fi: usize, pc: int, w0: int, w1: int, w2: int, w3: int) {
        let v = self.rget(bp, w2).clone();
        self.frames[fi].pc = pc + 1;
        let atom = w3;
        let mut f = Val::Undef;
        let mut done = false;
        if let Val::Obj(o) = &v {
            let o = *o;
            let slot = self.find(o, atom);
            if slot >= 0 && (self.attr_at(o, slot) & P_ACCESSOR) == 0 {
                f = self.objs[o as usize].vals[slot as usize].clone();
                done = true;
            } else if slot < 0 {
                let site = self.rbase_of(fi) + pc;
                let h = self.pic_get(site, o, atom);
                if h != -1 && (h < 0 || self.pic[(self.pic_way + 4) as usize] == 0) {
                    f = if h >= 0 { self.objs[h as usize].vals[self.pic[(self.pic_way + 3) as usize] as usize] } else { Val::Undef };
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
        if fo >= 0 && self.objs[fo as usize].class == C_FUNCTION {
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
        while self.stack.len() < want {
            self.stack.push(Val::Undef);
        }
        if w3 > 0 {
            let v = self.stack[fpos as usize].clone();
            self.stack[(bp + w3 - 1) as usize] = v;
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
        let pi = self.objs[fo as usize].func as usize;
        if self.protos[pi].rstate == 0 {
            unpack_code(&mut self.protos[pi]);
                let e = translate(&mut self.protos[pi], &mut self.rcode, &mut self.kvals, &mut self.rtab);
            if !e.is_empty() {
                self.throw_type(e.as_str());
                return false;
            }
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
            self.stack[i as usize] = Val::Undef;
            i += 1;
        }
        while self.stack.len() < end {
            self.stack.push(Val::Undef);
        }
        let mut nt = Val::Undef;
        if arrow && !self.colds[self.objs[fo as usize].cold as usize].elems2.is_empty() {
            nt = self.colds[self.objs[fo as usize].cold as usize].elems2[0].clone();
        }
        let mut this_val = if arrow { self.colds[self.objs[fo as usize].cold as usize].prim.clone() } else { self.stack[(bp - 1) as usize].clone() };
        if !arrow && !strict {
            if matches!(this_val, Val::Undef) || matches!(this_val, Val::Null) {
                this_val = Val::Obj(self.global);
            } else if !is_obj(&this_val) {
                let bo = self.to_object(&this_val);
                this_val = Val::Obj(bo);
            }
        }
        let mut env = self.objs[fo as usize].env as int;
        if env_size > 0 {
            let e = self.alloc(C_ENV, env);
            let mut k = 0;
            while k < env_size {
                self.push_elem(e, Val::Undef);
                k += 1;
            }
            env = e;
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

