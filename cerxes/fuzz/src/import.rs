// SPDX-License-Identifier: AGPL-3.0-or-later
//
// `cerxes-fuzz import`: turns other engines' regression tests into seeds.
//
// The tests of JavaScriptCore (JSTests/stress), SpiderMonkey (jit-test,
// non262/regress), ChakraCore (test/), Hermes (test/hermes), QuickJS
// (tests/) and V8 (mjsunit regress) are where those engines keep the
// programs that once crashed them: the bug classes a new engine is most
// likely to have too. Each test calls its shell's helpers (`shouldBe`,
// `assertEq`, `noInline`, `WScript.Echo`, `%OptimizeFunctionOnNextCall` …),
// so a shim written in plain JavaScript goes in front of it: assertions
// print `FAIL …` and go on (the register / stack VM and snapshot oracles
// then compare the lines), JIT hints are no-ops, `transferArrayBuffer` and
// `detachArrayBuffer` really detach. Loop bounds of 1000 and more in loop
// heads become 300: still past the JIT threshold (64), but a test takes
// milliseconds instead of seconds. Tests that need what CErXes has not got
// (wasm, workers, new globals, modules, files) are left out.
//
// The output is one .js file per test whose first line says where it comes
// from: `/* foreign: jsc JSTests/stress/foo.js reentrancy,jit */`. The
// tests keep their engines' licenses (BSD, MPL 2.0, MIT, BSD-3), so they
// are fetched (seeds/fetch-engines.sh), not committed.

use std::path::{Path, PathBuf};

