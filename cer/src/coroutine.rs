// SPDX-License-Identifier: AGPL-3.0-or-later
//! Generators and async functions: coroutines over the one interpreter.
//!
//! A coroutine is a C_GENERATOR object. While suspended it holds its frame:
//! `elems` is the frame's stack from the function value up, `elems2` the
//! rest (see the G_ slots), and the frame's try handlers after them, three
//! values each. Resuming pushes all of it back on top of the current stack
//! and runs the interpreter until the frame yields (OP_YIELD saves it again)
//! or returns. A generator's `next` / `return` / `throw` resume it; an async
//! function's call starts its coroutine at once and each `await` resumes it
//! from a promise reaction.

use ranger::prelude::*;

use crate::builtins::*;
use crate::value::*;
use crate::vm::*;

// coroutine states (JsObj.pos)
pub const GS_START: int = 0;
pub const GS_YIELD: int = 1;
pub const GS_RUNNING: int = 2;
pub const GS_DONE: int = 3;

// resume modes
pub const GM_NEXT: int = 0;
pub const GM_RETURN: int = 1;
pub const GM_THROW: int = 2;

// the slots of elems2
const G_PC: usize = 0;
const G_THIS: usize = 1;
const G_NEW_TARGET: usize = 2;
const G_FOBJ: usize = 3;
const G_ENV: usize = 4;
const G_ARGS: usize = 5;
/// an async call's promise; an async generator's queue of requests
const G_PROMISE: usize = 6;
/// an async generator waiting on an `await` (true), not on a request
const G_AWAIT: usize = 7;
const G_HANDLERS: usize = 8;

pub const NF_GEN_NEXT: int = 900;
pub const NF_GEN_RETURN: int = 901;
pub const NF_GEN_THROW: int = 902;
pub const NF_ASYNC_OK: int = 903;
pub const NF_ASYNC_ERR: int = 904;
pub const NF_AGEN_NEXT: int = 905;
pub const NF_AGEN_RETURN: int = 906;
pub const NF_AGEN_THROW: int = 907;
pub const NF_AGEN_AWAIT_OK: int = 908;
pub const NF_AGEN_AWAIT_ERR: int = 909;
pub const NF_ASYNC_FROM_SYNC: int = 910;

fn obj_val(o: int) -> Val {
    if o >= 0 {
        Val::Obj(o)
    } else {
        Val::Undef
    }
}

fn num_of(v: &Val) -> int {
    match v {
        Val::Num(n) => *n as int,
        _ => -1,
    }
}

