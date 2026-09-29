// SPDX-License-Identifier: AGPL-3.0-or-later
//! The playground's C ABI over CEr.
//!
//!   cer_alloc(n) -> ptr          a buffer the page writes a script into
//!   cer_free(ptr, n)
//!   cer_new() -> engine          a fresh engine (built-ins and prelude run)
//!   cer_eval(engine, ptr, n)     runs the script; 0 ok, 1 uncaught / parse error
//!   cer_result_ptr() / _len()    what the last eval printed, then its value
//!                                or its error, as UTF-8 lines
//!   cer_drop(engine)

use cer::Engine;
use std::cell::RefCell;

thread_local! {
    static RESULT: RefCell<Vec<u8>> = RefCell::new(Vec::new());
}

#[no_mangle]
pub extern "C" fn cer_alloc(n: usize) -> *mut u8 {
    let mut v: Vec<u8> = Vec::with_capacity(n.max(1));
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

#[no_mangle]
pub unsafe extern "C" fn cer_free(p: *mut u8, n: usize) {
    drop(Vec::from_raw_parts(p, 0, n.max(1)));
}

#[no_mangle]
pub extern "C" fn cer_new() -> *mut Engine {
    Box::into_raw(Box::new(Engine::new()))
}

#[no_mangle]
pub unsafe extern "C" fn cer_drop(e: *mut Engine) {
    drop(Box::from_raw(e));
}

#[no_mangle]
pub unsafe extern "C" fn cer_eval(e: *mut Engine, p: *const u8, n: usize) -> i32 {
    let engine = &mut *e;
    let src = String::from_utf8_lossy(std::slice::from_raw_parts(p, n)).into_owned();
    engine.clear_output();
    let r = engine.eval(src.as_str());
    let failed = !engine.error.is_empty();
    let mut text = engine.output();
    if failed {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&r);
    }
    RESULT.with(|b| *b.borrow_mut() = text.into_bytes());
    if failed {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn cer_result_ptr() -> *const u8 {
    RESULT.with(|b| b.borrow().as_ptr())
}

#[no_mangle]
pub extern "C" fn cer_result_len() -> usize {
    RESULT.with(|b| b.borrow().len())
}
