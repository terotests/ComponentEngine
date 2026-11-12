// CErXes: TypeScript and JSX read into CEr's tree. Runs the cases of
// tests/cases.txt, and CEr's own cases (../cer/tests/cases.txt, plain
// JavaScript) both as JavaScript and as TSX, each in a fresh engine;
// bench/cases.mjs runs the same cases on the Ranger builds.
use cerxes::Engine;

/// (syntax, title, script, expected) of every case in a cases file; a
/// case's head is `==== <syntax> [title]`
fn cases(file: &str) -> Vec<(String, String, String, String)> {
    let text = std::fs::read_to_string(format!("{}/{}", env!("CARGO_MANIFEST_DIR"), file)).unwrap();
    let mut out = Vec::new();
    let mut cur: Option<(String, Vec<String>, Vec<String>, bool)> = None;
    let mut push = |c: Option<(String, Vec<String>, Vec<String>, bool)>, out: &mut Vec<(String, String, String, String)>| {
        if let Some((head, s, e, _)) = c {
            let mut parts = head.splitn(2, ' ');
            let mode = parts.next().unwrap_or("").to_string();
            let title = parts.next().unwrap_or("").to_string();
            out.push((mode, title, s.join("\n"), e.join("\n").trim_end().to_string()));
        }
    };
    for line in text.lines() {
        if let Some(head) = line.strip_prefix("==== ") {
            push(cur.take(), &mut out);
            cur = Some((head.trim().to_string(), Vec::new(), Vec::new(), false));
        } else if let Some(c) = cur.as_mut() {
            if line == "----" && !c.3 {
                c.3 = true;
            } else if c.3 {
                if !line.starts_with('#') {
                    c.2.push(line.to_string());
                }
            } else {
                c.1.push(line.to_string());
            }
        }
    }
    push(cur.take(), &mut out);
    out
}

/// Runs `all` with each case's syntax, or with `force` when given.
fn check(all: &[(String, String, String, String)], force: Option<&str>) -> Vec<String> {
    check_with(all, force, false)
}

/// … each script run from its compiled unit's text when `through_text`
/// (as a host's code cache runs it).
fn check_with(all: &[(String, String, String, String)], force: Option<&str>, through_text: bool) -> Vec<String> {
    check_in(all, force, through_text, None)
}

/// … each case in an engine made from `snap` when given.
fn check_in(all: &[(String, String, String, String)], force: Option<&str>, through_text: bool, snap: Option<&str>) -> Vec<String> {
    check_rc(all, force, through_text, snap, false)
}

/// … with `rc`, a reclaim at every safe point, each checked against a full
/// mark (Vm::rc_stress).
fn check_rc(all: &[(String, String, String, String)], force: Option<&str>, through_text: bool, snap: Option<&str>, rc: bool) -> Vec<String> {
    check_vm(all, force, through_text, snap, rc, false)
}

/// … on the register machine (rvm.rs) when `regs`.
fn check_vm(all: &[(String, String, String, String)], force: Option<&str>, through_text: bool, snap: Option<&str>, rc: bool, regs: bool) -> Vec<String> {
    let mut failed = Vec::new();
    for (mode, title, src, want) in all.iter() {
        let m = force.unwrap_or(mode.as_str());
        let mut e = match snap {
            Some(t) => Engine::from_snapshot(t),
            None => Engine::new(),
        };
        assert!(e.error.is_empty(), "{}", e.error);
        e.through_text = through_text;
        if rc {
            e.vm.rc_stress();
        }
        e.vm.regs = regs;
        e.set_syntax(m == "tsx" || m == "ts", m == "tsx" || m == "jsx");
        let got = e.eval(src);
        if &got != want || e.vm.rc_errors > 0 {
            failed.push(format!("[{} {}] {}\n  want: {}\n  got:  {}\n  rc: {}", m, title, src, want, got, e.vm.rc_log.join("; ")));
        }
    }
    failed
}

