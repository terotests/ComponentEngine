// SPDX-License-Identifier: AGPL-3.0-or-later
//! Regular expressions: a pattern is parsed to a small tree, compiled to
//! instructions and run by a backtracking matcher over UTF-16 units.
//!
//! Supported: alternation, groups (capturing, `(?:`, named `(?<n>`),
//! quantifiers `* + ? {n} {n,} {n,m}` greedy and lazy, classes with ranges
//! and `\d \w \s`, `.`, anchors `^ $ \b \B`, back references `\1 \k<n>`,
//! lookahead and lookbehind, and the flags `g i m s u y`.

use ranger::prelude::*;

const R_EMPTY: int = 0;
/// The deepest group nesting a pattern may have: deeper, it is a
/// SyntaxError rather than a stack overflow in the passes that recurse.
const MAX_NESTING: int = 1000;
const R_CHAR: int = 1;
const R_ANY: int = 2;
const R_CLASS: int = 3;
const R_SEQ: int = 4;
const R_ALT: int = 5;
const R_GROUP: int = 6;
const R_REPEAT: int = 7;
const R_BOL: int = 8;
const R_EOL: int = 9;
const R_WORDB: int = 10;
const R_NWORDB: int = 11;
const R_BACKREF: int = 12;
const R_LOOK: int = 13;
const R_NAMEDREF: int = 14;
/// `(?ims-ims:…)`: c holds the flags added (bits 1 i, 2 m, 4 s) and, << 3,
/// the flags removed
const R_MODS: int = 15;

const I_CHAR: int = 1;
const I_ANY: int = 2;
const I_CLASS: int = 3;
const I_SPLIT: int = 4;
const I_JMP: int = 5;
const I_SAVE: int = 6;
const I_BOL: int = 7;
const I_EOL: int = 8;
const I_WORDB: int = 9;
const I_NWORDB: int = 10;
const I_BACKREF: int = 11;
const I_LOOK: int = 12;
const I_MATCH: int = 13;
const I_SETPOS: int = 14;
const I_CHKPOS: int = 15;
const I_ANYNL: int = 16;
/// caps[a..=b] = -1 (a repetition's groups, at each iteration)
const I_CLEAR: int = 17;
/// a repetition of the one-unit matcher at pc + 1 (I_CHAR, I_CLASS, I_ANY,
/// I_ANYNL), any number of times (b 1 greedy, 0 lazy), then pc + 2: one
/// backtrack entry for the whole run, not one per unit
const I_STAR: int = 18;
/// a counted loop's head (`x{min,max}` too big to write out): a is the
/// count register, b the pc after the loop; the next instruction is an
/// I_CNTDATA with min in a, max in b (-1: no bound). Count < min: the body
/// (pc + 2); count == max: b; else both, the body first when greedy.
const I_CNTBR: int = 19;
const I_CNTBR_LAZY: int = 20;
const I_CNTDATA: int = 21;
/// regs[a] = 0 (a counted loop starts)
const I_CNTSET: int = 22;
/// the end of a counted loop's body: fails when the iteration was an
/// optional one (count >= b's I_CNTDATA min) that matched nothing
/// (regs[b] still the position it started at), else regs[a] += 1
const I_CNTINC: int = 23;

struct RNode {
    kind: int,
    c: int,
    list: Vec<int>,
    min: int,
    max: int,
    greedy: bool,
    name: String,
}

#[derive(Clone, Copy)]
struct Inst {
    op: int,
    a: int,
    b: int,
}

pub struct RClass {
    pub ranges: Vec<int>,
    pub negate: bool,
}

pub struct Regex {
    code: Vec<Inst>,
    classes: Vec<RClass>,
    pub ngroups: int,
    pub names: Vec<String>,
    pub name_index: Vec<int>,
    pub global: bool,
    pub ignore_case: bool,
    pub multiline: bool,
    pub dot_all: bool,
    pub unicode: bool,
    pub sticky: bool,
    /// `d`: exec reports where each group matched (`indices`)
    pub has_indices: bool,
    /// `v`
    pub unicode_sets: bool,
    pub error: String,
    nregs: int,
    /// the first unit every match starts with, -1 when not one
    first: int,
    /// fset[u]: an ASCII unit u can start a match (empty: any unit can)
    fset: Vec<bool>,
    /// a unit past ASCII can start a match (with fset)
    fhi: bool,
    /// cmap[class * 256 + ic * 128 + u]: class_has for an ASCII unit u
    cmap: Vec<bool>,
    /// units every match holds, starting lit_min..=lit_max units after
    /// the match's start (lit_max -1: no bound); empty when none is known.
    /// exec skips ahead to where the next one could start a match
    lit: Vec<int>,
    lit_min: int,
    lit_max: int,
    /// every match starts at a `^`: 1 one that matches only at the input's
    /// start, 2 one that may also match after a line terminator (`m`);
    /// 0 when a match can start anywhere. exec tries no other start
    anchor: int,
}

/// What req_lit knows of a node: its width in units (max -1: no bound)
/// and the longest literal every match of it holds, at lmin..=lmax units
/// from its start.
struct LitInfo {
    min: int,
    max: int,
    lit: Vec<int>,
    lmin: int,
    lmax: int,
}

fn add_w(a: int, b: int) -> int {
    if a < 0 || b < 0 {
        -1
    } else {
        a + b
    }
}

/// LitInfo of node n (ic: case-insensitive here; uni: units of a code
/// point may be two).
fn req_lit(p: &RParser, n: int, ic: bool, uni: bool) -> LitInfo {
    let nd = &p.nodes[n as usize];
    let k = nd.kind;
    let cw = if uni { 2 } else { 1 };
    if k == R_CHAR {
        let w = if nd.c > 0xffff { 2 } else { 1 };
        let mut lit: Vec<int> = Vec::new();
        if !ic && nd.c < 0xd800 {
            lit.push(nd.c);
        }
        return LitInfo { min: w, max: w, lit: lit, lmin: 0, lmax: 0 };
    }
    if k == R_ANY || k == R_CLASS {
        return LitInfo { min: 1, max: cw, lit: Vec::new(), lmin: 0, lmax: 0 };
    }
    if k == R_EMPTY || k == R_BOL || k == R_EOL || k == R_WORDB || k == R_NWORDB || k == R_LOOK {
        return LitInfo { min: 0, max: 0, lit: Vec::new(), lmin: 0, lmax: 0 };
    }
    if k == R_GROUP {
        return req_lit(p, nd.list[0], ic, uni);
    }
    if k == R_MODS {
        let add = nd.c & 7;
        let remove = (nd.c >> 3) & 7;
        let ic2 = if (add & 1) != 0 { true } else if (remove & 1) != 0 { false } else { ic };
        return req_lit(p, nd.list[0], ic2, uni);
    }
    if k == R_SEQ {
        let mut out = LitInfo { min: 0, max: 0, lit: Vec::new(), lmin: 0, lmax: 0 };
        // the run of characters being read, and where it started
        let mut run: Vec<int> = Vec::new();
        let mut rmin: int = 0;
        let mut rmax: int = 0;
        for x in nd.list.iter() {
            let c = req_lit(p, *x, ic, uni);
            let one = c.min == 1 && c.max == 1 && c.lit.len() == 1;
            if one {
                if run.is_empty() {
                    rmin = out.min;
                    rmax = out.max;
                }
                run.push(c.lit[0]);
            } else {
                if run.len() > out.lit.len() {
                    out.lit = run.clone();
                    out.lmin = rmin;
                    out.lmax = rmax;
                }
                run.clear();
                if c.lit.len() > out.lit.len() {
                    out.lit = c.lit.clone();
                    out.lmin = out.min + c.lmin;
                    out.lmax = add_w(out.max, c.lmax);
                }
            }
            out.min += c.min;
            out.max = add_w(out.max, c.max);
        }
        if run.len() > out.lit.len() {
            out.lit = run;
            out.lmin = rmin;
            out.lmax = rmax;
        }
        return out;
    }
    if k == R_ALT {
        let mut out = LitInfo { min: -1, max: 0, lit: Vec::new(), lmin: 0, lmax: 0 };
        for x in nd.list.iter() {
            let c = req_lit(p, *x, ic, uni);
            if out.min < 0 || c.min < out.min {
                out.min = c.min;
            }
            if out.max >= 0 && (c.max < 0 || c.max > out.max) {
                out.max = c.max;
            }
        }
        if out.min < 0 {
            out.min = 0;
        }
        return out;
    }
    if k == R_REPEAT {
        let c = req_lit(p, nd.list[0], ic, uni);
        let min = nd.min * c.min;
        let max = if nd.max < 0 || c.max < 0 { -1 } else { nd.max * c.max };
        if nd.min >= 1 {
            // the first iteration's literal
            return LitInfo { min: min, max: max, lit: c.lit, lmin: c.lmin, lmax: c.lmax };
        }
        return LitInfo { min: min, max: max, lit: Vec::new(), lmin: 0, lmax: 0 };
    }
    // back references: any width
    LitInfo { min: 0, max: -1, lit: Vec::new(), lmin: 0, lmax: 0 }
}

struct RParser {
    /// how deep `[` classes are nested (the v flag), MAX_NESTING at most
    class_depth: int,
    src: Vec<int>,
    pos: int,
    nodes: Vec<RNode>,
    classes: Vec<RClass>,
    ngroups: int,
    names: Vec<String>,
    name_index: Vec<int>,
    /// each named group's alternatives: [disjunction, alternative, …]
    name_paths: Vec<Vec<int>>,
    /// the open disjunctions, as pairs like name_paths
    alt_path: Vec<int>,
    ndisj: int,
    /// the pattern has a named group: \k is then a reference
    has_names: bool,
    /// the \k<name> references, checked at the end
    refs: Vec<String>,
    error: String,
    unicode: bool,
    /// the capturing groups in the whole pattern
    total_groups: int,
    ignore_case: bool,
    /// the v flag: classes with nesting, `&&` and `--`
    vmode: bool,
    /// the ranges of the class escape v_char read last
    v_pending: Vec<int>,
    /// the strings (not one code point) of the v-mode operand read last
    v_ostrs: Vec<Vec<int>>,
    /// a syntax check only (`check`): property escapes are not decoded
    check_only: bool,
}

fn is_digit(c: int) -> bool {
    c >= 48 && c <= 57
}

fn is_word(c: int) -> bool {
    (c >= 48 && c <= 57) || (c >= 65 && c <= 90) || (c >= 97 && c <= 122) || c == 95
}


fn is_line_term(c: int) -> bool {
    c == 10 || c == 13 || c == 0x2028 || c == 0x2029
}

fn hexv(c: int) -> int {
    if c >= 48 && c <= 57 {
        return c - 48;
    }
    if c >= 97 && c <= 102 {
        return c - 87;
    }
    if c >= 65 && c <= 70 {
        return c - 55;
    }
    -1
}

pub fn fold(c: int) -> int {
    if c >= 65 && c <= 90 {
        return c + 32;
    }
    if c < 128 {
        return c;
    }
    match char::from_u32(c as u32) {
        Some(ch) => {
            let s = ch.to_lowercase().to_string();
            let mut out = c;
            let mut n = 0;
            for l in s.chars() {
                out = l as int;
                n += 1;
            }
            if n == 1 {
                out
            } else {
                c
            }
        }
        None => c,
    }
}

/// Canonicalize without /u: the upper case, unless that takes a
/// non-ASCII character to ASCII.
fn canon(c: int) -> int {
    let u = upper(c);
    if c >= 128 && u < 128 {
        return c;
    }
    u
}

/// Simple case folding (what /iu compares): the lower case of the upper
/// case, so ſ and K fold with s and k; the Turkish dotted and dotless i
/// fold to themselves.
fn fold_u(c: int) -> int {
    if c < 128 {
        return fold(c);
    }
    if c == 0x130 || c == 0x131 {
        return c;
    }
    fold(upper(c))
}

fn upper(c: int) -> int {
    if c >= 97 && c <= 122 {
        return c - 32;
    }
    if c < 128 {
        return c;
    }
    match char::from_u32(c as u32) {
        Some(ch) => {
            let s = ch.to_uppercase().to_string();
            let mut out = c;
            let mut n = 0;
            for l in s.chars() {
                out = l as int;
                n += 1;
            }
            if n == 1 {
                out
            } else {
                c
            }
        }
        None => c,
    }
}

