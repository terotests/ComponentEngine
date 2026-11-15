// CErXes: the host's limits on a script's run time (Engine::set_time_limit)
// and on regex backtracking (Vm::set_regex_step_limit).
use cerxes::Engine;
use std::time::Instant;

/// Scripts that never return: each under native code of its own kind
/// (a getter, a comparator, a promise job...), where a slice cannot stop.
const RUNAWAYS: &[&str] = &[
    "for(;;){}",
    "JSON.stringify({get a(){ for(;;){} }})",
    "new Promise(function(){ for(;;){} })",
    "Math.max({valueOf(){ for(;;){} }})",
    "[...{[Symbol.iterator](){ for(;;){} }}]",
    "`${ {toString(){ for(;;){} }} }`",
    "Reflect.construct(function(){ for(;;){} }, [])",
    "'a'.replace(/a/, function(){ for(;;){} })",
    "[2,1].sort(function(){ for(;;){} })",
    "Promise.resolve().then(function(){ for(;;){} }); 1",
    "(async function(){ await 1; for(;;){} })(); 1",
    "({get a(){ for(;;){} }}).a",
    "function* g(){ for(;;){} } g().next()",
    "eval('for(;;){}')",
    "function* g(){ try{ yield 1 } finally { for(;;){} } } var i=g(); i.next(); i.return()",
    "(async()=>{ await {then(){ for(;;){} }} })(); 1",
    "try { for(;;){} } catch (e) { for(;;){} } finally { for(;;){} }",
    "(3n**(10n**6n)).toString().length",
    "/^(a+)+$/.test('a'.repeat(40)+'!')",
    "new Array(1e7).fill('xy').join(',').length",
    "var a=[]; for(var i=0;i<4e5;i++)a.push('k'+((i*7919)%1000003)); a.sort(function(x,y){return x.localeCompare(y)}).length",
];

fn engine(regs: bool, limit: f64) -> Engine {
    let mut e = Engine::new();
    e.vm.regs = regs;
    e.set_syntax(false, false);
    e.set_time_limit(limit);
    e
}

#[test]
fn time_limit_stops_a_script_anywhere() {
    for regs in [true, false] {
        for src in RUNAWAYS {
            let mut e = engine(regs, 100.0);
            let t0 = Instant::now();
            let r = e.eval(src);
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            assert!(e.timed_out(), "regs {}: {} => {}", regs, src, r);
            assert!(r.starts_with("Uncaught Error: script time limit exceeded"), "regs {}: {} => {}", regs, src, r);
            assert!(ms < 600.0, "regs {}: {} took {} ms", regs, src, ms);
            // the engine goes on: the next call has a limit of its own
            assert_eq!(e.eval("[1,2,3].map(function(x){return x*2}).join()"), "2,4,6", "regs {}: after {}", regs, src);
            assert!(!e.timed_out());
        }
    }
}

#[test]
fn no_catch_or_finally_runs_past_the_limit() {
    for regs in [true, false] {
        let mut e = engine(regs, 50.0);
        let r = e.eval("var log = []; try { try { for(;;){} } finally { log.push('inner finally') } } catch (x) { log.push('catch') } finally { log.push('finally') }");
        assert!(e.timed_out(), "{}", r);
        assert_eq!(e.eval("log.length"), "0");
        // a native that swallows exceptions (a promise executor) does not
        // keep the script going either
        let r = e.eval("new Promise(function(){ for(;;){} }); log.push('after'); 1");
        assert!(e.timed_out(), "{}", r);
        assert_eq!(e.eval("log.length"), "0");
    }
}

#[test]
fn a_script_under_the_limit_is_not_touched() {
    for regs in [true, false] {
        let mut e = engine(regs, 5000.0);
        let r = e.eval("var s = 0; for (var i = 0; i < 300000; i++) { s += [i].indexOf(i) + 1; } try { throw new Error('x') } catch (x) { s += 1 } finally { s += 1 } s");
        assert_eq!(r, "300002");
        assert!(!e.timed_out());
        let r = e.eval("var t = new Float64Array(100000); for (var i = 0; i < t.length; i++) t[i] = (i * 7919) % 1003; t.sort(); [t[0], t[t.length - 1], (12345678901234567890n ** 20n).toString().length].join()");
        assert_eq!(r, "0,1002,382");
    }
}

#[test]
fn time_limit_and_slices_together() {
    // a loop at the top yields every slice; each resume is a call with a
    // limit of its own, so the script runs on to its end
    for regs in [true, false] {
        let mut e = engine(regs, 200.0);
        e.set_slice(5.0);
        let mut r = e.eval("var n = 0; while (n < 3000000) n++; n");
        let mut stops = 0;
        let t0 = Instant::now();
        while e.is_suspended() {
            stops += 1;
            r = e.resume();
        }
        assert_eq!(r, "3000000", "regs {}", regs);
        assert!(stops >= 1 || t0.elapsed().as_millis() < 5, "regs {}", regs);
        assert!(!e.timed_out());
        // under a getter the slice cannot stop: the limit ends it
        let r = e.eval("({get a(){ for(;;){} }}).a");
        assert!(!e.is_suspended());
        assert!(e.timed_out(), "regs {}: {}", regs, r);
    }
}

