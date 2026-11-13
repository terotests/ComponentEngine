// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Random JavaScript programs and mutations of existing ones.
//
// A generated program is deterministic (no Date.now, Math.random or
// timing), its loops are bounded, and every statement sits in its own
// try/catch that prints the error's constructor name, so one throw does not
// end the run and two engines can be compared line by line (`__p`).

pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub fn next(&mut self) -> u64 {
        // xorshift64*
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
    pub fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }
    pub fn pick<'a>(&mut self, xs: &[&'a str]) -> &'a str {
        xs[self.below(xs.len())]
    }
}

/// What every generated program starts with: `__p` prints a value the same
/// way in every engine (type, then a stable text).
pub const PRELUDE: &str = r#"var __out = typeof print === 'function' ? print : console.log;
function __s(v, d) {
  if (d > 3) return '…';
  var t = typeof v;
  if (v === null) return 'null';
  if (t === 'number') return Object.is(v, -0) ? '-0' : String(v);
  if (t === 'bigint') return v + 'n';
  if (t === 'string') return JSON.stringify(v.length > 200 ? v.slice(0, 200) : v);
  if (t === 'symbol') return 'symbol';
  if (t === 'function') return 'function';
  if (t !== 'object') return String(v);
  if (Array.isArray(v)) { var a = []; for (var i = 0; i < v.length && i < 20; i++) a.push(i in v ? __s(v[i], d + 1) : '_'); return '[' + a.join(',') + ']'; }
  if (v instanceof Map) return 'Map(' + v.size + ')';
  if (v instanceof Set) return 'Set(' + v.size + ')';
  if (v instanceof Error) return 'Error:' + v.name;
  if (v instanceof RegExp) return 'RegExp:' + v.source + '/' + v.flags;
  if (ArrayBuffer.isView(v)) return Object.prototype.toString.call(v) + v.length;
  var k = Object.keys(v).slice(0, 20), o = [];
  for (var j = 0; j < k.length; j++) { var x; try { x = __s(v[k[j]], d + 1); } catch (e) { x = '!'; } o.push(k[j] + ':' + x); }
  return '{' + o.join(',') + '}';
}
function __p(tag, v) { var s; try { s = __s(v, 0); } catch (e) { s = '!' + (e && e.name); } __out(tag + ' ' + s); }
function __e(tag, e) { __out(tag + ' throw ' + (e && e.constructor && e.constructor.name)); }
"#;

pub struct Gen {
    pub rng: Rng,
    vars: Vec<String>,
    funcs: Vec<String>,
    classes: Vec<String>,
    depth: usize,
    stmt_no: usize,
    /// lets the generator write programs that run long or allocate much
    /// (resource-limit oracle); off for differential runs
    pub heavy: bool,
}

