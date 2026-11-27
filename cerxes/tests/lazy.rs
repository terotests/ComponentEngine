// CErXes: lazy compiling (pcomp lazy_stub, Vm::lazy_compile). A function
// compiled at its first call does what the same function compiled with its
// script does: the scripts here run with every function that can be lazy
// (Vm.lazy_toks 1, lazy_src 0) and with none, on both machines, through a
// unit's text and from a snapshot taken before the functions ran.
use cerxes::Engine;

const SCRIPTS: &[&str] = &[
    // the bindings around: let / const / var / parameters, TDZ, const writes
    "let a = 1; const b = 2; var c = 3; function f(x, y = 3, ...r) { return x + y + a + b + c + r.length; } [f.length, f.name, f(1), f(1, 2, 3, 4)].join()",
    "function tdz() { return later; } var r = []; try { tdz(); } catch (e) { r.push(e.name); } let later = 5; r.push(tdz()); r.join()",
    "const k = 1; var r = []; try { (function () { k = 2; })(); } catch (e) { r.push(e.name); } try { [function () { [k] = [3]; }][0](); } catch (e) { r.push(e.name); } r.push(k); r.join()",
    "function outer() { let c = 10; function inner(d) { c += d; return c; } return [inner(1), inner(2), c]; } outer().join()",
    "function mk() { let k = 0; return function () { return ++k; }; } var m = mk(); m(); m() + ',' + mk()()",
    "var fs = []; for (let i = 0; i < 3; i++) { fs.push(function () { return i; }); } fs.map(function (h) { return h(); }).join()",
    "function shadow(node) { var c = node + 1; return function () { var node = 5; return node + c; }; } var node = 100; shadow(1)() + node",
    // the function itself: its name, kind, text, `this`, `arguments`
    "var g = function named(n) { return n <= 1 ? 1 : n * named(n - 1); }; g.name + g(5)",
    "function* gen(n) { for (let i = 0; i < n; i++) yield i; } [...gen(3)].join()",
    "var out = []; async function af(x) { await null; return x * 2; } af(4).then(function (v) { out.push(v); }); out",
    "function f(a, b) { return a + b; } f.toString()",
    "function strictf() { 'use strict'; return this; } function sloppy() { return typeof this; } [String(strictf()), sloppy()].join()",
    "function args(a) { arguments[0] = 9; return a + ':' + arguments.length; } args(1, 2, 3)",
    "function sh(a) { [].shift.call(arguments); return a; } function nm(a) { return function inner(b) { [].splice.call(arguments, 0, 1); return b; }(a, 7); } sh(1, 2) + ',' + nm(1)",
    "function lens(a, b = 1, c) {} function lens2({ a }, [b], ...c) {} lens.length + ',' + lens2.length",
    "var o = { m: function () { return this.v; }, v: 4 }; o.m()",
    // templates, classes and private names around and inside
    "var a = 1, b = 2; function tpl(x) { return `v=${x + a}:${`${b}`}`; } tpl(1)",
    "class A { #x = 5; m() { function g(o) { return o.#x + 1; } return g(this); } static s() { return function () { class B extends A { n() { return super.m() * 10; } } return new B().n(); }; } } new A().m() + ',' + A.s()()",
    "function withCls() { class P { #p = 3; get p() { return this.#p; } } return new P().p; } withCls()",
    // nested lazy functions, and functions inside a lazy one's own scope
    "function a1(x) { function a2(y) { function a3(z) { return x + y + z; } return a3(3); } return a2(2); } a1(1)",
    "function big(n) { var s = 0; for (var i = 0; i < n; i++) { s += (function (j) { return j * 2; })(i); } return s; } big(10)",
];

fn engine(regs: bool, lazy: bool, through_text: bool) -> Engine {
    let mut e = Engine::new();
    e.vm.regs = regs;
    e.through_text = through_text;
    e.set_syntax(false, false);
    if lazy {
        e.vm.lazy_toks = 1;
        e.vm.lazy_src = 0;
    } else {
        e.vm.lazy_toks = 0;
    }
    e
}

fn run(e: &mut Engine, src: &str) -> String {
    let r = e.eval(src);
    // the promise jobs' results
    let late = e.eval("typeof out === 'undefined' ? '' : String(out)");
    format!("{} | {}", r, late)
}

#[test]
fn lazy_same_as_eager() {
    for src in SCRIPTS.iter() {
        for regs in [true, false] {
            let want = run(&mut engine(regs, false, false), src);
            for tt in [false, true] {
                let got = run(&mut engine(regs, true, tt), src);
                assert_eq!(got, want, "regs {} through_text {}: {}", regs, tt, src);
            }
        }
    }
}

#[test]
fn lazy_stubs_in_snapshots() {
    // the functions are defined, the snapshot taken before any ran
    let def = "var keep = 7; function later(x) { var t = 0; for (var i = 0; i < x; i++) { t += i * keep; } return t; } var g = (function () { let hidden = 3; return function (y) { return y + hidden + keep; }; })(); 1";
    let mut e = engine(true, true, false);
    e.eval(def);
    let snap = e.snapshot();
    let mut back = Engine::from_snapshot(snap.as_str());
    assert!(back.error.is_empty(), "{}", back.error);
    assert_eq!(back.eval("later(4) + ',' + g(1)"), "42,11");
}

#[test]
fn small_script_reports_early_errors_of_uncalled_functions() {
    // the defaults: a small script is compiled whole
    let mut e = Engine::new();
    e.set_syntax(false, false);
    let r = e.eval("function never() { return 1 +; } 1");
    assert!(r.contains("SyntaxError"), "{}", r);
}

#[test]
fn big_script_compiles_functions_at_their_first_call() {
    // the defaults: in a script of Vm.lazy_src chars or more a function of
    // Vm.lazy_toks tokens is not compiled before it is called
    let mut src = String::new();
    let mut i = 0;
    while src.len() < 40000 {
        src.push_str(format!("function f{}(a, b) {{ var x = a + b * {}; if (x > 10) {{ x = x - 1; }} else {{ x = x + 2; }} for (var k = 0; k < 2; k++) {{ x += k; }} return x; }}\n", i, i).as_str());
        i += 1;
    }
    src.push_str("function broken() { var y = 1; if (y) { y = y + 1; } else { y = y - 1; } for (var k = 0; k < 2; k++) { y += k; } return y +; }\n");
    src.push_str("var r = f3(1, 2) + f10(2, 3); try { broken(); } catch (e) { r = r + ' ' + e.name; } r");
    let mut e = Engine::new();
    e.set_syntax(false, false);
    assert_eq!(e.eval(src.as_str()), "42 SyntaxError");
}