/// The helpers of the engines' test shells, defined only when the test did
/// not define them itself (a test's own `function shouldBe` wins; a test's
/// `let assert` is not in the way, because the shim sets properties of
/// globalThis, not bindings).
pub const SHIM: &str = r#"(function (g) {
  var out = typeof g.print === 'function' ? g.print : function () {};
  function def(n, v) { if (g[n] === undefined) g[n] = v; }
  function str(v) { try { return typeof v === 'symbol' ? 'symbol' : String(v); } catch (e) { return '?'; } }
  function same(a, b) { return a === b ? (a !== 0 || 1 / a === 1 / b) : (a !== a && b !== b); }
  function fail(m) { out('FAIL ' + m); }
  function noop() {}
  function id(f) { return f; }
  function throws(f, c, m) { try { f(); } catch (e) { if (c && typeof c === 'function' && !(e instanceof c)) fail('wrong error ' + str(e) + ' ' + (m || '')); return e; } fail('no throw ' + (m || '')); }
  var seed = 1;
  Math.random = function () { seed = (seed * 1103515245 + 12345) % 2147483648; return seed / 2147483648; };
  // a clock that does not move: `new Date()` and Date.now() print the same
  // in every engine configuration and after a snapshot
  var RD = Date, T0 = 1700000000000;
  var D = function Date(a, b, c, d, e, f, h) {
    var n = arguments.length;
    if (!new.target) return new RD(T0).toString();
    return n === 0 ? new RD(T0) : n === 1 ? new RD(a) : new RD(a, b, n > 2 ? c : 1, n > 3 ? d : 0, n > 4 ? e : 0, n > 5 ? f : 0, n > 6 ? h : 0);
  };
  D.prototype = RD.prototype; D.UTC = RD.UTC; D.parse = RD.parse; D.now = function () { return T0; };
  try { Object.defineProperty(RD.prototype, 'constructor', { value: D, writable: true, configurable: true }); } catch (e) {}
  g.Date = D;
  if (typeof g.performance === 'object' && g.performance) { try { g.performance.now = function () { return 0; }; } catch (e) {} }
  def('print', out);
  def('testLoopCount', 300);
  // JavaScriptCore
  def('shouldBe', function (a, b, m) { if (!same(a, b)) fail('shouldBe ' + str(a) + ' ' + str(b) + ' ' + (m || '')); });
  def('shouldThrow', function (f, m) { var e = throws(f); if (e && m !== undefined && typeof m === 'string' && str(e) !== m) fail('message ' + str(e)); });
  def('shouldNotThrow', function (f) { try { f(); } catch (e) { fail('threw ' + str(e)); } });
  ['noInline', 'noDFG', 'noFTL', 'noOSRExitFuzzing', 'ensureArrayStorage', 'makeMasquerader', 'OSRExit', 'forceGCSlowPaths', 'edenGC', 'fullGC', 'gcHeapSize', 'reoptimizationRetryCount', 'drainMicrotasks', 'releaseWeakRefs', 'finalizeUnconditionally', 'optimizeNextInvocation', 'abort', 'setTimeout', 'minorgc', 'gczeal', 'schedulegc', 'bailout', 'setJitCompilerOption', 'enableShellAllocationMetadataBuilder', 'printBugNumber', 'printStatus', 'startgc', 'verifyprebarriers', 'telemetryLog', 'enableGeckoProfiling', 'disableGeckoProfiling', 'enableTrackAllocations', 'resetFinalizeCount', 'quit'].forEach(function (n) { def(n, n === 'noInline' || n === 'noDFG' || n === 'noFTL' ? id : noop); });
  def('numberOfDFGCompiles', function () { return 100; });
  def('isFinalTier', function () { return true; });
  def('inIon', function () { return true; });
  def('inJit', function () { return true; });
  def('isAsmJSCompilationAvailable', function () { return false; });
  def('getJitCompilerOptions', function () { return {}; });
  def('oomTest', function (f) { try { f(); } catch (e) {} });
  def('detachArrayBuffer', function (b) { if (b && b.transfer) b.transfer(); });
  def('transferArrayBuffer', g.detachArrayBuffer);
  def('createBuiltin', function (s) { return Function('return ' + s)(); });
  def('$vm', new Proxy({}, { get: function (t, k) { return k === 'detachArrayBuffer' ? g.detachArrayBuffer : noop; } }));
  // SpiderMonkey
  def('assertEq', function (a, b, m) { if (!same(a, b)) fail('assertEq ' + str(a) + ' ' + str(b) + ' ' + (m || '')); });
  def('assertDeepEq', function (a, b) { try { if (JSON.stringify(a) !== JSON.stringify(b)) fail('assertDeepEq'); } catch (e) {} });
  def('assertThrowsInstanceOf', function (f, c, m) { throws(f, c, m); });
  def('assertThrowsValue', function (f, v) { try { f(); } catch (e) { if (!same(e, v)) fail('thrown ' + str(e)); return; } fail('no throw'); });
  def('reportCompare', function (a, b, m) { if (!same(a, b) && str(a) !== str(b)) fail('reportCompare ' + str(a) + ' ' + str(b) + ' ' + (m || '')); });
  def('reportMatch', function (re, s) { if (!re.test(str(s))) fail('reportMatch'); });
  // V8
  def('assertEquals', function (a, b, m) { if (!same(a, b) && !(typeof a === 'object' && a !== null && JSON.stringify(a) === JSON.stringify(b))) fail('assertEquals ' + str(a) + ' ' + str(b) + ' ' + (m || '')); });
  def('assertSame', g.assertEq);
  def('assertTrue', function (v, m) { if (v !== true) fail('assertTrue ' + (m || '')); });
  def('assertFalse', function (v, m) { if (v !== false) fail('assertFalse ' + (m || '')); });
  def('assertThrows', function (f, c, m) { throws(typeof f === 'string' ? function () { (0, eval)(f); } : f, c, m); });
  def('assertDoesNotThrow', function (f) { try { typeof f === 'string' ? (0, eval)(f) : f(); } catch (e) { fail('threw ' + str(e)); } });
  def('assertInstanceof', function (o, c) { if (!(o instanceof c)) fail('assertInstanceof'); });
  def('assertUnreachable', function () { fail('unreachable'); });
  def('assertNull', function (v) { if (v !== null) fail('assertNull'); });
  def('assertArrayEquals', function (a, b) { if (JSON.stringify(a) !== JSON.stringify(b)) fail('assertArrayEquals'); });
  // ChakraCore (and QuickJS's assert(actual, expected))
  var assert = function (a, b, m) { if (arguments.length > 1 ? !same(a, b) : !a) fail('assert ' + str(a) + ' ' + (m || '')); };
  assert.areEqual = function (a, b, m) { if (!same(a, b) && !(typeof a === 'object' && a !== null && JSON.stringify(a) === JSON.stringify(b))) fail('areEqual ' + str(a) + ' ' + str(b) + ' ' + (m || '')); };
  assert.areNotEqual = function (a, b) { if (same(a, b)) fail('areNotEqual'); };
  assert.isTrue = function (v, m) { if (v !== true) fail('isTrue ' + (m || '')); };
  assert.isFalse = function (v, m) { if (v !== false) fail('isFalse ' + (m || '')); };
  assert.isUndefined = function (v) { if (v !== undefined) fail('isUndefined'); };
  assert.isNotUndefined = function (v) { if (v === undefined) fail('isNotUndefined'); };
  assert.throws = function (f, c, m) { throws(f, c, m); };
  assert.doesNotThrow = function (f) { try { f(); } catch (e) { fail('threw ' + str(e)); } };
  assert.fail = function (m) { fail('fail ' + (m || '')); };
  assert.strictEqual = assert.areEqual;
  assert.sameValue = assert.areEqual;
  def('assert', assert);
  def('WScript', { Echo: function () { out(Array.prototype.map.call(arguments, str).join(' ')); }, LoadScriptFile: noop, SetTimeout: noop, Arguments: [], Flag: noop, Quit: noop, Platform: {} });
  def('testRunner', { run: function (ts) { for (var i = 0; i < ts.length; i++) { try { ts[i].body(); } catch (e) { out('test ' + (ts[i].name || i) + ' threw ' + str(e)); } } }, runTests: function (ts) { g.testRunner.run(ts); } });
  def('HermesInternal', new Proxy({}, { get: function () { return noop; } }));
  def('helpers', { writeln: out, printObject: out });
})(globalThis);
"#;

