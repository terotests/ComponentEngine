// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Bug-class-aware mutation (fuzzer v3): changes a seed so that user code
// runs where an engine is most likely to assume it does not. Every hook is
// the shape of a published bug of some engine (seeds/engines/README.md):
//
// - `arg`      a number argument of a builtin call becomes an object whose
//              valueOf / toString / Symbol.toPrimitive changes the receiver
//              first (JSC CVE-2016-4622 slice, V8 and QuickJS fill /
//              copyWithin / lastIndexOf, Chakra join / splice)
// - `species`  the receiver gets a constructor whose Symbol.species changes
//              it or returns something else (SpiderMonkey CVE-2019-9810,
//              V8 / Hermes concat / map / filter)
// - `proto`    an index getter / setter on Array.prototype or
//              Object.prototype that changes a variable (SpiderMonkey
//              CVE-2019-17026 StoreElementHole, CVE-2019-11707 pop, holes)
// - `warm`     a call statement run 200 times (past the JIT threshold) and
//              then once more with an edge value (JSC CVE-2020-9802,
//              SpiderMonkey CVE-2024-29943 range analysis, Chakra InitProto)
// - `proxy`    an argument wrapped in a Proxy whose traps change things
// - `detach`   a typed array's buffer transferred / resized halfway
// - `then`     Object.prototype.then for one resolve (SpiderMonkey
//              CVE-2022-1802 class, V8 await / async iterator bugs)
// - `drop`     a callback that drops the last reference to what the builtin
//              walks and collects (UAF class of QuickJS, Hermes and other
//              refcounted / moving collectors; CErXes counts references)
// - `getter`   an object literal property becomes a getter with an effect
// - `regexp`   RegExp.prototype.exec or lastIndex changed under a regexp use
//
// The changes are textual (no parser), so a few make the program a
// SyntaxError; most stay well-formed because a hook goes where an
// expression or a statement already was.

use crate::gen::Rng;

/// The names a program declares (`var x`, `let x`, `const x`,
/// `function f`, parameters are left out): what an effect can change.
fn declared(src: &str) -> (Vec<String>, Vec<String>) {
    let mut vars = Vec::new();
    let mut funcs = Vec::new();
    let b = src.as_bytes();
    for kw in ["var ", "let ", "const ", "function "] {
        let mut from = 0;
        while let Some(p) = src[from..].find(kw) {
            let at = from + p;
            let s = at + kw.len();
            from = s;
            if at > 0 {
                let before = b[at - 1];
                if before.is_ascii_alphanumeric() || before == b'_' || before == b'$' || before == b'.' {
                    continue;
                }
            }
            let mut e = s;
            while e < b.len() && (b[e].is_ascii_alphanumeric() || b[e] == b'_' || b[e] == b'$') {
                e += 1;
            }
            if e > s && !b[s].is_ascii_digit() {
                let n = src[s..e].to_string();
                let list = if kw == "function " { &mut funcs } else { &mut vars };
                if !list.contains(&n) && list.len() < 64 {
                    list.push(n);
                }
            }
        }
    }
    (vars, funcs)
}

/// Statements that change `t` (any value) under a running builtin.
fn effect(rng: &mut Rng, t: &str) -> String {
    let e = match rng.below(14) {
        0 => format!("{}.length = 0", t),
        1 => format!("{}.length = 1", t),
        2 => format!("{}.length = 100000", t),
        3 => format!("{}[0] = {{}}", t),
        4 => format!("{}[0] = 1.5", t),
        5 => format!("Object.setPrototypeOf({}, null)", t),
        6 => format!("Object.freeze({})", t),
        7 => format!("{}.buffer && ({}.buffer.transfer ? {}.buffer.transfer() : {}.buffer.resize && {}.buffer.resize(0))", t, t, t, t, t),
        8 => format!("{}.__proto__ = {{ 0: 'p', 1: 'q', length: 3 }}", t),
        9 => format!("delete {}[1]", t),
        10 => format!("{}.push && {}.push(1, 2, 3)", t, t),
        11 => format!("{}.x = 1, delete {}.x, {}[3] = 3", t, t, t),
        12 => format!("Object.defineProperty({}, 'length', {{ writable: false }})", t),
        _ => format!("{}.fill && {}.fill(undefined)", t, t),
    };
    // the effect must not end the program with an uncaught TypeError
    format!("try {{ {} }} catch (__x) {{}}", e)
}