const NUMS: &[&str] = &[
    "0", "-0", "1", "-1", "2", "3", "7", "10", "0.5", "-1.5", "1e21", "1e-7", "255", "256", "65535", "65536",
    "2147483647", "2147483648", "-2147483648", "4294967295", "4294967296", "9007199254740991", "9007199254740993",
    "NaN", "Infinity", "-Infinity", "Number.MAX_VALUE", "Number.MIN_VALUE", "Number.EPSILON", "0x7f", "0o17", "0b101",
    "1_000",
];
/// Numbers at the edges as expressions, so the engine computes them
/// (a literal -0 takes another path than one that comes out of %).
pub const NEDGES: &[&str] = &[
    "(-0)", "(0 * -1)", "(-1 % 1)", "(-0 % 5)", "(1 / -Infinity)", "Math.round(-0.4)", "(2147483647 + 1)", "(-2147483648 - 1)",
    "(2 ** 31)", "(2 ** 32)", "(2 ** 53)", "(2 ** 53 + 1)", "(0.1 + 0.2)", "(-2147483648 / -1)", "(4294967295 >>> 0)", "(-1 >>> 0)",
    "(1 << 31)", "(Number.MAX_SAFE_INTEGER + 2)", "(5e-324)", "(-5e-324)", "(1e308 * 10)", "(NaN ** 0)", "(-8 % 3)", "(8 % -3)",
    "((-2) ** 0.5)", "(0 / 0)", "(1e21 + 1)", "(-1e-7)", "(65535 * 65537)", "(0x7fffffff * 2)", "(-(2 ** 31))", "(1e300)", "(2 ** -1074)",
];
const STRS: &[&str] = &[
    "''", "'a'", "'abc'", "'ä'", "'\\u{1F600}'", "'\\uD800'", "'0'", "'-1'", "'1e3'", "' 12 '", "'length'",
    "'__proto__'", "'constructor'", "'toString'", "'a,b,c'", "'\\n'", "'aaaaaaaaaaaaaaaa'", "'İ'", "'ß'", "'𝒳'",
];
const BIGS: &[&str] = &["0n", "1n", "-1n", "(2n ** 64n)", "(-(2n ** 63n))", "123456789012345678901234567890n"];
const BINOPS: &[&str] = &[
    "+", "-", "*", "/", "%", "**", "&", "|", "^", "<<", ">>", ">>>", "==", "!=", "===", "!==", "<", "<=", ">", ">=",
    "&&", "||", "??", "in", "instanceof", ",",
];
const UNOPS: &[&str] = &["-", "+", "!", "~", "typeof ", "void "];
const ARR_M: &[&str] = &[
    "push(1)", "pop()", "shift()", "unshift(0)", "slice(1)", "slice(-2, -1)", "splice(1, 1)", "splice(0, 0, 9, 8)",
    "concat([1,2])", "join('-')", "reverse()", "sort()", "sort((a, b) => b - a)", "indexOf(1)", "lastIndexOf(1)",
    "includes(NaN)", "find(x => x)", "findLast(x => x)", "findIndex(x => !x)", "filter(x => x)", "map(x => [x])",
    "flat(2)", "flatMap(x => [x, x])", "reduce((a, b) => a + b, 0)", "reduceRight((a, b) => a + b)", "fill(7, 1)",
    "copyWithin(0, 1)", "keys()", "entries()", "at(-1)", "with(0, 5)", "toSorted()", "toReversed()",
    "toSpliced(0, 1)", "every(x => x)", "some(x => x)", "forEach(x => x)", "length", "length = 2", "length = 0",
];
const STR_M: &[&str] = &[
    "length", "charAt(1)", "charCodeAt(0)", "codePointAt(0)", "at(-1)", "slice(1, -1)", "substring(2, 0)",
    "substr(-2)", "indexOf('a')", "split('')", "split(',', 2)", "split(/(a)/)", "replace('a', 'b')",
    "replace(/a/g, '$&$&')", "replaceAll('a', '$1')", "toUpperCase()", "toLowerCase()", "trim()", "padStart(5, 'xy')",
    "padEnd(3)", "repeat(3)", "startsWith('a')", "endsWith('c')", "normalize('NFD')", "localeCompare('b')",
    "match(/a(b)?/)", "matchAll(/a/g)", "search(/c/)", "concat(1, null)", "isWellFormed()", "toWellFormed()",
];
const MATH: &[&str] = &[
    "Math.floor", "Math.ceil", "Math.round", "Math.trunc", "Math.sign", "Math.abs", "Math.sqrt", "Math.cbrt",
    "Math.log2", "Math.clz32", "Math.fround", "Math.expm1", "Math.hypot", "Math.max", "Math.min", "Math.atan2",
    "Math.imul", "Number", "String", "Boolean", "parseInt", "parseFloat", "Number.isInteger", "Number.isSafeInteger",
    "isNaN", "Object.keys", "Object.values", "Object.entries", "Object.getOwnPropertyNames", "Object.freeze",
    "Object.isFrozen", "Array.from", "Array.of", "Array.isArray", "JSON.stringify", "Symbol", "BigInt",
    "encodeURIComponent", "decodeURIComponent", "escape", "structuredClone",
];
const REGEX: &[&str] = &[
    "/a+b*/g", "/(a|b)*c/", "/^\\d{2,4}$/m", "/(?<y>\\d+)-(?<m>\\d+)/", "/\\bfoo\\b/i", "/[\\u{1F600}-\\u{1F64F}]/u",
    "/(?=a)a/", "/(?<!b)a/", "/(a)\\1/", "/./s", "/[^]/y", "/\\p{L}+/u", "/(?:x|y){0,3}?/", "/[[a-z]--[aeiou]]/v",
];

impl Gen {
    pub fn new(seed: u64) -> Gen {
        Gen { rng: Rng::new(seed), vars: Vec::new(), funcs: Vec::new(), classes: Vec::new(), depth: 0, stmt_no: 0, heavy: false }
    }

    /// A whole program: the prelude, some declarations, then statements.
    pub fn program(&mut self) -> String {
        let mut s = String::from(PRELUDE);
        let n = 4 + self.rng.below(5);
        for i in 0..n {
            let name = format!("v{}", i);
            s.push_str(&format!("var {} = {};\n", name, self.literal()));
            self.vars.push(name);
        }
        for i in 0..1 + self.rng.below(3) {
            let name = format!("f{}", i);
            let body = self.block(2);
            s.push_str(&format!("function {}(a, b = {}, ...r) {{ {} return {}; }}\n", name, self.atom(), body, self.expr()));
            self.funcs.push(name);
        }
        if self.rng.chance(50) {
            let c = String::from("C0");
            s.push_str(&format!(
                "class C0 {{ #p = {}; static s = {}; constructor(x) {{ this.x = x; }} get g() {{ return this.#p; }} set g(v) {{ this.#p = v; }} m(y) {{ return {}; }} static {{ C0.t = 1; }} }}\n",
                self.atom(), self.atom(), self.expr()
            ));
            s.push_str(&format!("class D0 extends C0 {{ constructor() {{ super({}); }} m(y) {{ return super.m(y) + 1; }} }}\n", self.atom()));
            self.classes.push(c);
            self.classes.push(String::from("D0"));
        }
        let n = 8 + self.rng.below(24);
        for _ in 0..n {
            let st = self.stmt();
            s.push_str(&st);
            s.push('\n');
        }
        if self.rng.chance(30) {
            s.push_str("Promise.resolve().then(() => __p('job', v0)).catch(e => __e('job', e));\n");
        }
        s
    }

    fn tag(&mut self) -> String {
        self.stmt_no += 1;
        format!("s{}", self.stmt_no)
    }

    fn var(&mut self) -> String {
        if self.vars.is_empty() {
            return String::from("undefined");
        }
        self.vars[self.rng.below(self.vars.len())].clone()
    }

