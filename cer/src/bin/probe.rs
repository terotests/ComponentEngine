// The conformance probes (bench/conformance.mjs): reads records separated
// by U+0001, each `name` U+0002 `script`, runs every script in a fresh
// engine and prints `name<TAB>result` per line (newlines in the result
// escaped as \n). A probe that crashes the engine answers `panic`.
use cer::Engine;

fn main() {
    let path = std::env::args().nth(1).expect("usage: probe <records>");
    let text = std::fs::read_to_string(&path).expect("cannot read the records");
    std::panic::set_hook(Box::new(|_| {}));
    // CER_FROM_SNAPSHOT=1: each probe in an engine made from a snapshot of
    // a new one (snapshot.rs)
    let snap = if std::env::var("CER_FROM_SNAPSHOT").map(|v| v == "1").unwrap_or(false) { Engine::new().snapshot() } else { String::new() };
    for rec in text.split('\u{1}') {
        if rec.is_empty() {
            continue;
        }
        let mut parts = rec.splitn(2, '\u{2}');
        let name = parts.next().unwrap_or("").to_string();
        let src = parts.next().unwrap_or("").to_string();
        let snap = snap.clone();
        let r = std::panic::catch_unwind(move || {
            let mut e = if snap.is_empty() { Engine::new() } else { Engine::from_snapshot(&snap) };
            // CER_THROUGH_TEXT=1: each probe from its unit's text, as a
            // host's code cache runs a script
            e.through_text = std::env::var("CER_THROUGH_TEXT").map(|v| v == "1").unwrap_or(false);
            // CER_REGS=1 / 0: on the register machine (rvm.rs) or the stack one
            if let Ok(v) = std::env::var("CER_REGS") {
        e.vm.regs = v == "1";
    }
            // CER_RC_CHECK=1: a reclaim at every safe point, each checked
            // against a full mark (Vm::rc_stress)
            let check = std::env::var("CER_RC_CHECK").map(|v| v == "1").unwrap_or(false);
            if check {
                e.vm.rc_stress();
            }
            let r = e.eval_typed(src.as_str());
            if check && e.vm.rc_errors > 0 {
                format!("rc-error {}: {}", e.vm.rc_errors, e.vm.rc_log.join("; "))
            } else {
                r
            }
        })
        .unwrap_or(String::from("panic"));
        println!("{}\t{}", name, r.replace('\\', "\\\\").replace('\n', "\\n"));
    }
}