/// Which engine a test directory is from, by its path.
fn engine_of(root: &Path) -> &'static str {
    let s = root.to_string_lossy().to_lowercase();
    if s.contains("jstests") || s.contains("webkit") {
        "jsc"
    } else if s.contains("jit-test") || s.contains("non262") || s.contains("firefox") || s.contains("gecko") {
        "sm"
    } else if s.contains("chakra") {
        "chakra"
    } else if s.contains("hermes") {
        "hermes"
    } else if s.contains("quickjs") {
        "qjs"
    } else if s.contains("mjsunit") || s.contains("v8") {
        "v8"
    } else {
        "other"
    }
}

/// Words that put a test in a bug class (the classes of seeds/README.md).
pub fn classes(src: &str) -> Vec<&'static str> {
    let has = |ws: &[&str]| ws.iter().any(|w| src.contains(w));
    let mut c = Vec::new();
    if has(&["valueOf", "toString()", "Symbol.toPrimitive", "get length", "defineProperty(Array.prototype", "__defineGetter__"]) {
        c.push("reentrancy");
    }
    if has(&["Symbol.species"]) {
        c.push("species");
    }
    if has(&["Proxy"]) {
        c.push("proxy");
    }
    if has(&["ArrayBuffer", "Int8Array", "Uint8Array", "Int32Array", "Float64Array", "DataView", "detach", "transfer"]) {
        c.push("typedarray");
    }
    if has(&["RegExp", "lastIndex", ".exec(", "Symbol.replace", "Symbol.split", "Symbol.match"]) {
        c.push("regexp");
    }
    if has(&["noInline", "optimizeNextInvocation", "%Optimize", "%PrepareFunction", "inIon", "|jit-test|", "noFTL", "noDFG", "testLoopCount"]) {
        c.push("jit");
    }
    if has(&["gc(", "fullGC", "edenGC", "minorgc", "WeakRef", "FinalizationRegistry", "WeakMap"]) {
        c.push("gc");
    }
    if has(&["function*", "yield", "async ", "await", "Promise"]) {
        c.push("iterator");
    }
    if has(&["arguments"]) {
        c.push("arguments");
    }
    if has(&["with (", "with(", "eval(", "new Function"]) {
        c.push("scope");
    }
    if has(&["BigInt", "0n", "2147483647", "0x7fffffff", "4294967295", "MAX_SAFE_INTEGER"]) {
        c.push("numeric");
    }
    if c.is_empty() {
        c.push("misc");
    }
    c
}

