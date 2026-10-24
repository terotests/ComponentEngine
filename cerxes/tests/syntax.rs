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
    let mut failed = Vec::new();
    for (mode, title, src, want) in all.iter() {
        let m = force.unwrap_or(mode.as_str());
        let mut e = match snap {
            Some(t) => Engine::from_snapshot(t),
            None => Engine::new(),
        };
        assert!(e.error.is_empty(), "{}", e.error);
        e.through_text = through_text;
        e.set_syntax(m == "tsx" || m == "ts", m == "tsx" || m == "jsx");
        let got = e.eval(src);
        if &got != want {
            failed.push(format!("[{} {}] {}\n  want: {}\n  got:  {}", m, title, src, want, got));
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
