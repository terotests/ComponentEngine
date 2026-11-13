// Runs a script file with CErXes: `cargo run --release --bin cerxes -- app.tsx`.
// The extension picks the syntax (see Engine::set_syntax_for_path).
//
// `cerxes --emit app.tsx` writes the script as JavaScript instead (src/emit.rs),
// `cerxes --runtime` what such a script expects defined before it runs,
// `cerxes --prelude-snapshot > src/prelude_snap.rs` the prelude Engine::new restores.
use cerxes::Engine;

#[path = "../../salloc/salloc.rs"]
mod salloc;

#[global_allocator]
static GLOBAL: salloc::SmallAlloc = salloc::SmallAlloc;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let usage = "usage: cerxes [--emit] <file.ts|.tsx|.jsx|.js> | cerxes --runtime";
    if args.len() > 1 && args[1] == "--runtime" {
        print!("{}", cerxes::emit::runtime());
        return;
    }
    if args.len() > 1 && args[1] == "--prelude-snapshot" {
        // src/prelude_snap.rs: what Engine::new restores instead of
        // compiling, as a raw string for include!
        print!("{}", cerxes::prelude_snapshot_rs(&Engine::compiled(true).snapshot()));
        return;
    }
    if args.len() > 1 && args[1] == "--emit" {
        let path = args.get(2).expect(usage);
        let src = std::fs::read_to_string(path).expect("cannot read the script");
        let r = cerxes::emit::emit_js_for_path(src.as_str(), path.as_str());
        if !r.error.is_empty() {
            eprintln!("{}", r.error);
            std::process::exit(1);
        }
        print!("{}", r.js);
        return;
    }
    let path = args.get(1).expect(usage);
    let src = std::fs::read_to_string(path).expect("cannot read the script");
    // CER_PCOMP=0: JavaScript through the parser and the tree compiler,
    // not the single-pass compiler (pcomp.rs)
    let mut e = Engine::new_with(std::env::var("CER_PCOMP").map(|v| v != "0").unwrap_or(true));
    // CERXES_RC=0: the collector alone frees objects; CERXES_RC_CHECK=1
    // checks the reference counts (slow); CERXES_STATS=1 prints the heap
    if std::env::var("CERXES_RC").map(|v| v == "0").unwrap_or(false) {
        e.vm.rc_on = false;
        e.vm.zct = Vec::new();
        for o in e.vm.objs.iter_mut() {
            o.zct = false;
        }
    }
    e.vm.rc_check = std::env::var("CERXES_RC_CHECK").map(|v| v == "1").unwrap_or(false);
    if std::env::var("CERXES_RC_CHECK").map(|v| v == "stress").unwrap_or(false) {
        e.vm.rc_stress();
    }
    // CER_REGS=1 / 0: on the register machine (rvm.rs) or the stack one
    if let Ok(v) = std::env::var("CER_REGS") {
        e.vm.regs = v == "1";
    }
    // CERXES_PROF=1: the sampling profiler (Vm::prof_tick); the functions
    // that took the most time are printed to stderr at the end
    let prof = std::env::var("CERXES_PROF").is_ok();
    if prof {
        e.vm.prof_on = true;
        e.vm.prof_t = cerxes::builtins::now_ms();
    }
    e.set_syntax_for_path(path.as_str());
    e.set_echo(true);
    // CERXES_SLICE_MS=n: the script runs in slices of n ms, resumed until
    // it ends (what a host that detaches the engine pays for its checks)
    let slice = std::env::var("CERXES_SLICE_MS").ok().and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
    e.set_slice(slice);
    // `--module m.js`: after the script, the module m.js runs (and the
    // modules it imports); `--base f.js`: the script's `import()` reads
    // relative to f.js. The files they name are handed to the loader first.
    let mut module = String::new();
    let mut base = String::new();
    let mut ai = 2;
    while ai + 1 < args.len() {
        if args[ai] == "--module" {
            module = absolute(args[ai + 1].as_str());
        } else if args[ai] == "--base" {
            base = absolute(args[ai + 1].as_str());
        }
        ai += 2;
    }
    if !module.is_empty() || !base.is_empty() {
        let mut files: Vec<(String, String)> = Vec::new();
        if !module.is_empty() {
            preload(module.as_str(), &mut files);
        }
        if !base.is_empty() {
            scan(src.as_str(), base.as_str(), &mut files);
        }
        let mut pre = String::new();
        for (p, t) in files.iter() {
            pre.push_str(format!("__cerxesESM.source({}, {});\n", js_str(p), js_str(t)).as_str());
        }
        if !base.is_empty() {
            pre.push_str(format!("__cerxesRef = {};\n", js_str(base.as_str())).as_str());
        }
        let r = e.eval(pre.as_str());
        if !e.error.is_empty() {
            eprintln!("{}", r);
            std::process::exit(1);
        }
    }
    let mut r = e.eval(src.as_str());
    let mut slices = 1;
    while e.is_suspended() {
        slices += 1;
        r = e.resume();
    }
    if !module.is_empty() && e.error.is_empty() {
        r = e.eval(format!("__cerxesESM.runMain({});", js_str(module.as_str())).as_str());
        while e.is_suspended() {
            r = e.resume();
        }
        if e.error.is_empty() {
            r = e.eval("__cerxesESM.check();");
        }
    }
    if slice > 0.0 {
        eprintln!("slices: {}", slices);
    }
    if prof {
        prof_report(&e, src.as_str());
    }
    // CER_DUMP=name: the register code of the functions of that name
    if let Ok(name) = std::env::var("CER_DUMP") {
        for p in e.vm.protos.iter() {
            if p.rstate == 1 && (name.is_empty() || p.name == name) {
                eprintln!("{}", cerxes::regs::dump(p, &e.vm.rcode));
            }
        }
    }
    if e.vm.rc_check || std::env::var("CERXES_STATS").is_ok() {
        let v = &e.vm;
        let free = v.objs.iter().filter(|o| o.free).count();
        eprintln!(
            "rc: errors={} freed={} reclaims={} gc_runs={} objs={} free={} live={} zct={} sizeof(JsObj)={}",
            v.rc_errors, v.rc_freed, v.reclaims, v.gc_runs, v.objs.len(), free, v.live, v.zct.len(),
            std::mem::size_of::<cerxes::value::JsObj>()
        );
        eprintln!("strs={} free={} shape_epoch={} pic={}", v.strs.len(), v.sfree.len() + v.sfreed.len(), v.shape_epoch, v.pic.len() / 14);
        if std::env::var("CERXES_MEM").is_ok() {
            // a rough heap breakdown in bytes (capacities; malloc headers not counted)
            let vs = std::mem::size_of::<cerxes::value::Val>();
            let (mut vals, mut elems, mut elems2, mut vecs, mut used) = (0usize, 0usize, 0usize, 0usize, 0usize);
            let mut free_cap = 0usize;
            let mut hist = std::collections::BTreeMap::new();
            for o in v.objs.iter() {
                let c = (o.vals.v.capacity() + v.evecs[o.elems as usize].v.capacity() + v.colds[o.cold as usize].elems2.capacity()) * vs;
                vals += o.vals.v.capacity() * vs;
                elems += v.evecs[o.elems as usize].v.capacity() * vs;
                elems2 += v.colds[o.cold as usize].elems2.capacity() * vs;
                used += (o.vals.v.len() + v.evecs[o.elems as usize].v.len() + v.colds[o.cold as usize].elems2.len()) * vs;
                vecs += (o.vals.v.capacity() > 0) as usize + (v.evecs[o.elems as usize].v.capacity() > 0) as usize + (v.colds[o.cold as usize].elems2.capacity() > 0) as usize;
                if o.free { free_cap += c; } else { *hist.entry(o.class).or_insert(0usize) += 1; }
            }
            let objs = v.objs.capacity() * std::mem::size_of::<cerxes::value::JsObj>();
            let strs: usize = v.strs.iter().map(|s| 40 + s.s.capacity()).sum();
            let shapes: usize = v.shapes.iter().map(|s| (s.keys.capacity() + s.attrs.capacity() + s.itab.capacity() + s.t_atom.capacity() + s.t_attr.capacity() + s.t_to.capacity()) * 8 + s.t_map.capacity() * 24 + std::mem::size_of::<cerxes::value::Shape>()).sum();
            let colds: usize = v.colds.iter().map(|c| c.index.capacity() * 24 + c.marks.capacity() * 8 + c.nums.capacity() * 8).sum();
            let code: usize = v.protos.iter().map(|p| p.code.capacity() * std::mem::size_of::<cerxes::value::Op>() + p.consts.capacity() * vs).sum::<usize>() + v.rcode.capacity() * std::mem::size_of::<cerxes::value::Op>();
            let mb = |x: usize| x as f64 / 1048576.0;
            eprintln!("mem MB: objs {:.1} ({} slots) vals {:.1} elems {:.1} elems2 {:.1} (used {:.1}, on free slots {:.1}, vecs {}) strs {:.1} ({}) shapes {:.1} ({}) colds {:.1} ({}) code {:.1} stack {:.1}",
                mb(objs), v.objs.len(), mb(vals), mb(elems), mb(elems2), mb(used), mb(free_cap), vecs, mb(strs), v.strs.len(), mb(shapes), v.shapes.len(), mb(colds), v.colds.len(), mb(code), mb(v.stack.capacity() * vs));
            {
                let (mut fcap, mut lcap, mut llen, mut big, mut shared) = (0usize, 0usize, 0usize, 0usize, 0usize);
                let mut seen = std::collections::HashMap::new();
                let mut dup = 0usize;
                for e in v.strs.iter() {
                    let s = &e.s;
                    if e.flag == 2 { fcap += s.capacity(); continue; }
                    lcap += s.capacity(); llen += s.len();
                    if s.capacity() > 4096 { big += s.capacity(); }
                    if std::rc::Rc::strong_count(s) > 1 { shared += 1; }
                    let n = seen.entry(s.as_str()).or_insert(0usize); *n += 1; if *n > 1 { dup += s.len(); }
                }
                if std::env::var("CERXES_MEM").map(|x| x == "big").unwrap_or(false) {
                    let mut b: Vec<(usize, usize, usize)> = v.strs.iter().enumerate().filter(|(_, e)| e.flag != 2 && e.s.capacity() > 4096).map(|(i, e)| (e.s.capacity(), e.s.len(), i)).collect();
                    b.sort(); b.reverse();
                    for (c, l, i) in b.iter().take(25) {
                        let t: String = v.strs[*i].s.chars().take(50).collect();
                        eprintln!("  big cap {} len {} flag {} rc {} {:?}", c, l, v.strs[*i].flag, std::rc::Rc::strong_count(&v.strs[*i].s), t);
                    }
                    eprintln!("  big count {}", b.len());
                }
                if std::env::var("CERXES_MEM").map(|x| x == "uniq").unwrap_or(false) {
                    let mut m: std::collections::HashMap<usize, (usize, usize, usize, usize)> = std::collections::HashMap::new();
                    for (i, e) in v.strs.iter().enumerate() {
                        if e.flag == 2 { continue; }
                        let p = std::rc::Rc::as_ptr(&e.s) as usize;
                        let x = m.entry(p).or_insert((e.s.capacity(), e.s.len(), 0, i));
                        x.2 += 1;
                    }
                    let mut b: Vec<(usize, usize, usize, usize)> = m.values().cloned().collect();
                    b.sort(); b.reverse();
                    let tot: usize = b.iter().map(|x| x.0).sum();
                    eprintln!("  uniq buffers {} cap {:.1} MB", b.len(), tot as f64 / 1e6);
                    for (c, l, n, i) in b.iter().take(30) {
                        let t: String = v.strs[*i].s.chars().take(40).collect();
                        eprintln!("  uniq cap {} len {} handles {} up {} flag {} {:?}", c, l, n, v.strs[*i].up, v.strs[*i].flag, t);
                    }
                }
                let pre = v.strs.iter().filter(|x| x.up >= 0).count();
                eprintln!("strs: live cap {:.1} MB len {:.1} MB (>4k: {:.1} MB) free-handle cap {:.1} MB; duplicate bytes {:.1} MB; shared Rc {}; prefixes {}",
                    mb(lcap), mb(llen), mb(big), mb(fcap), mb(dup), shared, pre);
            }
            {
                let (mut e2, mut pr, mut hm, mut ps, mut fu, mut en, mut co, mut el) = (0, 0, 0, 0, 0, 0, 0, 0);
                for o in v.objs.iter().filter(|o| !o.free) {
                    if !v.colds[o.cold as usize].elems2.is_empty() { e2 += 1; }
                    if !matches!((v.colds[o.cold as usize].prim).kind(), cerxes::value::VK::Undef) { pr += 1; }
                    if v.colds[o.cold as usize].home >= 0 { hm += 1; }
                    if o.pos != 0 { ps += 1; }
                    if o.func >= 0 { fu += 1; }
                    if o.env >= 0 { en += 1; }
                    if o.cold > 0 { co += 1; }
                    if !v.evecs[o.elems as usize].v.is_empty() { el += 1; }
                }
                eprintln!("fields in use: elems2 {} prim {} home {} pos {} func {} env {} cold {} elems {}", e2, pr, hm, ps, fu, en, co, el);
            }
            {
                let opz = std::mem::size_of::<cerxes::value::Op>();
                let sc: usize = v.protos.iter().map(|p| p.code.capacity() * opz).sum();
                let kc: usize = v.protos.iter().map(|p| p.consts.capacity() * vs).sum();
                eprintln!("code: stack {:.1} MB ({} protos, op {} B) consts {:.1} rcode {:.1} kvals {:.1} intern {}", mb(sc), v.protos.len(), opz, mb(kc), mb(v.rcode.capacity() * opz), mb(v.kvals.capacity() * vs), v.atoms.len());
            }
            let mut h: Vec<(usize, i64)> = hist.iter().map(|(k, n)| (*n, *k as i64)).collect();
            h.sort();
            h.reverse();
            eprintln!("live by class: {:?}", &h[..h.len().min(12)]);
        }
        for l in v.rc_log.iter() {
            eprintln!("{}", l);
        }
    }
    if !e.error.is_empty() {
        eprintln!("{}", r);
        std::process::exit(1);
    }
}