#[test]
fn cerxes_cases() {
    let all = cases("tests/cases.txt");
    assert!(all.len() > 50);
    let failed = check(&all, None);
    assert!(failed.is_empty(), "{} of {} cases failed:\n{}", failed.len(), all.len(), failed.join("\n"));
}

#[test]
fn cer_cases_as_javascript() {
    let all = cases("../cer/tests/cases.txt");
    assert!(all.len() > 40);
    let failed = check(&all, None);
    assert!(failed.is_empty(), "{} of {} cases failed:\n{}", failed.len(), all.len(), failed.join("\n"));
}

#[test]
fn cer_cases_as_tsx() {
    let all = cases("../cer/tests/cases.txt");
    let failed = check(&all, Some("tsx"));
    assert!(failed.is_empty(), "{} of {} cases failed:\n{}", failed.len(), all.len(), failed.join("\n"));
}

/// Every case on the register machine, which the library suite and the
/// hosts run (bench/libs.mjs, CER_REGS=1).
#[test]
fn cases_on_the_register_machine() {
    let mut all = cases("tests/cases.txt");
    all.extend(cases("../cer/tests/cases.txt"));
    let failed = check_vm(&all, None, false, None, false, true);
    assert!(failed.is_empty(), "{} of {} cases failed:\n{}", failed.len(), all.len(), failed.join("\n"));
}

#[test]
fn cases_through_unit_text() {
    let mut all = cases("tests/cases.txt");
    all.extend(cases("../cer/tests/cases.txt"));
    let failed = check_with(&all, None, true);
    assert!(failed.is_empty(), "{} of {} cases failed:\n{}", failed.len(), all.len(), failed.join("\n"));
}

#[test]
fn cases_in_restored_engines() {
    let mut all = cases("tests/cases.txt");
    all.extend(cases("../cer/tests/cases.txt"));
    let snap = Engine::new().snapshot();
    assert!(!snap.is_empty());
    let failed = check_in(&all, None, false, Some(snap.as_str()));
    assert!(failed.is_empty(), "{} of {} cases failed:\n{}", failed.len(), all.len(), failed.join("\n"));
}

/// Every case with reference counting checked at every safe point, in new
/// and in restored engines.
#[test]
fn cases_rc_checked() {
    let mut all = cases("tests/cases.txt");
    all.extend(cases("../cer/tests/cases.txt"));
    let mut failed = check_rc(&all, None, false, None, true);
    let snap = Engine::new().snapshot();
    failed.extend(check_rc(&all, None, false, Some(snap.as_str()), true));
    assert!(failed.is_empty(), "{} of {} cases failed:\n{}", failed.len(), all.len(), failed.join("\n"));
}