/// The hook object of `arg`: user code inside a conversion.
fn conv(rng: &mut Rng, eff: &str, v: &str) -> String {
    match rng.below(3) {
        0 => format!("{{ valueOf() {{ {}; return {}; }} }}", eff, v),
        1 => format!("{{ toString() {{ {}; return '{}'; }}, valueOf: undefined }}", eff, v.replace('\'', "")),
        _ => format!("{{ [Symbol.toPrimitive]() {{ {}; return {}; }} }}", eff, v),
    }
}

/// Builtins whose number arguments are read after the receiver is.
const ARG_METHODS: &[&str] = &[
    "slice", "splice", "fill", "copyWithin", "indexOf", "lastIndexOf", "includes", "at", "with", "toSpliced", "subarray",
    "set", "join", "flat", "substring", "substr", "padStart", "padEnd", "repeat", "charAt", "charCodeAt", "codePointAt",
    "getInt8", "getUint16", "getInt32", "getFloat64", "setInt8", "setUint16", "setInt32", "setFloat64", "toString",
    "toFixed", "toPrecision", "resize", "lastIndexOf", "startsWith", "endsWith", "split", "exec", "apply", "push",
];

/// Byte spans `(name_start, open_paren, close_paren)` of calls `.m(…)`.
fn calls<'a>(src: &'a str, names: &[&str]) -> Vec<(usize, usize, usize)> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'.' {
            let s = i + 1;
            let mut e = s;
            while e < b.len() && (b[e].is_ascii_alphanumeric() || b[e] == b'_') {
                e += 1;
            }
            if e < b.len() && b[e] == b'(' && names.contains(&&src[s..e]) {
                let mut d = 0;
                let mut k = e;
                while k < b.len() {
                    match b[k] {
                        b'(' => d += 1,
                        b')' => {
                            d -= 1;
                            if d == 0 {
                                break;
                            }
                        }
                        b'\n' if k - e > 400 => break,
                        _ => {}
                    }
                    k += 1;
                }
                if k < b.len() && b[k] == b')' {
                    out.push((s, e, k));
                }
            }
            i = e.max(i + 1);
        } else {
            i += 1;
        }
    }
    out
}

/// The identifier right before `.m(` (the receiver), if it is one.
fn receiver(src: &str, dot_name_start: usize) -> Option<&str> {
    let b = src.as_bytes();
    let end = dot_name_start.checked_sub(1)?; // the '.'
    let mut s = end;
    while s > 0 && (b[s - 1].is_ascii_alphanumeric() || b[s - 1] == b'_' || b[s - 1] == b'$') {
        s -= 1;
    }
    if s == end || b[s].is_ascii_digit() {
        return None;
    }
    if s > 0 && b[s - 1] == b'.' {
        return None;
    }
    Some(&src[s..end])
}

/// Number literals inside `src[a..b]`: their spans.
fn numbers(src: &str, a: usize, b: usize) -> Vec<(usize, usize)> {
    let by = src.as_bytes();
    let mut out = Vec::new();
    let mut i = a;
    while i < b {
        let word_before = i > 0 && (by[i - 1].is_ascii_alphanumeric() || by[i - 1] == b'_' || by[i - 1] == b'$' || by[i - 1] == b'.');
        if by[i].is_ascii_digit() && !word_before {
            let s = i;
            while i < b && (by[i].is_ascii_alphanumeric() || by[i] == b'.' || by[i] == b'_') {
                i += 1;
            }
            out.push((s, i));
        } else {
            i += 1;
        }
    }
    out
}

/// Line starts of statements at the top level of the program (a line that
/// starts at column 0 and is not inside an open bracket).
fn top_lines(src: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut depth: i32 = 0;
    let mut start = 0;
    for line in src.split_inclusive('\n') {
        if depth == 0 && !line.starts_with(' ') && !line.starts_with('\t') && !line.starts_with('}') && !line.starts_with(')') {
            out.push(start);
        }
        for c in line.bytes() {
            match c {
                b'{' | b'(' | b'[' => depth += 1,
                b'}' | b')' | b']' => depth -= 1,
                _ => {}
            }
        }
        if depth < 0 {
            depth = 0;
        }
        start += line.len();
    }
    out
}