impl RParser {
    fn cur(&self) -> int {
        if self.pos < self.src.len() as int {
            self.src[self.pos as usize]
        } else {
            -1
        }
    }

    fn peek(&self, k: int) -> int {
        let i = self.pos + k;
        if i < self.src.len() as int {
            self.src[i as usize]
        } else {
            -1
        }
    }

    fn fail(&mut self, msg: &str) {
        if self.error.is_empty() {
            self.error = String::from(msg);
        }
        self.pos = self.src.len() as int;
    }

    fn node(&mut self, kind: int) -> int {
        self.nodes.push(RNode { kind: kind, c: 0, list: Vec::new(), min: 0, max: 0, greedy: true, name: String::new() });
        (self.nodes.len() as int) - 1
    }

    fn disjunction(&mut self) -> int {
        if (self.alt_path.len() as int) > 2 * MAX_NESTING {
            // the parser and every pass over the tree recurse on groups
            self.fail("Invalid regular expression: too deeply nested");
            return self.node(R_EMPTY);
        }
        let id = self.ndisj;
        self.ndisj += 1;
        self.alt_path.push(id);
        self.alt_path.push(0);
        let first = self.alternative();
        if self.cur() != 124 {
            self.alt_path.pop();
            self.alt_path.pop();
            return first;
        }
        let n = self.node(R_ALT);
        let mut alts: Vec<int> = vec![first];
        let mut k: int = 0;
        while self.cur() == 124 {
            self.pos += 1;
            k += 1;
            let last = self.alt_path.len() - 1;
            self.alt_path[last] = k;
            let a = self.alternative();
            alts.push(a);
        }
        self.alt_path.pop();
        self.alt_path.pop();
        self.nodes[n as usize].list = alts;
        n
    }

    /// A group name after `<` up to `>` (RegExpIdentifierName), or fails.
    fn group_name(&mut self) -> String {
        let mut name = String::new();
        let mut first = true;
        loop {
            let mut c = self.cur();
            if c == 62 && !first {
                self.pos += 1;
                return name;
            }
            if c < 0 {
                self.fail("Invalid capture group name");
                return name;
            }
            self.pos += 1;
            if c == 92 {
                if self.cur() != 117 {
                    self.fail("Invalid capture group name");
                    return name;
                }
                self.pos += 1;
                c = self.name_u_escape();
                if c < 0 {
                    self.fail("Invalid capture group name");
                    return name;
                }
            } else if c >= 0xD800 && c <= 0xDBFF && self.cur() >= 0xDC00 && self.cur() <= 0xDFFF {
                c = 0x10000 + ((c - 0xD800) << 10) + (self.cur() - 0xDC00);
                self.pos += 1;
            }
            let ok = if c == 36 || c == 95 {
                true
            } else if c < 128 {
                (c >= 65 && c <= 90) || (c >= 97 && c <= 122) || (!first && c >= 48 && c <= 57)
            } else if c >= 0xD800 && c <= 0xDFFF {
                false
            } else if !first && (c == 0x200C || c == 0x200D) {
                true
            } else {
                match char::from_u32(c as u32) {
                    Some(ch) => id_wide(ch, first),
                    None => false,
                }
            };
            if !ok {
                self.fail("Invalid capture group name");
                return name;
            }
            crate::jsstr::push_cp(&mut name, c);
            first = false;
        }
    }

    /// After `\u` in a group name: XXXX (a surrogate pair of two such
    /// escapes joins) or {X…}; -1 when malformed
    fn name_u_escape(&mut self) -> int {
        if self.cur() == 123 {
            self.pos += 1;
            let mut v: int = 0;
            let mut n = 0;
            while hexv(self.cur()) >= 0 {
                v = v * 16 + hexv(self.cur());
                if v > 0x10FFFF {
                    return -1;
                }
                self.pos += 1;
                n += 1;
            }
            if n == 0 || self.cur() != 125 {
                return -1;
            }
            self.pos += 1;
            return v;
        }
        let v = self.hex4();
        if v < 0 {
            return -1;
        }
        if v >= 0xD800 && v <= 0xDBFF && self.cur() == 92 && self.peek(1) == 117 {
            let save = self.pos;
            self.pos += 2;
            let t = self.hex4();
            if t >= 0xDC00 && t <= 0xDFFF {
                return 0x10000 + ((v - 0xD800) << 10) + (t - 0xDC00);
            }
            self.pos = save;
        }
        v
    }

    fn hex4(&mut self) -> int {
        let mut v: int = 0;
        let mut i = 0;
        while i < 4 {
            let h = hexv(self.cur());
            if h < 0 {
                return -1;
            }
            v = v * 16 + h;
            self.pos += 1;
            i += 1;
        }
        v
    }

    /// A named group: a SyntaxError for a name already used where both
    /// groups could match (not in different alternatives)
    fn add_name(&mut self, name: String, cap: int) {
        let mut i: usize = 0;
        while i < self.names.len() {
            if self.names[i].as_str() == name.as_str() {
                let other = self.name_paths[i].clone();
                let mut apart = false;
                let mut k: usize = 0;
                while k + 1 < other.len() && k + 1 < self.alt_path.len() {
                    if other[k] != self.alt_path[k] {
                        break;
                    }
                    if other[k + 1] != self.alt_path[k + 1] {
                        apart = true;
                        break;
                    }
                    k += 2;
                }
                if !apart {
                    self.fail("Invalid regular expression: duplicate capture group name");
                }
            }
            i += 1;
        }
        self.names.push(name);
        self.name_index.push(cap);
        self.name_paths.push(self.alt_path.clone());
    }

    fn alternative(&mut self) -> int {
        let n = self.node(R_SEQ);
        let mut items: Vec<int> = Vec::new();
        while self.cur() >= 0 && self.cur() != 124 && self.cur() != 41 {
            let t = self.term();
            if t >= 0 {
                items.push(t);
            }
            if !self.error.is_empty() {
                break;
            }
        }
        self.nodes[n as usize].list = items;
        n
    }

    fn term(&mut self) -> int {
        let c = self.cur();
        let atom: int;
        if c == 94 {
            self.pos += 1;
            return self.node(R_BOL);
        }
        if c == 36 {
            self.pos += 1;
            return self.node(R_EOL);
        }
        if c == 92 && (self.peek(1) == 98 || self.peek(1) == 66) {
            let b = self.peek(1) == 98;
            self.pos += 2;
            return self.node(if b { R_WORDB } else { R_NWORDB });
        }
        if c == 40 && self.peek(1) == 63 && (self.peek(2) == 61 || self.peek(2) == 33 || (self.peek(2) == 60 && (self.peek(3) == 61 || self.peek(3) == 33))) {
            // lookaround
            let behind = self.peek(2) == 60;
            let neg = if behind { self.peek(3) == 33 } else { self.peek(2) == 33 };
            self.pos += if behind { 4 } else { 3 };
            let inner = self.disjunction();
            if self.cur() != 41 {
                self.fail("Invalid regular expression: missing )");
                return -1;
            }
            self.pos += 1;
            let n = self.node(R_LOOK);
            self.nodes[n as usize].list = vec![inner];
            self.nodes[n as usize].c = (if neg { 1 } else { 0 }) + (if behind { 2 } else { 0 });
            // a lookahead may take a quantifier (Annex B, not under u); a
            // lookbehind not
            if behind || self.unicode {
                return n;
            }
            atom = n;
        } else if c == 40 {
            self.pos += 1;
            let mut cap: int = -1;
            let mut name = String::new();
            let mut mods: int = -1;
            if self.cur() == 63 && self.peek(1) == 58 {
                self.pos += 2;
            } else if self.cur() == 63 && self.peek(1) == 60 {
                self.pos += 2;
                name = self.group_name();
                self.ngroups += 1;
                cap = self.ngroups;
                self.add_name(name.clone(), cap);
            } else if self.cur() == 63 && (self.peek(1) == 105 || self.peek(1) == 109 || self.peek(1) == 115 || self.peek(1) == 45) {
                // (?ims-ims: …) pattern modifiers
                self.pos += 1;
                let mut add: int = 0;
                let mut remove: int = 0;
                let mut minus = false;
                loop {
                    let f = self.cur();
                    let bit = if f == 105 { 1 } else if f == 109 { 2 } else if f == 115 { 4 } else { 0 };
                    if f == 45 && !minus {
                        minus = true;
                    } else if bit != 0 && ((add | remove) & bit) == 0 {
                        if minus {
                            remove |= bit;
                        } else {
                            add |= bit;
                        }
                    } else {
                        break;
                    }
                    self.pos += 1;
                }
                if self.cur() != 58 || (add == 0 && remove == 0) {
                    self.fail("Invalid regular expression: invalid group");
                    return -1;
                }
                self.pos += 1;
                let inner = self.disjunction();
                if self.cur() != 41 {
                    self.fail("Invalid regular expression: missing )");
                    return -1;
                }
                self.pos += 1;
                let n = self.node(R_MODS);
                self.nodes[n as usize].c = add | (remove << 3);
                self.nodes[n as usize].list = vec![inner];
                mods = n;
            } else if self.cur() == 63 {
                self.fail("Invalid regular expression: invalid group");
                return -1;
            } else {
                self.ngroups += 1;
                cap = self.ngroups;
            }
            if mods >= 0 {
                atom = mods;
            } else {
                let inner = self.disjunction();
                if self.cur() != 41 {
                    self.fail("Invalid regular expression: missing )");
                    return -1;
                }
                self.pos += 1;
                let n = self.node(R_GROUP);
                self.nodes[n as usize].c = cap;
                self.nodes[n as usize].list = vec![inner];
                self.nodes[n as usize].name = name;
                atom = n;
            }
        } else if c == 46 {
            self.pos += 1;
            atom = self.node(R_ANY);
        } else if c == 91 {
            atom = if self.vmode { self.class_v_node() } else { self.class() };
        } else if c == 92 {
            atom = self.escape();
        } else if c == 42 || c == 43 || c == 63 {
            self.fail("Invalid regular expression: nothing to repeat");
            return -1;
        } else if c == 123 && (self.unicode || self.braced_quantifier()) {
            self.fail("Invalid regular expression: nothing to repeat");
            return -1;
        } else if c == 41 {
            self.fail("Invalid regular expression: unmatched )");
            return -1;
        } else {
            let mut cp = c;
            self.pos += 1;
            // a surrogate pair is one character under /u
            if self.unicode && cp >= 0xd800 && cp <= 0xdbff && self.cur() >= 0xdc00 && self.cur() <= 0xdfff {
                cp = 0x10000 + ((cp - 0xd800) << 10) + (self.cur() - 0xdc00);
                self.pos += 1;
            }
            atom = self.node(R_CHAR);
            self.nodes[atom as usize].c = cp;
        }
        self.quantifier(atom)
    }

    /// `{n}`, `{n,}` or `{n,m}` at the position (Annex B keeps any other
    /// `{` a literal).
    fn braced_quantifier(&self) -> bool {
        let mut k: int = 1;
        let mut digits = 0;
        while is_digit(self.peek(k)) {
            k += 1;
            digits += 1;
        }
        if digits == 0 {
            return false;
        }
        if self.peek(k) == 44 {
            k += 1;
            while is_digit(self.peek(k)) {
                k += 1;
            }
        }
        self.peek(k) == 125
    }

