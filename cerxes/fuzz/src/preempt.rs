// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The preemption oracle (audit A3, A4). A host runs CErXes in slices
// (`set_slice`) to keep a script from holding the thread; that only works
// when the engine gives control back in every place a script can loop.
// Generated programs never loop forever, so this writes programs that do:
// an endless loop inside each way user code can be called (a getter,
// valueOf, a sort comparator, a Promise job, …), up to two deep, and long
// native calls. Each runs in 50 ms (or 5 ms) slices; every slice must end
// within its length + 20 ms. A slice that runs longer is a finding with
// the call form as its signature; no yield at all is caught by the
// driver's short timeout.

use crate::gen::Rng;
use cerxes::Engine;
use std::time::Instant;

/// (name, template): `BODY` is where the loop or the next form goes.
const FORMS: &[(&str, &str)] = &[
    ("plain", "BODY"),
    ("function", "function f() { BODY } f();"),
    ("arrow", "(() => { BODY })();"),
    ("getter", "var o = { get g() { BODY } }; o.g;"),
    ("setter", "var o = { set s(v) { BODY } }; o.s = 1;"),
    ("toString", "String({ toString() { BODY } });"),
    ("valueOf", "+{ valueOf() { BODY } };"),
    ("toPrimitive", "`${{ [Symbol.toPrimitive]() { BODY } }}`;"),
    ("generator", "function* g() { BODY } g().next();"),
    ("eval", "eval(\"BODY\");"),
    ("Function", "new Function(\"BODY\")();"),
    ("sort", "[3, 1, 2].sort(() => { BODY });"),
    ("replace", "'aaa'.replace(/a/g, () => { BODY });"),
    ("map", "[1].map(() => { BODY });"),
    ("forEach", "[1].forEach(() => { BODY });"),
    ("reduce", "[1, 2].reduce(() => { BODY });"),
    ("Array.from", "Array.from([1], () => { BODY });"),
    ("then", "Promise.resolve().then(() => { BODY });"),
    ("async-await", "(async () => { await 0; BODY })();"),
    ("async", "(async () => { BODY })();"),
    ("constructor", "new (class { constructor() { BODY } })();"),
    ("static-block", "class K { static { BODY } }"),
    ("proxy-get", "new Proxy({}, { get() { BODY } }).x;"),
    ("reviver", "JSON.parse('[1]', () => { BODY });"),
    ("toJSON", "JSON.stringify({ toJSON() { BODY } });"),
    ("call", "(function () { BODY }).call(null);"),
    ("Reflect.apply", "Reflect.apply(function () { BODY }, null, []);"),
    ("bind", "(function () { BODY }).bind(null)();"),
    ("iterator", "for (const x of { [Symbol.iterator]() { return { next() { BODY } }; } }) {}"),
    ("spread", "[...{ [Symbol.iterator]() { return { next() { BODY } }; } }];"),
    ("tagged", "(function () { BODY })`x`;"),
    ("finally", "try { } finally { BODY }"),
    ("hasInstance", "1 instanceof { [Symbol.hasInstance]() { BODY } };"),
    ("regexp-exec", "var re = /a/; re.exec = () => { BODY }; re.test('a');"),
    ("join", "[{ toString() { BODY } }].join();"),
    ("Map.forEach", "new Map([[1, 1]]).forEach(() => { BODY });"),
    ("defineProperty", "Object.defineProperty({}, 'x', { get value() { BODY } });"),
    ("species", "var a = [1]; a.constructor = { [Symbol.species]: function () { BODY } }; a.map(x => x);"),
];

/// Loops that never end, written several ways.
const LOOPS: &[&str] = &["for (;;) {}", "while (true) {}", "do {} while (1);", "for (var i = 0; ; i++) {}", "for (;;) { var q = [1, 2].length; }"];