    fn literal(&mut self) -> String {
        match self.rng.below(11) {
            0 | 1 => self.rng.pick(NUMS).to_string(),
            2 => self.rng.pick(STRS).to_string(),
            3 => format!("[{}, {}, , {}]", self.atom(), self.atom(), self.atom()),
            4 => format!("{{ a: {}, b: {}, [{}]: 1, get c() {{ return {}; }} }}", self.atom(), self.atom(), self.rng.pick(STRS), self.atom()),
            5 => self.rng.pick(BIGS).to_string(),
            6 => format!("new Map([[{}, {}]])", self.atom(), self.atom()),
            7 => format!("new {}([1, 2, 300, -1])", self.rng.pick(&["Uint8Array", "Int16Array", "Float64Array", "Uint8ClampedArray", "Int32Array", "Float32Array"])),
            8 => self.rng.pick(REGEX).to_string(),
            9 => self.rng.pick(&["null", "undefined", "true", "false", "Symbol.iterator", "Math"]).to_string(),
            _ => format!("`t${{{}}}x`", self.atom()),
        }
    }

    fn atom(&mut self) -> String {
        if self.rng.chance(6) {
            return self.rng.pick(NEDGES).to_string();
        }
        if self.rng.chance(45) && !self.vars.is_empty() {
            self.var()
        } else {
            match self.rng.below(4) {
                0 => self.rng.pick(STRS).to_string(),
                1 => self.rng.pick(BIGS).to_string(),
                _ => self.rng.pick(NUMS).to_string(),
            }
        }
    }

    pub fn expr(&mut self) -> String {
        if self.depth > 4 {
            return self.atom();
        }
        self.depth += 1;
        let r = match self.rng.below(22) {
            // operands in parentheses: `-1 ** 2` is a SyntaxError
            0 | 1 | 2 => format!("(({}) {} ({}))", self.expr(), self.rng.pick(BINOPS), self.expr()),
            // `- -1` written as `--1` would be a decrement
            3 => format!("({}({}))", self.rng.pick(UNOPS), self.expr()),
            4 => format!("({}.{})", self.var(), self.rng.pick(ARR_M)),
            5 => format!("String({}).{}", self.atom(), self.rng.pick(STR_M)),
            6 => format!("{}({}, {})", self.rng.pick(MATH), self.expr(), self.atom()),
            7 => format!("({} ? {} : {})", self.expr(), self.expr(), self.expr()),
            8 => {
                let f = if self.funcs.is_empty() { String::from("String") } else { self.funcs[self.rng.below(self.funcs.len())].clone() };
                format!("{}({}, {}, ...[{}])", f, self.expr(), self.atom(), self.atom())
            }
            9 => format!("[...{}, {}]", self.iterable(), self.atom()),
            10 => format!("{{ ...{}, k: {} }}", self.var(), self.expr()),
            11 => format!("(({{ a, b = {}, ...r }}) => [a, b, r])({})", self.atom(), self.var()),
            12 => format!("(([x, , y = {}, ...z]) => [x, y, z])({})", self.atom(), self.iterable()),
            13 => format!("{}?.{}?.(1)", self.var(), self.rng.pick(&["a", "b", "length", "x", "toString", "valueOf"])),
            14 => format!("{}.exec({})", self.rng.pick(REGEX), self.rng.pick(STRS)),
            15 => {
                if self.classes.is_empty() {
                    format!("new Date(0).toISOString()")
                } else {
                    let c = self.classes[self.rng.below(self.classes.len())].clone();
                    format!("new {}({}).m({})", c, self.atom(), self.expr())
                }
            }
            16 => format!("(function* () {{ yield {}; yield* {}; return {}; }})().next()", self.expr(), self.iterable(), self.atom()),
            17 => format!("new Proxy({}, {{ get(t, k) {{ return typeof k === 'string' ? k.length : undefined; }} }}).{}", self.rng.pick(&["{}", "[]", "function () {}"]), self.rng.pick(&["a", "length", "xyz"])),
            18 => format!("JSON.parse(JSON.stringify({}) ?? 'null')", self.expr()),
            19 => format!("(() => {{ try {{ return {}; }} finally {{ ({}); }} }})()", self.expr(), self.expr()),
            20 => format!("Reflect.ownKeys({}).length", self.var()),
            _ => self.literal(),
        };
        self.depth -= 1;
        r
    }

    fn iterable(&mut self) -> String {
        match self.rng.below(5) {
            0 => format!("[{}, {}]", self.atom(), self.atom()),
            1 => self.rng.pick(STRS).to_string(),
            2 => format!("new Set([{}, {}])", self.atom(), self.atom()),
            3 => format!("new Map([[1, {}]]).keys()", self.atom()),
            _ => format!("[...'ab', {}]", self.atom()),
        }
    }

    fn block(&mut self, n: usize) -> String {
        let mut s = String::new();
        for _ in 0..n {
            let e = self.expr();
            s.push_str(&format!("({}); ", e));
        }
        s
    }

