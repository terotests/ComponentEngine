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
    if !e.error.is_empty() {
        eprintln!("{}", r);
        std::process::exit(1);
    }
}