const EDGES: &[&str] = &["-0", "2 ** 31", "-(2 ** 31)", "2 ** 32", "2 ** 53", "NaN", "Infinity", "-1", "1.5", "'1'", "{}", "null", "undefined", "[]", "1n", "0x7fffffff", "-2147483648"];

/// The program after one class hook; `None` when that hook found no place.
fn hook_once(rng: &mut Rng, src: &str, which: usize) -> Option<(String, &'static str)> {
    let (vars, funcs) = declared(src);
    let pick_var = |rng: &mut Rng| -> String {
        if vars.is_empty() {
            String::from("globalThis.__v")
        } else {
            vars[rng.below(vars.len())].clone()
        }
    };
    let tops = top_lines(src);
    let insert_top = |rng: &mut Rng, stmt: &str| -> String {
        let at = if tops.is_empty() { src.len() } else { tops[rng.below(tops.len())] };
        // after the shim and declarations rather than at the very start
        format!("{}{}\n{}", &src[..at], stmt, &src[at..])
    };
    match which {
        // arg: a number argument of a builtin call becomes a conversion hook
        0 => {
            let cs = calls(src, ARG_METHODS);
            if cs.is_empty() {
                return None;
            }
            let (s, open, close) = cs[rng.below(cs.len())];
            let t = receiver(src, s).map(|r| r.to_string()).unwrap_or_else(|| pick_var(rng));
            let eff = effect(rng, &t);
            let nums = numbers(src, open + 1, close);
            if nums.is_empty() {
                // no number: add one as a further argument
                let v = rng.pick(&["0", "1", "2", "-1", "10"]).to_string();
                let h = conv(rng, &eff, &v);
                let sep = if src[open + 1..close].trim().is_empty() { "" } else { ", " };
                return Some((format!("{}{}{}{}", &src[..close], sep, h, &src[close..]), "arg"));
            }
            let (a, b) = nums[rng.below(nums.len())];
            let h = conv(rng, &eff, &src[a..b]);
            Some((format!("{}{}{}", &src[..a], h, &src[b..]), "arg"))
        }
        // species: the receiver of a species-creating builtin gets a
        // constructor whose species changes it
        1 => {
            let cs = calls(src, &["map", "filter", "slice", "splice", "concat", "flat", "flatMap", "subarray", "split", "then", "finally"]);
            let cand: Vec<_> = cs.iter().filter_map(|&(s, _, _)| receiver(src, s).map(|r| (s, r.to_string()))).collect();
            if cand.is_empty() {
                return None;
            }
            let (s, r) = cand[rng.below(cand.len())].clone();
            let eff = effect(rng, &r);
            let ret = rng.pick(&["new Array(n)", "{}", "[]", "new Uint8Array(2)", "r", "Object.freeze([])", "new Proxy([], {})"]).to_string();
            let ret = if ret == "r" { r.clone() } else { ret };
            let setup = format!(
                "try {{ {r}.constructor = function () {{}}; {r}.constructor[Symbol.species] = function (n) {{ {}; return {}; }}; }} catch (__x) {{}} ",
                eff, ret, r = r
            );
            // at the start of the line of the call
            let ls = src[..s].rfind('\n').map(|p| p + 1).unwrap_or(0);
            Some((format!("{}{}{}", &src[..ls], setup, &src[ls..]), "species"))
        }
        // proto: an index accessor on a prototype (removed again at the end)
        2 => {
            let t = pick_var(rng);
            let eff = effect(rng, &t);
            let proto = rng.pick(&["Array.prototype", "Object.prototype", "Array.prototype", "Function.prototype", "Uint8Array.prototype.__proto__"]);
            let k = rng.pick(&["0", "1", "2", "3", "7", "'length'", "'x'", "4294967294"]);
            let stmt = format!(
                "try {{ Object.defineProperty({p}, {k}, {{ get() {{ {}; return 'pg'; }}, set(v) {{ {}; }}, configurable: true }}); }} catch (__x) {{}}",
                eff, effect(rng, &t), p = proto, k = k
            );
            let mut s = insert_top(rng, &stmt);
            s.push_str(&format!("\ntry {{ delete {}[{}]; }} catch (__x) {{}}\n", proto, k));
            Some((s, "proto"))
        }
        // warm: a top-level call statement run past the JIT threshold, then
        // once more with an edge value in place of an argument
        3 => {
            let lines: Vec<(usize, &str)> = tops.iter().map(|&a| {
                let e = src[a..].find('\n').map(|p| a + p).unwrap_or(src.len());
                (a, &src[a..e])
            }).filter(|(_, l)| {
                let t = l.trim_end();
                t.ends_with(");") && !t.starts_with("var ") && !t.starts_with("let ") && !t.starts_with("const ") && !t.starts_with("function") && t.len() < 300 && t.matches('(').count() == t.matches(')').count()
            }).collect();
            if lines.is_empty() {
                // no call statement: call a declared function
                if funcs.is_empty() {
                    return None;
                }
                let f = funcs[rng.below(funcs.len())].clone();
                let e = rng.pick(EDGES);
                let stmt = format!("for (let __i = 0; __i < 200; __i++) try {{ {f}(__i, [1.5, 2.5], {{ a: __i }}); }} catch (__x) {{}}\ntry {{ print('warm', {f}({}, [1.5, {{}}], {{ b: 1 }})); }} catch (__x) {{ print('warm throw', __x && __x.name); }}", e, f = f);
                let mut s = String::from(src);
                s.push_str("\n");
                s.push_str(&stmt);
                s.push('\n');
                return Some((s, "warm"));
            }
            let (a, l) = lines[rng.below(lines.len())];
            let l = l.trim_end().trim_end_matches(';').to_string();
            let nums = numbers(&l, 0, l.len());
            let perturbed = if nums.is_empty() {
                l.clone()
            } else {
                let (x, y) = nums[rng.below(nums.len())];
                format!("{}({}){}", &l[..x], rng.pick(EDGES), &l[y..])
            };
            let stmt = format!(
                "for (let __i = 0; __i < 200; __i++) try {{ {}; }} catch (__x) {{}}\ntry {{ {}; }} catch (__x) {{ print('warm throw', __x && __x.name); }}\n",
                l, perturbed
            );
            let e = src[a..].find('\n').map(|p| a + p + 1).unwrap_or(src.len());
            Some((format!("{}{}{}", &src[..e], stmt, &src[e..]), "warm"))
        }
        // proxy: an identifier argument wrapped in a Proxy with effects
        4 => {
            let cs = calls(src, ARG_METHODS.iter().chain(["call", "assign", "keys", "entries", "from", "stringify", "concat", "sort", "map", "forEach", "reduce", "every", "some", "find", "construct", "ownKeys", "getPrototypeOf"].iter()).copied().collect::<Vec<_>>().as_slice());
            let mut spots = Vec::new();
            for &(_, open, close) in &cs {
                for v in &vars {
                    if let Some(p) = src[open + 1..close].find(v.as_str()) {
                        let a = open + 1 + p;
                        let b = a + v.len();
                        let by = src.as_bytes();
                        let clean = (a == 0 || !(by[a - 1].is_ascii_alphanumeric() || by[a - 1] == b'.' || by[a - 1] == b'_')) && (b >= by.len() || !(by[b].is_ascii_alphanumeric() || by[b] == b'_'));
                        if clean {
                            spots.push((a, b));
                        }
                    }
                }
            }
            if spots.is_empty() {
                return None;
            }
            let (a, b) = spots[rng.below(spots.len())];
            let t = pick_var(rng);
            let eff = effect(rng, &t);
            let trap = rng.pick(&["get(t, k, r) { EFF; return Reflect.get(t, k, r); }", "has(t, k) { EFF; return Reflect.has(t, k); }", "ownKeys(t) { EFF; return Reflect.ownKeys(t); }", "getOwnPropertyDescriptor(t, k) { EFF; return Reflect.getOwnPropertyDescriptor(t, k); }", "getPrototypeOf(t) { EFF; return Reflect.getPrototypeOf(t); }", "set(t, k, v, r) { EFF; return Reflect.set(t, k, v, r); }", "defineProperty(t, k, d) { EFF; return Reflect.defineProperty(t, k, d); }"]);
            let h = format!("new Proxy({}, {{ {} }})", &src[a..b], trap.replace("EFF", &eff));
            Some((format!("{}{}{}", &src[..a], h, &src[b..]), "proxy"))
        }
        // detach: a typed array or buffer variable loses its buffer at a top
        // level line after its declaration
        5 => {
            let tvars: Vec<&String> = vars.iter().filter(|v| {
                let d = src.find(&format!("{} = new ", v));
                d.map(|p| {
                    let r = &src[p..(p + 60).min(src.len())];
                    r.contains("Array(") && !r.contains("= new Array(") || r.contains("ArrayBuffer(") || r.contains("DataView(")
                }).unwrap_or(false)
            }).collect();
            if tvars.is_empty() {
                return None;
            }
            let t = tvars[rng.below(tvars.len())].clone();
            let decl = src.find(&format!("{} = new ", t)).unwrap_or(0);
            let after: Vec<usize> = tops.iter().copied().filter(|&a| a > decl).collect();
            let at = if after.is_empty() { src.len() } else { after[rng.below(after.len())] };
            let how = rng.pick(&["transfer()", "transfer(0)", "transferToFixedLength()", "resize && B.resize(0)", "resize && B.resize(1)"]);
            let stmt = format!(
                "try {{ var __b = {t} instanceof ArrayBuffer ? {t} : {t}.buffer; __b.{} ; }} catch (__x) {{}}",
                how.replace("B.", "__b."), t = t
            );
            Some((format!("{}{}\n{}", &src[..at], stmt, &src[at..]), "detach"))
        }
        // then: one resolve sees Object.prototype.then
        6 => {
            let t = pick_var(rng);
            let stmt = format!(
                "Object.defineProperty(Object.prototype, 'then', {{ configurable: true, get() {{ delete Object.prototype.then; {}; return undefined; }} }});",
                effect(rng, &t)
            );
            Some((insert_top(rng, &stmt), "then"))
        }
        // drop: a callback drops the last reference and collects
        7 => {
            let b = src.as_bytes();
            let mut arrows = Vec::new();
            let mut from = 0;
            while let Some(p) = src[from..].find("=> ") {
                let at = from + p + 3;
                if at < b.len() && b[at] != b'{' {
                    arrows.push(at);
                }
                from = at;
            }
            let t = pick_var(rng);
            let drop = format!("({} = null, typeof gc === 'function' && gc(), 0)", t);
            if arrows.is_empty() || vars.is_empty() {
                let mut fb = Vec::new();
                let mut from = 0;
                while let Some(p) = src[from..].find("function (") {
                    let at = from + p;
                    if let Some(q) = src[at..].find('{') {
                        fb.push(at + q + 1);
                    }
                    from = at + 10;
                }
                if fb.is_empty() || vars.is_empty() {
                    return None;
                }
                let at = fb[rng.below(fb.len())];
                return Some((format!("{} try {{ {}; }} catch (__x) {{}} {}", &src[..at], drop, &src[at..]), "drop"));
            }
            let at = arrows[rng.below(arrows.len())];
            Some((format!("{}{} || {}", &src[..at], drop, &src[at..]), "drop"))
        }
        // getter: `name: value` in an object literal becomes a getter
        8 => {
            let b = src.as_bytes();
            let mut spots = Vec::new();
            for (i, w) in src.match_indices(": ") {
                let mut s = i;
                while s > 0 && (b[s - 1].is_ascii_alphanumeric() || b[s - 1] == b'_') {
                    s -= 1;
                }
                if s == i || s == 0 || b[s].is_ascii_digit() {
                    continue;
                }
                let pre = b[s - 1];
                if pre != b'{' && pre != b' ' {
                    continue;
                }
                // the value ends at the next `,` or `}` on this line
                let rest = &src[i + w.len()..];
                let end = rest.find(|c| c == ',' || c == '}' || c == '\n').unwrap_or(0);
                let val = rest[..end].trim();
                if val.is_empty() || val.contains('(') || val.contains('?') || val.contains('{') {
                    continue;
                }
                // `{` before the name on the same line: probably a literal
                let ls = src[..s].rfind('\n').map(|p| p + 1).unwrap_or(0);
                if !src[ls..s].contains('{') || src[ls..s].contains("case ") {
                    continue;
                }
                spots.push((s, i + w.len() + end));
            }
            if spots.is_empty() {
                return None;
            }
            let (s, e) = spots[rng.below(spots.len())];
            let colon = src[s..e].find(": ").unwrap() + s;
            let name = &src[s..colon];
            let val = src[colon + 2..e].trim();
            let t = pick_var(rng);
            let g = format!("get {}() {{ {}; return {}; }}", name, effect(rng, &t), val);
            Some((format!("{}{}{}", &src[..s], g, &src[e..]), "getter"))
        }
        // regexp: exec or lastIndex changed under a regexp use
        _ => {
            if !src.contains("RegExp") && !src.contains(".exec(") && !src.contains(".test(") && !src.contains(".replace(") && !src.contains(".match") && !src.contains(".split(") {
                return None;
            }
            let t = pick_var(rng);
            let stmt = match rng.below(3) {
                0 => format!(
                    "try {{ var __ex = RegExp.prototype.exec; let __n = 0; RegExp.prototype.exec = function (s) {{ if (__n++ === 2) {{ {}; RegExp.prototype.exec = __ex; }} return __ex.call(this, s); }}; }} catch (__x) {{}}",
                    effect(rng, &t)
                ),
                1 => format!(
                    "try {{ Object.defineProperty(RegExp.prototype, 'flags', {{ configurable: true, get() {{ delete RegExp.prototype.flags; {}; return 'g'; }} }}); }} catch (__x) {{}}",
                    effect(rng, &t)
                ),
                _ => format!(
                    "try {{ var __ex2 = RegExp.prototype.exec; RegExp.prototype.exec = function (s) {{ RegExp.prototype.exec = __ex2; this.lastIndex = {{ valueOf() {{ {}; return {}; }} }}; return {}; }}; }} catch (__x) {{}}",
                    effect(rng, &t), rng.pick(&["0", "-1", "2 ** 31", "1e10"]), rng.pick(&["null", "__ex2.call(this, s)", "{ length: 2 ** 31, index: -1, 0: 'x' }", "{ length: 1, index: 1e9, 0: undefined }"])
                ),
            };
            Some((insert_top(rng, &stmt), "regexp"))
        }
    }
}