    fn quantifier(&mut self, atom: int) -> int {
        let c = self.cur();
        let min: int;
        let max: int;
        if c == 42 {
            min = 0;
            max = -1;
            self.pos += 1;
        } else if c == 43 {
            min = 1;
            max = -1;
            self.pos += 1;
        } else if c == 63 {
            min = 0;
            max = 1;
            self.pos += 1;
        } else if c == 123 {
            // {n}, {n,}, {n,m}; otherwise a literal brace
            let save = self.pos;
            self.pos += 1;
            let mut a: int = 0;
            let mut digits = 0;
            while is_digit(self.cur()) {
                a = a * 10 + (self.cur() - 48);
                self.pos += 1;
                digits += 1;
            }
            if digits == 0 {
                self.pos = save;
                return atom;
            }
            let mut b = a;
            if self.cur() == 44 {
                self.pos += 1;
                if self.cur() == 125 {
                    b = -1;
                } else {
                    b = 0;
                    let mut d2 = 0;
                    while is_digit(self.cur()) {
                        b = b * 10 + (self.cur() - 48);
                        self.pos += 1;
                        d2 += 1;
                    }
                    if d2 == 0 {
                        self.pos = save;
                        return atom;
                    }
                }
            }
            if self.cur() != 125 {
                self.pos = save;
                return atom;
            }
            self.pos += 1;
            if b >= 0 && b < a {
                self.fail("Invalid regular expression: numbers out of order in {} quantifier");
                return atom;
            }
            min = a;
            max = b;
        } else {
            return atom;
        }
        let mut greedy = true;
        if self.cur() == 63 {
            greedy = false;
            self.pos += 1;
        }
        let k = self.nodes[atom as usize].kind;
        if k == R_BOL || k == R_EOL || k == R_WORDB || k == R_NWORDB {
            self.fail("Invalid regular expression: nothing to repeat");
        }
        let n = self.node(R_REPEAT);
        self.nodes[n as usize].list = vec![atom];
        self.nodes[n as usize].min = min;
        self.nodes[n as usize].max = max;
        self.nodes[n as usize].greedy = greedy;
        n
    }

    /// `\p{…}` / `\P{…}` at the `p` / `P`: the matching ranges.
    fn property_escape(&mut self) -> Vec<int> {
        let neg = self.cur() == 80;
        self.pos += 2;
        let mut spec = String::new();
        while self.cur() >= 0 && self.cur() != 125 {
            crate::jsstr::push_cp(&mut spec, self.cur());
            self.pos += 1;
        }
        self.pos += 1;
        if self.check_only {
            // a syntax check: the name alone
            if property_line(spec.as_str()).is_empty() {
                self.fail("Invalid regular expression: invalid property name");
                return Vec::new();
            }
            let mut one: Vec<int> = Vec::new();
            one.push(0);
            one.push(0);
            return one;
        }
        let r = unicode_property(spec.as_str());
        if r.is_empty() {
            self.fail("Invalid regular expression: invalid property name");
            return r;
        }
        if neg {
            return complement(&r);
        }
        r
    }

    fn add_class_escape(&mut self, ranges: &mut Vec<int>, c: int) -> bool {
        // \d \w \s and their negations inside a class
        if c == 100 {
            ranges.push(48);
            ranges.push(57);
            return true;
        }
        if c == 119 {
            ranges.push(48);
            ranges.push(57);
            ranges.push(65);
            ranges.push(90);
            ranges.push(95);
            ranges.push(95);
            ranges.push(97);
            ranges.push(122);
            if self.unicode && self.ignore_case {
                // ſ and K fold to word characters under /iu
                ranges.push(0x17f);
                ranges.push(0x17f);
                ranges.push(0x212a);
                ranges.push(0x212a);
            }
            return true;
        }
        if c == 115 {
            let sp = vec![32, 32, 9, 13, 0xa0, 0xa0, 0x1680, 0x1680, 0x2000, 0x200a, 0x2028, 0x2029, 0x202f, 0x202f, 0x205f, 0x205f, 0x3000, 0x3000, 0xfeff, 0xfeff];
            for x in sp {
                ranges.push(x);
            }
            return true;
        }
        if c == 68 || c == 87 || c == 83 {
            // the complement of \d \w \s
            let mut inner: Vec<int> = Vec::new();
            self.add_class_escape(&mut inner, c + 32);
            // sort pairs
            let mut pairs: Vec<(int, int)> = Vec::new();
            let mut i: usize = 0;
            while i < inner.len() {
                pairs.push((inner[i], inner[i + 1]));
                i += 2;
            }
            let mut a: usize = 1;
            while a < pairs.len() {
                let mut b = a;
                while b > 0 && pairs[b - 1].0 > pairs[b].0 {
                    let t = pairs[b];
                    pairs[b] = pairs[b - 1];
                    pairs[b - 1] = t;
                    b -= 1;
                }
                a += 1;
            }
            let mut lo: int = 0;
            for (x, y) in pairs {
                if x > lo {
                    ranges.push(lo);
                    ranges.push(x - 1);
                }
                if y + 1 > lo {
                    lo = y + 1;
                }
            }
            ranges.push(lo);
            ranges.push(0x10ffff);
            return true;
        }
        false
    }

    /// One escaped character value (not a class escape); -1 for none.
    fn char_escape(&mut self, in_class: bool) -> int {
        let c = self.cur();
        self.pos += 1;
        if c == 110 {
            return 10;
        }
        if c == 116 {
            return 9;
        }
        if c == 114 {
            return 13;
        }
        if c == 118 {
            return 11;
        }
        if c == 102 {
            return 12;
        }
        if c == 48 && !is_digit(self.cur()) {
            return 0;
        }
        if c == 98 && in_class {
            return 8;
        }
        if c == 45 && in_class {
            return 45;
        }
        if c == 99 {
            let l = self.cur();
            if (l >= 65 && l <= 90) || (l >= 97 && l <= 122) {
                self.pos += 1;
                return l % 32;
            }
            if self.unicode {
                self.fail("Invalid unicode escape");
                return 0;
            }
            self.pos -= 1;
            return 92;
        }
        if c == 120 {
            let a = hexv(self.cur());
            let b = hexv(self.peek(1));
            if a >= 0 && b >= 0 {
                self.pos += 2;
                return a * 16 + b;
            }
            if self.unicode {
                self.fail("Invalid escape");
                return 0;
            }
            return 120;
        }
        if c == 117 {
            if self.cur() == 123 && self.unicode {
                self.pos += 1;
                let mut v: int = 0;
                let mut digits: int = 0;
                while self.cur() >= 0 && self.cur() != 125 {
                    let h = hexv(self.cur());
                    if h < 0 || v > 0x10ffff {
                        self.fail("Invalid Unicode escape");
                        return 0;
                    }
                    v = v * 16 + h;
                    digits += 1;
                    self.pos += 1;
                }
                if digits == 0 || v > 0x10ffff || self.cur() != 125 {
                    self.fail("Invalid Unicode escape");
                    return 0;
                }
                self.pos += 1;
                return v;
            }
            let mut v: int = 0;
            let mut k = 0;
            while k < 4 {
                let h = hexv(self.peek(k));
                if h < 0 {
                    if self.unicode {
                        self.fail("Invalid Unicode escape");
                        return 0;
                    }
                    return 117;
                }
                v = v * 16 + h;
                k += 1;
            }
            self.pos += 4;
            if self.unicode && v >= 0xd800 && v <= 0xdbff && self.cur() == 92 && self.peek(1) == 117 {
                let mut lo: int = 0;
                let mut ok = true;
                let mut j = 2;
                while j < 6 {
                    let h = hexv(self.peek(j));
                    if h < 0 {
                        ok = false;
                        break;
                    }
                    lo = lo * 16 + h;
                    j += 1;
                }
                if ok && lo >= 0xdc00 && lo <= 0xdfff {
                    self.pos += 6;
                    return 0x10000 + ((v - 0xd800) << 10) + (lo - 0xdc00);
                }
            }
            return v;
        }
        if is_digit(c) && self.unicode {
            self.fail("Invalid decimal escape");
            return 0;
        }
        if is_digit(c) && (in_class || !self.unicode) {
            // a legacy octal escape (up to \377); \8 and \9 are the digits
            let mut v = c - 48;
            if v < 8 {
                while is_digit(self.cur()) && self.cur() < 56 && v < 32 {
                    v = v * 8 + (self.cur() - 48);
                    self.pos += 1;
                }
                return v;
            }
            return c;
        }
        // an identity escape: under u only a syntax character or `/`
        if self.unicode && !(c == 94 || c == 36 || c == 92 || c == 46 || c == 42 || c == 43 || c == 63 || c == 40 || c == 41 || c == 91 || c == 93 || c == 123 || c == 125 || c == 124 || c == 47 || (in_class && c == 45) || (in_class && self.vmode && v_punct(c))) {
            self.fail("Invalid escape");
            return 0;
        }
        c
    }

    fn class(&mut self) -> int {
        self.pos += 1;
        let mut negate = false;
        if self.cur() == 94 {
            negate = true;
            self.pos += 1;
        }
        let mut ranges: Vec<int> = Vec::new();
        loop {
            let c = self.cur();
            if c < 0 {
                self.fail("Invalid regular expression: missing /");
                break;
            }
            if c == 93 {
                self.pos += 1;
                break;
            }
            let mut lo: int;
            if c == 92 {
                self.pos += 1;
                let e = self.cur();
                if e == 100 || e == 68 || e == 119 || e == 87 || e == 115 || e == 83 {
                    self.pos += 1;
                    self.add_class_escape(&mut ranges, e);
                    if self.unicode && self.cur() == 45 && self.peek(1) != 93 && self.peek(1) >= 0 {
                        self.fail("Invalid character class");
                        break;
                    }
                    continue;
                }
                if (e == 112 || e == 80) && self.unicode && self.peek(1) == 123 {
                    let pr = self.property_escape();
                    for x in pr {
                        ranges.push(x);
                    }
                    if self.cur() == 45 && self.peek(1) != 93 && self.peek(1) >= 0 {
                        self.fail("Invalid character class");
                        break;
                    }
                    continue;
                }
                lo = self.char_escape(true);
            } else {
                lo = c;
                self.pos += 1;
                if self.unicode && lo >= 0xd800 && lo <= 0xdbff && self.cur() >= 0xdc00 && self.cur() <= 0xdfff {
                    lo = 0x10000 + ((lo - 0xd800) << 10) + (self.cur() - 0xdc00);
                    self.pos += 1;
                }
            }
            if self.cur() == 45 && self.peek(1) != 93 && self.peek(1) >= 0 {
                self.pos += 1;
                let mut hi: int;
                if self.cur() == 92 {
                    self.pos += 1;
                    let e = self.cur();
                    if (e == 100 || e == 68 || e == 119 || e == 87 || e == 115 || e == 83 || ((e == 112 || e == 80) && self.peek(1) == 123)) && self.unicode {
                        self.fail("Invalid character class");
                        break;
                    }
                    if e == 100 || e == 68 || e == 119 || e == 87 || e == 115 || e == 83 {
                        // [a-\d]: a, -, and the class
                        self.pos += 1;
                        ranges.push(lo);
                        ranges.push(lo);
                        ranges.push(45);
                        ranges.push(45);
                        self.add_class_escape(&mut ranges, e);
                        continue;
                    }
                    hi = self.char_escape(true);
                } else {
                    hi = self.cur();
                    self.pos += 1;
                    if self.unicode && hi >= 0xd800 && hi <= 0xdbff && self.cur() >= 0xdc00 && self.cur() <= 0xdfff {
                        hi = 0x10000 + ((hi - 0xd800) << 10) + (self.cur() - 0xdc00);
                        self.pos += 1;
                    }
                }
                if hi < lo {
                    self.fail("Invalid regular expression: range out of order in character class");
                    break;
                }
                ranges.push(lo);
                ranges.push(hi);
            } else {
                ranges.push(lo);
                ranges.push(lo);
            }
        }
        self.classes.push(RClass { ranges: norm_ranges(&ranges), negate: negate });
        let n = self.node(R_CLASS);
        self.nodes[n as usize].c = (self.classes.len() as int) - 1;
        n
    }

    /// A v-mode class as a node.
    fn class_v_node(&mut self) -> int {
        let r = self.class_v();
        let strs = self.v_ostrs.clone();
        self.classes.push(RClass { ranges: r, negate: false });
        let n = self.node(R_CLASS);
        self.nodes[n as usize].c = (self.classes.len() as int) - 1;
        if strs.is_empty() {
            return n;
        }
        // a class of strings: the longest strings first, then the class
        let mut order: Vec<int> = Vec::new();
        let mut i: int = 0;
        while i < strs.len() as int {
            let mut j: int = 0;
            while j < order.len() as int && strs[order[j as usize] as usize].len() >= strs[i as usize].len() {
                j += 1;
            }
            order.insert(j as usize, i);
            i += 1;
        }
        let alt = self.node(R_ALT);
        let mut alts: Vec<int> = Vec::new();
        let mut empty = false;
        for k in order {
            let st = strs[k as usize].clone();
            if st.is_empty() {
                empty = true;
                continue;
            }
            let seq = self.node(R_SEQ);
            let mut items: Vec<int> = Vec::new();
            for ch in st {
                let cn = self.node(R_CHAR);
                self.nodes[cn as usize].c = ch;
                items.push(cn);
            }
            self.nodes[seq as usize].list = items;
            alts.push(seq);
        }
        alts.push(n);
        if empty {
            let e = self.node(R_SEQ);
            alts.push(e);
        }
        self.nodes[alt as usize].list = alts;
        alt
    }