/// What the test needs that CErXes has not got (or that only that engine's
/// shell can give): such tests are left out.
const NEEDS: &[&str] = &[
    "WebAssembly", "wasm", "load(", "loadFile", "loadRelativeToScript", "newGlobal", "evalcx", "Worker", "$262.agent",
    "readFile", "readline(", "os.file", "os.getenv", "createGlobalObject", "evalInWorker", "LoadModule", "import(",
    "Debugger(", "new Debugger", "getSelfHostedValue", "wrapWithProto", "nukeCCW", "grayRoot", "createIsHTMLDDA",
    "document.", "window.", "Atomics.wait", "evaluate(", "offThreadCompile", "parseModule", "registerModule",
    "WScript.LoadScript", "WScript.LoadModule", "Intl.", "SharedArrayBuffer", "Realm.", "d8.", "%DebugPrint",
    "setInterval", "$.agent", "HermesInternal.getEpilogue", "serialize(", "deserialize(",
];

/// A loop bound of 1000 or more in a loop head becomes 300.
fn shrink_loops(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    for line in src.lines() {
        let t = line.trim_start();
        if t.starts_with("for") || t.starts_with("while") || t.contains(" for (") || t.contains(" while (") {
            out.push_str(&shrink_numbers(line));
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

fn shrink_numbers(l: &str) -> String {
    let b = l.as_bytes();
    let mut out = String::with_capacity(l.len());
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        let word_before = i > 0 && (b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_' || b[i - 1] == b'$' || b[i - 1] == b'.');
        if c.is_ascii_digit() && !word_before {
            let s = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] == b'.') {
                i += 1;
            }
            let tok = &l[s..i];
            let v: Option<f64> = tok.replace('_', "").parse().ok();
            match v {
                Some(x) if x >= 1000.0 && x <= 1e12 => out.push_str("300"),
                _ => out.push_str(tok),
            }
        } else {
            out.push(c as char);
            i += 1;
        }
    }
    out
}

/// Directories of an engine's tests that are about things CErXes has not
/// got (wasm, asm.js, the debugger, Intl, time travel, modules …).
const SKIP_DIRS: &[&str] = &[
    "wasm", "asm.js", "asmjs", "ASMJSParser", "AsmJs", "WasmSpec", "Debugger", "debug", "TTBasic", "TTExecuteBasic",
    "es6module", "native-tests", "benchmarks", "Intl", "intl", "modules", "module", "xdr", "gc/bug-1", "sharedbuf",
    "atomics", "worker", "self-hosting", "parser/lazy", "ctypes", "ion/bug-wasm", "test262", "262", "binast",
    "heap-analysis", "profiler", "stream", "realms", "cacheir/wasm", "fuses", "inspector",
];

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut es: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(d) => d.filter_map(|e| e.ok().map(|e| e.path())).collect(),
        Err(_) => return,
    };
    es.sort();
    for p in es {
        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if p.is_dir() {
            if !SKIP_DIRS.iter().any(|d| name.eq_ignore_ascii_case(d)) {
                walk(&p, out);
            }
        } else if name.ends_with(".js") {
            out.push(p);
        }
    }
}

pub struct Stats {
    pub read: usize,
    pub written: usize,
    pub skipped_needs: usize,
    pub skipped_size: usize,
}