    fn stmt(&mut self) -> String {
        let t = self.tag();
        let body = match self.rng.below(23) {
            0 | 1 | 2 | 3 => format!("__p('{}', {});", t, self.expr()),
            4 => {
                let v = self.var();
                format!("{} = {}; __p('{}', {});", v, self.expr(), t, v)
            }
            5 => {
                let v = self.var();
                format!("{}[{}] = {}; __p('{}', {});", v, self.atom(), self.expr(), t, v)
            }
            6 => format!("for (let i = 0; i < {}; i++) {{ {} }} __p('{}', 1);", self.rng.below(50), self.block(1), t),
            7 => format!("{{ let acc = []; for (const x of {}) acc.push(x); __p('{}', acc); }}", self.iterable(), t),
            8 => format!("{{ let acc = []; for (const k in {}) acc.push(k); __p('{}', acc); }}", self.var(), t),
            9 => format!("switch ({}) {{ case {}: __p('{}', 1); break; case {}: __p('{}', 2); default: __p('{}', 3); }}", self.expr(), self.atom(), t, self.atom(), t, t),
            10 => format!("{{ let n = 0; while (n < {}) {{ n++; if (n % 3 === 0) continue; if (n > 20) break; }} __p('{}', n); }}", self.rng.below(40), t),
            11 => format!("(async () => {{ const x = await {}; __p('{}', x); }})().catch(e => __e('{}', e));", self.expr(), t, t),
            12 => format!("{{ const {{ a = {}, ...rest }} = {}; __p('{}', [a, rest]); }}", self.atom(), self.var(), t),
            13 => format!("__p('{}', Object.getOwnPropertyDescriptor({}, {}));", t, self.var(), self.atom()),
            14 if self.heavy => self.heavy_stmt(&t),
            16 | 17 => self.side_stmt(&t),
            18 => self.hot_stmt(&t),
            19 => self.finally_stmt(&t),
            20 => self.edge_stmt(&t),
            21 => self.reentry_stmt(&t),
            _ => format!("label{}: for (const a of [1, 2, 3]) {{ for (const b of [4, 5]) {{ if (a * b > {}) break label{}; }} }} __p('{}', 0);", t, self.atom(), t, t),
        };
        format!("try {{ {} }} catch (e) {{ __e('{}', e); }}", body, t)
    }

    // ------------------------------------------------------------------
    // Patterns from the audit (F1–F3, F7, F8) and from V8's bug history
    // (thread "Test CErXes against known V8 bugs", classes in
    // seeds/README.md). Every one is bounded and deterministic, so the
    // engines can still be compared line by line.

    /// A builtin whose argument, callback or element runs user code that
    /// changes the receiver halfway (V8 class array-builtin-reentrancy,
    /// typedarray-detach-resize; audit F1, F7).
    fn side_stmt(&mut self, t: &str) -> String {
        if self.rng.chance(30) {
            return self.typed_side_stmt(t);
        }
        let init = self.rng.pick(&[
            "[0, 1, 2, 3, 4, 5]", "[0, 1, , 3, 4]", "[0.5, 1.5, 2.5, 3.5]", "['a', 'b', 'c', 'd', 'e']",
            "[{}, [], 2, 'x', null, undefined]", "Array.from({ length: 9 }, (_, i) => i)", "[1, 2, 3]",
        ]);
        let effect = self.array_effect("a");
        let ret = self.rng.pick(&["0", "1", "2", "-1", "-2", "4", "10", "-0", "1.5", "NaN", "Infinity", "-Infinity", "2 ** 32"]);
        let s = match self.rng.below(3) {
            0 => format!("{{ valueOf() {{ {}; return {}; }} }}", effect, ret),
            1 => format!("{{ toString() {{ {}; return '{}'; }}, valueOf: undefined }}", effect, ret.replace('\'', "")),
            _ => format!("{{ [Symbol.toPrimitive]() {{ {}; return {}; }} }}", effect, ret),
        };
        let once = format!("(x, i) => {{ if (i === {}) {{ {}; }} return x; }}", self.rng.below(3), effect);
        let call = match self.rng.below(30) {
            0 => format!("a.fill(7, s)"),
            1 => format!("a.fill(s, 1)"),
            2 => format!("a.fill(7, 0, s)"),
            3 => format!("a.copyWithin(s, 1)"),
            4 => format!("a.copyWithin(0, s)"),
            5 => format!("a.copyWithin(0, 1, s)"),
            6 => format!("a.splice(s, 1)"),
            7 => format!("a.splice(0, s, 'x', 'y')"),
            8 => format!("a.slice(s)"),
            9 => format!("a.with(s, 'w')"),
            10 => format!("a.indexOf(3, s)"),
            11 => format!("a.lastIndexOf(1, s)"),
            12 => format!("a.includes(undefined, s)"),
            13 => format!("a.at(s)"),
            14 => format!("a.join(s)"),
            15 => format!("a.flat(s)"),
            16 => format!("a.toSpliced(s, 1)"),
            17 => format!("a[s]"),
            18 => format!("(a[s] = 'k', a)"),
            19 => format!("a.sort((x, y) => {{ {}; return (x > y) - (x < y); }})", effect),
            20 => format!("a.map({})", once),
            21 => format!("a.filter({})", once),
            22 => format!("a.reduce((p, x, i) => {{ if (i === 1) {{ {}; }} return p + ',' + x; }}, '')", effect),
            23 => format!("a.findLastIndex({})", once),
            24 => format!("(a[1] = {{ toString() {{ {}; return 'e'; }} }}, a.join())", effect),
            25 => format!("(a[2] = {{ toString() {{ {}; return 'e'; }} }}, String(a))", effect),
            26 => format!("(a.constructor = {{ [Symbol.species]: function (n) {{ return [9]; }} }}, a.map(x => x))"),
            27 => format!("Array.prototype.concat.call(a, s, a)"),
            28 => format!("a.entries().toArray ? [...a.entries()].length : 0"),
            _ => format!("Array.from(a, {})", once),
        };
        format!(
            "{{ const a = {}; const s = {}; try {{ __p('{}', {}); }} catch (e) {{ __e('{}', e); }} __p('{}', [a.length, a]); }} delete Array.prototype[1]; delete Array.prototype[3];",
            init, s, t, call, t, t
        )
    }