impl Vm {
    /// The generator prototypes and the async function prototype.
    pub fn setup_coroutines(&mut self) {
        let op = self.object_proto;
        let fp = self.function_proto;
        let a_tag = self.intern("@@toStringTag");
        // %IteratorPrototype%
        let itp = self.alloc(C_OBJECT, op);
        self.roots.push(itp);
        self.sym_method(itp, A_ITERATOR, "[Symbol.iterator]", NF_ITER_SELF, 0);
        self.iterator_proto = itp;
        let ip = self.iter_proto;
        self.objs[ip as usize].proto = itp;
        self.objs[ip as usize].add(a_tag, str_val("Array Iterator"), P_HIDDEN | P_READONLY);
        // the other built-in iterators: the same machinery, own prototypes
        let mut made: Vec<int> = Vec::new();
        for tagname in vec!["Map Iterator", "Set Iterator", "String Iterator"] {
            let p = self.alloc(C_OBJECT, itp);
            self.roots.push(p);
            self.method(p, "next", NF_ITER_NEXT, 0);
            self.objs[p as usize].add(a_tag, str_val(tagname), P_HIDDEN | P_READONLY);
            made.push(p);
        }
        self.map_iter_proto = made[0];
        self.set_iter_proto = made[1];
        self.string_iter_proto = made[2];
        // %GeneratorPrototype%
        let gp = self.alloc(C_OBJECT, itp);
        self.roots.push(gp);
        self.method(gp, "next", NF_GEN_NEXT, 1);
        self.method(gp, "return", NF_GEN_RETURN, 1);
        self.method(gp, "throw", NF_GEN_THROW, 1);
        self.objs[gp as usize].add(a_tag, str_val("Generator"), P_HIDDEN | P_READONLY);
        self.generator_proto = gp;
        // %GeneratorFunction.prototype%
        // %AsyncIteratorPrototype%, %AsyncGeneratorPrototype% and the
        // async generator functions' prototype
        let a_async_iter = self.intern("@@asyncIterator");
        let aip = self.alloc(C_OBJECT, op);
        self.roots.push(aip);
        self.sym_method(aip, a_async_iter, "[Symbol.asyncIterator]", NF_ITER_SELF, 0);
        let agp = self.alloc(C_OBJECT, aip);
        self.roots.push(agp);
        self.method(agp, "next", NF_AGEN_NEXT, 1);
        self.method(agp, "return", NF_AGEN_RETURN, 1);
        self.method(agp, "throw", NF_AGEN_THROW, 1);
        self.objs[agp as usize].add(a_tag, str_val("AsyncGenerator"), P_HIDDEN | P_READONLY);
        self.async_generator_proto = agp;
        let agfp = self.alloc(C_OBJECT, fp);
        self.roots.push(agfp);
        self.objs[agfp as usize].add(A_PROTOTYPE, Val::Obj(agp), P_HIDDEN | P_READONLY);
        self.objs[agp as usize].add(A_CONSTRUCTOR, Val::Obj(agfp), P_HIDDEN | P_READONLY);
        self.objs[agfp as usize].add(a_tag, str_val("AsyncGeneratorFunction"), P_HIDDEN | P_READONLY);
        self.async_generator_function_proto = agfp;
        let gfp = self.alloc(C_OBJECT, fp);
        self.roots.push(gfp);
        self.objs[gfp as usize].add(A_PROTOTYPE, Val::Obj(gp), P_HIDDEN | P_READONLY);
        self.objs[gp as usize].add(A_CONSTRUCTOR, Val::Obj(gfp), P_HIDDEN | P_READONLY);
        self.objs[gfp as usize].add(a_tag, str_val("GeneratorFunction"), P_HIDDEN | P_READONLY);
        self.generator_function_proto = gfp;
        // %AsyncFunction.prototype%
        let afp = self.alloc(C_OBJECT, fp);
        self.roots.push(afp);
        self.objs[afp as usize].add(a_tag, str_val("AsyncFunction"), P_HIDDEN | P_READONLY);
        self.async_function_proto = afp;
        let g = self.global;
        self.method(g, "__setAsyncFromSync", NF_ASYNC_FROM_SYNC, 1);
    }

    /// The prototype a closure of `pi` gets.
    pub fn closure_proto(&self, pi: int) -> int {
        let p = &self.protos[pi as usize];
        if p.generator && p.is_async {
            self.async_generator_function_proto
        } else if p.generator {
            self.generator_function_proto
        } else if p.is_async {
            self.async_function_proto
        } else {
            self.function_proto
        }
    }

    /// OP_GEN_START: the top frame (`fi`, stopped at `pc`) becomes a
    /// suspended coroutine; the call answers the generator, or for an async
    /// function, its promise after running to the first `await`.
    pub fn gen_start(&mut self, fi: usize, pc: int) {
        let fo = self.frames[fi].fobj;
        let pi = self.frames[fi].proto;
        let is_async = self.protos[pi as usize].is_async && !self.protos[pi as usize].generator;
        let async_gen = self.protos[pi as usize].is_async && self.protos[pi as usize].generator;
        let mut proto = if async_gen { self.async_generator_proto } else { self.generator_proto };
        if !is_async {
            let p = self.get_obj(fo, A_PROTOTYPE, &Val::Obj(fo));
            if let Val::Obj(po) = p {
                proto = po;
            }
        }
        let g = self.alloc(C_GENERATOR, if is_async { -1 } else { proto });
        self.objs[g as usize].func = pi;
        self.frames[fi].gen = g;
        self.frames[fi].pc = pc;
        self.gen_save(g);
        self.objs[g as usize].pos = GS_START;
        if async_gen {
            let q = self.new_array(Vec::new());
            self.objs[g as usize].elems2[G_PROMISE] = Val::Obj(q);
        }
        if !is_async {
            self.stack.push(Val::Obj(g));
            return;
        }
        let p = self.new_promise();
        self.objs[g as usize].elems2[G_PROMISE] = Val::Obj(p);
        self.temp_roots.push(Val::Obj(g));
        self.async_step(g, GM_NEXT, Val::Undef);
        self.temp_roots.pop();
        self.stack.push(Val::Obj(p));
    }