/// Converts the tests under `root` into `out/<engine>/<n>-<name>.js`.
/// `max` limits how many (0: all); regression tests (a bug number in the
/// name or a bug link in the text) come first.
pub fn import(root: &Path, out: &Path, max: usize, engine: Option<&str>) -> std::io::Result<Stats> {
    let eng = engine.map(|e| e.to_string()).unwrap_or_else(|| engine_of(root).to_string());
    let mut files = Vec::new();
    walk(root, &mut files);
    let mut st = Stats { read: 0, written: 0, skipped_needs: 0, skipped_size: 0 };
    let mut picked: Vec<(bool, PathBuf, String)> = Vec::new();
    for f in files {
        st.read += 1;
        let text = match std::fs::read_to_string(&f) {
            Ok(t) => t,
            Err(_) => continue,
        };
        if text.len() > 24 * 1024 || text.len() < 20 {
            st.skipped_size += 1;
            continue;
        }
        // module syntax at the start of a line
        let module = text.lines().any(|l| l.starts_with("import ") || l.starts_with("export "));
        if module || NEEDS.iter().any(|n| text.contains(n)) {
            st.skipped_needs += 1;
            continue;
        }
        let rel = f.strip_prefix(root).unwrap_or(&f).to_string_lossy().replace('\\', "/");
        let low = rel.to_lowercase();
        let regress = low.contains("regress") || low.contains("bug") || low.contains("crash") || low.contains("cve") || low.contains("issue")
            || text.contains("bugs.webkit.org") || text.contains("webkit.org/b/") || text.contains("rdar://") || text.contains("bugzilla")
            || text.contains("crbug") || text.contains("CVE-") || low.chars().filter(|c| c.is_ascii_digit()).count() >= 5;
        picked.push((regress, f.clone(), rel));
    }
    // regression tests first, then the rest, each in path order
    picked.sort_by(|a, b| b.0.cmp(&a.0).then(a.2.cmp(&b.2)));
    if max > 0 && picked.len() > max {
        picked.truncate(max);
    }
    let dir = out.join(&eng);
    std::fs::create_dir_all(&dir)?;
    for (n, (_, f, rel)) in picked.iter().enumerate() {
        let text = std::fs::read_to_string(f)?;
        // V8 natives (%OptimizeFunctionOnNextCall(f)) are not JavaScript
        let text = strip_natives(&text);
        let body = shrink_loops(&text);
        let cls = classes(&body).join(",");
        let name: String = rel.rsplit('/').next().unwrap_or("t.js").chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' }).collect();
        let path = dir.join(format!("{:05}-{}", n, name));
        let s = format!("/* foreign: {} {} {} */\n{}{}", eng, rel, cls, SHIM, body);
        std::fs::write(path, s)?;
        st.written += 1;
    }
    Ok(st)
}

/// `%Name(args)` → `void [args]` (the arguments are still evaluated).
fn strip_natives(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 1 < b.len() && b[i + 1].is_ascii_uppercase() && (i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b')' || b[i - 1] == b']')) {
            let mut j = i + 1;
            while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
                j += 1;
            }
            if j < b.len() && b[j] == b'(' {
                out.push_str("void [");
                // find the matching parenthesis
                let mut depth = 0;
                let mut k = j;
                while k < b.len() {
                    if b[k] == b'(' {
                        depth += 1;
                    } else if b[k] == b')' {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    k += 1;
                }
                out.push_str(&src[j + 1..k.min(src.len())]);
                out.push(']');
                i = k + 1;
                continue;
            }
        }
        // copy one UTF-8 character
        let ch = src[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shrinks_loop_heads_only() {
        assert_eq!(shrink_loops("for (let i = 0; i < 100000; i++) a[1e6] = 2;\nx = 5000;\n"), "for (let i = 0; i < 300; i++) a[300] = 2;\nx = 5000;\n");
    }
    #[test]
    fn strips_v8_natives() {
        assert_eq!(strip_natives("%OptimizeFunctionOnNextCall(f); x = 5 % Y(1);"), "void [f]; x = 5 % Y(1);");
    }
}