#[test]
fn time_limit_from_the_host_vm_calls() {
    // a host that calls functions itself (Vm::call_top, run_jobs)
    let mut e = engine(true, 100.0);
    e.eval("function spin() { for(;;){} } function later() { Promise.resolve().then(spin); return 1; }");
    let a = e.vm.intern("spin");
    let g = e.vm.global;
    let f = e.vm.get_obj(g, a, &cerxes::value::Val::Obj(g));
    let t0 = Instant::now();
    e.vm.call_top(f, cerxes::value::Val::Undef(), Vec::new());
    assert!(e.vm.throwing && e.vm.terminated);
    assert!(t0.elapsed().as_millis() < 600);
    e.vm.throwing = false;
    e.vm.exc = cerxes::value::Val::Undef();
    let a = e.vm.intern("later");
    let f = e.vm.get_obj(g, a, &cerxes::value::Val::Obj(g));
    let r = e.vm.call_top(f, cerxes::value::Val::Undef(), Vec::new());
    assert!(!e.vm.throwing);
    assert_eq!(e.vm.display(&r), "1");
    let t0 = Instant::now();
    e.vm.run_jobs();
    assert!(e.vm.throwing && e.vm.terminated);
    assert!(t0.elapsed().as_millis() < 600);
}

#[test]
fn regex_backtracking_limit() {
    for regs in [true, false] {
        let mut e = engine(regs, 0.0);
        // the default budget: an exponential match throws, catchably
        let r = e.eval("try { /^(a+)+$/.test('a'.repeat(40) + '!') } catch (x) { x.name + ': ' + x.message }");
        assert_eq!(r, "RangeError: regular expression backtracking limit exceeded");
        assert_eq!(e.eval("/^(a+)+$/.test('aaaa')"), "true");
        // a budget of the host's
        e.vm.set_regex_step_limit(1000.0);
        let r = e.eval("try { /^(a+)+$/.test('a'.repeat(16) + '!') } catch (x) { x.name }");
        assert_eq!(r, "RangeError");
        assert_eq!(e.eval("'x-y-z'.split(/-/).join()"), "x,y,z");
    }
}

/// `src` in slices of `ms`, a snapshot at every stop and the next slice in
/// an engine made from it; then `check` in the last engine. Answers
/// (result of src, result of check, stops).
fn sliced(src: &str, check: &str, ms: f64, regs: bool) -> (String, String, i64) {
    let mut e = engine(regs, 0.0);
    e.set_slice(ms);
    let mut r = e.eval(src);
    let mut stops = 0;
    while e.is_suspended() {
        stops += 1;
        let snap = e.snapshot();
        assert!(!snap.is_empty(), "no snapshot while suspended: {}", src);
        let mut next = Engine::from_snapshot(&snap);
        assert!(next.error.is_empty(), "restore: {}", next.error);
        next.vm.regs = regs;
        next.set_slice(ms);
        r = next.resume();
        e = next;
    }
    let c = e.eval(check);
    (r, c, stops)
}

#[test]
fn promise_jobs_and_async_bodies_stop_at_slices() {
    let cases: &[(&str, &str)] = &[
        ("var log=[]; Promise.resolve(5).then(function(v){ var n=0; while(n<3000000) n++; log.push(v+n); return v*2 }).then(function(v){ log.push(v) }); 7", "log.join()"),
        ("var log=[]; (async function(){ var x = await 3; var n=0; while(n<3000000) n++; log.push(x+n); var y = await 4; while(n<6000000) n++; return y+n })().then(function(v){ log.push('done '+v) }); 8", "log.join()"),
        ("queueMicrotask(function(){ var n=0; while(n<3000000) n++; globalThis.qm=n }); 9", "qm"),
        ("(async function(){ await 1; try { var n=0; while(n<3000000) n++; throw new Error('boom') } catch(e) { return e.message } })().then(function(v){ globalThis.r=v }); 10", "r"),
        ("(async function(){ await 1; var n=0; while(n<3000000) n++; throw new Error('late') })().catch(function(e){ globalThis.r2=e.message }); 11", "r2"),
        ("var t={then(ok){ var n=0; while(n<3000000) n++; ok(n) }}; Promise.resolve(t).then(function(v){ globalThis.r3=v }); 12", "r3"),
        ("var log=[]; for (var i=0;i<3;i++) (async function(k){ await null; var n=0; while(n<1000000) n++; log.push(k); await null; log.push(k+10) })(i); 13", "log.join()"),
    ];
    for regs in [true, false] {
        for (src, check) in cases {
            let mut plain = engine(regs, 0.0);
            let want = plain.eval(src);
            let want_c = plain.eval(check);
            let (got, got_c, stops) = sliced(src, check, 2.0, regs);
            assert_eq!(got, want, "regs {}: {}", regs, src);
            assert_eq!(got_c, want_c, "regs {}: {}", regs, src);
            assert!(stops >= 1, "regs {}: no stop in {}", regs, src);
        }
    }
}
