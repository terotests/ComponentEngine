// SPDX-License-Identifier: AGPL-3.0-or-later
//! The EVG demo page's C ABI over CErXes (TypeScript + JSX).
//!
//!   cx_alloc(n) -> ptr           a buffer the page writes text into
//!   cx_free(ptr, n)
//!   cx_new() -> engine           a fresh engine (built-ins, prelude, JSX runtime)
//!   cx_new_with(flags) -> engine as cx_new; flags bit 0: the stack machine
//!   cx_eval(engine, ptr, n)      runs a TSX script; 0 ok, 1 uncaught / parse error
//!   cx_call(engine, name, nlen, arg, alen)
//!                                calls the global function `name` with one
//!                                string argument; 0 ok, 1 it threw
//!   cx_result_ptr() / _len()     the last eval's or call's value (a string as
//!                                itself) or its error
//!   cx_output_ptr() / _len()     what console.log printed since the last
//!                                eval or call, as UTF-8 lines
//!   cx_drop(engine)
//!   cx_emit(src, slen, path, plen)
//!                                writes the script as JavaScript (cerxes::emit),
//!                                read as the file `path`'s extension says;
//!                                0 ok (the result is the JavaScript), 1 a
//!                                syntax error (the result is the error)
//!   cx_emit_module(src, slen, path, plen)
//!                                a module the host gives as source
//!                                (defineSourceModule) as the JavaScript
//!                                function that runs it; 0 ok, 1 a syntax
//!                                error
//!   cx_runtime()                 the result is what an emitted script needs
//!                                defined before it runs (the JSX runtime,
//!                                defineModule)
//!
//! Time slices, for the sandbox demo (a script that never ends shares the
//! page with others):
//!
//!   cx_set_slice(engine, ms)     scripts stop where `ms` milliseconds ran
//!                                out (at a branch, call or return); 0: no
//!                                slices
//!   cx_start(engine, ptr, n)     as cx_eval; 2 when the slice ran out
//!   cx_resume(engine)            another slice; 0 done, 1 threw, 2 the
//!                                slice ran out again
//!   cx_set_global(engine, name, nlen, val, vlen)
//!                                sets the global `name` to a string, also
//!                                while a script is suspended (no code runs)
//!   cx_get_global(engine, name, nlen)
//!                                the result is the global's value as text
//!                                (a string as itself)
//!   cx_snapshot(engine)          the result is the engine's whole state as
//!                                text, a suspended script included; 0 ok,
//!                                1 not now (a script is running)
//!   cx_from_snapshot(ptr, n) -> engine
//!                                an engine from that text (0 when it is not
//!                                a snapshot); cx_resume goes on with the
//!                                script where it stopped
//!
//! A frame of the page is a `cx_call`, not an `eval`: an eval compiles its
//! script into the engine for good, a call only runs what is there.

use cerxes::value::*;
use cerxes::Engine;
use std::cell::RefCell;

// small blocks from size-class free lists (dlmalloc costs about a tenth of
// a React update in wasm)
#[path = "../../../cerxes/salloc/salloc.rs"]
mod salloc;

#[global_allocator]
static GLOBAL: salloc::SmallAlloc = salloc::SmallAlloc;

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
    // on the register machine (rvm.rs), as the cerxes CLI with CER_REGS=1
    let mut e = Engine::new();
    e.vm.regs = true;
    Box::into_raw(Box::new(e))
}