/// One to three class hooks on `src`; returns the program and the hooks
/// it got (`arg+warm`), which the finding's .txt reports.
pub fn inject(rng: &mut Rng, src: &str) -> (String, String) {
    let mut s = String::from(src);
    let mut names: Vec<&str> = Vec::new();
    let n = 1 + rng.below(3);
    let mut tries = 0;
    while names.len() < n && tries < 12 {
        tries += 1;
        let which = rng.below(10);
        if let Some((t, name)) = hook_once(rng, &s, which) {
            s = t;
            names.push(name);
        }
    }
    (s, names.join("+"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn finds_receivers_and_numbers() {
        let s = "var a = [1, 2, 3];\na.slice(0, 2);\n";
        let cs = calls(s, &["slice"]);
        assert_eq!(cs.len(), 1);
        assert_eq!(receiver(s, cs[0].0), Some("a"));
        assert_eq!(numbers(s, cs[0].1, cs[0].2).len(), 2);
        let (v, _) = declared(s);
        assert_eq!(v, vec![String::from("a")]);
    }
    #[test]
    fn every_hook_kind_applies_somewhere() {
        let s = "var a = [1, 2, 3];\nvar t = new Uint8Array(8);\nvar o = { x: 1, y: 2 };\nfunction f(n) { return a[n] + t[n]; }\nprint(a.slice(0, 2).map(x => x + 1));\nprint(/a/.exec('ba'));\nf(1);\nPromise.resolve(1).then(v => print(v));\nObject.keys(o);\n";
        let mut seen = std::collections::HashSet::new();
        for k in 0..10 {
            let mut rng = Rng::new(k as u64 + 1);
            if let Some((_, name)) = hook_once(&mut rng, s, k) {
                seen.insert(name);
            }
        }
        assert_eq!(seen.len(), 10, "{:?}", seen);
    }
}