    /// Moves the top frame, its stack and its handlers into `g` and pops
    /// them.
    pub fn gen_save(&mut self, g: int) {
        let fi = self.frames.len() - 1;
        let bp = self.frames[fi].bp;
        let start = (bp - 2) as usize;
        let mut saved: Vec<Val> = Vec::new();
        let mut i = start;
        while i < self.stack.len() {
            saved.push(self.stack[i].clone());
            i += 1;
        }
        self.stack.truncate(start);
        let promise = if self.objs[g as usize].elems2.len() > G_PROMISE { self.objs[g as usize].elems2[G_PROMISE].clone() } else { Val::Undef };
        let awaiting = if self.objs[g as usize].elems2.len() > G_AWAIT { self.objs[g as usize].elems2[G_AWAIT].clone() } else { Val::Bool(false) };
        let mut rest: Vec<Val> = Vec::new();
        {
            let f = &self.frames[fi];
            rest.push(Val::Num(f.pc as double));
            rest.push(f.this_val.clone());
            rest.push(f.new_target.clone());
            rest.push(obj_val(f.fobj));
            rest.push(obj_val(f.env));
            rest.push(obj_val(f.args_obj));
        }
        rest.push(promise);
        rest.push(awaiting);
        // the frame's handlers, innermost last
        let depth = fi as int;
        let mut first = self.handlers.len();
        while first > 0 && self.handlers[first - 1].frame >= depth {
            first -= 1;
        }
        let mut h = first;
        while h < self.handlers.len() {
            rest.push(Val::Num(self.handlers[h].catch_pc as double));
            rest.push(Val::Num((self.handlers[h].sp - (start as int)) as double));
            rest.push(obj_val(self.handlers[h].env));
            h += 1;
        }
        self.handlers.truncate(first);
        self.frames.pop();
        self.objs[g as usize].elems = saved;
        self.objs[g as usize].elems2 = rest;
    }

    /// Runs `g` from where it stopped with `v` as the value of its `yield`
    /// (mode next), or as an exception (throw) or a return there. Answers
    /// the yielded or returned value; `gen_done` tells which. An exception
    /// out of the coroutine is left thrown.
    pub fn gen_resume(&mut self, g: int, mode: int, v: Val) -> Val {
        let state = self.objs[g as usize].pos;
        self.gen_done = true;
        self.gen_raw = false;
        if state == GS_RUNNING {
            self.throw_type("Generator is already running");
            return Val::Undef;
        }
        if state == GS_DONE || (state == GS_START && mode != GM_NEXT) {
            self.gen_finish(g);
            if mode == GM_THROW {
                self.throw_val(v);
                return Val::Undef;
            }
            return if mode == GM_RETURN { v } else { Val::Undef };
        }
        let base = self.frames.len() as int;
        let start = self.stack.len() as int;
        let saved = self.objs[g as usize].elems.clone();
        for x in saved {
            self.stack.push(x);
        }
        let rest = self.objs[g as usize].elems2.clone();
        let args_obj = obj_of(&rest[G_ARGS]);
        self.frames.push(Frame {
            proto: self.objs[g as usize].func,
            fobj: obj_of(&rest[G_FOBJ]),
            pc: num_of(&rest[G_PC]),
            bp: start + 2,
            env: obj_of(&rest[G_ENV]),
            this_val: rest[G_THIS].clone(),
            new_target: rest[G_NEW_TARGET].clone(),
            args_obj: args_obj,
            construct: false,
            gen: g,
        });
        let mut i = G_HANDLERS;
        while i + 2 < rest.len() {
            self.handlers.push(Handler {
                frame: base,
                catch_pc: num_of(&rest[i]),
                sp: start + num_of(&rest[i + 1]),
                env: obj_of(&rest[i + 2]),
            });
            i += 3;
        }
        self.objs[g as usize].elems = Vec::new();
        self.objs[g as usize].pos = GS_RUNNING;
        if state == GS_YIELD {
            self.stack.push(v);
            self.stack.push(Val::Num(mode as double));
        }
        self.gen_yielded = false;
        self.native_depth += 1;
        self.run(base);
        self.native_depth -= 1;
        let yielded = self.gen_yielded;
        self.gen_yielded = false;
        if self.throwing {
            self.gen_finish(g);
            self.gen_done = true;
            return Val::Undef;
        }
        let r = self.stack.pop().unwrap();
        if yielded {
            self.objs[g as usize].pos = GS_YIELD;
            self.gen_done = false;
        } else {
            self.gen_finish(g);
            self.gen_done = true;
            self.gen_raw = false;
        }
        r
    }

