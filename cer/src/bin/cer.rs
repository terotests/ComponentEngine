// Runs a script file with CEr: `cargo run --release --bin cer -- file.js`
use cer::Engine;

fn main() {
    let path = std::env::args().nth(1).expect("usage: cer <file.js>");
    let src = std::fs::read_to_string(&path).expect("cannot read the script");
    // compile in one pass from the tokens (pcomp.rs); CER_PCOMP=0: the tree compiler
    let pcomp = std::env::var("CER_PCOMP").map(|v| v != "0").unwrap_or(true);
    let mut e = Engine::new_with(pcomp);
    // CER_REGS=1 / 0: on the register machine (rvm.rs) or the stack one
    if let Ok(v) = std::env::var("CER_REGS") {
        e.vm.regs = v == "1";
    }
    e.set_echo(true);
    let r = e.eval(src.as_str());
    // CER_RDUMP=1: the register code of the functions that ran
    if std::env::var("CER_RDUMP").map(|v| v == "1").unwrap_or(false) {
        let first = e.vm.prelude_protos as usize;
        for p in e.vm.protos.iter().skip(first) {
            if p.rstate == 1 || std::env::var("CER_RDUMP_ALL").is_ok() {
                eprintln!("{}", cer::regs::dump(p, &e.vm.rcode));
            }
        }
    }
    if !e.error.is_empty() {
        eprintln!("{}", r);
        std::process::exit(1);
    }
}
