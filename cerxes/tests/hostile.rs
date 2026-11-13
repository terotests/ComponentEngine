// CErXes: snapshots and code-cache units that were tampered with are refused
// before they run (snapshot::restore, unit::decode -> unit::code_ok).
use cerxes::Engine;

fn engine() -> Engine {
    let mut e = Engine::new();
    e.vm.regs = true;
    e.set_syntax(false, false);
    e
}

fn refused(text: &str, what: &str) {
    let e = Engine::from_snapshot(text);
    assert_eq!(e.error, "not a snapshot", "{}", what);
}

#[test]
fn a_good_snapshot_still_restores() {
    let mut e = engine();
    e.set_slice(0.01);
    e.eval("var s = 0; for (var i = 0; i < 1e7; i++) { s += i; } s");
    assert!(e.is_suspended());
    let mut e2 = Engine::from_snapshot(&e.snapshot());
    assert!(e2.error.is_empty(), "{}", e2.error);
    e2.vm.regs = true;
    let mut r = e2.resume();
    while e2.is_suspended() { r = e2.resume(); }
    assert_eq!(r, "49999995000000");
}

#[test]
fn free_list_entry_naming_no_object() {
    let mut e = engine();
    e.eval("var x = 1;");
    e.vm.free_list.push(50_000_000);
    refused(&e.snapshot(), "free list past the heap");
    let mut e = engine();
    e.eval("var x = {};");
    let g = e.vm.global;
    e.vm.free_list.push(g as _);
    refused(&e.snapshot(), "free list naming a live object");
}

#[test]
fn suspended_frame_past_its_function() {
    let mut e = engine();
    e.set_slice(0.01);
    e.eval("var s = 0; for (var i = 0; i < 1e7; i++) { s += i; } s");
    assert!(e.is_suspended());
    let n = e.vm.frames.len();
    e.vm.frames[n - 1].pc = 100_000_000;
    refused(&e.snapshot(), "pc past the function");
}

#[test]
fn suspended_frame_without_room_for_its_registers() {
    let mut e = engine();
    e.set_slice(0.01);
    e.eval("function f(a,b,c,d,e,f,g,h){ var s = 0; for (var i = 0; i < 1e7; i++) { s += i+a+b+c+d+e+f+g+h; } return s } f(1,2,3,4,5,6,7,8)");
    assert!(e.is_suspended());
    e.vm.stack.truncate(1);
    for f in e.vm.frames.iter_mut() { f.bp = 1; }
    refused(&e.snapshot(), "frame base below its registers");
}

#[test]
fn mutated_units_are_refused_or_run() {
    // every operand of every instruction set to values a forged unit would
    // use: decode refuses it, or it runs to a result or a JS error (a
    // panic or a crash fails the test)
    let src = "function f(a, b) { var t = { x: a, y: [b, a] }; for (var i = 0; i < 3; i++) { t.x += i; } return (function () { return t.x + a + b + t.y.length; })(); } f(1, 2) + 'x'";
    let mut e = engine();
    let u = e.compile(src);
    let text = cerxes::unit::encode(&u, src);
    let base = cerxes::unit::decode(&text, src);
    assert!(base.entry >= 0);
    let bad: [i32; 5] = [-1, 255, 65535, i32::MAX, i32::MIN];
    let (mut refused_n, mut ran) = (0, 0);
    for pi in 0..base.protos.len() {
        for oi in 0..base.protos[pi].code.len() {
            for field in 0..2 {
                for v in bad {
                    let mut u2 = cerxes::unit::decode(&text, src);
                    let op = &mut u2.protos[pi].code[oi];
                    if field == 0 { op.a = v } else { op.b = v }
                    let u3 = cerxes::unit::decode(&cerxes::unit::encode(&u2, src), src);
                    if u3.entry < 0 { refused_n += 1; continue; }
                    let mut e = engine();
                    e.set_time_limit(200.0);
                    e.run(u3);
                    ran += 1;
                }
            }
        }
    }
    assert!(refused_n > 0 && ran > 0, "refused {} ran {}", refused_n, ran);
}