    fn gen_finish(&mut self, g: int) {
        self.objs[g as usize].pos = GS_DONE;
        self.objs[g as usize].elems = Vec::new();
        let n = self.objs[g as usize].elems2.len();
        if n > G_HANDLERS {
            self.objs[g as usize].elems2.truncate(G_HANDLERS);
        }
    }

    /// OP_YIELD in the frame `fi` stopped at `pc`: saves the coroutine and
    /// hands `v` to its resumer.
    pub fn gen_yield(&mut self, fi: usize, pc: int, v: Val, kind: int) {
        let g = self.frames[fi].gen;
        self.frames[fi].pc = pc;
        self.gen_save(g);
        self.stack.push(v);
        self.gen_yielded = true;
        self.gen_raw = kind == 1;
        self.gen_awaiting = kind == 2;
    }

    pub fn iter_result(&mut self, v: Val, done: bool) -> Val {
        let r = self.new_object();
        self.objs[r as usize].add(A_VALUE, v, 0);
        self.objs[r as usize].add(A_DONE, Val::Bool(done), 0);
        Val::Obj(r)
    }

    /// One `yield*` step: [iter, received, mode]; see OP_YIELD_STAR. Answers
    /// 0 to yield the result now on the stack, 1 when done (the value
    /// replaces the iterator), 2 to return with the value.
    pub fn yield_star_step(&mut self) -> int {
        let mode = num_of(&self.stack.pop().unwrap());
        let received = self.stack.pop().unwrap();
        let it = self.stack[self.stack.len() - 1].clone();
        let name = if mode == GM_THROW {
            "throw"
        } else if mode == GM_RETURN {
            "return"
        } else {
            "next"
        };
        let a = self.intern(name);
        let m = self.get(&it, a);
        if self.throwing {
            return 0;
        }
        if matches!(m, Val::Undef) || matches!(m, Val::Null) {
            if mode == GM_RETURN {
                self.stack.push(received);
                return 2;
            }
            if mode == GM_THROW {
                // the protocol is broken: close the inner iterator
                self.iter_close(&it);
                if !self.throwing {
                    self.throw_type("The iterator does not provide a 'throw' method");
                }
                return 0;
            }
        }
        let r = self.call_value(m, it, vec![received]);
        if self.throwing {
            return 0;
        }
        if !is_obj(&r) {
            self.throw_type("Iterator result is not an object");
            return 0;
        }
        let done = self.get(&r, A_DONE);
        if self.throwing {
            return 0;
        }
        if truthy(&done) {
            let v = self.get(&r, A_VALUE);
            if mode == GM_RETURN {
                self.stack.push(v);
                return 2;
            }
            let n = self.stack.len();
            self.stack[n - 1] = v;
            return 1;
        }
        self.stack.push(r);
        0
    }

    /// Calls `it.return()` when there is one (closing an iterator early).
    pub fn iter_close(&mut self, it: &Val) {
        if !is_obj(it) || self.class_of(it) == C_ITER {
            return;
        }
        let a = self.intern("return");
        let m = self.get(it, a);
        if self.throwing || !self.is_callable(&m) {
            return;
        }
        let r = self.call_value(m, it.clone(), Vec::new());
        if !self.throwing && !is_obj(&r) {
            self.throw_type("Iterator result is not an object");
        }
    }