    /// A v-mode class at its `[`: operands joined by union, `&&` or `--`,
    /// classes nested; the (normalized) ranges it matches.
    fn class_v(&mut self) -> Vec<int> {
        if self.class_depth >= MAX_NESTING {
            self.fail("Invalid regular expression: too deeply nested");
            return Vec::new();
        }
        self.class_depth += 1;
        let r = self.class_v_inner();
        self.class_depth -= 1;
        r
    }

    fn class_v_inner(&mut self) -> Vec<int> {
        self.pos += 1;
        let mut neg = false;
        if self.cur() == 94 {
            neg = true;
            self.pos += 1;
        }
        let mut acc: Vec<int> = Vec::new();
        let mut strs: Vec<Vec<int>> = Vec::new();
        if self.cur() != 93 {
            acc = self.v_operand();
            strs = self.v_ostrs.clone();
            if self.cur() == 38 && self.peek(1) == 38 {
                while self.cur() == 38 && self.peek(1) == 38 && self.error.is_empty() {
                    self.pos += 2;
                    let o = self.v_operand();
                    let os = self.v_ostrs.clone();
                    acc = intersect(&acc, &o);
                    let mut keep: Vec<Vec<int>> = Vec::new();
                    for x in strs {
                        if has_seq(&os, &x) {
                            keep.push(x);
                        }
                    }
                    strs = keep;
                }
            } else if self.cur() == 45 && self.peek(1) == 45 {
                while self.cur() == 45 && self.peek(1) == 45 && self.error.is_empty() {
                    self.pos += 2;
                    let o = self.v_operand();
                    let os = self.v_ostrs.clone();
                    acc = intersect(&acc, &complement(&o));
                    let mut keep: Vec<Vec<int>> = Vec::new();
                    for x in strs {
                        if !has_seq(&os, &x) {
                            keep.push(x);
                        }
                    }
                    strs = keep;
                }
            } else {
                while self.cur() >= 0 && self.cur() != 93 && self.error.is_empty() {
                    let o = self.v_operand();
                    for x in o {
                        acc.push(x);
                    }
                    let os = self.v_ostrs.clone();
                    for x in os {
                        if !has_seq(&strs, &x) {
                            strs.push(x);
                        }
                    }
                }
                acc = norm_ranges(&acc);
            }
        }
        if self.cur() != 93 {
            self.fail("Invalid regular expression: invalid set operation in character class");
            return Vec::new();
        }
        self.pos += 1;
        if neg {
            if !strs.is_empty() {
                self.fail("Invalid regular expression: negated character class may contain strings");
                return Vec::new();
            }
            self.v_ostrs = Vec::new();
            return complement(&acc);
        }
        self.v_ostrs = strs;
        acc
    }

    /// One operand of a v-mode class: a nested class, a class escape, a
    /// class string disjunction, or a character or character range; its
    /// strings are left in `v_ostrs`.
    fn v_operand(&mut self) -> Vec<int> {
        self.v_ostrs = Vec::new();
        let c = self.cur();
        if c < 0 {
            self.fail("Invalid regular expression: missing /");
            return Vec::new();
        }
        if c == 91 {
            return self.class_v();
        }
        if c == 92 && self.peek(1) == 113 && self.peek(2) == 123 {
            return self.class_strings();
        }
        let lo = self.v_char();
        if lo < -1 {
            // a class escape, already read
            return self.v_pending.clone();
        }
        if self.cur() == 45 && self.peek(1) != 45 && self.peek(1) != 93 {
            self.pos += 1;
            let hi = self.v_char();
            if hi < lo {
                self.fail("Invalid regular expression: range out of order in character class");
                return Vec::new();
            }
            return vec![lo, hi];
        }
        vec![lo, lo]
    }

    /// `\q{a|bc|}` at its `\`: one code point strings as ranges, the
    /// others in `v_ostrs`.
    fn class_strings(&mut self) -> Vec<int> {
        self.pos += 3;
        let mut ranges: Vec<int> = Vec::new();
        let mut strs: Vec<Vec<int>> = Vec::new();
        let mut cur: Vec<int> = Vec::new();
        loop {
            let c = self.cur();
            if c < 0 {
                self.fail("Invalid regular expression: unterminated class string");
                return Vec::new();
            }
            if c == 124 || c == 125 {
                self.pos += 1;
                if cur.len() == 1 {
                    ranges.push(cur[0]);
                    ranges.push(cur[0]);
                } else if !has_seq(&strs, &cur) {
                    strs.push(cur.clone());
                }
                cur = Vec::new();
                if c == 125 {
                    break;
                }
                continue;
            }
            let ch = self.v_char();
            if ch < -1 || !self.error.is_empty() {
                self.fail("Invalid escape");
                return Vec::new();
            }
            cur.push(ch);
        }
        self.v_ostrs = strs;
        norm_ranges(&ranges)
    }

    /// One character of a v-mode class; -2 when it was a class escape,
    /// whose ranges are left in `v_pending`.
    fn v_char(&mut self) -> int {
        let c = self.cur();
        if c == 92 {
            self.pos += 1;
            let e = self.cur();
            if e == 100 || e == 68 || e == 119 || e == 87 || e == 115 || e == 83 {
                self.pos += 1;
                let mut r: Vec<int> = Vec::new();
                self.add_class_escape(&mut r, e);
                self.v_pending = norm_ranges(&r);
                return -2;
            }
            if (e == 112 || e == 80) && self.peek(1) == 123 {
                self.v_pending = self.property_escape();
                return -2;
            }
            return self.char_escape(true);
        }
        self.pos += 1;
        if c >= 0xd800 && c <= 0xdbff && self.cur() >= 0xdc00 && self.cur() <= 0xdfff {
            let v = 0x10000 + ((c - 0xd800) << 10) + (self.cur() - 0xdc00);
            self.pos += 1;
            return v;
        }
        c
    }

    fn escape(&mut self) -> int {
        self.pos += 1;
        let c = self.cur();
        if c < 0 {
            self.fail("Invalid regular expression: \\ at end of pattern");
            return -1;
        }
        if (c == 112 || c == 80) && self.unicode && self.peek(1) == 123 {
            let ranges = self.property_escape();
            self.classes.push(RClass { ranges: ranges, negate: false });
            let n = self.node(R_CLASS);
            self.nodes[n as usize].c = (self.classes.len() as int) - 1;
            return n;
        }
        if c == 100 || c == 68 || c == 119 || c == 87 || c == 115 || c == 83 {
            self.pos += 1;
            let mut ranges: Vec<int> = Vec::new();
            self.add_class_escape(&mut ranges, c);
            self.classes.push(RClass { ranges: norm_ranges(&ranges), negate: false });
            let n = self.node(R_CLASS);
            self.nodes[n as usize].c = (self.classes.len() as int) - 1;
            return n;
        }
        if c >= 49 && c <= 57 {
            // a back reference \1..\99
            let start = self.pos;
            let mut v: int = 0;
            while is_digit(self.cur()) {
                v = v * 10 + (self.cur() - 48);
                self.pos += 1;
            }
            if v > self.total_groups && self.unicode {
                self.fail("Invalid escape");
                return -1;
            }
            if v > self.total_groups && !self.unicode {
                // no such group: an octal escape (Annex B)
                self.pos = start;
                let ch = self.char_escape(false);
                let n = self.node(R_CHAR);
                self.nodes[n as usize].c = ch;
                return n;
            }
            let n = self.node(R_BACKREF);
            self.nodes[n as usize].c = v;
            return n;
        }
        if c == 107 && (self.unicode || self.has_names) {
            if self.peek(1) != 60 {
                self.fail("Invalid named reference");
                return -1;
            }
            self.pos += 2;
            let name = self.group_name();
            self.refs.push(name.clone());
            let n = self.node(R_NAMEDREF);
            self.nodes[n as usize].name = name;
            return n;
        }
        let v = self.char_escape(false);
        let n = self.node(R_CHAR);
        self.nodes[n as usize].c = v;
        n
    }
}

/// The alternatives' texts when every one is only characters (or
/// empty), at most 4096 units in all; empty otherwise.
fn lit_strings(p: &RParser, list: &Vec<int>) -> Vec<Vec<int>> {
    let mut out: Vec<Vec<int>> = Vec::new();
    let mut total: usize = 0;
    for x in list.iter() {
        let nd = &p.nodes[*x as usize];
        let mut s: Vec<int> = Vec::new();
        if nd.kind == R_CHAR {
            s.push(nd.c);
        } else if nd.kind == R_SEQ {
            for y in nd.list.iter() {
                let c = &p.nodes[*y as usize];
                if c.kind != R_CHAR {
                    return Vec::new();
                }
                s.push(c.c);
            }
        } else if nd.kind != R_EMPTY {
            return Vec::new();
        }
        total += s.len();
        if total > 4096 {
            return Vec::new();
        }
        out.push(s);
    }
    out
}

struct Gen {
    code: Vec<Inst>,
    nregs: int,
}

impl Gen {
    fn emit(&mut self, op: int, a: int, b: int) -> int {
        self.code.push(Inst { op: op, a: a, b: b });
        (self.code.len() as int) - 1
    }

    /// The alternation of literal strings `strs` (all sharing their first
    /// `depth` units, already matched) as a trie: the strings that go on
    /// with the same unit share it. Alternatives with different next units
    /// never both match, so grouping them keeps the order matches are
    /// tried in; a string that ends here is a barrier no group crosses.
    fn gen_lits(&mut self, strs: &Vec<Vec<int>>, depth: usize) {
        // the branches in order: (next unit or -1 for an end, members)
        let mut keys: Vec<int> = Vec::new();
        let mut members: Vec<Vec<int>> = Vec::new();
        let mut seg_start: usize = 0;
        let mut i: usize = 0;
        while i < strs.len() {
            let s = &strs[i];
            if s.len() == depth {
                keys.push(-1);
                members.push(vec![i as int]);
                seg_start = keys.len();
            } else {
                let c = s[depth];
                let mut found: int = -1;
                let mut j = seg_start;
                while j < keys.len() {
                    if keys[j] == c {
                        found = j as int;
                        break;
                    }
                    j += 1;
                }
                if found >= 0 {
                    members[found as usize].push(i as int);
                } else {
                    keys.push(c);
                    members.push(vec![i as int]);
                }
            }
            i += 1;
        }
        let cnt = keys.len();
        let mut ends: Vec<int> = Vec::new();
        let mut b: usize = 0;
        while b < cnt {
            let split = if b + 1 < cnt { self.emit(I_SPLIT, 0, 0) } else { -1 };
            if split >= 0 {
                self.code[split as usize].a = split + 1;
            }
            if keys[b] >= 0 {
                self.emit(I_CHAR, keys[b], 0);
                let mut sub: Vec<Vec<int>> = Vec::new();
                for m in members[b].iter() {
                    sub.push(strs[*m as usize].clone());
                }
                self.gen_lits(&sub, depth + 1);
            }
            if split >= 0 {
                let j = self.emit(I_JMP, 0, 0);
                ends.push(j);
                let here = self.code.len() as int;
                self.code[split as usize].b = here;
            }
            b += 1;
        }
        let here = self.code.len() as int;
        for j in ends {
            self.code[j as usize].a = here;
        }
    }

