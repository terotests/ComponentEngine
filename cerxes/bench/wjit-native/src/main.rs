// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The baseline JIT (cer/src/wjit.rs) outside the browser: the same
// wasm32-wasip1 CErXes build that cerxes/bench/wjit.mjs runs in Node, in a
// native wasmtime host. CErXes' own code and every module the JIT writes are
// compiled to machine code by Cranelift (ahead of the run for CErXes, at
// install time for the JIT's modules), so the JIT's output runs as real
// native code without V8. A measurement tool, not part of the product.
//
//   wjit-native [--jit=0|1] [--min=N] [--mask=a,b] [--wasm=path] [--runs=N] file.js...
//
// Prints each file's answer and output, and the eval time of the best run.

use anyhow::{anyhow, Result};
use std::time::Instant;
use wasmtime::*;
use wasmtime_wasi::preview1::WasiP1Ctx;

const HELPERS: [&str; 8] = ["get_elem", "set_elem", "get_prop", "set_prop", "op", "call", "slow", "ret"];

struct Host {
    wasi: WasiP1Ctx,
    /// the runtime module (look, codes, hstore, callx...), imported by every
    /// batch module as "rt"
    rt: Option<Instance>,
    helpers: Vec<Func>,
    modules: u32,
    evicted: u32,
    compile_s: f64,
}

fn table_of(c: &mut Caller<'_, Host>) -> Table {
    c.get_export("__indirect_function_table").and_then(|e| e.into_table()).expect("table export")
}

fn install(mut c: Caller<'_, Host>, ptr: i32, len: i32) -> Result<i32> {
    let t0 = Instant::now();
    let mem = c.get_export("memory").and_then(|e| e.into_memory()).ok_or_else(|| anyhow!("memory"))?;
    let table = table_of(&mut c);
    let bytes = mem.data(&c)[ptr as usize..(ptr + len) as usize].to_vec();
    let module = Module::new(c.engine(), &bytes).map_err(|e| {
        let _ = std::fs::write("/tmp/wjit-bad.wasm", &bytes);
        anyhow!("wjit: invalid module (/tmp/wjit-bad.wasm): {e}")
    })?;
    if c.data().helpers.is_empty() {
        // from the function table: an export of a wasip1 module runs the
        // constructors and destructors around every call
        let hf = c.get_export("cerxes_jit_helper").and_then(|e| e.into_func()).ok_or_else(|| anyhow!("cerxes_jit_helper"))?;
        let hf = hf.typed::<i32, i32>(&c)?;
        let mut hs = Vec::new();
        for k in 0..HELPERS.len() {
            let i = hf.call(&mut c, k as i32)?;
            let r = table.get(&mut c, i as u64).ok_or_else(|| anyhow!("helper slot"))?;
            hs.push(*r.unwrap_func().ok_or_else(|| anyhow!("helper null"))?);
        }
        c.data_mut().helpers = hs;
    }
    let mut imports = Vec::new();
    for imp in module.imports() {
        let e: Extern = match (imp.module(), imp.name()) {
            ("env", "memory") => mem.into(),
            ("env", "table") => table.into(),
            ("env", h) => {
                let k = HELPERS.iter().position(|x| *x == h).ok_or_else(|| anyhow!("unknown import env.{h}"))?;
                c.data().helpers[k].into()
            }
            ("rt", n) => {
                let rt = c.data().rt.ok_or_else(|| anyhow!("rt import before the runtime module"))?;
                rt.get_export(&mut c, n).ok_or_else(|| anyhow!("rt.{n}"))?
            }
            (m, n) => return Err(anyhow!("unknown import {m}.{n}")),
        };
        imports.push(e);
    }
    let inst = Instance::new(&mut c, &module, &imports)?;
    if inst.get_export(&mut c, "look").is_some() {
        c.data_mut().rt = Some(inst);
        c.data_mut().compile_s += t0.elapsed().as_secs_f64();
        return Ok(0);
    }
    // a batch: f0, f1, ... into consecutive slots; the first one's index
    let mut fs = Vec::new();
    while let Some(f) = inst.get_func(&mut c, &format!("f{}", fs.len())) {
        fs.push(f);
    }
    let idx = table.grow(&mut c, fs.len() as u64, Ref::Func(None))?;
    for (k, f) in fs.into_iter().enumerate() {
        table.set(&mut c, idx + k as u64, Ref::Func(Some(f)))?;
    }
    let h = c.data_mut();
    h.modules += 1;
    h.compile_s += t0.elapsed().as_secs_f64();
    Ok(idx as i32)
}

// an evicted module's slots get the runtime's `gone` (back to the
// interpreter at once), so that nothing holds the module any more
fn release(mut c: Caller<'_, Host>, base: i32, n: i32) -> Result<()> {
    let table = table_of(&mut c);
    let rt = c.data().rt.ok_or_else(|| anyhow!("release before rt"))?;
    let gone = rt.get_func(&mut c, "gone").ok_or_else(|| anyhow!("rt.gone"))?;
    for k in 0..n {
        table.set(&mut c, (base + k) as u64, Ref::Func(Some(gone)))?;
    }
    c.data_mut().evicted += n as u32;
    Ok(())
}

