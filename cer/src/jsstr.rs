// SPDX-License-Identifier: AGPL-3.0-or-later
//! JavaScript strings are sequences of UTF-16 units; ours are Rust strings
//! (UTF-8). Positions here are UTF-16 positions, as JavaScript counts them.
//! An ASCII string (the common case) is indexed by its bytes directly.

use ranger::prelude::*;

pub fn is_ascii(s: &str) -> bool {
    let r: bool = ranger::native!(
        rust: { ascii_scan(s) },
        es6: "/^[\\x00-\\x7f]*$/.test({s})",
    );
    r
}

#[ranger::target(rust)]
fn ascii_scan(s: &str) -> bool {
    s.is_ascii()
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
fn ascii_scan(s: &str) -> bool {
    for b in s.bytes() {
        if b >= 128 {
            return false;
        }
    }
    true
}

pub fn units(s: &str) -> Vec<int> {
    let r: Vec<int> = ranger::native!(rust: { units_of(s) }, es6: "Array.from({{ length: {s}.length }}, (_, i) => {s}.charCodeAt(i))");
    r
}

/// Rust: an ASCII string's bytes in one go (vectorised)
#[ranger::target(rust)]
fn ascii_units(s: &str) -> Vec<int> {
    s.bytes().map(|b| b as int).collect()
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
fn ascii_units(s: &str) -> Vec<int> {
    let mut out: Vec<int> = Vec::with_capacity(s.as_bytes().len());
    for b in s.bytes() {
        out.push(b as int);
    }
    out
}

fn units_of(s: &str) -> Vec<int> {
    if is_ascii(s) {
        return ascii_units(s);
    }
    let mut out: Vec<int> = Vec::with_capacity(s.as_bytes().len());
    for c in s.chars() {
        let cp = c as int;
        if cp >= 0x10000 {
            let v = cp - 0x10000;
            out.push(0xd800 + (v >> 10));
            out.push(0xdc00 + (v & 0x3ff));
        } else {
            out.push(cp);
        }
    }
    out
}

pub fn from_units(u: &Vec<int>, a: int, b: int) -> String {
    let mut s = String::new();
    let mut i = a;
    while i < b {
        let c = u[i as usize];
        if c >= 0xd800 && c <= 0xdbff && i + 1 < b {
            let d = u[(i + 1) as usize];
            if d >= 0xdc00 && d <= 0xdfff {
                let cp = 0x10000 + ((c - 0xd800) << 10) + (d - 0xdc00);
                push_cp(&mut s, cp);
                i += 2;
                continue;
            }
        }
        push_cp(&mut s, c);
        i += 1;
    }
    s
}

/// from_units(u, a, b) where u = units(s): for an ASCII s (as many bytes
/// as units) the bytes [a, b) themselves.
pub fn sub_units(s: &str, u: &Vec<int>, a: int, b: int) -> String {
    let r: String = ranger::native!(rust: { sub_units_of(s, u, a, b) }, es6: "{s}.slice({a}, {b})");
    r
}

fn sub_units_of(s: &str, u: &Vec<int>, a: int, b: int) -> String {
    if s.as_bytes().len() == u.len() && a >= 0 && a <= b && (b as usize) <= u.len() {
        return String::from(&s[(a as usize)..(b as usize)]);
    }
    from_units(u, a, b)
}

pub fn push_cp(s: &mut String, cp: int) {
    match char::from_u32(cp as u32) {
        Some(c) => s.push(c),
        None => s.push('\u{fffd}'),
    }
}

pub fn from_unit(c: int) -> String {
    let mut s = String::new();
    push_cp(&mut s, c);
    s
}

/// The length in UTF-16 units: JavaScript's `length`.
pub fn len(s: &str) -> int {
    let n: int = ranger::native!(rust: { len_units(s) }, es6: "{s}.length");
    n
}

/// Rust: counted over the bytes (vectorised): every byte that starts a
/// character is one unit, and a 4-byte character one more.
#[ranger::target(rust)]
fn len_units(s: &str) -> int {
    if s.is_ascii() {
        return s.as_bytes().len() as int;
    }
    let mut n: usize = 0;
    for &b in s.as_bytes() {
        n += ((b & 0xc0) != 0x80) as usize + (b >= 0xf0) as usize;
    }
    n as int
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
fn len_units(s: &str) -> int {
    let mut n: int = 0;
    let mut ascii = true;
    for x in s.bytes() {
        if x >= 128 {
            ascii = false;
            break;
        }
    }
    if ascii {
        return s.as_bytes().len() as int;
    }
    for c in s.chars() {
        n += if (c as int) >= 0x10000 { 2 } else { 1 };
    }
    n
}

/// The unit at `i`, -1 out of range.
pub fn at(s: &str, i: int) -> int {
    let c: int = ranger::native!(
        rust: { at_unit(s, i) },
        es6: "(({i}) >= 0 && ({i}) < {s}.length ? {s}.charCodeAt({i}) : -1)",
    );
    c
}

fn at_unit(s: &str, i: int) -> int {
    if i < 0 {
        return -1;
    }
    if (i as usize) < s.as_bytes().len() && is_ascii(s) {
        return s.as_bytes()[i as usize] as int;
    }
    let u = units(s);
    if i >= u.len() as int {
        return -1;
    }
    u[i as usize]
}

/// Units [a, b) clamped to the string.
pub fn slice(s: &str, a: int, b: int) -> String {
    let r: String = ranger::native!(rust: { slice_units(s, a, b) }, es6: "{s}.substring(Math.max(0, {a}), Math.max(Math.max(0, {a}), {b}))");
    r
}

fn slice_units(s: &str, a: int, b: int) -> String {
    if a >= b {
        return String::new();
    }
    if is_ascii(s) {
        let n = s.as_bytes().len() as int;
        let x = if a < 0 { 0 } else if a > n { n } else { a };
        let y = if b < 0 { 0 } else if b > n { n } else { b };
        if x >= y {
            return String::new();
        }
        return String::from(&s[(x as usize)..(y as usize)]);
    }
    let u = units(s);
    let n = u.len() as int;
    let x = if a < 0 { 0 } else if a > n { n } else { a };
    let y = if b < 0 { 0 } else if b > n { n } else { b };
    from_units(&u, x, y)
}

/// The string iterator's steps (Vm::string_iter): the code point at
/// position `i` (Rust: a byte offset, else a UTF-16 position), how far it
/// moves the position, and the end position.
pub fn cp_at(s: &str, i: int) -> int {
    let r: int = ranger::native!(rust: { cp_at_byte(s, i) }, es6: "{s}.codePointAt({i})");
    r
}

#[ranger::target(rust)]
fn cp_at_byte(s: &str, i: int) -> int {
    match s[(i as usize)..].chars().next() {
        Some(c) => c as int,
        None => -1,
    }
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
fn cp_at_byte(s: &str, i: int) -> int {
    let mut p: int = 0;
    for c in s.chars() {
        if p == i {
            return c as int;
        }
        p += cp_width(c as int);
    }
    -1
}

pub fn cp_width(cp: int) -> int {
    let r: int = ranger::native!(rust: { cp_width_utf8(cp) }, es6: "({cp} > 0xffff ? 2 : 1)");
    r
}

fn cp_width_utf8(cp: int) -> int {
    if cp < 0x80 {
        1
    } else if cp < 0x800 {
        2
    } else if cp < 0x10000 {
        3
    } else {
        4
    }
}

pub fn cp_end(s: &str) -> int {
    let r: int = ranger::native!(rust: { s.as_bytes().len() as int }, es6: "{s}.length");
    r
}

/// The first `n` bytes of `s` (a string table prefix, Vm::sref; only Rust
/// appends in place, so only Rust has them).
#[ranger::target(rust)]
pub fn prefix(s: &str, n: int) -> &str {
    &s[..(n as usize)]
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn prefix(s: &str, _n: int) -> &str {
    s
}

/// The `n` bytes of `s` from byte `a` (a front view's text, Vm::sref_front;
/// Rust only, like prefix).
#[ranger::target(rust)]
pub fn mid(s: &str, a: int, n: int) -> &str {
    &s[(a as usize)..((a + n) as usize)]
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn mid(s: &str, _a: int, _n: int) -> &str {
    s
}

/// Chars [a, b) of `s` (a function's own text, sort_kind). Rust: when
/// the text before b is ASCII, chars are bytes and no decoding is needed.
#[ranger::target(rust)]
pub fn char_range(s: &str, a: int, b: int) -> String {
    let (ua, ub) = (a as usize, b as usize);
    if ub <= s.len() && s.as_bytes()[..ub].is_ascii() {
        return String::from(&s[ua..ub]);
    }
    s.chars().skip(ua).take(ub - ua).collect()
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn char_range(s: &str, a: int, b: int) -> String {
    let mut text = String::new();
    let mut i: int = 0;
    for c in s.chars() {
        if i >= b {
            break;
        }
        if i >= a {
            text.push(c);
        }
        i += 1;
    }
    text
}

/// Byte `i` of an ASCII string, i in range.
pub fn byte_at(s: &str, i: int) -> int {
    let r: int = ranger::native!(rust: { s.as_bytes()[i as usize] as int }, es6: "{s}.charCodeAt({i})");
    r
}

/// Bytes [a, b) of an ASCII string, a and b in range.
pub fn byte_slice(s: &str, a: int, b: int) -> String {
    let r: String = ranger::native!(rust: { String::from(&s[(a as usize)..(b as usize)]) }, es6: "{s}.substring({a}, {b})");
    r
}

/// The first `pat` at or after `from` in an ASCII string (pat ASCII too).
pub fn byte_index_of(s: &str, pat: &str, from: int) -> int {
    let n = s.as_bytes().len() as int;
    let start = if from < 0 { 0 } else { from };
    if start > n {
        return if pat.is_empty() { n } else { -1 };
    }
    let r: int = ranger::native!(
        rust: {
            match s[(start as usize)..].find(pat) {
                Some(p) => start + (p as int),
                None => -1,
            }
        },
        es6: "{s}.indexOf({pat}, {start})",
    );
    r
}

pub fn index_of(s: &str, pat: &str, from: int) -> int {
    let r: int = ranger::native!(rust: { index_of_units(s, pat, from) }, es6: "{s}.indexOf({pat}, Math.max(0, {from}))");
    r
}

fn index_of_units(s: &str, pat: &str, from: int) -> int {
    if is_ascii(s) && is_ascii(pat) {
        let n = s.as_bytes().len() as int;
        let start = if from < 0 { 0 } else { from };
        if start > n {
            return if pat.is_empty() { n } else { -1 };
        }
        return match s[(start as usize)..].find(pat) {
            Some(p) => start + (p as int),
            None => -1,
        };
    }
    let u = units(s);
    let p = units(pat);
    let n = u.len() as int;
    let m = p.len() as int;
    let mut i = if from < 0 { 0 } else { from };
    while i + m <= n {
        let mut k: int = 0;
        while k < m && u[(i + k) as usize] == p[k as usize] {
            k += 1;
        }
        if k == m {
            return i;
        }
        i += 1;
    }
    -1
}

pub fn last_index_of(s: &str, pat: &str, from: int) -> int {
    let u = units(s);
    let p = units(pat);
    let n = u.len() as int;
    let m = p.len() as int;
    let mut i = if from > n - m { n - m } else { from };
    while i >= 0 {
        let mut k: int = 0;
        while k < m && u[(i + k) as usize] == p[k as usize] {
            k += 1;
        }
        if k == m {
            return i;
        }
        i -= 1;
    }
    -1
}

/// Compares by UTF-16 units, as `<` on strings does.
pub fn compare(a: &str, b: &str) -> int {
    let r: int = ranger::native!(rust: { compare_units(a, b) }, es6: "({a} < {b} ? -1 : ({a} > {b} ? 1 : 0))");
    r
}

fn compare_units(a: &str, b: &str) -> int {
    if is_ascii(a) && is_ascii(b) {
        return if a < b {
            -1
        } else if a > b {
            1
        } else {
            0
        };
    }
    let x = units(a);
    let y = units(b);
    let n = if x.len() < y.len() { x.len() } else { y.len() };
    let mut i: usize = 0;
    while i < n {
        if x[i] != y[i] {
            return if x[i] < y[i] { -1 } else { 1 };
        }
        i += 1;
    }
    if x.len() < y.len() {
        -1
    } else if x.len() > y.len() {
        1
    } else {
        0
    }
}

fn upper_special(c: char) -> String {
    let pairs = ["ßSS", "ŉ\u{2bc}N", "ǰJ\u{30c}", "ﬀFF", "ﬁFI", "ﬂFL", "ﬃFFI", "ﬄFFL", "ﬅST", "ﬆST", "ևԵՒ"];
    for p in pairs.iter() {
        let cs = p.chars().collect::<Vec<char>>();
        if cs[0] == c {
            let mut r = String::new();
            let mut k: usize = 1;
            while k < cs.len() {
                r.push(cs[k]);
                k += 1;
            }
            return r;
        }
    }
    String::new()
}

/// An ASCII `s` in upper (or lower) case; "" when `s` is not ASCII.
#[ranger::target(rust)]
fn ascii_case(s: &str, upper: bool) -> String {
    if !s.is_ascii() {
        return String::new();
    }
    if upper {
        s.to_ascii_uppercase()
    } else {
        s.to_ascii_lowercase()
    }
}

/// The other targets map every letter.
#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
fn ascii_case(s: &str, upper: bool) -> String {
    String::new()
}

/// Whether `s` is ASCII with no capital letter (its own lower case).
#[ranger::target(rust)]
pub fn ascii_lower(s: &str) -> bool {
    s.is_ascii() && !s.bytes().any(|b| b.is_ascii_uppercase())
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn ascii_lower(s: &str) -> bool {
    false
}

pub fn to_upper(s: &str) -> String {
    let a = ascii_case(s, true);
    if !a.is_empty() {
        return a;
    }
    let mut out = String::new();
    for c in s.chars() {
        // SpecialCasing's unconditional mappings to more than one letter,
        // which not every target's own case mapping writes
        let special = upper_special(c);
        if !special.is_empty() {
            out.push_str(special.as_str());
            continue;
        }
        out.push_str(c.to_uppercase().to_string().as_str());
    }
    out
}

pub fn to_lower(s: &str) -> String {
    // ASCII: no final sigma or dotted capital I to care about
    let a = ascii_case(s, false);
    if !a.is_empty() {
        return a;
    }
    let mut out = String::new();
    let cs = s.chars().collect::<Vec<char>>();
    let n = cs.len();
    let mut i: usize = 0;
    while i < n {
        let c = cs[i];
        if c == 'Σ' {
            // final sigma: a letter before, none after
            let before = i > 0 && cs[i - 1].is_alphabetic();
            let after = i + 1 < n && cs[i + 1].is_alphabetic();
            out.push(if before && !after { 'ς' } else { 'σ' });
            i += 1;
            continue;
        }
        if c == 'İ' {
            out.push_str("i\u{307}");
            i += 1;
            continue;
        }
        out.push_str(c.to_lowercase().to_string().as_str());
        i += 1;
    }
    out
}

/// The first byte of `s`, -1 for "".
pub fn first_byte(s: &str) -> int {
    if s.as_bytes().len() == 0 {
        return -1;
    }
    s.as_bytes()[0] as int
}

/// A canonical array index ("0", "17"; not "017" or "-1"), or -1.
pub fn array_index(s: &str) -> int {
    let n = s.as_bytes().len();
    // up to 9 digits: the value stays inside a 32-bit int
    if n == 0 || n > 9 {
        return -1;
    }
    if s.as_bytes()[0] == 48 && n > 1 {
        return -1;
    }
    let mut v: int = 0;
    for x in s.bytes() {
        if x < 48 || x > 57 {
            return -1;
        }
        v = v * 10 + ((x as int) - 48);
    }
    v
}

/// A hash of a string's text (Map / Set string keys): 31 bits, >= 0.
#[ranger::target(rust)]
pub fn str_hash(s: &str) -> int {
    let b = s.as_bytes();
    let mut h: u64 = b.len() as u64;
    let mut i: usize = 0;
    while i + 8 <= b.len() {
        let w = u64::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3], b[i + 4], b[i + 5], b[i + 6], b[i + 7]]);
        h = (h.rotate_left(5) ^ w).wrapping_mul(0x517cc1b727220a95);
        i += 8;
    }
    while i < b.len() {
        h = (h.rotate_left(5) ^ (b[i] as u64)).wrapping_mul(0x517cc1b727220a95);
        i += 1;
    }
    (h >> 33) as int
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn str_hash(s: &str) -> int {
    let mut h: int = 0;
    for c in s.chars() {
        h = (h * 31 + (c as int)) % 1073741789;
    }
    h
}