/// The path made absolute and normalized (as the module loader resolves).
fn absolute(p: &str) -> String {
    let full = if p.starts_with('/') {
        String::from(p)
    } else {
        let cwd = std::env::current_dir().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
        format!("{}/{}", cwd, p)
    };
    normalize(full.as_str())
}

fn normalize(p: &str) -> String {
    let parts: Vec<&str> = p.split('/').collect();
    let mut out: Vec<&str> = Vec::new();
    for (i, s) in parts.iter().enumerate() {
        if *s == "." || (s.is_empty() && i > 0 && i < parts.len() - 1) {
            continue;
        }
        if *s == ".." && !out.is_empty() && *out.last().unwrap() != ".." && !out.last().unwrap().is_empty() {
            out.pop();
            continue;
        }
        out.push(s);
    }
    out.join("/")
}

/// The file and, recursively, the .js files its string literals name.
fn preload(path: &str, files: &mut Vec<(String, String)>) {
    if files.iter().any(|(p, _)| p == path) {
        return;
    }
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => return,
    };
    files.push((String::from(path), text.clone()));
    scan(text.as_str(), path, files);
}

/// Preloads the .js files that string literals in `text` name, relative
/// to `from`.
fn scan(text: &str, from: &str, files: &mut Vec<(String, String)>) {
    let dir = match from.rfind('/') {
        Some(i) => &from[..i + 1],
        None => "",
    };
    let cs: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < cs.len() {
        let q = cs[i];
        if q == '\'' || q == '"' {
            let mut j = i + 1;
            let mut lit = String::new();
            while j < cs.len() && cs[j] != q && cs[j] != '\n' {
                if cs[j] == '\\' {
                    j += 1;
                }
                if j < cs.len() {
                    lit.push(cs[j]);
                }
                j += 1;
            }
            if lit.ends_with(".js") && !lit.contains(' ') {
                let p = if lit.starts_with('/') { normalize(lit.as_str()) } else { normalize(format!("{}{}", dir, lit).as_str()) };
                preload(p.as_str(), files);
            }
            i = j + 1;
            continue;
        }
        i += 1;
    }
}