    /// Resumes an async call's coroutine and wires the next `await`.
    pub fn async_step(&mut self, g: int, mode: int, v: Val) {
        let p = obj_of(&self.objs[g as usize].elems2[G_PROMISE].clone());
        let r = self.gen_resume(g, mode, v);
        if self.throwing {
            self.throwing = false;
            let e = self.exc.clone();
            self.exc = Val::Undef;
            if p >= 0 {
                self.settle(p, 2, e);
            }
            return;
        }
        if self.gen_done {
            if p >= 0 {
                self.settle(p, 1, r);
            }
            return;
        }
        // await r: resume with its outcome
        let awaited = if self.class_of(&r) == C_PROMISE {
            obj_of(&r)
        } else {
            let q = self.new_promise();
            self.settle(q, 1, r);
            q
        };
        let ok = self.native_fn("", NF_ASYNC_OK, 1);
        self.objs[ok as usize].env = g;
        let err = self.native_fn("", NF_ASYNC_ERR, 1);
        self.objs[err as usize].env = g;
        self.promise_then(awaited, Val::Obj(ok), Val::Obj(err));
    }

    /// Runs an async generator through its queued requests: each resumes
    /// it, an `await` suspends the run until the awaited promise settles,
    /// a yield or the end answers the first request.
    pub fn agen_run(&mut self, g: int, mode0: int, v0: Val, after_await: bool) {
        let mut mode = mode0;
        let mut v = v0;
        let mut resuming = after_await;
        loop {
            let q = obj_of(&self.objs[g as usize].elems2[G_PROMISE].clone());
            if q < 0 {
                return;
            }
            if !resuming {
                if self.objs[q as usize].elems.is_empty() {
                    return;
                }
                let req = obj_of(&self.objs[q as usize].elems[0].clone());
                mode = num_of(&self.objs[req as usize].elems[0].clone());
                v = self.objs[req as usize].elems[1].clone();
            }
            resuming = false;
            self.temp_roots.push(Val::Obj(g));
            let r = self.gen_resume(g, mode, v.clone());
            self.temp_roots.pop();
            let q2 = obj_of(&self.objs[g as usize].elems2[G_PROMISE].clone());
            if q2 < 0 || self.objs[q2 as usize].elems.is_empty() {
                return;
            }
            let req = obj_of(&self.objs[q2 as usize].elems[0].clone());
            let p = obj_of(&self.objs[req as usize].elems[2].clone());
            if self.throwing {
                self.throwing = false;
                let e = self.exc.clone();
                self.exc = Val::Undef;
                self.objs[q2 as usize].elems.remove(0);
                self.settle(p, 2, e);
                continue;
            }
            if !self.gen_done && self.gen_awaiting {
                self.gen_awaiting = false;
                self.objs[g as usize].elems2[G_AWAIT] = Val::Bool(true);
                let awaited = if self.class_of(&r) == C_PROMISE {
                    obj_of(&r)
                } else {
                    let w = self.new_promise();
                    self.settle(w, 1, r);
                    w
                };
                let ok = self.native_fn("", NF_AGEN_AWAIT_OK, 1);
                self.objs[ok as usize].env = g;
                let err = self.native_fn("", NF_AGEN_AWAIT_ERR, 1);
                self.objs[err as usize].env = g;
                self.promise_then(awaited, Val::Obj(ok), Val::Obj(err));
                return;
            }
            let done = self.gen_done;
            self.objs[q2 as usize].elems.remove(0);
            let res = self.iter_result(r, done);
            self.settle(p, 1, res);
        }
    }