    /// Something user code does to an array in the middle of a builtin.
    fn array_effect(&mut self, a: &str) -> String {
        let k = self.rng.pick(&["0", "1", "2", "3", "100"]);
        match self.rng.below(14) {
            0 | 1 => format!("{}.length = {}", a, k),
            2 => format!("{}.push(9, 9, 9, 9)", a),
            3 => format!("delete {}[1]", a),
            4 => format!("{}.length = 0", a),
            5 => format!("Array.prototype[1] = 'P'"),
            6 => format!("Array.prototype[3] = 'Q'"),
            7 => format!("Object.defineProperty({}, 0, {{ get() {{ return 'g'; }}, configurable: true }})", a),
            8 => format!("{}.splice(0, 2)", a),
            9 => format!("Object.freeze({})", a),
            10 => format!("{}.unshift(-1, -2)", a),
            11 => format!("{}[1e4] = 1", a),
            12 => format!("{}.reverse()", a),
            _ => format!("Object.setPrototypeOf({}, Object.prototype)", a),
        }
    }

    /// The same for typed arrays: the buffer shrinks, grows or is
    /// detached while a builtin holds its length.
    fn typed_side_stmt(&mut self, t: &str) -> String {
        let ty = self.rng.pick(&["Uint8Array", "Int16Array", "Float64Array", "Int32Array", "Uint8ClampedArray", "BigInt64Array"]);
        let fill = if ty == "BigInt64Array" { "1n" } else { "1" };
        let effect = self.rng.pick(&["b.resize(0)", "b.resize(8)", "b.resize(64)", "b.transfer()", "b.transfer(8)", "b.transferToFixedLength()", "ta.set([5])"]);
        let ret = self.rng.pick(&["0", "1", "2", "8", "-1", "16"]);
        let s = format!("{{ valueOf() {{ {}; return {}; }} }}", effect, ret);
        let call = match self.rng.below(13) {
            0 => format!("ta.fill({}, s)", fill),
            1 => format!("ta.set([{}], s)", fill),
            2 => format!("ta.subarray(s)"),
            3 => format!("ta.slice(s)"),
            4 => format!("ta.copyWithin(s, 0)"),
            5 => format!("ta.sort((x, y) => {{ {}; return 0; }})", effect),
            6 => format!("new {}(b, s)", ty),
            7 => format!("new DataView(b).getInt8(s)"),
            8 => format!("ta.indexOf({}, s)", fill),
            9 => format!("Array.prototype.join.call(ta, {{ toString() {{ {}; return '-'; }} }})", effect),
            10 => format!("ta.map((x, i) => {{ if (i === 0) {{ {}; }} return x; }})", effect),
            11 => format!("ta.with(s, {})", fill),
            _ => format!("ta.at(s)"),
        };
        format!(
            "{{ const b = new ArrayBuffer(32, {{ maxByteLength: 64 }}); const ta = new {}(b); const s = {}; try {{ __p('{}', {}); }} catch (e) {{ __e('{}', e); }} __p('{}', [b.byteLength, ta.length]); }}",
            ty, s, t, call, t, t
        )
    }