/// A realm set up, kept, and carried on in another engine.
#[test]
fn snapshot_keeps_a_realm() {
    let mut a = Engine::new();
    a.set_syntax(false, false);
    let setup = r#"
        var log = [];
        class Shape { constructor(n) { this.n = n; } get twice() { return this.n * 2; } static of(n) { return new Shape(n); } }
        const counter = (() => { let c = 0; return () => ++c; })();
        counter(); counter();
        var m = new Map([[1, 'one'], ['k', { deep: [1, 2, 3] }]]);
        var st = new Set(['a', 'b']);
        var wkey = {}; var wm = new WeakMap([[wkey, 'weak']]); var ws = new WeakSet([wkey]);
        var sym = Symbol('tag'), reg = Symbol.for('shared');
        var tagged = { [sym]: 'symval', [reg]: 7 };
        var re = /(\d+)-(?<word>[a-zé]+)/gu;
        var big = 12345678901234567890n;
        var nums = new Float64Array([1.5, -0, NaN, Infinity]);
        var many = {}; for (var i = 0; i < 40; i++) many['k' + i] = i;
        delete many.k3;
        var frozen = Object.freeze({ x: 1 });
        var acc = Object.defineProperty({}, 'v', { get() { return 'got'; }, enumerable: false });
        var later = Promise.resolve(5).then(v => log.push('then ' + v));
        function* gen() { yield 1; yield 2; }
        var g = gen(); g.next();
        var d = new Date(0);
        var err = new TypeError('kept');
        'set up';
    "#;
    assert_eq!(a.eval(setup), "set up");
    let snap = a.snapshot();
    assert!(!snap.is_empty());
    let mut b = Engine::from_snapshot(&snap);
    assert!(b.error.is_empty(), "{}", b.error);
    b.set_syntax(false, false);
    let check = r#"
        [Shape.of(4).twice, counter(), m.get('k').deep.join('+'), m.size, st.has('b'), wm.get(wkey), ws.has(wkey), m.get(1),
         tagged[sym], tagged[Symbol.for('shared')], Symbol.keyFor(reg), sym.toString(),
         '12-abé 7-x'.replace(re, '$<word>$1'), (big * 2n).toString(), Array.from(nums).map(x => Object.is(x, -0) ? '-0' : String(x)).join(','),
         Object.keys(many).length, many.k39, 'k3' in many, Object.isFrozen(frozen), acc.v, Object.keys(acc).length,
         log.join(';'), g.next().value, d.toISOString(), err instanceof TypeError, err.message, typeof later.then,
         [3, 1, 2].sort().join(''), JSON.stringify({ a: [1, { b: null }] }), 'abc'.toUpperCase(), Math.max(1, 9)].join('|')
    "#;
    let want = a.eval(check);
    let got = b.eval(check);
    assert_eq!(got, want);
    assert!(got.contains("then 5"), "{}", got);
    // a snapshot of the restored engine is the same text
    let mut c = Engine::from_snapshot(&b.snapshot());
    c.set_syntax(false, false);
    assert_eq!(c.eval("counter()"), b.eval("counter()"));
    // a damaged text is refused
    let bad = Engine::from_snapshot(&snap[..snap.len() / 2]);
    assert!(!bad.error.is_empty());
}

/// A host's call (Engine::call) and the promise jobs after it run with safe
/// points: reclaims there, checked, free nothing still reachable.
#[test]
fn host_calls_reclaim_checked() {
    let mut e = Engine::new();
    e.vm.rc_stress();
    e.eval(
        "var keep = []; function work() { var t = []; for (var i = 0; i < 200; i++) { t.push({ i: i, a: [i, { q: i }] }); } keep.push(t[7]); \
         Promise.resolve(t[9]).then(function (o) { keep.push(o); return { r: o.a[1] }; }).then(function (x) { keep.push(x.r); }); \
         return keep.length; } \
         function sum() { var s = 0; for (var i = 0; i < keep.length; i++) { s += keep[i].i !== undefined ? keep[i].i : keep[i].q; } return s; }",
    );
    for _ in 0..20 {
        e.call("work");
    }
    let got = e.call("sum");
    assert_eq!(got, "500");
    assert!(e.vm.reclaims > 0);
    assert_eq!(e.vm.rc_errors, 0, "{}", e.vm.rc_log.join("; "));
}

#[test]
fn host_call_result_outlives_its_jobs() {
    // Engine::call runs the promise jobs after the call with safe points;
    // the result it returns is held through them
    let mut e = Engine::new();
    e.vm.rc_stress();
    e.eval(
        "var n = 0; function f() { n++; var s = 'r' + n + ':' + [1, 2, 3].join('-'); \
         Promise.resolve(1).then(function () { var t = []; for (var i = 0; i < 500; i++) t.push('x' + i); return t; }); \
         return s; }",
    );
    for k in 1..20 {
        assert_eq!(e.call("f"), format!("r{}:1-2-3", k));
    }
    assert_eq!(e.vm.rc_errors, 0, "{}", e.vm.rc_log.join("; "));
}