/// Native calls that take long without a loop in JavaScript.
const NATIVES: &[(&str, &str)] = &[
    ("native:bigint-pow", "(3n ** 300000n).toString().length;"),
    ("native:indexOf", "'ab'.repeat(2 ** 23).indexOf('ba', 1) + 'a'.repeat(2 ** 24).lastIndexOf('b');"),
    ("native:sort", "var a = []; for (var i = 0; i < 300000; i++) a.push('k' + ((i * 7919) % 300000)); a.sort();"),
    ("native:localeCompare-sort", "var a = []; for (var i = 0; i < 100000; i++) a.push('k' + ((i * 7919) % 100000)); a.sort((x, y) => x.localeCompare(y));"),
    ("native:json", "JSON.parse(JSON.stringify(new Array(400000).fill({ a: 1, b: [1, 2] }))).length;"),
    ("native:redos", "/(a+)+$/.test('a'.repeat(26) + 'b');"),
    ("native:join", "new Array(2000000).fill('xy').join(',').length;"),
    ("native:bigint-shift", "(1n << 2n ** 24n).toString(16).length;"),
    ("native:repeat-replace", "'ab'.repeat(2 ** 21).replace(/b/g, 'cc').length;"),
];

/// A program and its signature (the call forms, outermost first).
pub fn program(rng: &mut Rng) -> (String, String) {
    if rng.chance(15) {
        let (name, src) = NATIVES[rng.below(NATIVES.len())];
        return (format!("/* preempt: {} */\n{}", name, src), name.to_string());
    }
    let depth = if rng.chance(40) { 2 } else { 1 };
    let mut names: Vec<&str> = Vec::new();
    let mut body = LOOPS[rng.below(LOOPS.len())].to_string();
    for _ in 0..depth {
        let (name, tpl) = FORMS[rng.below(FORMS.len())];
        // eval / Function take the body as a string literal
        let b = if tpl.contains("\"BODY\"") { body.replace('\\', "\\\\").replace('"', "\\\"") } else { body.clone() };
        body = tpl.replace("BODY", &b);
        names.insert(0, name);
    }
    let sig = names.join(">");
    (format!("/* preempt: {} */\n{}", sig, body), sig)
}

/// The longest slice of the program in `slice_ms` slices, over at most
/// `total_ms` of wall time.
fn longest_slice(src: &str, regs: bool, slice_ms: f64, total_ms: f64) -> f64 {
    let mut e = Engine::new();
    e.vm.regs = regs;
    e.set_syntax(false, false);
    e.set_slice(slice_ms);
    let t0 = Instant::now();
    e.eval(src);
    let mut max = t0.elapsed().as_secs_f64() * 1000.0;
    while e.is_suspended() && t0.elapsed().as_secs_f64() * 1000.0 < total_ms {
        let t1 = Instant::now();
        e.resume();
        max = max.max(t1.elapsed().as_secs_f64() * 1000.0);
    }
    max
}

/// Slack over the slice length before a slice is late.
pub const LATE_MS: f64 = 20.0;

/// `ok`, or `preempt\t<sig>\t<detail>` when a slice ran late twice.
pub fn judge(src: &str, sig: &str, seed: u64) -> String {
    let mut rng = Rng::new(seed ^ 0x9E37);
    let regs = rng.chance(75);
    let slice = if rng.chance(70) { 50.0 } else { 5.0 };
    crate::stage::set(if regs { "preempt register VM" } else { "preempt stack VM" });
    let a = longest_slice(src, regs, slice, 400.0);
    if a <= slice + LATE_MS {
        return String::from("ok");
    }
    // a loaded machine can stall one slice: it must happen again
    let b = longest_slice(src, regs, slice, 400.0);
    if b <= slice + LATE_MS {
        return String::from("ok");
    }
    format!(
        "preempt\t{}\t{} ms slices on the {} VM: a slice took {:.0} ms, then {:.0} ms (allowed {:.0})",
        sig,
        slice,
        if regs { "register" } else { "stack" },
        a,
        b,
        slice + LATE_MS
    )
}
