// snaprun FILE.js [slice_ms]: runs a script in slices, snapshotting the engine
// at every suspend and restoring it into a fresh Engine before resuming.
// Prints the console lines; exit 1 on an uncaught error (like the cerxes CLI).
use cerxes::Engine;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let src = std::fs::read_to_string(&a[1]).expect("read");
    let slice: f64 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(0.05);
    let mut e = Engine::new();
    e.set_syntax(false, false);
    e.set_slice(slice);
    let mut r = e.eval(&src);
    let mut n = 0u64;
    while e.is_suspended() {
        for i in 0..e.output_count() { println!("{}", e.output_at(i)); }
        e.clear_output();
        let snap = e.snapshot();
        if !snap.is_empty() {
            let mut e2 = Engine::from_snapshot(&snap);
            if !e2.error.is_empty() { eprintln!("from_snapshot: {}", e2.error); std::process::exit(3); }
            e2.set_slice(slice);
            e2.syntax = e.syntax.clone();
            e = e2;
            n += 1;
        }
        r = e.resume();
    }
    for i in 0..e.output_count() { println!("{}", e.output_at(i)); }
    eprintln!("snapshots: {}", n);
    if !e.error.is_empty() { eprintln!("Uncaught {}", r); std::process::exit(1); }
}