#[test]
fn prelude_snapshot_is_current() {
    // Engine::new restores src/prelude_snap.rs instead of compiling the
    // prelude; after a change to the prelude or the built-ins, write it
    // again: cargo run --release --bin cerxes -- --prelude-snapshot > src/prelude_snap.rs
    let fresh = cerxes::prelude_snapshot_rs(&Engine::compiled(true).snapshot());
    let kept = include_str!("../src/prelude_snap.rs");
    assert!(fresh == kept, "src/prelude_snap.rs is stale: cargo run --release --bin cerxes -- --prelude-snapshot > src/prelude_snap.rs");
    let mut e = Engine::new();
    assert!(e.error.is_empty());
    e.eval("print([3,1,2].sort().join(), typeof Symbol.iterator, JSON.stringify(new Map([[1,2]]).size))");
    assert_eq!(e.vm.out, vec![String::from("1,2,3 symbol 1")]);
}

/// Runs `src` in slices of `ms`, on the register machine when `regs`, a
/// snapshot at every stop and the next slice in an engine made from it;
/// answers the result, the output and the number of stops.
fn run_in_slices(src: &str, ms: f64, regs: bool) -> (String, Vec<String>, i64) {
    let mut e = Engine::new();
    e.vm.regs = regs;
    e.set_slice(ms);
    let mut r = e.eval(src);
    let mut stops = 0;
    while e.is_suspended() {
        stops += 1;
        let snap = e.snapshot();
        assert!(!snap.is_empty(), "no snapshot while suspended");
        let mut next = Engine::from_snapshot(&snap);
        assert!(next.error.is_empty(), "restore: {}", next.error);
        next.vm.regs = regs;
        next.set_slice(ms);
        r = next.resume();
        e = next;
    }
    (r, e.vm.out.clone(), stops)
}

/// `src` in slices on both machines answers as it does in one go, and
/// stops at least `min_stops` times on each.
fn check_slices(src: &str, ms: f64, min_stops: i64) {
    for regs in [false, true] {
        let mut plain = Engine::new();
        plain.vm.regs = regs;
        let want = plain.eval(src);
        let (got, out, stops) = run_in_slices(src, ms, regs);
        assert!(stops >= min_stops, "regs {}: {} stops", regs, stops);
        assert_eq!(got, want, "regs {}", regs);
        assert_eq!(out, plain.vm.out, "regs {}", regs);
    }
}

