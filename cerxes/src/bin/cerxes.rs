// Runs a script file with CErXes: `cargo run --release --bin cerxes -- app.tsx`.
// The extension picks the syntax (see Engine::set_syntax_for_path).
use cerxes::Engine;

fn main() {
    let path = std::env::args().nth(1).expect("usage: cerxes <file.ts|.tsx|.jsx|.js>");
    let src = std::fs::read_to_string(&path).expect("cannot read the script");
    let mut e = Engine::new();
    e.set_syntax_for_path(path.as_str());
    e.set_echo(true);
    let r = e.eval(src.as_str());
    if !e.error.is_empty() {
        eprintln!("{}", r);
        std::process::exit(1);
    }
}