    /// A function warmed up with one shape, type or range of index, then
    /// given the edge (V8 class jit-type-range-assumptions; audit F8).
    /// 160 rounds pass the JIT's threshold of 64.
    fn hot_stmt(&mut self, t: &str) -> String {
        let init = self.rng.pick(&[
            "[1, 2, 3, 4]", "[1.5, 2.5, 3.5, 4.5]", "['a', 'b', 'c', 'd']", "[1, , 3, 4]", "new Int32Array([1, 2, 3, 4])",
            "new Float64Array([1, 2, 3, 4])", "{ 0: 1, 1: 2, 2: 3, 3: 4, length: 4, x: 1 }", "[[1], [2], [3], [4]]",
        ]);
        let body = self.rng.pick(&[
            "a[i]", "a[i] + 1", "a[i] | 0", "(a[i] * 2) >>> 0", "a.length", "a.x", "(a[i] = i, a[i])", "a[i] ?? -1",
            "a[i] === undefined", "typeof a[i]", "-a[i]", "a[i] % 3", "a[i] << 31", "i in a", "a.at(i)",
        ]);
        let edge = self.rng.pick(&[
            "2 ** 32", "2 ** 32 + 1", "2 ** 31", "4294967295", "-1", "-0", "NaN", "'1'", "1.5", "2 ** 53", "2 ** 52 + 1", "1e21", "-2147483648", "8",
        ]);
        let pert = self.rng.pick(&[
            "a.push(0.5)", "a[1] = 'x'", "delete a[1]", "Array.prototype[1] = 'P'", "Object.prototype[2] = 'O'", "a.length = 1", "Object.defineProperty(a, 2, { get() { return 'G'; } })",
            "a.x = 5", "Object.freeze(a)", "a[100] = 1", "Object.setPrototypeOf(a, { 1: 'q', 5: 'r' })", "a.length = 0", "a[0] = {}", "a[3] = 2 ** 31",
        ]);
        let p = 64 + self.rng.below(90);
        let idx = if self.rng.chance(60) { format!("(i === {} ? {} : i % 4)", p + 1, edge) } else { String::from("i % 4") };
        let rounds = if self.rng.chance(50) { 160 } else { 70 };
        let r = format!(
            "{{ function h(a, i) {{ return {}; }} const a = {}; const seen = []; try {{ for (let i = 0; i < {}; i++) {{ if (i === {}) {{ {}; }} const r = h(a, {}); if (i >= {}) seen.push(r); }} }} finally {{ delete Array.prototype[1]; delete Object.prototype[2]; }} __p('{}', seen); }}",
            body, init, rounds, p, pert, idx, p.min(rounds - 4), t
        );
        if self.rng.chance(25) {
            // an int32 sum that crosses the boundary inside a hot loop
            let op = self.rng.pick(&["+", "-", "*"]);
            let start = self.rng.pick(&["2147483600", "-2147483600", "1073741824", "0x3fffffff", "65536"]);
            return format!(
                "{{ let s = {}; const out = []; for (let i = 0; i < 160; i++) {{ s = s {} (i * 7); if (i % 40 === 39) out.push(s, s | 0, s >>> 0); }} __p('{}', out); }} {}",
                start, op, t, r
            );
        }
        r
    }

    /// Control flow leaving a try through its finally: return, break,
    /// continue and throw in either (audit F2: register VM).
    fn finally_stmt(&mut self, t: &str) -> String {
        let conds = ["i === 1", "j === 2", "false", "true", "(i + j) % 2 === 1", "i === j", "j === 0"];
        let mut c = || conds[self.rng.below(conds.len())].to_string();
        let (c1, c2, c3, c4, c5) = (c(), c(), c(), c(), c());
        let fin = self.rng.pick(&["break", "continue", "return 'fr'", "continue L", "break L", "log.push('n')", "throw 'ft'"]);
        let fc = self.rng.pick(&conds);
        let gen = self.rng.chance(25);
        let (kw, call, ret_tag) = if gen { ("function*", "[...cf()]", "yield 'y' + i + j; ") } else { ("function", "cf()", "") };
        format!(
            "{{ const log = []; {} cf() {{ L: for (let i = 0; i < 3; i++) {{ for (let j = 0; j < 3; j++) {{ try {{ log.push('t' + i + j); {}if ({}) return 'r' + i; if ({}) throw i; if ({}) continue L; if ({}) break L; }} catch (e) {{ log.push('c' + e); if ({}) return 'cr'; }} finally {{ log.push('f'); if ({}) {}; }} }} }} return 'end'; }} let r; try {{ r = {}; }} catch (e) {{ r = 'x' + e; }} __p('{}', [r, log.join()]); }}",
            kw, ret_tag, c1, c2, c3, c4, c5, fc, fin, call, t
        )
    }

    /// Numbers at the edges computed, not written as literals: -0 out of
    /// %, *, Math.round; int32 and uint32 boundaries; radix conversion
    /// (audit F3; V8 class size-limits-int-overflow: (1e300).toString(3)).
    fn edge_stmt(&mut self, t: &str) -> String {
        let x = self.rng.pick(NEDGES);
        let y = self.rng.pick(NEDGES);
        let op = self.rng.pick(&["+", "-", "*", "/", "%", "**", "&", "|", "^", "<<", ">>", ">>>", "<", "==="]);
        let f = self.rng.pick(&[
            "Math.round", "Math.trunc", "Math.sign", "Math.floor", "Math.ceil", "Math.fround", "Math.expm1", "Math.atan2", "Math.min", "Math.max", "Math.hypot", "Math.imul", "Math.clz32", "Math.abs", "Math.cbrt",
        ]);
        let radix = 2 + self.rng.below(35);
        let fixed = self.rng.pick(&["toFixed(2)", "toPrecision(3)", "toExponential(1)", "toFixed(0)", "toString(16)", "toLocaleString()"]);
        if self.rng.chance(30) {
            // the same in a loop the register machine keeps in int32 slots
            return format!(
                "{{ let s = {}; const y = {}; for (let i = 0; i < 70; i++) s = (s {} y) | 0; __p('{}', [s, Object.is(s, -0)]); }}",
                x, y, op, t
            );
        }
        format!(
            "{{ const x = {}, y = {}; const z = x {} y; __p('{}', [z, Object.is(z, -0), 1 / z, {}(x, y), z | 0, z >>> 0, String(x * y), z.toString({}), z.{}]); }}",
            x, y, op, t, f, radix, fixed
        )
    }