    pub fn call_native_co(&mut self, id: int, fobj: int, this: Val, args: Vec<Val>) -> Val {
        let a0 = if args.is_empty() { Val::Undef } else { args[0].clone() };
        if id == NF_AGEN_AWAIT_OK || id == NF_AGEN_AWAIT_ERR {
            let g = self.objs[fobj as usize].env;
            self.objs[g as usize].elems2[G_AWAIT] = Val::Bool(false);
            self.agen_run(g, if id == NF_AGEN_AWAIT_OK { GM_NEXT } else { GM_THROW }, a0, true);
            return Val::Undef;
        }
        if id == NF_ASYNC_FROM_SYNC {
            self.async_from_sync = obj_of(&a0);
            if self.async_from_sync >= 0 {
                self.roots.push(self.async_from_sync);
            }
            return Val::Undef;
        }
        if id == NF_AGEN_NEXT || id == NF_AGEN_RETURN || id == NF_AGEN_THROW {
            let p = self.new_promise();
            let g = obj_of(&this);
            let ok = g >= 0 && self.objs[g as usize].class == C_GENERATOR && {
                let pi = self.objs[g as usize].func as usize;
                self.protos[pi].is_async && self.protos[pi].generator
            };
            if !ok {
                let e = self.new_error(self.type_error_proto, "AsyncGenerator method called on incompatible receiver");
                self.settle(p, 2, Val::Obj(e));
                return Val::Obj(p);
            }
            let mode = if id == NF_AGEN_RETURN {
                GM_RETURN
            } else if id == NF_AGEN_THROW {
                GM_THROW
            } else {
                GM_NEXT
            };
            let req = self.new_array(vec![Val::Num(mode as double), a0, Val::Obj(p)]);
            let q = obj_of(&self.objs[g as usize].elems2[G_PROMISE].clone());
            self.objs[q as usize].elems.push(Val::Obj(req));
            let idle = self.objs[g as usize].pos != GS_RUNNING && !truthy(&self.objs[g as usize].elems2[G_AWAIT].clone());
            if idle && self.objs[q as usize].elems.len() == 1 {
                self.agen_run(g, GM_NEXT, Val::Undef, false);
            }
            return Val::Obj(p);
        }
        if id == NF_ASYNC_OK || id == NF_ASYNC_ERR {
            let g = self.objs[fobj as usize].env;
            self.async_step(g, if id == NF_ASYNC_OK { GM_NEXT } else { GM_THROW }, a0);
            return Val::Undef;
        }
        let g = obj_of(&this);
        if g < 0 || self.objs[g as usize].class != C_GENERATOR || self.protos[self.objs[g as usize].func as usize].is_async {
            self.throw_type("next method called on incompatible receiver");
            return Val::Undef;
        }
        let mode = if id == NF_GEN_RETURN {
            GM_RETURN
        } else if id == NF_GEN_THROW {
            GM_THROW
        } else {
            GM_NEXT
        };
        let r = self.gen_resume(g, mode, a0);
        if self.throwing {
            return Val::Undef;
        }
        if self.gen_raw && !self.gen_done {
            return r;
        }
        let done = self.gen_done;
        self.iter_result(r, done)
    }
}

impl Vm {
    /// Closes `it` after an exception, which stays the one thrown.
    pub fn iter_close_on_throw(&mut self, it: &Val) {
        let e = self.exc.clone();
        self.throwing = false;
        self.exc = Val::Undef;
        self.temp_roots.push(e.clone());
        self.iter_close(it);
        self.temp_roots.pop();
        self.throwing = true;
        self.exc = e;
    }

    /// `new Map(iterable)` / `new Set(iterable)` the general way: each
    /// entry through `adder`, the iterator closed when one fails.
    pub fn fill_collection(&mut self, m: int, src: &Val, adder: Val, is_map: bool) {
        let it = self.iter_values(src);
        if self.throwing {
            return;
        }
        self.temp_roots.push(it.clone());
        loop {
            let next = self.iter_next(&it);
            if self.throwing {
                break;
            }
            let v = match next {
                Some(x) => x,
                None => break,
            };
            if is_map {
                if !is_obj(&v) {
                    self.throw_type("Iterator value is not an entry object");
                    self.iter_close_on_throw(&it);
                    break;
                }
                let k = self.get_index(&v, 0);
                let val = if self.throwing { Val::Undef } else { self.get_index(&v, 1) };
                if !self.throwing {
                    self.call_value(adder.clone(), Val::Obj(m), vec![k, val]);
                }
            } else {
                self.call_value(adder.clone(), Val::Obj(m), vec![v]);
            }
            if self.throwing {
                self.iter_close_on_throw(&it);
                break;
            }
        }
        self.temp_roots.pop();
    }
}
