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
        for l in v.rc_log.iter() {
            eprintln!("{}", l);
        }
    }
    if !e.error.is_empty() {
        eprintln!("{}", r);
        std::process::exit(1);
    }
}
