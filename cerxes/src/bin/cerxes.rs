// Runs a script file with CErXes: `cargo run --release --bin cerxes -- app.tsx`.
// The extension picks the syntax (see Engine::set_syntax_for_path).
//
// `cerxes --emit app.tsx` writes the script as JavaScript instead (src/emit.rs),
// `cerxes --runtime` what such a script expects defined before it runs.
use cerxes::Engine;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let usage = "usage: cerxes [--emit] <file.ts|.tsx|.jsx|.js> | cerxes --runtime";
    if args.len() > 1 && args[1] == "--runtime" {
        print!("{}", cerxes::emit::runtime());
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
    let mut e = Engine::new();
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
    // CER_PCOMP=1: eval and Function code through the single-pass compiler
    if let Ok(v) = std::env::var("CER_PCOMP") {
        e.vm.pcomp = v == "1";
    }
    e.set_syntax_for_path(path.as_str());
    e.set_echo(true);
    let r = e.eval(src.as_str());
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
                let c = (o.vals.capacity() + o.elems.capacity() + v.colds[o.cold as usize].elems2.capacity()) * vs;
                vals += o.vals.capacity() * vs;
                elems += o.elems.capacity() * vs;
                elems2 += v.colds[o.cold as usize].elems2.capacity() * vs;
                used += (o.vals.len() + o.elems.len() + v.colds[o.cold as usize].elems2.len()) * vs;
                vecs += (o.vals.capacity() > 0) as usize + (o.elems.capacity() > 0) as usize + (v.colds[o.cold as usize].elems2.capacity() > 0) as usize;
                if o.free { free_cap += c; } else { *hist.entry(o.class).or_insert(0usize) += 1; }
            }
            let objs = v.objs.capacity() * std::mem::size_of::<cerxes::value::JsObj>();
            let strs: usize = v.strs.iter().map(|s| 40 + s.capacity()).sum();
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
                for (i, s) in v.strs.iter().enumerate() {
                    if v.sflag[i] == 2 { fcap += s.capacity(); continue; }
                    lcap += s.capacity(); llen += s.len();
                    if s.capacity() > 4096 { big += s.capacity(); }
                    if std::rc::Rc::strong_count(s) > 1 { shared += 1; }
                    let n = seen.entry(s.as_str()).or_insert(0usize); *n += 1; if *n > 1 { dup += s.len(); }
                }
                if std::env::var("CERXES_MEM").map(|x| x == "big").unwrap_or(false) {
                    let mut b: Vec<(usize, usize, usize)> = v.strs.iter().enumerate().filter(|(i, s)| v.sflag[*i] != 2 && s.capacity() > 4096).map(|(i, s)| (s.capacity(), s.len(), i)).collect();
                    b.sort(); b.reverse();
                    for (c, l, i) in b.iter().take(25) {
                        let t: String = v.strs[*i].chars().take(50).collect();
                        eprintln!("  big cap {} len {} flag {} rc {} {:?}", c, l, v.sflag[*i], std::rc::Rc::strong_count(&v.strs[*i]), t);
                    }
                    eprintln!("  big count {}", b.len());
                }
                let pre = v.sup.iter().filter(|x| **x >= 0).count();
                eprintln!("strs: live cap {:.1} MB len {:.1} MB (>4k: {:.1} MB) free-handle cap {:.1} MB; duplicate bytes {:.1} MB; shared Rc {}; prefixes {}",
                    mb(lcap), mb(llen), mb(big), mb(fcap), mb(dup), shared, pre);
            }
            {
                let (mut e2, mut pr, mut hm, mut ps, mut fu, mut en, mut co, mut el) = (0, 0, 0, 0, 0, 0, 0, 0);
                for o in v.objs.iter().filter(|o| !o.free) {
                    if !v.colds[o.cold as usize].elems2.is_empty() { e2 += 1; }
                    if !matches!(v.colds[o.cold as usize].prim, cerxes::value::Val::Undef) { pr += 1; }
                    if v.colds[o.cold as usize].home >= 0 { hm += 1; }
                    if o.pos != 0 { ps += 1; }
                    if o.func >= 0 { fu += 1; }
                    if o.env >= 0 { en += 1; }
                    if o.cold > 0 { co += 1; }
                    if !o.elems.is_empty() { el += 1; }
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