/// `cx_new` with a choice of machine, for the playground's variants:
/// `flags` bit 0 runs the engine on the stack machine (vm.rs) instead of the
/// register machine.
#[no_mangle]
pub extern "C" fn cx_new_with(flags: i32) -> *mut Engine {
    let mut e = Engine::new();
    e.vm.regs = flags & 1 == 0;
    Box::into_raw(Box::new(e))
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
    let av = engine.vm.string_val(arg);
    let r = engine.vm.call_value(f, Val::Undef, vec![av]);
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
            Val::Str(s) => engine.vm.sref(*s).to_string(),
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

#[no_mangle]
pub unsafe extern "C" fn cx_emit(p: *const u8, n: usize, pp: *const u8, pn: usize) -> i32 {
    let src = text(p, n);
    let path = text(pp, pn);
    let r = cerxes::emit::emit_js_for_path(src.as_str(), path.as_str());
    let failed = !r.error.is_empty();
    OUTPUT.with(|b| b.borrow_mut().clear());
    RESULT.with(|b| *b.borrow_mut() = if failed { r.error.into_bytes() } else { r.js.into_bytes() });
    if failed {
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn cx_emit_module(p: *const u8, n: usize, pp: *const u8, pn: usize) -> i32 {
    let src = text(p, n);
    let path = text(pp, pn);
    let r = cerxes::emit::emit_module_js(src.as_str(), path.as_str());
    let failed = !r.error.is_empty();
    OUTPUT.with(|b| b.borrow_mut().clear());
    RESULT.with(|b| *b.borrow_mut() = if failed { r.error.into_bytes() } else { r.js.into_bytes() });
    if failed {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn cx_runtime() {
    OUTPUT.with(|b| b.borrow_mut().clear());
    RESULT.with(|b| *b.borrow_mut() = cerxes::emit::runtime().into_bytes());
}

/// The baseline JIT (cerxes::wjit): `on` 1 turns it on for this engine,
/// a function is compiled after `min` back edges. The host must give the
/// import cerxes_jit.install (a build with the `wjit` feature); 1 when this
/// build can run compiled code.
#[no_mangle]
pub unsafe extern "C" fn cx_jit(e: *mut Engine, on: i32, min: i32) -> i32 {
    let engine = &mut *e;
    let ok = cerxes::wjit::available();
    engine.vm.jit_on = on != 0 && ok;
    engine.vm.jit_min = min as i64;
    if ok {
        1
    } else {
        0
    }
}

/// The JIT's counters as the result text (cx_result_ptr / _len).
#[no_mangle]
pub extern "C" fn cx_jit_stats() {
    let s = cerxes::wjit::stats();
    let t = format!(
        "{{\"compiled\":{},\"refused\":{},\"dropped\":{},\"bytes\":{},\"entries\":{},\"opsDone\":{},\"opsAll\":{},\"exits\":{:?}}}",
        s.compiled, s.refused, s.dropped, s.bytes, s.entries, s.ops_done, s.ops_all, s.exits
    );
    let sz = cerxes::wjit::OPSIZE.with(|t| t.get());
    let mut by: Vec<(u64, usize)> = sz.iter().enumerate().filter(|(_, &b)| b > 0).map(|(k, &b)| (b, k)).collect();
    by.sort();
    by.reverse();
    let mut t = t;
    if !by.is_empty() {
        t.pop();
        let parts: Vec<String> = by[..by.len().min(24)].iter().map(|(b, k)| format!("[{},{}]", k, b)).collect();
        t = format!("{},\"opsize\":[{}]}}", t, parts.join(","));
    }
    // SAFETY: read on the VM's thread
    let oc: [u32; 256] = unsafe { *std::ptr::addr_of!(cerxes::wjit::OPCOUNT) };
    let mut byc: Vec<(u32, usize)> = oc.iter().enumerate().filter(|(_, &b)| b > 0).map(|(k, &b)| (b, k)).collect();
    if !byc.is_empty() {
        byc.sort();
        byc.reverse();
        t.pop();
        let parts: Vec<String> = byc.iter().enumerate().filter(|(i, (_, k))| *i < 30 || *k >= 100).map(|(_, x)| x).map(|(b, k)| format!("[{},{}]", k, b)).collect();
        t = format!("{},\"opcount\":[{}]}}", t, parts.join(","));
    }
    let nats = cerxes::wjit::NATS.with(|v| v.take());
    if !nats.is_empty() {
        let mut bn: Vec<(u32, usize)> = nats.iter().enumerate().filter(|(_, &b)| b > 0).map(|(k, &b)| (b, k)).collect();
        bn.sort();
        bn.reverse();
        t.pop();
        let parts: Vec<String> = bn[..bn.len().min(20)].iter().map(|(b, k)| format!("[{},{}]", k, b)).collect();
        t = format!("{},\"nats\":[{}]}}", t, parts.join(","));
    }
    RESULT.with(|b| *b.borrow_mut() = t.into_bytes());
}

/// 0 done, 1 threw or a syntax error, 2 suspended (the slice ran out).
fn slice_status(engine: &mut Engine, value: String) -> i32 {
    if engine.is_suspended() {
        let out = engine.output();
        engine.clear_output();
        OUTPUT.with(|b| *b.borrow_mut() = out.into_bytes());
        RESULT.with(|b| b.borrow_mut().clear());
        return 2;
    }
    finish(engine, value)
}

#[no_mangle]
pub unsafe extern "C" fn cx_set_slice(e: *mut Engine, ms: f64) {
    (*e).set_slice(ms);
}

#[no_mangle]
pub unsafe extern "C" fn cx_start(e: *mut Engine, p: *const u8, n: usize) -> i32 {
    let engine = &mut *e;
    let src = text(p, n);
    let r = engine.eval(src.as_str());
    slice_status(engine, r)
}

#[no_mangle]
pub unsafe extern "C" fn cx_resume(e: *mut Engine) -> i32 {
    let engine = &mut *e;
    engine.error = String::new();
    let r = engine.resume();
    slice_status(engine, r)
}

#[no_mangle]
pub unsafe extern "C" fn cx_set_global(e: *mut Engine, np: *const u8, nn: usize, vp: *const u8, vn: usize) {
    let engine = &mut *e;
    let name = text(np, nn);
    let a = engine.vm.intern(name.as_str());
    let v = engine.vm.string_val(text(vp, vn));
    let g = engine.vm.global;
    engine.vm.set_obj(g, a, v);
}

#[no_mangle]
pub unsafe extern "C" fn cx_get_global(e: *mut Engine, np: *const u8, nn: usize) {
    let engine = &mut *e;
    let name = text(np, nn);
    let a = engine.vm.intern(name.as_str());
    let g = engine.vm.global;
    let v = engine.vm.get_obj(g, a, &Val::Obj(g));
    let s = match &v {
        Val::Str(s) => engine.vm.sref(*s).to_string(),
        _ => engine.vm.display(&v),
    };
    RESULT.with(|b| *b.borrow_mut() = s.into_bytes());
}

#[no_mangle]
pub unsafe extern "C" fn cx_snapshot(e: *mut Engine) -> i32 {
    let s = (*e).snapshot();
    let failed = s.is_empty();
    RESULT.with(|b| *b.borrow_mut() = s.into_bytes());
    if failed {
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn cx_from_snapshot(p: *const u8, n: usize) -> *mut Engine {
    let mut e = Engine::from_snapshot(text(p, n).as_str());
    if !e.error.is_empty() {
        return std::ptr::null_mut();
    }
    e.vm.regs = true;
    Box::into_raw(Box::new(e))
}