fn opt<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    let p = format!("--{name}=");
    args.iter().find_map(|a| a.strip_prefix(p.as_str()))
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let default_wasm = here.join("../../../playground/cerxes-wasm/target-jit/wasm32-wasip1/release/cerxes_wasm.wasm");
    let wasm = opt(&args, "wasm").map(std::path::PathBuf::from).unwrap_or(default_wasm);
    let jit = opt(&args, "jit").unwrap_or("1") == "1";
    let min: i32 = opt(&args, "min").unwrap_or("64").parse()?;
    let runs: u32 = opt(&args, "runs").unwrap_or("1").parse()?;
    let quiet = args.iter().any(|a| a == "--quiet");
    let mask: Vec<u32> = opt(&args, "mask").map(|m| m.split(',').filter(|s| !s.is_empty()).map(|s| s.parse().unwrap()).collect()).unwrap_or_default();
    let files: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();

    let mut cfg = Config::new();
    cfg.cranelift_opt_level(OptLevel::Speed);
    let engine = Engine::new(&cfg)?;
    let t0 = Instant::now();
    let cerxes = Module::from_file(&engine, &wasm)?;
    eprintln!("cerxes.wasm compiled in {:.0} ms", t0.elapsed().as_secs_f64() * 1e3);

    let mut linker: Linker<Host> = Linker::new(&engine);
    wasmtime_wasi::preview1::add_to_linker_sync(&mut linker, |h: &mut Host| &mut h.wasi)?;
    linker.func_wrap("cerxes_jit", "install", install)?;
    linker.func_wrap("cerxes_jit", "release", release)?;

    for f in files {
        let src = std::fs::read(f)?;
        let mut best = f64::INFINITY;
        for run in 0..runs {
            let wasi = wasmtime_wasi::WasiCtxBuilder::new().inherit_stdout().inherit_stderr().build_p1();
            let mut store = Store::new(&engine, Host { wasi, rt: None, helpers: Vec::new(), modules: 0, evicted: 0, compile_s: 0.0 });
            let inst = linker.instantiate(&mut store, &cerxes)?;
            if let Some(init) = inst.get_typed_func::<(), ()>(&mut store, "_initialize").ok() {
                init.call(&mut store, ())?;
            }
            let mem = inst.get_memory(&mut store, "memory").unwrap();
            let e = inst.get_typed_func::<(), i32>(&mut store, "cx_new")?.call(&mut store, ())?;
            let (mut lo, mut hi) = (0i32, 0i32);
            for k in &mask {
                if *k < 32 { lo |= 1 << k } else { hi |= 1 << (k - 32) }
            }
            inst.get_typed_func::<(i32, i32), ()>(&mut store, "cerxes_jit_mask")?.call(&mut store, (lo, hi))?;
            let ok = inst.get_typed_func::<(i32, i32, i32), i32>(&mut store, "cx_jit")?.call(&mut store, (e, jit as i32, min))?;
            if jit && ok == 0 {
                return Err(anyhow!("this build cannot run compiled code"));
            }
            let p = inst.get_typed_func::<i32, i32>(&mut store, "cx_alloc")?.call(&mut store, src.len() as i32)?;
            mem.data_mut(&mut store)[p as usize..p as usize + src.len()].copy_from_slice(&src);
            let t = Instant::now();
            let failed = inst.get_typed_func::<(i32, i32, i32), i32>(&mut store, "cx_eval")?.call(&mut store, (e, p, src.len() as i32))?;
            let dt = t.elapsed().as_secs_f64();
            best = best.min(dt);
            let text = |store: &mut Store<Host>, pn: &str, ln: &str| -> Result<String> {
                let p = inst.get_typed_func::<(), i32>(&mut *store, pn)?.call(&mut *store, ())? as usize;
                let n = inst.get_typed_func::<(), i32>(&mut *store, ln)?.call(&mut *store, ())? as usize;
                Ok(String::from_utf8_lossy(&mem.data(&*store)[p..p + n]).into_owned())
            };
            let value = text(&mut store, "cx_result_ptr", "cx_result_len")?;
            let output = text(&mut store, "cx_output_ptr", "cx_output_len")?;
            if !quiet || run + 1 == runs {
                if !output.is_empty() {
                    println!("{}", output.trim_end());
                }
                println!("{f}: {}{}", if failed != 0 { "FAILED " } else { "" }, value.chars().take(200).collect::<String>());
                let h = store.data();
                println!("time {:.1} ms (best {:.1}), jit {} modules {} evicted {} compile {:.1} ms", dt * 1e3, best * 1e3, jit, h.modules, h.evicted, h.compile_s * 1e3);
            }
        }
    }
    Ok(())
}
