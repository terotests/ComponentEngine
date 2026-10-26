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
    e.set_syntax_for_path(path.as_str());
    e.set_echo(true);
    let r = e.eval(src.as_str());
    if std::env::var("CERXES_STATS").is_ok() {
        e.vm.gc();
        let v = &e.vm;
        let mut free = 0usize; let mut vc = 0usize; let mut ec = 0usize; let mut hm = 0usize;
        for o in v.objs.iter() { if o.free { free += 1; } vc += o.vals.capacity(); ec += o.elems.capacity() + o.elems2.capacity(); hm += o.c().index.capacity(); }
        let mut cls = [(0usize,0usize,0usize,0usize,0usize);24];
        for o in v.objs.iter() { if o.free { continue; } let c = &mut cls[o.class as usize]; c.0 += 1; c.1 += o.vals.len(); c.2 += o.vals.capacity(); c.3 += o.elems.len(); c.4 += o.elems.capacity(); }
        for (i, c) in cls.iter().enumerate() { if c.0 > 0 { eprintln!("class {} n={} vals={}/{} elems={}/{}", i, c.0, c.1, c.2, c.3, c.4); } }
        eprintln!("stats sizeof(JsObj)={} sizeof(Val)={} objs={} free={} gc_runs={} vals_cap={} elems_cap={} index_cap={} shapes={}",
            std::mem::size_of::<cerxes::value::JsObj>(), std::mem::size_of::<cerxes::value::Val>(), v.objs.len(), free, v.gc_runs, vc, ec, hm, v.shapes.len());
    }
    if !e.error.is_empty() {
        eprintln!("{}", r);
        std::process::exit(1);
    }
}