    /// Getters, traps, revivers and iterators that change what a builtin
    /// walks (V8 classes proxy-getter-reentrancy, regexp-reentrancy-limits,
    /// iterator-generator-promise-gc, class-private-scope).
    fn reentry_stmt(&mut self, t: &str) -> String {
        let body = match self.rng.below(16) {
            0 => format!("const o = {{ a: 1, get b() {{ delete this.c; this.d = 4; return 2; }}, c: 3 }}; __p('{}', [Object.assign({{}}, o), Object.entries(o), Object.values(o)]);", t),
            1 => format!("const o = {{ x: 1, y: {{ toJSON() {{ delete o.z; o.w = 5; return 'j'; }} }}, z: 3 }}; __p('{}', JSON.stringify(o));", t),
            2 => format!("__p('{}', JSON.parse('[1,[2,3],{{\"a\":4,\"b\":5}},6]', function (k, v) {{ if (k === '0') {{ this.length = 1; }} if (k === 'a') delete this.b; return v; }}));", t),
            3 => format!("const p = new Proxy({{}}, {{ ownKeys() {{ return ['a', 'a']; }}, getOwnPropertyDescriptor() {{ return {{ value: 1, enumerable: true, configurable: true }}; }} }}); __p('{}', Object.keys(p));", t),
            4 => format!("const re = /a/g; re.lastIndex = {{ valueOf() {{ re.compile('b', 'g'); return 1; }} }}; __p('{}', [re.exec('abab'), re.lastIndex, re.source]);", t),
            5 => format!("const re = /a/; re.exec = function () {{ return {{ length: 2, 0: 'x', index: 5 }}; }}; __p('{}', ['zaz'.replace(re, '$&$&'), re.test('q'), 'aa'.match(re)]);", t),
            6 => format!("const re = /x/y; let n = 0; Object.defineProperty(re, 'flags', {{ get() {{ n++; return 'g'; }} }}); __p('{}', ['axbx'.replaceAll(re, '-'), n]);", t),
            7 => format!("let c = 0; const it = {{ [Symbol.iterator]() {{ return {{ get next() {{ c++; let k = 0; return () => (k++ < 3 && c < 8 ? {{ value: c, done: false }} : {{ done: true }}); }} }}; }} }}; __p('{}', [[...it], c]);", t),
            8 => format!("let closed = 0; const it = {{ [Symbol.iterator]() {{ let i = 0; return {{ next() {{ return {{ value: i++, done: i > 5 }}; }}, return() {{ closed++; return {{}}; }} }}; }} }}; for (const x of it) {{ if (x === 2) break; }} try {{ const [p, q] = it; }} catch (e) {{}} __p('{}', closed);", t),
            9 => format!("class P {{ #x = 1; static has(o) {{ return #x in o; }} static get(o) {{ return o.#x; }} }} const pr = new Proxy(new P(), {{}}); let r; try {{ r = P.get(pr); }} catch (e) {{ r = e.constructor.name; }} __p('{}', [P.has(new P()), P.has(pr), r]);", t),
            10 => format!("class B {{ constructor() {{ return Object.freeze({{ k: 1 }}); }} }} class S extends B {{ #f = 2; static f(o) {{ return o.#f; }} }} let r; try {{ r = S.f(new S()); }} catch (e) {{ r = e.constructor.name; }} __p('{}', r);", t),
            11 => format!("let order = []; const th = {{ then(res) {{ order.push('then'); res(1); res(2); }} }}; Promise.all([th, th]).then(v => __p('{}', [v, order]));", t),
            12 => format!("const m = new Map([[1, 1], [2, 2], [3, 3]]); const seen = []; for (const [k] of m) {{ seen.push(k); if (k === 1) {{ m.delete(2); m.set(4, 4); }} if (seen.length > 10) break; }} __p('{}', seen);", t),
            13 => format!("const s = new Set([1, 2, 3]); const seen = []; s.forEach(v => {{ seen.push(v); if (v < 3 && seen.length < 8) {{ s.delete(v); s.add(v + 10); }} }}); __p('{}', seen);", t),
            14 => format!("const o = {{ get x() {{ Object.defineProperty(o, 'y', {{ enumerable: false }}); return 1; }}, y: 2, z: 3 }}; const ks = []; for (const k in o) ks.push(k); __p('{}', [ks, Object.groupBy ? Object.keys(Object.groupBy([1, 2, 3], v => v % 2)) : 0]);", t),
            _ => format!("const a = [1, 2, 3]; const r = a.concat({{ length: 2, 0: 'x', 1: 'y', [Symbol.isConcatSpreadable]: true }}, Object.assign([9], {{ [Symbol.isConcatSpreadable]: false }})); __p('{}', r);", t),
        };
        format!("{{ {} }}", body)
    }