#[test]
fn slices_stop_in_loops_and_go_on_from_snapshots() {
    // a loop at the top, in a function under try / finally, and in map and
    // forEach callbacks: each stops when its slice runs out
    check_slices(r#"
        var o = { n: 0, t: 0, s: '' };
        function f(x) { return x % 7; }
        try { while (o.n < 600000) { o.n++; o.t += f(o.n); if (o.n % 200000 == 0) o.s += o.n + ','; } }
        finally { o.s += 'end'; }
        var r = [1, 2].map(function (x) { var k = 0; while (k < 300000) k++; return x * k; });
        var s = 0; [5, 6].forEach(function (x) { var j = 0; while (j < 300000) { j++; s += x; } });
        print(o.n, o.t, o.s, r.join(), s); o.t + s
    "#, 1.0, 1);
}

#[test]
fn slices_stop_in_while_true_and_at_calls_and_returns() {
    // while (true) left with break; recursion with no loop at all stops at
    // its calls and returns
    check_slices(r#"
        var n = 0; while (true) { n++; if (n == 500000) break; }
        function fib(k) { return k < 2 ? k : fib(k - 1) + fib(k - 2); }
        print(n, fib(24)); n
    "#, 1.0, 1);
}

#[test]
fn slices_stop_at_every_kind_of_branch() {
    // a slice that is over at once: the run stops every SLICE_TICKS
    // branches, calls and returns, so it stops at many kinds of them
    check_slices(r#"
        var log = [];
        function sw(x) { switch (x % 5) { case 0: return 'a'; case 1: return 'b'; case 'x': return 'c'; default: return 'd'; } }
        function nul(o) { return (o ?? { v: 1 }).v + (o?.w ?? 2) + (o && o.v || 3); }
        class P { constructor(v) { this.v = v; } get d() { return this.v * 2; } }
        var acc = 0;
        for (var i = 0; i < 4000; i++) {
            acc += sw(i).length + nul(i % 3 ? null : { v: i, w: i });
            for (var k in { a: 1, b: 2 }) acc += k.length;
            for (var x of [i, i + 1]) acc += x % 3;
            try { if (i % 97 == 0) throw new Error('e' + i); acc += new P(i).d % 11; }
            catch (e) { log.push(e.message); }
            acc += [3, 1, 2].sort(function (a, b) { return a - b; })[0];
            acc += i % 2 == 0 ? 1 : (i % 3 === 0 ? 2 : 3);
        }
        print(acc, log.length, log.slice(0, 3).join()); acc
    "#, 0.0000001, 20);
}

#[test]
fn proxy_get_checks_its_invariants() {
    // the get trap runs natively for plain targets: same answers as the
    // prelude's hook (Node gives the same)
    let mut e = Engine::new();
    let r = e.eval(r#"
        var t = {}; Object.defineProperty(t, 'x', { value: 1, writable: false, configurable: false });
        Object.defineProperty(t, 'y', { set: function (v) {}, configurable: false });
        t.z = 5;
        var p = new Proxy(t, { get: function (tt, k, r) { return k === 'z' ? 7 : 2; } });
        var out = [];
        try { p.x; out.push('no'); } catch (e) { out.push(e.constructor.name); }
        try { p.y; out.push('no'); } catch (e) { out.push(e.constructor.name); }
        out.push(p.z, p.w);
        var q = new Proxy({ a: 1 }, {}); out.push(q.a, q.b);
        var cnt = 0; var h = {}; Object.defineProperty(h, 'get', { get: function () { cnt++; return undefined; } });
        var r = new Proxy({ a: 3 }, h); out.push(r.a, cnt);
        try { new Proxy({}, { get: 5 }).a; } catch (e) { out.push(e.constructor.name); }
        var rec = new Proxy({}, { get: function (tt, k, rr) { return rr === s; } }); var s = Object.create(rec); out.push(s.foo);
        out.join()
    "#);
    assert_eq!(r, "TypeError,TypeError,7,2,1,,3,1,TypeError,true");
}

#[test]
fn proxy_set_checks_its_invariants() {
    // the set trap runs natively for plain targets (Node gives the same)
    let mut e = Engine::new();
    let r = e.eval(r#"
        var out = [];
        function t(f) { try { out.push(String(f())); } catch (e) { out.push(e.constructor.name); } }
        var tg = {}; Object.defineProperty(tg, 'ro', { value: 1, writable: false, configurable: false });
        Object.defineProperty(tg, 'acc', { get: function () { return 1; }, configurable: false });
        var p = new Proxy(tg, { set: function (t, k, v, r) { return true; } });
        t(function () { 'use strict'; p.ro = 2; return 'ok'; });
        t(function () { 'use strict'; p.ro = 1; return 'ok'; });
        t(function () { 'use strict'; p.acc = 1; return 'ok'; });
        t(function () { p.x = 5; return tg.x; });
        var q = new Proxy({}, { set: function (t, k, v, r) { t[k] = v * 2; return r === q; } });
        t(function () { q.a = 3; return q.a; });
        var z = new Proxy({}, { set: function () { return false; } });
        t(function () { z.a = 1; return 'sloppy'; });
        t(function () { 'use strict'; z.a = 1; return 'x'; });
        t(function () { new Proxy({}, { set: 5 }).a = 1; });
        var n = new Proxy({}, {}); t(function () { n.a = 4; return n.a; });
        out.join()
    "#);
    assert_eq!(r, "TypeError,ok,TypeError,undefined,6,sloppy,TypeError,TypeError,4");
}

#[test]
fn numbers_with_few_decimals_print_as_javascript_does() {
    // the short way (few decimals) and the general one give Node's text
    let mut e = Engine::new();
    let r = e.eval(r#"
        [0.5, 65.515, -655.1540484651923, 0.000001, 9979327917099, 1.2, 0.1 + 0.2, 123456789012345.6,
         -0.25, 1e15 + 0.5, 5e-7, 0.001, 12.0625, -3.5e-6].map(String).join()
    "#);
    assert_eq!(r, "0.5,65.515,-655.1540484651923,0.000001,9979327917099,1.2,0.30000000000000004,123456789012345.6,-0.25,1000000000000000.5,5e-7,0.001,12.0625,-0.0000035");
}

#[test]
fn hot_natives_off_the_stack_answer_as_their_builtins() {
    // Math.min/max, String(x), hasOwnProperty and Object.prototype.toString
    // have short ways; Node gives the same
    let mut e = Engine::new();
    let r = e.eval(r#"
var o = {a:1}; var out = [];
out.push(Math.min(3,1,2), Math.max(3,1,2), Math.min(), Math.max(), Math.min(1,NaN), Math.max(-0,0), 1/Math.min(0,-0), 1/Math.max(-0,0), Math.min('2', 5), Math.max(1,2,3,4,5,6,7,8,9,10));
out.push(String('x'), String(1.5), String(-0), String(), String(null), String(Symbol('q')), String([1,2]), String(1e21));
out.push(o.hasOwnProperty('a'), o.hasOwnProperty('b'), [1].hasOwnProperty(0), [1].hasOwnProperty('length'), Object.prototype.hasOwnProperty.call('ab', 1));
var ts = Object.prototype.toString;
out.push(ts.call([]), ts.call({}), ts.call(null), ts.call(undefined), ts.call(1), ts.call('s'), ts.call(function(){}), ts.call(new Map), ts.call({[Symbol.toStringTag]: 'Z'}), ts.call([]), ts.call(/x/), ts.call(new Date(0)));
try { ({}).hasOwnProperty({toString: function(){ throw new Error('boom'); }}); } catch (e) { out.push(e.message); }
out.join('|')
    "#);
    assert_eq!(r, "1|3|Infinity|-Infinity|NaN|0|-Infinity|Infinity|2|10|x|1.5|0||null|Symbol(q)|1,2|1e+21|true|false|true|true|true|[object Array]|[object Object]|[object Null]|[object Undefined]|[object Number]|[object String]|[object Function]|[object Map]|[object Z]|[object Array]|[object RegExp]|[object Date]|boom");
}

#[test]
fn cached_property_adds_respect_setters_proxies_and_extensibility() {
    // a set op caches the shape transition of an add; later objects on the
    // same op still meet setters, frozen prototypes and proxies (as Node)
    let mut e = Engine::new();
    let r = e.eval(r#"
        "use strict";
        var out = [];
        function mk(o) { o.x = 1; o.y = 2; return o; }
        for (var i = 0; i < 5; i++) mk({});
        var P = {}; var o1 = Object.create(P);
        Object.defineProperty(P, 'y', { set: function (v) { out.push('setter ' + v); }, configurable: true });
        mk(o1); out.push(JSON.stringify(Object.keys(o1)));
        var ne = Object.preventExtensions({}); try { mk(ne); } catch (e) { out.push(e.constructor.name); }
        var Q = Object.freeze({ x: 0 }); var o2 = Object.create(Q); try { mk(o2); } catch (e) { out.push(e.constructor.name); }
        var pr = new Proxy({}, { set: function (t, k, v, r) { out.push('trap ' + k); return true; } });
        var o3 = Object.create(pr); mk(o3); out.push(JSON.stringify(Object.keys(o3)));
        var o4 = mk({}); out.push(o4.x + o4.y, JSON.stringify(o4));
        out.join('|')
    "#);
    assert_eq!(r, "setter 2|[\"x\"]|TypeError|TypeError|trap x|trap y|[]|3|{\"x\":1,\"y\":2}");
}