    fn gen(&mut self, p: &RParser, n: int, flags: int, backward: bool) {
        let nd = &p.nodes[n as usize];
        let k = nd.kind;
        if k == R_EMPTY {
            return;
        }
        let ic = flags & 1;
        if k == R_CHAR {
            self.emit(I_CHAR, nd.c, ic);
            return;
        }
        if k == R_ANY {
            self.emit(if (flags & 4) != 0 { I_ANYNL } else { I_ANY }, 0, 0);
            return;
        }
        if k == R_CLASS {
            self.emit(I_CLASS, nd.c, ic);
            return;
        }
        if k == R_MODS {
            let add = nd.c & 7;
            let remove = (nd.c >> 3) & 7;
            let inner = nd.list[0];
            self.gen(p, inner, (flags | add) & (7 - remove), backward);
            return;
        }
        if k == R_SEQ {
            let list = nd.list.clone();
            if backward {
                let mut i = (list.len() as int) - 1;
                while i >= 0 {
                    self.gen(p, list[i as usize], flags, backward);
                    i -= 1;
                }
            } else {
                for x in list {
                    self.gen(p, x, flags, backward);
                }
            }
            return;
        }
        if k == R_ALT {
            let list = nd.list.clone();
            // alternatives that are only characters: a trie (lit_alts)
            if !backward && ic == 0 && list.len() >= 4 {
                let strs = lit_strings(p, &list);
                if !strs.is_empty() {
                    self.gen_lits(&strs, 0);
                    return;
                }
            }
            let mut ends: Vec<int> = Vec::new();
            let cnt = list.len();
            let mut i: usize = 0;
            for x in list {
                if i + 1 < cnt {
                    let split = self.emit(I_SPLIT, 0, 0);
                    self.code[split as usize].a = split + 1;
                    self.gen(p, x, flags, backward);
                    let j = self.emit(I_JMP, 0, 0);
                    ends.push(j);
                    let here = self.code.len() as int;
                    self.code[split as usize].b = here;
                } else {
                    self.gen(p, x, flags, backward);
                }
                i += 1;
            }
            let here = self.code.len() as int;
            for j in ends {
                self.code[j as usize].a = here;
            }
            return;
        }
        if k == R_GROUP {
            let cap = nd.c;
            let inner = nd.list[0];
            if cap >= 0 {
                self.emit(I_SAVE, if backward { cap * 2 + 1 } else { cap * 2 }, 0);
            }
            self.gen(p, inner, flags, backward);
            if cap >= 0 {
                self.emit(I_SAVE, if backward { cap * 2 } else { cap * 2 + 1 }, 0);
            }
            return;
        }
        if k == R_REPEAT {
            let inner = nd.list[0];
            let (min, max, greedy) = (nd.min, nd.max, nd.greedy);
            // each iteration starts with its groups unset (RepeatMatcher)
            let mut lo: int = -1;
            let mut hi: int = -1;
            cap_range(p, inner, &mut lo, &mut hi);
            // written out the repetition takes max (or min) copies of the
            // body: past a few hundred instructions (`(a{0,1000}){0,1000}`
            // would be a million) it becomes a loop with a count register
            let copies = if max > min { max } else { min };
            if copies > 4 {
                let mark = self.code.len();
                let nregs0 = self.nregs;
                self.gen(p, inner, flags, backward);
                let body_len = (self.code.len() - mark) as int + 2;
                self.code.truncate(mark);
                self.nregs = nregs0;
                if body_len * copies > 512 {
                    let cnt = self.nregs;
                    let at = self.nregs + 1;
                    self.nregs += 2;
                    self.emit(I_CNTSET, cnt, 0);
                    let head = self.emit(if greedy { I_CNTBR } else { I_CNTBR_LAZY }, cnt, 0);
                    self.emit(I_CNTDATA, min, max);
                    self.emit(I_SETPOS, at, 0);
                    if lo >= 0 {
                        self.emit(I_CLEAR, lo * 2, hi * 2 + 1);
                    }
                    self.gen(p, inner, flags, backward);
                    self.emit(I_CNTINC, cnt, at);
                    self.emit(I_JMP, head, 0);
                    let end = self.code.len() as int;
                    self.code[head as usize].b = end;
                    return;
                }
            }
            let mut i = 0;
            while i < min {
                if lo >= 0 {
                    self.emit(I_CLEAR, lo * 2, hi * 2 + 1);
                }
                self.gen(p, inner, flags, backward);
                i += 1;
            }
            let ik = p.nodes[inner as usize].kind;
            if max < 0 && lo < 0 && !backward && !p.unicode && (ik == R_CHAR || ik == R_CLASS || ik == R_ANY) {
                self.emit(I_STAR, 0, if greedy { 1 } else { 0 });
                self.gen(p, inner, flags, backward);
                return;
            }
            if max < 0 {
                let reg = self.nregs;
                self.nregs += 1;
                let top = self.emit(I_SPLIT, 0, 0);
                let body = self.code.len() as int;
                self.emit(I_SETPOS, reg, 0);
                if lo >= 0 {
                    self.emit(I_CLEAR, lo * 2, hi * 2 + 1);
                }
                self.gen(p, inner, flags, backward);
                self.emit(I_CHKPOS, reg, 0);
                self.emit(I_JMP, top, 0);
                let end = self.code.len() as int;
                if greedy {
                    self.code[top as usize].a = body;
                    self.code[top as usize].b = end;
                } else {
                    self.code[top as usize].a = end;
                    self.code[top as usize].b = body;
                }
                return;
            }
            let mut splits: Vec<int> = Vec::new();
            let mut j = min;
            while j < max {
                let s = self.emit(I_SPLIT, 0, 0);
                splits.push(s);
                let body = self.code.len() as int;
                if lo >= 0 {
                    self.emit(I_CLEAR, lo * 2, hi * 2 + 1);
                }
                self.gen(p, inner, flags, backward);
                if greedy {
                    self.code[s as usize].a = body;
                } else {
                    self.code[s as usize].b = body;
                }
                j += 1;
            }
            let end = self.code.len() as int;
            for s in splits {
                if greedy {
                    self.code[s as usize].b = end;
                } else {
                    self.code[s as usize].a = end;
                }
            }
            return;
        }
        if k == R_BOL {
            self.emit(I_BOL, (flags >> 1) & 1, 0);
            return;
        }
        if k == R_EOL {
            self.emit(I_EOL, (flags >> 1) & 1, 0);
            return;
        }
        if k == R_WORDB {
            self.emit(I_WORDB, 0, 0);
            return;
        }
        if k == R_NWORDB {
            self.emit(I_NWORDB, 0, 0);
            return;
        }
        if k == R_BACKREF {
            self.emit(I_BACKREF, nd.c, (if backward { 1 } else { 0 }) + 2 * ic);
            return;
        }
        if k == R_NAMEDREF {
            // a duplicate name: one reference each; the groups that did
            // not match match the empty string
            let mut i: usize = 0;
            while i < p.names.len() {
                if p.names[i].as_str() == nd.name.as_str() {
                    self.emit(I_BACKREF, p.name_index[i], (if backward { 1 } else { 0 }) + 2 * ic);
                }
                i += 1;
            }
            return;
        }
        if k == R_LOOK {
            let kind = nd.c;
            let inner = nd.list[0];
            let look = self.emit(I_LOOK, kind, 0);
            let jmp = self.emit(I_JMP, 0, 0);
            let start = self.code.len() as int;
            self.gen(p, inner, flags, kind >= 2);
            self.emit(I_MATCH, 0, 0);
            let after = self.code.len() as int;
            self.code[look as usize].b = start;
            self.code[jmp as usize].a = after;
            return;
        }
    }
}

