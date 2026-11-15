// snapfix FILE.js: runs a script to its end, then snapshots, restores and
// snapshots again a few times; prints where each snapshot first differs
// from the one before it (a snapshot of a restored engine should be the
// same text: the snapshot-roundtrip oracle).
use cerxes::Engine;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let src = std::fs::read_to_string(&a[1]).expect("read");
    let mut e = Engine::new();
    e.set_syntax(false, false);
    e.set_slice(50.0);
    e.eval(&src);
    while e.is_suspended() {
        e.resume();
    }
    let mut prev = e.snapshot();
    println!("snapshot 0: {} chars; again without restore: {}", prev.len(), if e.snapshot() == prev { "same" } else { "DIFFERENT" });
    for round in 1..4 {
        let mut e2 = Engine::from_snapshot(&prev);
        e2.syntax = e.syntax.clone();
        let s = e2.snapshot();
        if s == prev {
            println!("snapshot {}: same", round);
        } else {
            let at = prev.chars().zip(s.chars()).take_while(|(x, y)| x == y).count();
            let ctx = |t: &str| t.chars().skip(at.saturating_sub(30)).take(80).collect::<String>();
            println!("snapshot {}: {} chars, differs at {}\n  {:?}\n  {:?}", round, s.len(), at, ctx(&prev), ctx(&s));
        }
        prev = s;
        e = e2;
    }
}
