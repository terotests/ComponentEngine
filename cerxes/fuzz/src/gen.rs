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
const STRS: &[&str] = &[
    "''", "'a'", "'abc'", "'ä'", "'\\u{1F600}'", "'\\uD800'", "'0'", "'-1'", "'1e3'", "' 12 '", "'length'",
    "'__proto__'", "'constructor'", "'toString'", "'a,b,c'", "'\\n'", "'aaaaaaaaaaaaaaaa'", "'İ'", "'ß'", "'𝒳'",
];
const BIGS: &[&str] = &["0n", "1n", "-1n", "2n ** 64n", "-(2n ** 63n)", "123456789012345678901234567890n"];
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
            0 | 1 | 2 => format!("({} {} {})", self.expr(), self.rng.pick(BINOPS), self.expr()),
            3 => format!("({}{})", self.rng.pick(UNOPS), self.expr()),
            4 => format!("{}.{}", self.var(), self.rng.pick(ARR_M)),
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
            19 => format!("(() => {{ try {{ return {}; }} finally {{ {}; }} }})()", self.expr(), self.expr()),
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
        let body = match self.rng.below(16) {
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
            _ => format!("label{}: for (const a of [1, 2, 3]) {{ for (const b of [4, 5]) {{ if (a * b > {}) break label{}; }} }} __p('{}', 0);", t, self.atom(), t, t),
        };
        format!("try {{ {} }} catch (e) {{ __e('{}', e); }}", body, t)
    }

    /// Statements that stress resources: deep recursion, big strings and
    /// arrays, deep nesting. The engine must throw (RangeError …) or finish,
    /// never crash or hang.
    fn heavy_stmt(&mut self, t: &str) -> String {
        match self.rng.below(9) {
            0 => format!("function r(n) {{ return n ? r(n - 1) + 1 : 0; }} __p('{}', r({}));", t, self.rng.pick(&["1e4", "1e5", "1e6"])),
            1 => format!("__p('{}', 'ab'.repeat({}).length);", t, self.rng.pick(&["1e6", "1e7", "2 ** 28", "2 ** 31"])),
            2 => format!("__p('{}', new Array({}).fill(0).length);", t, self.rng.pick(&["1e6", "1e7", "2 ** 32 - 1"])),
            3 => format!("{{ let o = {{}}; for (let i = 0; i < {}; i++) o = {{ o }}; __p('{}', JSON.stringify(o).length); }}", self.rng.pick(&["1e3", "1e4", "1e5"]), t),
            4 => format!("{{ let a = []; for (let i = 0; i < {}; i++) a = [a]; __p('{}', String(a).length); }}", self.rng.pick(&["1e3", "1e4", "1e5"]), t),
            5 => format!("__p('{}', /(a*)*b/.test('a'.repeat({})));", t, self.rng.pick(&["20", "26", "30"])),
            6 => format!("__p('{}', eval('(' .repeat({}) + '1' + ')'.repeat({})));", t, 2000, 2000),
            7 => format!("{{ const a = []; for (let i = 0; i < 1e5; i++) a.push({{ i, s: 'x' + i }}); a.sort((x, y) => y.i - x.i); __p('{}', a.length); }}", t),
            _ => format!("{{ let p = Promise.resolve(0); for (let i = 0; i < 1e4; i++) p = p.then(x => x + 1); p.then(x => __p('{}', x)); }}", t),
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