/// The capturing groups of a pattern: `(` not followed by `?`, or `(?<name>`.
fn count_groups(src: &Vec<int>) -> int {
    let mut n: int = 0;
    let mut i: usize = 0;
    let mut in_class = false;
    while i < src.len() {
        let c = src[i];
        if c == 92 {
            i += 2;
            continue;
        }
        if in_class {
            if c == 93 {
                in_class = false;
            }
        } else if c == 91 {
            in_class = true;
        } else if c == 40 {
            if i + 1 >= src.len() || src[i + 1] != 63 {
                n += 1;
            } else if i + 3 < src.len() && src[i + 2] == 60 && src[i + 3] != 61 && src[i + 3] != 33 {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

/// The lowest and highest capturing group under node n (-1: none).
fn cap_range(p: &RParser, n: int, lo: &mut int, hi: &mut int) {
    let nd = &p.nodes[n as usize];
    if nd.kind == R_GROUP && nd.c >= 0 {
        if *lo < 0 || nd.c < *lo {
            *lo = nd.c;
        }
        if nd.c > *hi {
            *hi = nd.c;
        }
    }
    if nd.kind == R_GROUP || nd.kind == R_SEQ || nd.kind == R_ALT || nd.kind == R_REPEAT || nd.kind == R_LOOK || nd.kind == R_MODS {
        let mut i: usize = 0;
        while i < nd.list.len() {
            cap_range(p, nd.list[i], lo, hi);
            i += 1;
        }
    }
}

/// The pattern has a named group `(?<name>` (not a lookbehind).
fn has_group_names(src: &Vec<int>) -> bool {
    let mut i: usize = 0;
    let mut in_class = false;
    while i < src.len() {
        let c = src[i];
        if c == 92 {
            i += 2;
            continue;
        }
        if in_class {
            if c == 93 {
                in_class = false;
            }
        } else if c == 91 {
            in_class = true;
        } else if c == 40 && i + 3 < src.len() && src[i + 1] == 63 && src[i + 2] == 60 && src[i + 3] != 61 && src[i + 3] != 33 {
            return true;
        }
        i += 1;
    }
    false
}

/// The flags and the pattern read (no code yet): `re` keeps the flags and
/// any error; the parser the pattern's nodes.
fn new_regex() -> Regex {
    Regex {
        code: Vec::new(),
        classes: Vec::new(),
        ngroups: 0,
        names: Vec::new(),
        name_index: Vec::new(),
        global: false,
        ignore_case: false,
        multiline: false,
        dot_all: false,
        unicode: false,
        sticky: false,
        has_indices: false,
        unicode_sets: false,
        error: String::new(),
        nregs: 0,
        first: -1,
        fset: Vec::new(),
        fhi: false,
        cmap: Vec::new(),
        lit: Vec::new(),
        lit_min: 0,
        lit_max: 0,
        anchor: 0,
    }
}

/// Reads the flags into `re` and the pattern into `p`; returns the root
/// node (an error goes to re.error).
fn parse_pattern(re: &mut Regex, p: &mut RParser, pattern: &str, flags: &str, check_only: bool) -> int {
    for c in flags.chars() {
        let dup = match c {
            'g' => re.global,
            'i' => re.ignore_case,
            'm' => re.multiline,
            's' => re.dot_all,
            'u' => re.unicode,
            'y' => re.sticky,
            'd' => re.has_indices,
            'v' => re.unicode,
            _ => {
                re.error = format!("Invalid regular expression flags '{}'", flags);
                return -1;
            }
        };
        if dup {
            re.error = format!("Invalid regular expression flags '{}'", flags);
            return -1;
        }
        match c {
            'g' => re.global = true,
            'i' => re.ignore_case = true,
            'm' => re.multiline = true,
            's' => re.dot_all = true,
            'u' => re.unicode = true,
            'v' => {
                re.unicode = true;
                re.unicode_sets = true;
            }
            'y' => re.sticky = true,
            'd' => re.has_indices = true,
            _ => {}
        }
    }
    p.src = crate::jsstr::units(pattern);
    p.unicode = re.unicode;
    p.ignore_case = re.ignore_case;
    p.vmode = crate::jsstr::index_of(flags, "v", 0) >= 0;
    p.check_only = check_only;
    p.total_groups = count_groups(&p.src);
    p.has_names = has_group_names(&p.src);
    let root = p.disjunction();
    if p.error.is_empty() && p.cur() >= 0 {
        p.error = String::from("Invalid regular expression: unmatched )");
    }
    if p.error.is_empty() {
        let mut i: usize = 0;
        while i < p.refs.len() {
            if !p.names.contains(&p.refs[i]) {
                p.error = String::from("Invalid named capture referenced");
            }
            i += 1;
        }
    }
    if !p.error.is_empty() {
        re.error = p.error.clone();
        return root;
    }
    // back references past the group count are octal escapes (Annex B)
    let ng = p.ngroups;
    let mut i: usize = 0;
    while i < p.nodes.len() {
        if p.nodes[i].kind == R_BACKREF && p.nodes[i].c > ng {
            if re.unicode {
                re.error = String::from("Invalid regular expression: invalid escape");
                return root;
            }
            let v = p.nodes[i].c;
            // \8 and \9 are the digits themselves
            p.nodes[i].kind = R_CHAR;
            p.nodes[i].c = if v < 8 { v } else { 48 + (v % 10) };
        }
        i += 1;
    }
    root
}

fn empty_parser() -> RParser {
    RParser {
        src: Vec::new(),
        pos: 0,
        nodes: Vec::new(),
        classes: Vec::new(),
        ngroups: 0,
        names: Vec::new(),
        name_index: Vec::new(),
        name_paths: Vec::new(),
        alt_path: Vec::new(),
        class_depth: 0,
        ndisj: 0,
        has_names: false,
        refs: Vec::new(),
        error: String::new(),
        unicode: false,
        total_groups: 0,
        ignore_case: false,
        vmode: false,
        v_pending: Vec::new(),
        v_ostrs: Vec::new(),
        check_only: false,
    }
}

/// Whether /pattern/flags is certainly valid by a quick scan: no u / v
/// flag, plain characters, common escapes, `.`, anchors, `|`, groups
/// `(` `(?:` `(?=` `(?!`, quantifiers after an atom and classes of plain
/// characters and ordered ASCII ranges. False means "read it fully".
fn simple_ok(pattern: &str, flags: &str) -> bool {
    let mut seen = String::new();
    for c in flags.chars() {
        if !(c == 'g' || c == 'i' || c == 'm' || c == 's' || c == 'y' || c == 'd') || seen.contains(c) {
            return false;
        }
        seen.push(c);
    }
    let cs: Vec<char> = pattern.chars().collect();
    let n = cs.len();
    let mut i: usize = 0;
    let mut depth: int = 0;
    // an atom precedes (a quantifier may follow)
    let mut atom = false;
    while i < n {
        let c = cs[i];
        if c == '\\' {
            if i + 1 >= n {
                return false;
            }
            let e = cs[i + 1];
            if !(e == 'd' || e == 'D' || e == 'w' || e == 'W' || e == 's' || e == 'S' || e == 'n' || e == 'r' || e == 't' || e == '.' || e == '\\' || e == '/' || e == '-' || e == '(' || e == ')' || e == '[' || e == ']' || e == '{' || e == '}' || e == '*' || e == '+' || e == '?' || e == '|' || e == '^' || e == '$' || e == 'b' || e == 'B') {
                return false;
            }
            atom = e != 'b' && e != 'B';
            i += 2;
            continue;
        }
        if c == '*' || c == '+' || c == '?' {
            if !atom {
                return false;
            }
            i += 1;
            if i < n && cs[i] == '?' {
                i += 1;
            }
            atom = false;
            continue;
        }
        if c == '{' {
            // {n}, {n,}, {n,m} with n <= m, after an atom
            if !atom {
                return false;
            }
            let mut j = i + 1;
            let mut lo: int = 0;
            let mut digits = 0;
            while j < n && cs[j] >= '0' && cs[j] <= '9' && digits < 6 {
                lo = lo * 10 + ((cs[j] as int) - 48);
                j += 1;
                digits += 1;
            }
            if digits == 0 || j >= n {
                return false;
            }
            if cs[j] == ',' {
                j += 1;
                let mut hi: int = 0;
                let mut hd = 0;
                while j < n && cs[j] >= '0' && cs[j] <= '9' && hd < 6 {
                    hi = hi * 10 + ((cs[j] as int) - 48);
                    j += 1;
                    hd += 1;
                }
                if hd > 0 && hi < lo {
                    return false;
                }
            }
            if j >= n || cs[j] != '}' {
                return false;
            }
            i = j + 1;
            if i < n && cs[i] == '?' {
                i += 1;
            }
            atom = false;
            continue;
        }
        if c == '(' {
            if i + 1 < n && cs[i + 1] == '?' {
                if i + 2 < n && (cs[i + 2] == ':' || cs[i + 2] == '=' || cs[i + 2] == '!') {
                    i += 3;
                } else {
                    return false;
                }
            } else {
                i += 1;
            }
            depth += 1;
            atom = false;
            continue;
        }
        if c == ')' {
            if depth == 0 {
                return false;
            }
            depth -= 1;
            atom = true;
            i += 1;
            continue;
        }
        if c == '[' {
            let mut j = i + 1;
            if j < n && cs[j] == '^' {
                j += 1;
            }
            let mut prev: int = -1;
            while j < n && cs[j] != ']' {
                let mut v: int = cs[j] as int;
                if cs[j] == '\\' {
                    if j + 1 >= n {
                        return false;
                    }
                    let e = cs[j + 1];
                    if e == 'd' || e == 'D' || e == 'w' || e == 'W' || e == 's' || e == 'S' {
                        prev = -1;
                        j += 2;
                        continue;
                    }
                    if !(e == 'n' || e == 'r' || e == 't' || e == '.' || e == '\\' || e == '/' || e == '-' || e == '(' || e == ')' || e == '[' || e == ']' || e == '{' || e == '}' || e == '*' || e == '+' || e == '?' || e == '|' || e == '^' || e == '$') {
                        return false;
                    }
                    v = if e == 'n' { 10 } else if e == 'r' { 13 } else if e == 't' { 9 } else { e as int };
                    j += 1;
                } else if cs[j] == '[' {
                    return false;
                } else if cs[j] == '-' && prev >= 0 && j + 1 < n && cs[j + 1] != ']' {
                    // a range prev-x: plain ASCII ends, in order
                    let x = cs[j + 1];
                    if x == '\\' || x == '[' || (x as int) > 127 || prev > 127 || (x as int) < prev {
                        return false;
                    }
                    prev = -1;
                    j += 2;
                    continue;
                }
                prev = v;
                j += 1;
            }
            if j >= n {
                return false;
            }
            i = j + 1;
            atom = true;
            continue;
        }
        if c == ']' || c == '}' {
            return false;
        }
        atom = !(c == '|' || c == '^' || c == '$');
        i += 1;
    }
    depth == 0
}

/// The error of the regular expression literal /pattern/flags ("" when it
/// is one), without compiling it.
pub fn check(pattern: &str, flags: &str) -> String {
    if simple_ok(pattern, flags) {
        return String::new();
    }
    check_memo(pattern, flags)
}

fn check_full(pattern: &str, flags: &str) -> String {
    let mut re = new_regex();
    let mut p = empty_parser();
    parse_pattern(&mut re, &mut p, pattern, flags, true);
    re.error
}

#[ranger::target(rust)]
thread_local! {
    static CHECKED: std::cell::RefCell<std::collections::HashMap<String, String>> = std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Rust: a literal read before is not read again.
#[ranger::target(rust)]
fn check_memo(pattern: &str, flags: &str) -> String {
    let key = format!("{}/{}", flags, pattern);
    if let Some(e) = CHECKED.with(|m| m.borrow().get(&key).cloned()) {
        return e;
    }
    let e = check_full(pattern, flags);
    CHECKED.with(|m| {
        let mut m = m.borrow_mut();
        if m.len() > 4096 {
            m.clear();
        }
        m.insert(key, e.clone());
    });
    e
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
fn check_memo(pattern: &str, flags: &str) -> String {
    check_full(pattern, flags)
}

pub fn compile(pattern: &str, flags: &str) -> Regex {
    let mut re = new_regex();
    let mut p = empty_parser();
    let root = parse_pattern(&mut re, &mut p, pattern, flags, false);
    if !re.error.is_empty() {
        return re;
    }
    let mut g = Gen { code: Vec::new(), nregs: 0 };
    g.emit(I_SAVE, 0, 0);
    let flags0 = (if re.ignore_case { 1 } else { 0 }) + (if re.multiline { 2 } else { 0 }) + (if re.dot_all { 4 } else { 0 });
    g.gen(&p, root, flags0, false);
    g.emit(I_SAVE, 1, 0);
    g.emit(I_MATCH, 0, 0);
    // a literal first unit lets the search skip ahead
    if g.code.len() > 1 && g.code[1].op == I_CHAR && g.code[1].b == 0 && g.code[1].a < 0x10000 {
        re.first = g.code[1].a;
    }
    re.code = g.code;
    re.nregs = g.nregs;
    if !re.sticky && !re.unicode_sets {
        let li = req_lit(&p, root, re.ignore_case, re.unicode);
        // one unit right at the start is `first` already
        if li.lit.len() >= 2 || (li.lit.len() == 1 && !(li.lmin == 0 && li.lmax == 0)) {
            re.lit = li.lit;
            re.lit_min = li.lmin;
            re.lit_max = li.lmax;
        }
    }
    re.classes = p.classes;
    re.ngroups = p.ngroups;
    re.names = p.names;
    re.name_index = p.name_index;
    if re.first < 0 && !re.sticky {
        re.start_set();
    }
    if !re.sticky {
        re.anchor = re.start_anchor();
    }
    re.class_maps();
    re
}

impl Regex {
    /// fset and fhi: the units a match can start with, from the code's
    /// first consuming instructions; left empty when a match may start
    /// with anything or be empty.
    fn start_set(&mut self) {
        let mut set: Vec<bool> = Vec::new();
        let mut k: int = 0;
        while k < 128 {
            set.push(false);
            k += 1;
        }
        let mut hi = false;
        let mut seen: Vec<bool> = Vec::new();
        let mut j: usize = 0;
        while j < self.code.len() {
            seen.push(false);
            j += 1;
        }
        let mut work: Vec<int> = Vec::new();
        work.push(1);
        while !work.is_empty() {
            let pc = work.pop().unwrap();
            if pc < 0 || pc as usize >= self.code.len() || seen[pc as usize] {
                continue;
            }
            seen[pc as usize] = true;
            let ins = self.code[pc as usize];
            if ins.op == I_CHAR {
                let c = ins.a;
                if c >= 128 {
                    if ins.b != 0 {
                        // a case fold may reach ASCII
                        return;
                    }
                    hi = true;
                } else {
                    set[c as usize] = true;
                    if ins.b != 0 {
                        hi = true;
                        let mut u: int = 0;
                        while u < 128 {
                            if self.eq(u, c, true) {
                                set[u as usize] = true;
                            }
                            u += 1;
                        }
                    }
                }
            } else if ins.op == I_CLASS {
                let mut u: int = 0;
                while u < 128 {
                    if self.class_has(ins.a, u, ins.b != 0) {
                        set[u as usize] = true;
                    }
                    u += 1;
                }
                hi = true;
            } else if ins.op == I_SPLIT {
                work.push(ins.a);
                work.push(ins.b);
            } else if ins.op == I_STAR {
                // its unit, or none and what follows
                work.push(pc + 1);
                work.push(pc + 2);
            } else if ins.op == I_JMP {
                work.push(ins.a);
            } else if ins.op == I_CNTBR || ins.op == I_CNTBR_LAZY {
                work.push(pc + 2);
                work.push(ins.b);
            } else if ins.op == I_SAVE || ins.op == I_CLEAR || ins.op == I_SETPOS || ins.op == I_CHKPOS || ins.op == I_CNTSET || ins.op == I_CNTINC || ins.op == I_BOL || ins.op == I_EOL || ins.op == I_WORDB || ins.op == I_NWORDB {
                // nothing consumed: what follows starts the match
                work.push(pc + 1);
            } else {
                // any unit, a back reference, a lookaround or an empty match
                return;
            }
        }
        self.fset = set;
        self.fhi = hi;
    }

    /// anchor: whether every path from the start meets a `^` before it
    /// consumes a unit or matches, and whether any of them is multiline.
    fn start_anchor(&self) -> int {
        let mut seen: Vec<bool> = Vec::new();
        let mut j: usize = 0;
        while j < self.code.len() {
            seen.push(false);
            j += 1;
        }
        let mut multi = false;
        let mut work: Vec<int> = Vec::new();
        work.push(1);
        while !work.is_empty() {
            let pc = work.pop().unwrap();
            if pc < 0 || pc as usize >= self.code.len() {
                return 0;
            }
            if seen[pc as usize] {
                continue;
            }
            seen[pc as usize] = true;
            let ins = self.code[pc as usize];
            if ins.op == I_BOL {
                if ins.a != 0 {
                    multi = true;
                }
            } else if ins.op == I_SPLIT {
                work.push(ins.a);
                work.push(ins.b);
            } else if ins.op == I_JMP {
                work.push(ins.a);
            } else if ins.op == I_CNTBR || ins.op == I_CNTBR_LAZY {
                work.push(pc + 2);
                work.push(ins.b);
            } else if ins.op == I_SAVE || ins.op == I_CLEAR || ins.op == I_SETPOS || ins.op == I_CHKPOS || ins.op == I_CNTSET || ins.op == I_CNTINC {
                work.push(pc + 1);
            } else {
                // a unit consumed, an assertion, a lookaround or the match
                // before any `^`: a match may start anywhere
                return 0;
            }
        }
        if multi {
            2
        } else {
            1
        }
    }

    /// cmap from the classes
    fn class_maps(&mut self) {
        let mut m: Vec<bool> = Vec::new();
        let mut ci: int = 0;
        while ci < self.classes.len() as int {
            let mut k: int = 0;
            while k < 256 {
                let v = self.class_slow(ci, k & 127, k >= 128);
                m.push(v);
                k += 1;
            }
            ci += 1;
        }
        self.cmap = m;
    }

    fn class_has(&self, ci: int, c: int, ic: bool) -> bool {
        if c >= 0 && c < 128 {
            let at = ci * 256 + (if ic { 128 } else { 0 }) + c;
            if (at as usize) < self.cmap.len() {
                return self.cmap[at as usize];
            }
        }
        self.class_slow(ci, c, ic)
    }

    fn class_slow(&self, ci: int, c: int, ic: bool) -> bool {
        let cl = &self.classes[ci as usize];
        let mut hit = in_ranges(&cl.ranges, c);
        if !hit && ic {
            let l = fold(c);
            let u = upper(c);
            hit = in_ranges(&cl.ranges, l) || in_ranges(&cl.ranges, u);
            if !hit && self.unicode {
                let f = fold_u(c);
                hit = in_ranges(&cl.ranges, f) || in_ranges(&cl.ranges, upper(f));
            }
        }
        if cl.negate {
            !hit
        } else {
            hit
        }
    }

    fn eq(&self, a: int, b: int, ic: bool) -> bool {
        if a == b {
            return true;
        }
        if !ic {
            return false;
        }
        if self.unicode {
            return fold_u(a) == fold_u(b);
        }
        canon(a) == canon(b)
    }

    /// A word character; under /iu also what folds to one (ſ, K).
    fn word_at(&self, input: &Vec<int>, i: int) -> bool {
        let c = input[i as usize];
        is_word(c) || (self.unicode && self.ignore_case && c >= 128 && is_word(fold_u(c)))
    }

    /// Whether the one-unit matcher `ins` (I_STAR's) takes unit c.
    #[inline]
    fn unit_ok(&self, ins: Inst, c: int) -> bool {
        if ins.op == I_CHAR {
            self.eq(c, ins.a, ins.b != 0)
        } else if ins.op == I_CLASS {
            self.class_has(ins.a, c, ins.b != 0)
        } else if ins.op == I_ANYNL {
            true
        } else {
            !is_line_term(c)
        }
    }

    /// The code point at `pos` (a surrogate pair is one under /u) and its
    /// width in units.
    fn unit_at(&self, input: &Vec<int>, pos: int, n: int) -> (int, int) {
        let c = input[pos as usize];
        if self.unicode && c >= 0xd800 && c <= 0xdbff && pos + 1 < n {
            let d = input[(pos + 1) as usize];
            if d >= 0xdc00 && d <= 0xdfff {
                return (0x10000 + ((c - 0xd800) << 10) + (d - 0xdc00), 2);
            }
        }
        (c, 1)
    }

    /// Runs the program from `pc` at `start`; answers the end position or
    /// -1. `back` runs it right to left (lookbehind).
    fn run(&self, input: &Vec<int>, start: int, pc0: int, caps: &mut Vec<int>, regs: &mut Vec<int>, back: bool, stack: &mut Vec<(int, int, int)>) -> int {
        let n = input.len() as int;
        // the backtrack entries of this run sit above base: a lookaround's
        // run shares the list and leaves it as it found it
        let base = stack.len();
        let mut pc = pc0;
        let mut pos = start;
        let mut steps: int = 0;
        let mut backs: int = 0;
        let limit = crate::builtins::regex_limit();
        loop {
            let ins = self.code[pc as usize];
            let mut ok = true;
            match ins.op {
                I_CHAR => {
                    if back {
                        if pos > 0 && self.eq(input[(pos - 1) as usize], ins.a, ins.b != 0) {
                            pos -= 1;
                            pc += 1;
                        } else {
                            ok = false;
                        }
                    } else if pos < n {
                        let (c, w) = self.unit_at(input, pos, n);
                        if self.eq(c, ins.a, ins.b != 0) {
                            pos += w;
                            pc += 1;
                        } else {
                            ok = false;
                        }
                    } else {
                        ok = false;
                    }
                }
                I_ANY | I_ANYNL => {
                    if back {
                        if pos > 0 && (ins.op == I_ANYNL || !is_line_term(input[(pos - 1) as usize])) {
                            pos -= 1;
                            pc += 1;
                        } else {
                            ok = false;
                        }
                    } else if pos < n {
                        let (c, w) = self.unit_at(input, pos, n);
                        if ins.op == I_ANYNL || !is_line_term(c) {
                            pos += w;
                            pc += 1;
                        } else {
                            ok = false;
                        }
                    } else {
                        ok = false;
                    }
                }
                I_CLASS => {
                    if back {
                        if pos > 0 && self.class_has(ins.a, input[(pos - 1) as usize], ins.b != 0) {
                            pos -= 1;
                            pc += 1;
                        } else {
                            ok = false;
                        }
                    } else if pos < n {
                        let (c, w) = self.unit_at(input, pos, n);
                        if self.class_has(ins.a, c, ins.b != 0) {
                            pos += w;
                            pc += 1;
                        } else {
                            ok = false;
                        }
                    } else {
                        ok = false;
                    }
                }
                I_STAR => {
                    steps += 1;
                    if crate::builtins::regex_time(steps) {
                        stack.truncate(base);
                        return -1;
                    }
                    let unit = self.code[(pc + 1) as usize];
                    if ins.b == 1 {
                        let mut e = pos;
                        while e < n && self.unit_ok(unit, input[e as usize]) {
                            e += 1;
                        }
                        if e > pos {
                            // (3, what follows, the shortest end), then
                            // (5, this end): each backtrack gives one back
                            stack.push((3, pc + 2, pos));
                            stack.push((5, e, 0));
                        }
                        pos = e;
                    } else {
                        // lazy: none first; a backtrack takes one more
                        stack.push((6, pc + 2, pos));
                    }
                    pc += 2;
                }
                I_SPLIT => {
                    // a run without splits ends (no loop goes back without
                    // one): the clock is looked at from here
                    steps += 1;
                    if crate::builtins::regex_time(steps) {
                        stack.truncate(base);
                        return -1;
                    }
                    stack.push((0, ins.b, pos));
                    pc = ins.a;
                }
                I_JMP => {
                    pc = ins.a;
                }
                I_SAVE => {
                    stack.push((1, ins.a, caps[ins.a as usize]));
                    caps[ins.a as usize] = pos;
                    pc += 1;
                }
                I_CLEAR => {
                    let mut k = ins.a;
                    while k <= ins.b {
                        if caps[k as usize] >= 0 {
                            stack.push((1, k, caps[k as usize]));
                            caps[k as usize] = -1;
                        }
                        k += 1;
                    }
                    pc += 1;
                }
                I_SETPOS => {
                    stack.push((2, ins.a, regs[ins.a as usize]));
                    regs[ins.a as usize] = pos;
                    pc += 1;
                }
                I_CNTSET => {
                    stack.push((2, ins.a, regs[ins.a as usize]));
                    regs[ins.a as usize] = 0;
                    pc += 1;
                }
                I_CNTBR | I_CNTBR_LAZY => {
                    steps += 1;
                    if crate::builtins::regex_time(steps) {
                        stack.truncate(base);
                        return -1;
                    }
                    let c = regs[ins.a as usize];
                    let d = self.code[(pc + 1) as usize];
                    if d.b >= 0 && c >= d.b {
                        pc = ins.b;
                    } else if c < d.a {
                        pc += 2;
                    } else if ins.op == I_CNTBR {
                        stack.push((0, ins.b, pos));
                        pc += 2;
                    } else {
                        stack.push((0, pc + 2, pos));
                        pc = ins.b;
                    }
                }
                I_CNTINC => {
                    let c = regs[ins.a as usize];
                    let head = self.code[(pc + 1) as usize].a;
                    let min = self.code[(head + 1) as usize].a;
                    if c >= min && regs[ins.b as usize] == pos {
                        ok = false;
                    } else {
                        stack.push((2, ins.a, c));
                        regs[ins.a as usize] = c + 1;
                        pc += 1;
                    }
                }
                I_CHKPOS => {
                    if regs[ins.a as usize] == pos {
                        ok = false;
                    } else {
                        pc += 1;
                    }
                }
                I_BOL => {
                    if pos == 0 || (ins.a != 0 && is_line_term(input[(pos - 1) as usize])) {
                        pc += 1;
                    } else {
                        ok = false;
                    }
                }
                I_EOL => {
                    if pos == n || (ins.a != 0 && is_line_term(input[pos as usize])) {
                        pc += 1;
                    } else {
                        ok = false;
                    }
                }
                I_WORDB | I_NWORDB => {
                    let a = pos > 0 && self.word_at(input, pos - 1);
                    let b = pos < n && self.word_at(input, pos);
                    let at = a != b;
                    if (ins.op == I_WORDB) == at {
                        pc += 1;
                    } else {
                        ok = false;
                    }
                }
                I_BACKREF => {
                    let s = caps[(ins.a * 2) as usize];
                    let e = caps[(ins.a * 2 + 1) as usize];
                    if s < 0 || e < 0 {
                        pc += 1;
                    } else {
                        let len = e - s;
                        if (ins.b & 1) == 1 {
                            if pos - len < 0 {
                                ok = false;
                            } else {
                                let mut k: int = 0;
                                while k < len {
                                    if !self.eq(input[(s + k) as usize], input[(pos - len + k) as usize], (ins.b & 2) != 0) {
                                        ok = false;
                                        break;
                                    }
                                    k += 1;
                                }
                                if ok {
                                    pos -= len;
                                    pc += 1;
                                }
                            }
                        } else if pos + len > n {
                            ok = false;
                        } else {
                            let mut k: int = 0;
                            while k < len {
                                if !self.eq(input[(s + k) as usize], input[(pos + k) as usize], (ins.b & 2) != 0) {
                                    ok = false;
                                    break;
                                }
                                k += 1;
                            }
                            if ok {
                                pos += len;
                                pc += 1;
                            }
                        }
                    }
                }
                I_LOOK => {
                    let kind = ins.a;
                    let neg = (kind & 1) == 1;
                    let behind = kind >= 2;
                    let saved = caps.clone();
                    let r = self.run(input, pos, ins.b, caps, regs, behind, stack);
                    let hit = r >= 0;
                    if hit == neg {
                        // failed: undo what the lookaround captured
                        let mut i: usize = 0;
                        while i < caps.len() {
                            caps[i] = saved[i];
                            i += 1;
                        }
                        ok = false;
                    } else {
                        if neg {
                            let mut i: usize = 0;
                            while i < caps.len() {
                                caps[i] = saved[i];
                                i += 1;
                            }
                        } else {
                            // captures set inside stay; undo them on backtrack
                            let mut i: usize = 0;
                            while i < caps.len() {
                                if caps[i] != saved[i] {
                                    stack.push((1, i as int, saved[i]));
                                }
                                i += 1;
                            }
                        }
                        pc += 1;
                    }
                }
                I_MATCH => {
                    stack.truncate(base);
                    return pos;
                }
                _ => {
                    ok = false;
                }
            }
            if !ok {
                // backtrack: the budget counts these (a match that never
                // goes back, however long, does not use it up)
                backs += 1;
                if backs > limit {
                    crate::builtins::regex_over();
                    stack.truncate(base);
                    return -1;
                }
                loop {
                    if stack.len() == base {
                        return -1;
                    }
                    let (kind, a, b) = stack.pop().unwrap();
                    if kind == 0 {
                        pc = a;
                        pos = b;
                        break;
                    } else if kind == 5 {
                        // a greedy run one unit shorter; (3, …) below it
                        let (k3, cont, least) = stack[stack.len() - 1];
                        let _ = k3;
                        let e = a - 1;
                        if e > least {
                            stack.push((5, e, 0));
                        } else {
                            stack.pop();
                        }
                        pc = cont;
                        pos = e;
                        break;
                    } else if kind == 6 {
                        // a lazy run one unit longer
                        let unit = self.code[(a - 1) as usize];
                        if b < n && self.unit_ok(unit, input[b as usize]) {
                            stack.push((6, a, b + 1));
                            pc = a;
                            pos = b + 1;
                            break;
                        }
                    } else if kind == 3 {
                        // reached only through kind 5's handling
                    } else if kind == 1 {
                        caps[a as usize] = b;
                    } else {
                        regs[a as usize] = b;
                    }
                }
            }
        }
    }

    /// Searches from `start`: the capture positions (start, end pairs; -1
    /// for unset), or an empty vector when there is no match.
    /// Where `lit` occurs first at or after `from`, -1 when it does not.
    fn find_lit(&self, input: &Vec<int>, from: int) -> int {
        let n = input.len() as int;
        let m = self.lit.len() as int;
        let c0 = self.lit[0];
        let mut i = if from < 0 { 0 } else { from };
        while i + m <= n {
            if input[i as usize] == c0 {
                let mut j: int = 1;
                while j < m && input[(i + j) as usize] == self.lit[j as usize] {
                    j += 1;
                }
                if j == m {
                    return i;
                }
            }
            i += 1;
        }
        -1
    }

    pub fn exec(&self, input: &Vec<int>, start: int) -> Vec<int> {
        if crate::builtins::regex_stopped() {
            // an attempt ran out of steps or time: no more tries until
            // Vm::rx_after has thrown
            return Vec::new();
        }
        let n = input.len() as int;
        let mut s = start;
        let ncap = ((self.ngroups + 1) * 2) as usize;
        let mut regs: Vec<int> = Vec::new();
        let mut r: int = 0;
        while r < self.nregs {
            regs.push(-1);
            r += 1;
        }
        // one capture list for every start: a failed try leaves it to be
        // cleared, not dropped
        let mut caps: Vec<int> = Vec::with_capacity(ncap);
        let mut stack: Vec<(int, int, int)> = Vec::new();
        // where lit was found last (-1: not looked for yet)
        let mut lp: int = -1;
        if self.anchor == 1 && s > 0 {
            // `^` matches only at 0
            return Vec::new();
        }
        while s <= n {
            if self.anchor == 2 && s > 0 && !is_line_term(input[(s - 1) as usize]) {
                // a multiline `^`: the next start after a line terminator
                while s < n && !is_line_term(input[s as usize]) {
                    s += 1;
                }
                s += 1;
                if s > n {
                    break;
                }
            }
            if !self.lit.is_empty() {
                if lp < s + self.lit_min {
                    lp = self.find_lit(input, s + self.lit_min);
                    if lp < 0 {
                        return Vec::new();
                    }
                }
                if self.lit_max >= 0 && lp - self.lit_max > s {
                    s = lp - self.lit_max;
                }
            }
            if self.first >= 0 && !self.sticky {
                while s < n && input[s as usize] != self.first {
                    s += 1;
                }
                if s >= n {
                    return Vec::new();
                }
            } else if !self.fset.is_empty() {
                // every match takes a unit first, one of fset's
                while s < n {
                    let u = input[s as usize];
                    if if u < 128 { self.fset[u as usize] } else { self.fhi } {
                        break;
                    }
                    s += 1;
                }
                if s >= n {
                    return Vec::new();
                }
            }
            caps.clear();
            let mut i: usize = 0;
            while i < ncap {
                caps.push(-1);
                i += 1;
            }
            let e = self.run(input, s, 0, &mut caps, &mut regs, false, &mut stack);
            if e >= 0 {
                return caps;
            }
            if self.sticky || self.anchor == 1 {
                break;
            }
            s += 1;
        }
        Vec::new()
    }
}

/// Whether `c` is in the ranges (sorted, disjoint pairs lo, hi).
fn in_ranges(r: &Vec<int>, c: int) -> bool {
    let n = (r.len() / 2) as int;
    if n <= 8 {
        let mut i: usize = 0;
        while i + 1 < r.len() {
            if c >= r[i] && c <= r[i + 1] {
                return true;
            }
            i += 2;
        }
        return false;
    }
    let mut lo: int = 0;
    let mut hi: int = n - 1;
    while lo <= hi {
        let mid = (lo + hi) / 2;
        if c < r[(mid * 2) as usize] {
            hi = mid - 1;
        } else if c > r[(mid * 2 + 1) as usize] {
            lo = mid + 1;
        } else {
            return true;
        }
    }
    false
}

/// Ranges sorted by start, overlapping and adjacent ones merged.
fn norm_ranges(r: &Vec<int>) -> Vec<int> {
    let mut los: Vec<int> = Vec::new();
    let mut his: Vec<int> = Vec::new();
    let mut i: usize = 0;
    while i + 1 < r.len() {
        // insertion by start: cheap for the mostly sorted input classes have
        let mut j = los.len();
        los.push(r[i]);
        his.push(r[i + 1]);
        while j > 0 && los[j - 1] > los[j] {
            let (tl, th) = (los[j], his[j]);
            los[j] = los[j - 1];
            his[j] = his[j - 1];
            los[j - 1] = tl;
            his[j - 1] = th;
            j -= 1;
        }
        i += 2;
    }
    let mut out: Vec<int> = Vec::new();
    let mut k: usize = 0;
    while k < los.len() {
        let n = out.len();
        if n > 0 && los[k] <= out[n - 1] + 1 {
            if his[k] > out[n - 1] {
                out[n - 1] = his[k];
            }
        } else {
            out.push(los[k]);
            out.push(his[k]);
        }
        k += 1;
    }
    out
}

/// The code points in both (normalized) range lists.
/// Whether `list` holds the code point string `x`.
fn has_seq(list: &Vec<Vec<int>>, x: &Vec<int>) -> bool {
    let mut i: usize = 0;
    while i < list.len() {
        let y = &list[i];
        if y.len() == x.len() {
            let mut j: usize = 0;
            while j < x.len() && y[j] == x[j] {
                j += 1;
            }
            if j == x.len() {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// A ClassSetReservedPunctuator: `& - ! # % , : ; < = > @ ` ~`
fn v_punct(c: int) -> bool {
    c == 38 || c == 45 || c == 33 || c == 35 || c == 37 || c == 44 || c == 58 || c == 59 || c == 60 || c == 61 || c == 62 || c == 64 || c == 96 || c == 126
}

fn intersect(a: &Vec<int>, b: &Vec<int>) -> Vec<int> {
    let mut out: Vec<int> = Vec::new();
    let mut i: usize = 0;
    let mut j: usize = 0;
    while i + 1 < a.len() && j + 1 < b.len() {
        let lo = if a[i] > b[j] { a[i] } else { b[j] };
        let hi = if a[i + 1] < b[j + 1] { a[i + 1] } else { b[j + 1] };
        if lo <= hi {
            out.push(lo);
            out.push(hi);
        }
        if a[i + 1] < b[j + 1] {
            i += 2;
        } else {
            j += 2;
        }
    }
    out
}

/// The code points not in the (normalized) ranges.
fn complement(r: &Vec<int>) -> Vec<int> {
    let mut out: Vec<int> = Vec::new();
    let mut next: int = 0;
    let mut i: usize = 0;
    while i + 1 < r.len() {
        if r[i] > next {
            out.push(next);
            out.push(r[i] - 1);
        }
        next = r[i + 1] + 1;
        i += 2;
    }
    if next <= 0x10ffff {
        out.push(next);
        out.push(0x10ffff);
    }
    out
}

/// The rest of the line `key` begins in the property table ("" if none).
/// Rust: by bytes (the table is ASCII).
#[ranger::target(rust)]
fn table_line(t: &str, key: &str) -> String {
    let pat = format!("\n{} ", key);
    match t.find(pat.as_str()) {
        Some(i) => {
            let start = i + pat.len();
            let end = match t[start..].find('\n') {
                Some(e) => start + e,
                None => t.len(),
            };
            String::from(&t[start..end])
        }
        None => String::new(),
    }
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
fn table_line(t: &str, key: &str) -> String {
    let pat = format!("\n{} ", key);
    let i = crate::jsstr::index_of(t, pat.as_str(), 0);
    if i < 0 {
        return String::new();
    }
    let start = i + crate::jsstr::len(pat.as_str());
    let mut end = crate::jsstr::index_of(t, "\n", start);
    if end < 0 {
        end = crate::jsstr::len(t);
    }
    crate::jsstr::slice(t, start, end)
}

fn decode_ranges(s: &str) -> Vec<int> {
    let mut out: Vec<int> = Vec::new();
    let mut prev: int = 0;
    let mut num: int = 0;
    let mut lo: int = 0;
    let mut first = true;
    for ch in s.chars() {
        let c = ch as int;
        if c >= 65 && c <= 80 {
            num = num * 16 + (c - 65);
            continue;
        }
        num = num * 16 + (c - 97);
        if first {
            lo = prev + num;
            first = false;
        } else {
            out.push(lo);
            out.push(lo + num);
            prev = lo + num + 1;
            first = true;
        }
        num = 0;
    }
    out
}

/// The ranges of the Unicode property in `\p{spec}`: `name=value` for
/// General_Category, Script and Script_Extensions, else a General_Category
/// value or a binary property. Empty when there is no such property.
pub fn unicode_property(spec: &str) -> Vec<int> {
    let line = property_line(spec);
    if line.is_empty() {
        return Vec::new();
    }
    decode_ranges(line.as_str())
}

#[ranger::target(rust)]
thread_local! {
    static UNI_TABLE: String = {
        let mut t = String::from("\n");
        t.push_str(crate::uniprops::table().as_str());
        t
    };
}

/// The encoded ranges of `\p{spec}` in the property table ("" if none).
/// Rust: the table is built once.
#[ranger::target(rust)]
fn property_line(spec: &str) -> String {
    UNI_TABLE.with(|t| property_line_in(t.as_str(), spec))
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
fn property_line(spec: &str) -> String {
    let mut t = String::from("\n");
    t.push_str(crate::uniprops::table().as_str());
    property_line_in(t.as_str(), spec)
}

fn property_line_in(t: &str, spec: &str) -> String {
    let eq = crate::jsstr::index_of(spec, "=", 0);
    if eq >= 0 {
        let name = crate::jsstr::slice(spec, 0, eq);
        let value = crate::jsstr::slice(spec, eq + 1, crate::jsstr::len(spec));
        let kind = if name.as_str() == "General_Category" || name.as_str() == "gc" {
            "G"
        } else if name.as_str() == "Script" || name.as_str() == "sc" {
            "S"
        } else if name.as_str() == "Script_Extensions" || name.as_str() == "scx" {
            "X"
        } else {
            return String::new();
        };
        let akind = if kind == "X" { "S" } else { kind };
        let mut canon = table_line(t, format!("={}{}", akind, value).as_str());
        if canon.is_empty() {
            canon = value;
        }
        if kind == "X" {
            let x = table_line(t, format!("X{}", canon).as_str());
            if !x.is_empty() {
                return x;
            }
        }
        return table_line(t, format!("{}{}", akind, canon).as_str());
    }
    for kind in vec!["G", "B"] {
        let mut canon = table_line(t, format!("={}{}", kind, spec).as_str());
        if canon.is_empty() {
            canon = String::from(spec);
        }
        let r = table_line(t, format!("{}{}", kind, canon).as_str());
        if !r.is_empty() {
            return r;
        }
    }
    String::new()
}

#[ranger::target(rust)]
thread_local! {
    static ID_TABLES: std::cell::RefCell<(Vec<int>, Vec<int>)> = std::cell::RefCell::new((Vec::new(), Vec::new()));
}

/// ID_Start (start) or ID_Continue for a non-ASCII `c`, from the Unicode
/// property table (decoded once, on the first non-ASCII identifier char).
#[ranger::target(rust)]
pub fn id_wide(c: char, start: bool) -> bool {
    ID_TABLES.with(|t| {
        let mut t = t.borrow_mut();
        if t.0.is_empty() {
            t.0 = unicode_property("ID_Start");
            t.1 = unicode_property("ID_Continue");
        }
        if start {
            in_ranges(&t.0, c as int)
        } else {
            in_ranges(&t.1, c as int)
        }
    })
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn id_wide(c: char, start: bool) -> bool {
    if start {
        c.is_alphabetic() && c != '\u{2e2f}'
    } else {
        (c.is_alphanumeric() && c != '\u{2e2f}') || c == '\u{200c}' || c == '\u{200d}'
    }
}
