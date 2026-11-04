// Compile time of a script: `ctime <file.js> [tree|pcomp] [times]` compiles
// it (no run) with the tree compiler (parser.rs + compiler.rs) or in one
// pass (pcomp.rs) and prints the time per compile.
use cer::Engine;

fn main() {
    let path = std::env::args().nth(1).expect("usage: ctime <file.js> [tree|pcomp] [times]");
    let mode = std::env::args().nth(2).unwrap_or(String::from("pcomp"));
    let times: usize = std::env::args().nth(3).map(|s| s.parse().unwrap_or(10)).unwrap_or(10);
    let src = std::fs::read_to_string(&path).expect("cannot read the script");
    let mut e = Engine::new();
    e.vm.pcomp = mode == "pcomp";
    let t0 = std::time::Instant::now();
    let mut ops = 0;
    for _ in 0..times {
        let u = e.compile(src.as_str());
        if !e.error.is_empty() {
            eprintln!("{}", e.error);
            std::process::exit(1);
        }
        ops = u.protos.iter().map(|p| p.code.len()).sum::<usize>();
    }
    let ms = t0.elapsed().as_secs_f64() * 1000.0 / times as f64;
    println!("{} {}: {:.2} ms per compile, {} ops", path, mode, ms, ops);
}
