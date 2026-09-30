// SPDX-License-Identifier: AGPL-3.0-or-later
//! The EVG demo page's C ABI over CErXes (TypeScript + JSX).
//!
//!   cx_alloc(n) -> ptr           a buffer the page writes text into
//!   cx_free(ptr, n)
//!   cx_new() -> engine           a fresh engine (built-ins, prelude, JSX runtime)
//!   cx_eval(engine, ptr, n)      runs a TSX script; 0 ok, 1 uncaught / parse error
//!   cx_call(engine, name, nlen, arg, alen)
//!                                calls the global function `name` with one
//!                                string argument; 0 ok, 1 it threw
//!   cx_result_ptr() / _len()     the last eval's or call's value (a string as
//!                                itself) or its error
//!   cx_output_ptr() / _len()     what console.log printed since the last
//!                                eval or call, as UTF-8 lines
//!   cx_drop(engine)
//!
//! A frame of the page is a `cx_call`, not an `eval`: an eval compiles its
//! script into the engine for good, a call only runs what is there.

use cerxes::value::*;
use cerxes::Engine;
use std::cell::RefCell;

thread_local! {
    static RESULT: RefCell<Vec<u8>> = RefCell::new(Vec::new());
    static OUTPUT: RefCell<Vec<u8>> = RefCell::new(Vec::new());
}

fn text(p: *const u8, n: usize) -> String {
    unsafe { String::from_utf8_lossy(std::slice::from_raw_parts(p, n)).into_owned() }
}

fn finish(engine: &mut Engine, value: String) -> i32 {
    let failed = !engine.error.is_empty();
    let out = engine.output();
    engine.clear_output();
    OUTPUT.with(|b| *b.borrow_mut() = out.into_bytes());
    RESULT.with(|b| *b.borrow_mut() = value.into_bytes());
    if failed {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn cx_alloc(n: usize) -> *mut u8 {
    let mut v: Vec<u8> = Vec::with_capacity(n.max(1));
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

#[no_mangle]
pub unsafe extern "C" fn cx_free(p: *mut u8, n: usize) {
    drop(Vec::from_raw_parts(p, 0, n.max(1)));
}

#[no_mangle]
pub extern "C" fn cx_new() -> *mut Engine {
    Box::into_raw(Box::new(Engine::new()))
}

#[no_mangle]
pub unsafe extern "C" fn cx_drop(e: *mut Engine) {
    drop(Box::from_raw(e));
}

#[no_mangle]
pub unsafe extern "C" fn cx_eval(e: *mut Engine, p: *const u8, n: usize) -> i32 {
    let engine = &mut *e;
    let src = text(p, n);
    let r = engine.eval(src.as_str());
    finish(engine, r)
}

#[no_mangle]
pub unsafe extern "C" fn cx_call(e: *mut Engine, np: *const u8, nn: usize, ap: *const u8, an: usize) -> i32 {
    let engine = &mut *e;
    engine.error = String::new();
    let name = text(np, nn);
    let arg = text(ap, an);
    let a = engine.vm.intern(name.as_str());
    let g = engine.vm.global;
    let f = engine.vm.get_obj(g, a, &Val::Obj(g));
    if !engine.vm.is_callable(&f) {
        engine.error = format!("{} is not a function", name);
        let msg = engine.error.clone();
        return finish(engine, msg);
    }
    // from the host: the collector may run during the frame (through
    // call_value it could not, and the heap grew until the page ran out)
    let r = engine.vm.call_from_host(f, Val::Undef, vec![string_val(arg)]);
    let value = if engine.vm.throwing {
        let exc = engine.vm.exc.clone();
        engine.vm.throwing = false;
        engine.vm.exc = Val::Undef;
        engine.vm.stack.clear();
        engine.vm.frames.clear();
        engine.vm.handlers.clear();
        engine.vm.native_depth = 0;
        let s = engine.vm.display(&exc);
        engine.error = format!("Uncaught {}", s);
        engine.error.clone()
    } else {
        engine.vm.run_jobs();
        match &r {
            Val::Str(s) => s.as_ref().clone(),
            _ => engine.vm.display(&r),
        }
    };
    finish(engine, value)
}

#[no_mangle]
pub extern "C" fn cx_result_ptr() -> *const u8 {
    RESULT.with(|b| b.borrow().as_ptr())
}

#[no_mangle]
pub extern "C" fn cx_result_len() -> usize {
    RESULT.with(|b| b.borrow().len())
}

#[no_mangle]
pub extern "C" fn cx_output_ptr() -> *const u8 {
    OUTPUT.with(|b| b.borrow().as_ptr())
}

#[no_mangle]
pub extern "C" fn cx_output_len() -> usize {
    OUTPUT.with(|b| b.borrow().len())
}
