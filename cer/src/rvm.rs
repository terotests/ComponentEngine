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

impl Vm {
    /// Register x of the frame at bp: a slot, or a constant when negative.
    #[inline]
    pub fn rget(&self, pi: usize, bp: int, x: int) -> &Val {
        if x >= 0 {
            &self.stack[(bp + x) as usize]
        } else {
            &self.protos[pi].kvals[(-1 - x) as usize]
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
            let fi = self.frames.len() - 1;
            let pi = self.frames[fi].proto as usize;
            let bp = self.frames[fi].bp;
            let mut pc = self.frames[fi].pc;
            if self.protos[pi].rstate == 0 {
                let e = translate(&mut self.protos[pi]);
                if !e.is_empty() {
                    self.throw_type(e.as_str());
                    continue 'frames;
                }
            }
            let want = (bp + self.protos[pi].fsize) as usize;
            while self.stack.len() < want {
                self.stack.push(Val::Undef);
            }
            loop {
                let ins = self.protos[pi].rcode[pc as usize];
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
                        let v = self.rget(pi, bp, w2).clone();
                        self.stack[(bp + w1) as usize] = v;
                        pc = next;
                        continue;
                    }
                    R_LOAD2 => {
                        let v = self.rget(pi, bp, w2).clone();
                        let u = self.rget(pi, bp, w3).clone();
                        self.stack[(bp + w1) as usize] = v;
                        self.stack[(bp + w1 + 1) as usize] = u;
                        pc = next;
                        continue;
                    }
                    R_ADD => {
                        let mut r = Val::Undef;
                        let mut done = false;
                        if let (Val::Num(x), Val::Num(y)) = (self.rget(pi, bp, w2), self.rget(pi, bp, w3)) {
                            r = Val::Num(*x + *y);
                            done = true;
                        }
                        if done {
                            self.stack[(bp + w1) as usize] = r;
                            pc = next;
                            continue;
                        }
                        let a = self.rget(pi, bp, w2).clone();
                        let b = self.rget(pi, bp, w3).clone();
                        self.frames[fi].pc = pc + 1;
                        let r = self.add_vals(&a, &b);
                        if !self.throwing {
                            self.stack[(bp + w1) as usize] = r;
                        }
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
                        if let (Val::Num(x), Val::Num(y)) = (self.rget(pi, bp, w2), self.rget(pi, bp, w3)) {
                            r = Val::Num(arith_num(code, *x, *y));
                            done = true;
                        }
                        if done {
                            self.stack[(bp + w1) as usize] = r;
                            pc = next;
                            continue;
                        }
                        let a = self.rget(pi, bp, w2).clone();
                        let b = self.rget(pi, bp, w3).clone();
                        self.frames[fi].pc = pc + 1;
                        let r = self.arith(code, &a, &b);
                        if !self.throwing {
                            self.stack[(bp + w1) as usize] = r;
                        }
                    }
                    R_CMP => {
                        let code = (w0 >> 8) & 255;
                        let r = self.rcompare(code, pi, bp, w2, w3, fi, pc);
                        if !self.throwing {
                            self.stack[(bp + w1) as usize] = Val::Bool(r);
                        }
                    }
                    R_CMP_JF => {
                        let code = (w0 >> 8) & 255;
                        let mut num = false;
                        let mut r = false;
                        if let (Val::Num(a), Val::Num(b)) = (self.rget(pi, bp, w1), self.rget(pi, bp, w2)) {
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
                            r = self.rcompare(code, pi, bp, w1, w2, fi, pc);
                        }
                        if !r && !self.throwing {
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
                        if let (Val::Num(a), Val::Num(b)) = (self.rget(pi, bp, w1), self.rget(pi, bp, w2)) {
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
                            r = self.rcompare(code, pi, bp, w1, w2, fi, pc);
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
                        pc = if truthy(self.rget(pi, bp, w1)) { next } else { w2 };
                        continue;
                    }
                    R_JT => {
                        pc = if truthy(self.rget(pi, bp, w1)) { w2 } else { next };
                        continue;
                    }
                    R_JF_KEEP => {
                        pc = if truthy(self.rget(pi, bp, w1)) { next } else { w2 };
                        continue;
                    }
                    R_JT_KEEP => {
                        pc = if truthy(self.rget(pi, bp, w1)) { w2 } else { next };
                        continue;
                    }
                    R_JNN_KEEP => {
                        let nullish = match self.rget(pi, bp, w1) {
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
                        if let Val::Num(x) = self.rget(pi, bp, w1) {
                            hit = *x == (w2 as double);
                        }
                        pc = if hit { w3 } else { next };
                        continue;
                    }
                    R_CASE_JT => {
                        let r = self.rcompare(OP_SEQ, pi, bp, w1, w2, fi, pc);
                        if r && !self.throwing {
                            npc = w3;
                        }
                    }
                    R_GET_PROP | R_GET_THIS_PROP => {
                        let c = self.protos[pi].code[pc as usize].c;
                        let mut o: int = -1;
                        if (w0 & 255) == R_GET_PROP {
                            if let Val::Obj(x) = self.rget(pi, bp, w2) {
                                o = *x;
                            }
                        } else if let Val::Obj(x) = &self.frames[fi].this_val {
                            o = *x;
                        }
                        let mut done = false;
                        if o >= 0 {
                            let slot = self.ic_get(o, w3, c, pi, pc + 1);
                            if slot >= 0 {
                                let r = self.objs[o as usize].vals[slot as usize].clone();
                                self.stack[(bp + w1) as usize] = r;
                                done = true;
                            } else if w3 == A_LENGTH && self.objs[o as usize].class == C_ARRAY && self.find(o, w3) < 0 {
                                let r = Val::Num(self.objs[o as usize].elems.len() as double);
                                self.stack[(bp + w1) as usize] = r;
                                done = true;
                            }
                        }
                        if done {
                            pc = next;
                            continue;
                        }
                        let v = if (w0 & 255) == R_GET_PROP { self.rget(pi, bp, w2).clone() } else { self.frames[fi].this_val.clone() };
                        self.frames[fi].pc = pc + 1;
                        let r = self.get(&v, w3);
                        if !self.throwing {
                            self.stack[(bp + w1) as usize] = r;
                        }
                    }
                    R_SET_PROP => {
                        let val = self.rget(pi, bp, w2).clone();
                        let o = obj_of(self.rget(pi, bp, w1));
                        let mut done = false;
                        if o >= 0 {
                            let c = self.protos[pi].code[pc as usize].c;
                            let slot = self.ic_set(o, w3, c, pi, pc + 1);
                            if slot >= 0 {
                                self.put_slot(o, slot, val.clone());
                                done = true;
                            }
                        }
                        if !done {
                            let target = self.rget(pi, bp, w1).clone();
                            self.frames[fi].pc = pc + 1;
                            self.set(&target, w3, val);
                        }
                    }
                    R_GET_ELEM => {
                        let mut r = Val::Undef;
                        let mut done = false;
                        if let (Val::Obj(o), Val::Num(n)) = (self.rget(pi, bp, w2), self.rget(pi, bp, w3)) {
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
                        let v = self.rget(pi, bp, w2).clone();
                        let kv = self.rget(pi, bp, w3).clone();
                        self.frames[fi].pc = pc + 1;
                        let r = self.get_elem(&v, &kv);
                        if !self.throwing {
                            self.stack[(bp + w1) as usize] = r;
                        }
                    }
                    R_SET_ELEM => {
                        let val = self.rget(pi, bp, w3).clone();
                        let o = obj_of(self.rget(pi, bp, w1));
                        let mut n: double = 0.0;
                        let mut num = false;
                        if let Val::Num(x) = self.rget(pi, bp, w2) {
                            n = *x;
                            num = true;
                        }
                        let mut done = false;
                        if o >= 0 && num {
                            if let Val::Obj(x) = val {
                                self.objs[x as usize].rc += 1;
                            }
                            let ob = &mut self.objs[o as usize];
                            let mut old: int = -1;
                            if ob.class == C_ARRAY {
                                let i = n as int;
                                if i >= 0 && (i as usize) < ob.elems.len() && (i as double) == n && ob.pos != 2 {
                                    old = obj_of(&ob.elems[i as usize]);
                                    ob.elems[i as usize] = val.clone();
                                    if (i as usize) < ob.elems2.len() {
                                        ob.elems2[i as usize] = Val::Bool(false);
                                    }
                                    done = true;
                                } else if i >= 0 && (i as usize) == ob.elems.len() && (i as double) == n && ob.extensible {
                                    ob.elems.push(val.clone());
                                    done = true;
                                }
                            }
                            if done {
                                self.dec_o(old);
                            } else {
                                self.dec(&val);
                            }
                        }
                        if !done {
                            let target = self.rget(pi, bp, w1).clone();
                            let kv = self.rget(pi, bp, w2).clone();
                            self.frames[fi].pc = pc + 1;
                            self.set_elem(&target, &kv, val);
                        }
                    }
                    R_GET_METHOD => {
                        let v = self.rget(pi, bp, w2).clone();
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
                            if plain {
                                self.renter(fo, fpos, argc);
                                continue 'frames;
                            }
                            self.stack.truncate((fpos + argc + 2) as usize);
                            self.enter_function(fo, argc, false, Val::Undef);
                            continue 'frames;
                        }
                        self.stack.truncate((fpos + argc + 2) as usize);
                        if self.call_op(argc) {
                            continue 'frames;
                        }
                        if self.throwing {
                            if !self.unwind(base) {
                                return;
                            }
                            continue 'frames;
                        }
                        while self.stack.len() < want {
                            self.stack.push(Val::Undef);
                        }
                        pc = next;
                        continue;
                    }
                    R_RET | R_RET_UNDEF => {
                        let mut v = if (w0 & 255) == R_RET { self.rget(pi, bp, w1).clone() } else { Val::Undef };
                        let fr = self.frames.pop().unwrap();
                        if fr.construct && !is_obj(&v) {
                            if !matches!(v, Val::Undef) && self.protos[fr.proto as usize].derived {
                                self.frames.push(fr);
                                self.frames[fi].pc = pc + 1;
                                self.throw_type("Derived constructors may only return object or undefined");
                                if !self.unwind(base) {
                                    return;
                                }
                                continue 'frames;
                            }
                            v = fr.this_val.clone();
                        }
                        let depth = self.frames.len() as int;
                        while !self.handlers.is_empty() && self.handlers[self.handlers.len() - 1].frame >= depth {
                            self.handlers.pop();
                        }
                        let fpos = (fr.bp - 2) as usize;
                        if (self.frames.len() as int) > base {
                            // into the caller's slot of f; the stack ends at
                            // the caller's frame again
                            let ci = self.frames.len() - 1;
                            let cp = self.frames[ci].proto as usize;
                            let cend = (self.frames[ci].bp + self.protos[cp].fsize) as usize;
                            self.stack[fpos] = v;
                            while self.stack.len() < cend {
                                self.stack.push(Val::Undef);
                            }
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
                        let v = self.rget(pi, bp, w1).clone();
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
                        let op = self.protos[pi].code[pc as usize];
                        let mut p2 = pc + 1;
                        self.frames[fi].pc = p2;
                        self.step(op, pi, bp, fi, &mut p2);
                        if !self.throwing {
                            let v = self.stack.pop().unwrap();
                            self.stack[(bp + w1) as usize] = v;
                        }
                    }
                    R_NOT => {
                        let r = !truthy(self.rget(pi, bp, w2));
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
                        let v = self.rget(pi, bp, w2).clone();
                        let o = obj_of(self.rget(pi, bp, w1));
                        // c caches the transition: shape before * 2^24 + after
                        let sh = self.objs[o as usize].shape;
                        let c = self.protos[pi].code[pc as usize].c;
                        if c >= 0 && (c >> 24) == sh {
                            self.objs[o as usize].shape = c & 16777215;
                            self.push_val(o, v);
                        } else {
                            self.frames[fi].pc = pc + 1;
                            self.define(o, w3, v, 0);
                            let sh2 = self.objs[o as usize].shape;
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
                        let b = self.rget(pi, bp, w2).clone();
                        let i = (bp + w1) as usize;
                        let mut done = false;
                        if let Val::Str(bs) = &b {
                            done = append_in_place(&mut self.stack[i], bs.as_str());
                        }
                        if !done {
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
                        let op = self.protos[pi].code[pc as usize];
                        let mut p2 = pc + 1;
                        self.frames[fi].pc = p2;
                        self.step(op, pi, bp, fi, &mut p2);
                        if !self.throwing {
                            let v = self.stack.pop().unwrap();
                            self.stack[(bp + w1) as usize] = v;
                        }
                    }
                    R_WIN => {
                        self.stack.truncate((bp + w1) as usize);
                        let op = self.protos[pi].code[pc as usize];
                        let mut p2 = pc + 1;
                        self.frames[fi].pc = p2;
                        let changed = self.win_op(op, pi, bp, fi, &mut p2);
                        if self.throwing {
                            if !self.unwind(base) {
                                return;
                            }
                            continue 'frames;
                        }
                        if changed {
                            continue 'frames;
                        }
                        while self.stack.len() < want {
                            self.stack.push(Val::Undef);
                        }
                        if self.stack.len() > want {
                            self.stack.truncate(want);
                        }
                        if p2 != pc + 1 {
                            npc = p2;
                        }
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

    /// A call of a JS function without `arguments`, a rest parameter or a
    /// class constructor's checks, from register code: [f, this, args…] at
    /// fpos. The callee's frame starts after `this` and the stack ends
    /// where the frame does.
    pub fn renter(&mut self, fo: int, fpos: int, argc: int) {
        let pi = self.objs[fo as usize].func as usize;
        if self.protos[pi].rstate == 0 {
            let e = translate(&mut self.protos[pi]);
            if !e.is_empty() {
                self.throw_type(e.as_str());
                return;
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
        if arrow && !self.objs[fo as usize].elems2.is_empty() {
            nt = self.objs[fo as usize].elems2[0].clone();
        }
        let mut this_val = if arrow { self.objs[fo as usize].prim.clone() } else { self.stack[(bp - 1) as usize].clone() };
        if !arrow && !strict {
            if matches!(this_val, Val::Undef) || matches!(this_val, Val::Null) {
                this_val = Val::Obj(self.global);
            } else if !is_obj(&this_val) {
                let bo = self.to_object(&this_val);
                this_val = Val::Obj(bo);
            }
        }
        let mut env = self.objs[fo as usize].env;
        if env_size > 0 {
            let e = self.alloc(C_ENV, env);
            let mut k = 0;
            while k < env_size {
                self.push_elem(e, Val::Undef);
                k += 1;
            }
            env = e;
        }
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
        });
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

    /// A comparison of two registers (OP_LT … OP_SNE).
    #[inline]
    pub fn rcompare(&mut self, code: int, pi: usize, bp: int, x: int, y: int, fi: usize, pc: int) -> bool {
        let mut num = false;
        let mut r = false;
        if let (Val::Num(a), Val::Num(b)) = (self.rget(pi, bp, x), self.rget(pi, bp, y)) {
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
        let q = if strict || code == OP_EQ || code == OP_NE { quick_eq(self.rget(pi, bp, x), self.rget(pi, bp, y), strict) } else { -1 };
        if q >= 0 {
            return (q == 1) == (code == OP_SEQ || code == OP_EQ);
        }
        let a = self.rget(pi, bp, x).clone();
        let b = self.rget(pi, bp, y).clone();
        self.frames[fi].pc = pc + 1;
        self.compare(code, &a, &b)
    }
}