/// A JavaScript string literal of s.
fn js_str(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\u{2028}' => o.push_str("\\u2028"),
            '\u{2029}' => o.push_str("\\u2029"),
            c if (c as u32) < 0x20 => o.push_str(format!("\\u{:04x}", c as u32).as_str()),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// CERXES_PROF's table: self and inclusive ms per function, the top 40 by
/// self time, with the script line where the function starts (functions of
/// the prelude count lines in the prelude).
fn prof_report(e: &Engine, src: &str) {
    let v = &e.vm;
    let total: f64 = v.prof_self.iter().sum();
    let mut idx: Vec<usize> = (0..v.prof_self.len()).filter(|&i| v.prof_self[i] > 0.0).collect();
    idx.sort_by(|a, b| v.prof_self[*b].partial_cmp(&v.prof_self[*a]).unwrap());
    let starts: Vec<usize> = src.char_indices().filter(|(_, c)| *c == '\n').map(|(i, _)| i).collect();
    eprintln!("profile: {:.1} ms sampled", total);
    eprintln!("  self ms   self %   incl ms  function");
    for &i in idx.iter().take(40) {
        let p = &v.protos[i];
        let at = p.src_start.max(0) as usize;
        let line = starts.partition_point(|&s| s < at) + 1;
        let name = if p.name.is_empty() { String::from("<anonymous>") } else { p.name.clone() };
        eprintln!("{:9.1} {:7.1}% {:9.1}  {} :{}", v.prof_self[i], 100.0 * v.prof_self[i] / total.max(1e-9), v.prof_incl[i], name, line);
    }
}