    /// Statements that stress resources: deep recursion, big strings and
    /// arrays, deep nesting. The engine must throw (RangeError …) or finish,
    /// never crash or hang.
    fn heavy_stmt(&mut self, t: &str) -> String {
        match self.rng.below(16) {
            0 => format!("function r(n) {{ return n ? r(n - 1) + 1 : 0; }} __p('{}', r({}));", t, self.rng.pick(&["1e4", "1e5", "1e6"])),
            1 => format!("__p('{}', 'ab'.repeat({}).length);", t, self.rng.pick(&["1e6", "1e7", "2 ** 28", "2 ** 31"])),
            2 => format!("__p('{}', new Array({}).fill(0).length);", t, self.rng.pick(&["1e6", "1e7", "2 ** 32 - 1"])),
            3 => format!("{{ let o = {{}}; for (let i = 0; i < {}; i++) o = {{ o }}; __p('{}', JSON.stringify(o).length); }}", self.rng.pick(&["1e3", "1e4", "1e5"]), t),
            4 => format!("{{ let a = []; for (let i = 0; i < {}; i++) a = [a]; __p('{}', String(a).length); }}", self.rng.pick(&["1e3", "1e4", "1e5"]), t),
            5 => format!("__p('{}', /(a*)*b/.test('a'.repeat({})));", t, self.rng.pick(&["20", "26", "30"])),
            6 => format!("__p('{}', eval('(' .repeat({}) + '1' + ')'.repeat({})));", t, 2000, 2000),
            7 => format!("{{ const a = []; for (let i = 0; i < 1e5; i++) a.push({{ i, s: 'x' + i }}); a.sort((x, y) => y.i - x.i); __p('{}', a.length); }}", t),
            8 => format!("{{ let p = Promise.resolve(0); for (let i = 0; i < 1e4; i++) p = p.then(x => x + 1); p.then(x => __p('{}', x)); }}", t),
            // V8 classes native-recursion-depth, proxy-getter-reentrancy,
            // regexp-reentrancy-limits, parser-depth: recursion through
            // builtins must throw a RangeError, not abort the process
            9 => format!("{{ let p = {{}}; for (let i = 0; i < {}; i++) p = new Proxy(p, {{}}); __p('{}', p.x); }}", self.rng.pick(&["1e3", "5e3", "5e4"]), t),
            10 => format!("{{ const o = {{ toString() {{ return String(this); }} }}; __p('{}', String(o)); }}", t),
            11 => format!("{{ __p('{}', new RegExp('('.repeat({}) + 'a' + ')'.repeat({})).test('a')); }}", t, 3000, 3000),
            12 => format!("{{ const g = {{ get x() {{ return this.x; }} }}; __p('{}', g.x); }}", t),
            13 => format!("{{ __p('{}', new Function(Array.from({{ length: 65536 }}, (_, i) => 'p' + i).join(','), 'return 1')()); }}", t),
            14 => format!("{{ let a = [1]; a.push(a); __p('{}', a.flat(Infinity).length); }}", t),
            _ => format!("{{ __p('{}', 'ab'.padEnd(2 ** 30).length); }}", t),
        }
    }
}

/// A byte-level or token-level change to a program: what finds parser and
/// compiler crashes that well-formed programs miss.
pub fn mutate(rng: &mut Rng, src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    if chars.is_empty() {
        return String::from("(");
    }
    let mut out = chars.clone();
    let rounds = 1 + rng.below(4);
    for _ in 0..rounds {
        let n = out.len().max(1);
        let a = rng.below(n);
        let b = (a + 1 + rng.below(32)).min(out.len());
        match rng.below(9) {
            0 => {
                // drop a span
                out.drain(a..b);
            }
            1 => {
                // repeat a span
                let span: Vec<char> = out[a..b].to_vec();
                for (i, c) in span.into_iter().enumerate() {
                    out.insert(b + i, c);
                }
            }
            2 => {
                // nest
                let open = rng.pick(&["(", "[", "{", "`${", "/*", "<a>", "f(", "async ", "yield ", "await "]);
                for (i, c) in open.chars().enumerate() {
                    out.insert(a + i, c);
                }
            }
            3 => {
                let tok = rng.pick(&[")", "]", "}", "`", "*/", "</a>", ";", ",", "=>", "?.", "...", "#x", "@d", "\\u{"]);
                for (i, c) in tok.chars().enumerate() {
                    out.insert(a.min(out.len()) + i, c);
                }
            }
            4 => {
                // swap a digit run for a boundary number
                if let Some(p) = (a..out.len()).find(|&i| out[i].is_ascii_digit()) {
                    let mut e = p;
                    while e < out.len() && out[e].is_ascii_digit() {
                        e += 1;
                    }
                    let num = rng.pick(NUMS);
                    out.splice(p..e, num.chars());
                }
            }
            5 => {
                // a random char, sometimes outside ASCII
                let c = *[ '\0', '\u{2028}', '\u{FEFF}', 'é', '\u{D7FF}', '"', '\'', '\\', '\n', '<', '>', '=', '?' ]
                    .get(rng.below(13))
                    .unwrap();
                out.insert(a, c);
            }
            6 => {
                // copy one span over another place
                let span: Vec<char> = out[a..b].to_vec();
                let at = rng.below(out.len() + 1);
                for (i, c) in span.into_iter().enumerate() {
                    out.insert(at + i, c);
                }
            }
            7 => {
                // deep nesting
                let depth = [10usize, 100, 1000, 5000][rng.below(4)];
                let (o, c) = *[("(", ")"), ("[", "]"), ("{a:", "}"), ("!", ""), ("-", ""), ("a?b:", "")].get(rng.below(6)).unwrap();
                let mut ins = String::new();
                for _ in 0..depth {
                    ins.push_str(o);
                }
                ins.push('1');
                for _ in 0..depth {
                    ins.push_str(c);
                }
                for (i, ch) in ins.chars().enumerate() {
                    out.insert(a + i, ch);
                }
            }
            _ => {
                if b > a {
                    out[a..b].reverse();
                }
            }
        }
        if out.len() > 200_000 {
            out.truncate(200_000);
        }
    }
    out.into_iter().collect()
}
