// SPDX-License-Identifier: AGPL-3.0-or-later
//! The native half of Intl (ECMA-402); the rest is JavaScript in
//! `prelude.rs` and `INTL` here. Three functions, which the prelude takes off the global
//! object before any script runs:
//!
//! - `__cerIntlTable(name)`: a table of `intl_data` as an array (of
//!   strings or numbers), for the number, date, plural and list formatting
//!   written in the prelude;
//! - `__cerIntlCollate(a, b, tag, flags)`: Intl.Collator's comparison and
//!   localeCompare's, here because a sort calls it n log n times;
//! - `__cerIntlSource()`: INTL below compiled, a function the prelude calls
//!   with its helpers the first time a script reaches for Intl.
//!
//! The collation is ComponentEngine's (engine/ComponentEngine.rgr,
//! D-COLLATE): three levels in the shape of UTS #10 — base letters, then
//! accents, then case — over the decomposed text, with the per-locale
//! tailorings of UnicodeTailor (Swedish å ä ö after z, Czech ch after h,
//! Spanish ñ after n, …) and, at full strength, code units as the last
//! word so that two different strings never compare equal.

use ranger::prelude::*;
use std::collections::HashMap;

use crate::value::*;
use crate::vm::*;

pub const NF_INTL_TABLE: int = 960;
pub const NF_INTL_COLLATE: int = 961;
pub const NF_INTL_SOURCE: int = 962;

/// flags of __cerIntlCollate
pub const CO_ACCENT: int = 1;
pub const CO_CASE: int = 2;
pub const CO_NUMERIC: int = 4;
pub const CO_IGNORE_PUNCT: int = 8;
pub const CO_UPPER_FIRST: int = 16;
/// Turkish and Azerbaijani casing: I is the capital of ı, İ of i
pub const CO_TURKISH: int = 32;

/// a primary weight is (100000 + code point) * 1000, or a punctuation rank
/// * 1000: the gaps are where a tailoring puts its letters
const LETTER_BASE: int = 100000;
const SCALE: int = 1000;

pub struct Intl {
    pub loaded: bool,
    pub punct: Vec<int>,
    pub expand: Vec<int>,
    pub marks: Vec<int>,
    pub bases: Vec<int>,
    pub tailor_locales: Vec<int>,
    pub tailor_entries: Vec<int>,
    /// where each code point's entry is in `bases` and `expand`
    pub base_at: HashMap<int, int>,
    pub expand_at: HashMap<int, int>,
    /// the tag the tailoring in use was selected for
    pub tag: String,
    /// the tailoring in use: where its entries start, and how many
    pub tailor_off: int,
    pub tailor_count: int,
}

impl Intl {
    pub fn new() -> Intl {
        Intl {
            loaded: false,
            punct: Vec::new(),
            expand: Vec::new(),
            marks: Vec::new(),
            bases: Vec::new(),
            tailor_locales: Vec::new(),
            tailor_entries: Vec::new(),
            base_at: HashMap::new(),
            expand_at: HashMap::new(),
            tag: String::from("-"),
            tailor_off: 0,
            tailor_count: 0,
        }
    }
}

fn numbers(s: &str) -> Vec<int> {
    let mut out: Vec<int> = Vec::new();
    let mut v: int = 0;
    let mut any = false;
    for b in s.bytes() {
        if b >= 48 && b <= 57 {
            v = v * 10 + ((b as int) - 48);
            any = true;
        } else {
            if any {
                out.push(v);
            }
            v = 0;
            any = false;
        }
    }
    if any {
        out.push(v);
    }
    out
}

fn load(t: &mut Intl) {
    if t.loaded {
        return;
    }
    t.loaded = true;
    t.punct = numbers(crate::intl_data::punct_runs().as_str());
    t.expand = numbers(crate::intl_data::expansions().as_str());
    t.marks = numbers(crate::intl_data::mark_ranks().as_str());
    t.bases = numbers(crate::intl_data::base_letters().as_str());
    t.tailor_locales = numbers(crate::intl_data::tailor_locales().as_str());
    t.tailor_entries = numbers(crate::intl_data::tailor_entries().as_str());
    let mut i: usize = 0;
    while i + 2 < t.bases.len() {
        t.base_at.insert(t.bases[i], i as int);
        i += 3 + (t.bases[i + 2] as usize);
    }
    let mut k: usize = 0;
    while k + 1 < t.expand.len() {
        t.expand_at.insert(t.expand[k], k as int);
        k += 2 + (t.expand[k + 1] as usize);
    }
}

/// Punctuation and symbols sort before digits and letters, in an order of
/// their own; -1 for anything else.
fn punct_rank(t: &Intl, cp: int) -> int {
    let mut lo: int = 0;
    let mut hi: int = (t.punct.len() as int) / 3 - 1;
    while lo <= hi {
        let mid = (lo + hi) / 2;
        let b = (mid * 3) as usize;
        if cp < t.punct[b] {
            hi = mid - 1;
        } else if cp > t.punct[b + 1] {
            lo = mid + 1;
        } else {
            return t.punct[b + 2] + (cp - t.punct[b]);
        }
    }
    -1
}

/// a combining mark's secondary rank, or -1
fn mark_rank(t: &Intl, cp: int) -> int {
    let mut lo: int = 0;
    let mut hi: int = (t.marks.len() as int) / 2 - 1;
    while lo <= hi {
        let mid = (lo + hi) / 2;
        let b = (mid * 2) as usize;
        if cp < t.marks[b] {
            hi = mid - 1;
        } else if cp > t.marks[b] {
            lo = mid + 1;
        } else {
            return t.marks[b + 1];
        }
    }
    -1
}

/// (secondary rank, base points…) of a letter filed under a simpler one
/// (ø under o), empty when there is none
fn base_for(t: &Intl, cp: int) -> Vec<int> {
    let mut out: Vec<int> = Vec::new();
    if let Some(at) = t.base_at.get(&cp) {
        let i = *at as usize;
        let cnt = t.bases[i + 2] as usize;
        out.push(t.bases[i + 1]);
        let mut k: usize = 0;
        while k < cnt {
            out.push(t.bases[i + 3 + k]);
            k += 1;
        }
    }
    out
}

/// the sequence a character compares as (ß as ss), empty for itself
fn expand_for(t: &Intl, cp: int) -> Vec<int> {
    let mut out: Vec<int> = Vec::new();
    if let Some(at) = t.expand_at.get(&cp) {
        let i = *at as usize;
        let cnt = t.expand[i + 1] as usize;
        let mut k: usize = 0;
        while k < cnt {
            out.push(t.expand[i + 2 + k]);
            k += 1;
        }
    }
    out
}

/// Selects the tailoring of a BCP 47 tag: the exact tag ("sr-latn"), else
/// its language ("sv-FI" → "sv"), else none.
fn select_locale(t: &mut Intl, tag: &str) {
    if t.tag.as_str() == tag {
        return;
    }
    t.tag = String::from(tag);
    t.tailor_off = 0;
    t.tailor_count = 0;
    if tag.is_empty() {
        return;
    }
    let want = crate::jsstr::to_lower(tag);
    let mut short = want.clone();
    let dash = crate::jsstr::index_of(want.as_str(), "-", 0);
    if dash > 0 {
        short = crate::jsstr::slice(want.as_str(), 0, dash);
    }
    let mut pass: int = 0;
    while pass < 2 {
        let probe = if pass == 0 { want.clone() } else { short.clone() };
        let mut cps: Vec<int> = Vec::new();
        for c in probe.chars() {
            cps.push(c as int);
        }
        let n = t.tailor_locales.len();
        let mut i: usize = 0;
        while i < n {
            let tlen = t.tailor_locales[i] as usize;
            let mut same = tlen == cps.len();
            let mut k: usize = 0;
            while same && k < tlen {
                if t.tailor_locales[i + 1 + k] != cps[k] {
                    same = false;
                }
                k += 1;
            }
            if same {
                t.tailor_off = t.tailor_locales[i + 1 + tlen];
                t.tailor_count = t.tailor_locales[i + 2 + tlen];
                return;
            }
            i += 3 + tlen;
        }
        pass += 1;
    }
}

/// The longest tailored element at `i` of the lowercased points, as
/// (weight, length); empty when none matches. Longest wins, so Hungarian
/// dzs beats dz.
fn tailor_match(t: &Intl, cps: &Vec<int>, i: usize) -> Vec<int> {
    let mut out: Vec<int> = Vec::new();
    if t.tailor_count == 0 {
        return out;
    }
    let n = cps.len();
    let mut best_len: usize = 0;
    let mut best_w: int = 0;
    let mut p = t.tailor_off as usize;
    let mut k: int = 0;
    while k < t.tailor_count {
        let anchor = t.tailor_entries[p];
        let off = t.tailor_entries[p + 1];
        let elen = t.tailor_entries[p + 2] as usize;
        if elen > best_len && i + elen <= n {
            let mut ok = true;
            let mut q: usize = 0;
            while ok && q < elen {
                if cps[i + q] != t.tailor_entries[p + 3 + q] {
                    ok = false;
                }
                q += 1;
            }
            if ok {
                best_len = elen;
                best_w = (LETTER_BASE + anchor) * SCALE + off;
            }
        }
        p += 3 + elen;
        k += 1;
    }
    if best_len > 0 {
        out.push(best_w);
        out.push(best_len as int);
    }
    out
}

fn lower_cp(cp: int) -> int {
    if cp >= 65 && cp <= 90 {
        return cp + 32;
    }
    if cp < 128 {
        return cp;
    }
    let mut s = String::new();
    crate::jsstr::push_cp(&mut s, cp);
    let low = crate::jsstr::to_lower(s.as_str());
    for c in low.chars() {
        return c as int;
    }
    cp
}

fn is_digit(cp: int) -> bool {
    cp >= 48 && cp <= 57
}

/// The three sort keys of a string, laid end to end as (primary count,
/// primaries…, secondary count, secondaries…, tertiary count, tertiaries…).
fn keys(t: &Intl, norm: &mut crate::normalize::Norm, s: &str, flags: int) -> Vec<int> {
    let mut prim: Vec<int> = Vec::new();
    let mut sec: Vec<int> = Vec::new();
    let mut ter: Vec<int> = Vec::new();
    let d = crate::normalize::normalize(norm, s, false);
    let mut cps: Vec<int> = Vec::new();
    let mut low: Vec<int> = Vec::new();
    let turkish = (flags & CO_TURKISH) != 0;
    for c in d.chars() {
        let cp = c as int;
        if turkish && cp == 0x307 && !cps.is_empty() && cps[cps.len() - 1] == 0x49 {
            // İ, decomposed: I and a dot above, the capital of i
            let last = low.len() - 1;
            low[last] = 0x69;
            continue;
        }
        cps.push(cp);
        low.push(if turkish && cp == 0x49 { 0x131 } else { lower_cp(cp) });
    }
    let upper_first = (flags & CO_UPPER_FIRST) != 0;
    let n = cps.len();
    let mut i: usize = 0;
    while i < n {
        let cp = cps[i];
        let lo = low[i];
        let upper: int = if (lo != cp) != upper_first { 1 } else { 0 };
        // a tailored element first: it may cover several points (Czech ch,
        // Swedish a + ring), and none of them reaches the steps below
        let tm = tailor_match(t, &low, i);
        if !tm.is_empty() {
            prim.push(tm[0]);
            let len = tm[1] as usize;
            let mut k: usize = 0;
            while k < len {
                ter.push(if k == 0 { upper } else { 0 });
                k += 1;
            }
            i += len;
            continue;
        }
        if (flags & CO_NUMERIC) != 0 && is_digit(cp) {
            // a run of digits by value: its length (leading zeros off)
            // in the gap after '0', then the digits
            let mut j = i;
            while j < n && cps[j] == 48 {
                j += 1;
            }
            let start = j;
            while j < n && is_digit(cps[j]) {
                j += 1;
            }
            if j == start && start > i {
                // all zeros: the number 0
                prim.push((LETTER_BASE + 48) * SCALE + 1);
                prim.push((LETTER_BASE + 48) * SCALE);
            } else {
                let mut end = start;
                while end < n && is_digit(cps[end]) {
                    end += 1;
                }
                prim.push((LETTER_BASE + 48) * SCALE + ((end - start) as int));
                let mut k = start;
                while k < end {
                    prim.push((LETTER_BASE + cps[k]) * SCALE);
                    k += 1;
                }
                j = end;
            }
            ter.push(0);
            i = j;
            continue;
        }
        let mr = mark_rank(t, cp);
        if mr >= 0 {
            sec.push(mr);
            i += 1;
            continue;
        }
        if lo == 0x131 {
            // dotless ı is a letter of its own, after every i
            prim.push((LETTER_BASE + 105) * SCALE + 900);
            ter.push(upper);
            i += 1;
            continue;
        }
        let mut points: Vec<int> = Vec::new();
        let mut base = base_for(t, cp);
        if base.is_empty() && lo != cp {
            base = base_for(t, lo);
        }
        if !base.is_empty() {
            sec.push(base[0]);
            let mut k: usize = 1;
            while k < base.len() {
                points.push(base[k]);
                k += 1;
            }
        } else {
            let ex = expand_for(t, cp);
            if !ex.is_empty() {
                points = ex;
            } else {
                points.push(lo);
            }
        }
        let mut k: usize = 0;
        while k < points.len() {
            let pr = punct_rank(t, points[k]);
            if pr >= 0 {
                if (flags & CO_IGNORE_PUNCT) == 0 {
                    prim.push(pr * SCALE);
                    ter.push(upper);
                }
            } else {
                prim.push((LETTER_BASE + points[k]) * SCALE);
                ter.push(upper);
            }
            k += 1;
        }
        i += 1;
    }
    let mut out: Vec<int> = Vec::new();
    out.push(prim.len() as int);
    out.extend(prim);
    out.push(sec.len() as int);
    out.extend(sec);
    out.push(ter.len() as int);
    out.extend(ter);
    out
}

/// one level of two keys, from where it starts in each
fn level_cmp(ka: &Vec<int>, ia: usize, kb: &Vec<int>, ib: usize) -> int {
    let na = ka[ia] as usize;
    let nb = kb[ib] as usize;
    let m = if na < nb { na } else { nb };
    let mut i: usize = 0;
    while i < m {
        let x = ka[ia + 1 + i];
        let y = kb[ib + 1 + i];
        if x < y {
            return -1;
        }
        if x > y {
            return 1;
        }
        i += 1;
    }
    if na < nb {
        return -1;
    }
    if na > nb {
        return 1;
    }
    0
}

fn without_punct(t: &Intl, s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if punct_rank(t, c as int) < 0 {
            out.push(c);
        }
    }
    out
}

pub fn collate(t: &mut Intl, norm: &mut crate::normalize::Norm, a: &str, b: &str, tag: &str, flags: int) -> int {
    if a == b {
        return 0;
    }
    load(t);
    select_locale(t, tag);
    let ka = keys(t, norm, a, flags);
    let kb = keys(t, norm, b, flags);
    let c = level_cmp(&ka, 0, &kb, 0);
    if c != 0 {
        return c;
    }
    let sa = 1 + (ka[0] as usize);
    let sb = 1 + (kb[0] as usize);
    if (flags & CO_ACCENT) != 0 {
        let c2 = level_cmp(&ka, sa, &kb, sb);
        if c2 != 0 {
            return c2;
        }
    }
    if (flags & CO_CASE) != 0 {
        let ta = sa + 1 + (ka[sa] as usize);
        let tb = sb + 1 + (kb[sb] as usize);
        let c3 = level_cmp(&ka, ta, &kb, tb);
        if c3 != 0 {
            return c3;
        }
    }
    // equal at every level asked for: a cut-short comparison answers
    // equal, which is what sensitivity 'base' is for; a full one falls
    // back to code units, since a sort needs a strict total order
    if (flags & CO_ACCENT) == 0 || (flags & CO_CASE) == 0 {
        return 0;
    }
    // ignoring punctuation, the code units of what is left
    let r = if (flags & CO_IGNORE_PUNCT) != 0 {
        let x = without_punct(t, a);
        let y = without_punct(t, b);
        crate::jsstr::compare(x.as_str(), y.as_str())
    } else {
        crate::jsstr::compare(a, b)
    };
    if r < 0 {
        return -1;
    }
    if r > 0 {
        return 1;
    }
    0
}

impl Vm {
    pub fn setup_intl(&mut self) {
        let g = self.global;
        self.method(g, "__cerIntlTable", NF_INTL_TABLE, 1);
        self.method(g, "__cerIntlCollate", NF_INTL_COLLATE, 4);
        self.method(g, "__cerIntlSource", NF_INTL_SOURCE, 0);
    }

    pub fn call_native_intl(&mut self, id: int, args: Vec<Val>) -> Val {
        if id == NF_INTL_SOURCE {
            return self.eval_source(INTL, false, false);
        }
        let mut s: Vec<String> = Vec::new();
        let mut i: usize = 0;
        while i < args.len() && i < 3 {
            let v = self.to_string(&args[i]);
            s.push(v);
            i += 1;
        }
        if self.throwing {
            return Val::Undef;
        }
        while s.len() < 3 {
            s.push(String::new());
        }
        if id == NF_INTL_TABLE {
            let name = s[0].clone();
            let text = crate::intl_data::table(name.as_str());
            let mut elems: Vec<Val> = Vec::new();
            if crate::intl_data::is_strings(name.as_str()) {
                for line in text.split('\n') {
                    elems.push(str_val(line));
                }
                elems.pop();
            } else {
                for v in numbers(text.as_str()) {
                    elems.push(Val::Num(v as double));
                }
            }
            let a = self.new_array(elems);
            return Val::Obj(a);
        }
        let flags = if args.len() > 3 { self.to_number(&args[3]) as int } else { CO_ACCENT | CO_CASE };
        let r = collate(&mut self.intl, &mut self.norm, s[0].as_str(), s[1].as_str(), s[2].as_str(), flags);
        Val::Num(r as double)
    }
}

/// Intl's JavaScript: NumberFormat, PluralRules, DateTimeFormat, Collator,
/// RelativeTimeFormat, ListFormat and the toLocale… methods over the tables
/// of `intl_data`, as one function of the prelude's helpers.
pub const INTL: &str = r#"(function (hide, tag, isObject, create, defineProperty, toIntegerOrInfinity, DP, dateOk, intlTable, intlCollate) {
var LD = null;
function ld() {
  if (LD) return LD;
  var d = {};
  ['supplement', 'tags', 'num_strings', 'num_ints', 'names', 'pattern_ints', 'pattern_strings', 'currency_strings', 'currency_ints', 'currency_count',
    'plural_keys', 'plural_entries', 'plural_pairs', 'list_separators', 'tailor_locales'].forEach(function (n) { d[n] = intlTable(n); });
  // where each date pattern starts, six per locale
  d.patAt = [];
  for (var p = 0, pi = d.pattern_ints; p < pi.length; p += 1 + pi[p] * 3) d.patAt.push(p);
  d.keyId = create(null);
  for (var k = 0; k < d.plural_keys.length; k++) d.keyId[d.plural_keys[k]] = k;
  d.plurals = [];
  // the tags with a collation tailoring (UnicodeTailor), which a Collator
  // supports whether or not the formatting tables carry them
  d.tailored = [];
  for (var t = 0, tl = d.tailor_locales; t < tl.length; t += 3 + tl[t]) {
    var s = '';
    for (var c = 0; c < tl[t]; c++) s += String.fromCharCode(tl[t + 1 + c]);
    d.tailored.push(s);
  }
  LD = d;
  return d;
}
var NARROW_CURRENCY = { USD: '$', EUR: '€', GBP: '£', JPY: '¥', CNY: '¥', KRW: '₩', INR: '₹', RUB: '₽', SEK: 'kr', NOK: 'kr', DKK: 'kr', ISK: 'kr', PLN: 'zł', CZK: 'Kč', HUF: 'Ft', TRY: '₺', UAH: '₴', ILS: '₪', THB: '฿', VND: '₫', BRL: 'R$', CAD: '$', AUD: '$', CHF: 'CHF' };
var CURRENCY_DIGITS = { JPY: 0, KRW: 0, ISK: 0, VND: 0, CLP: 0, PYG: 0, UGX: 0, XAF: 0, XOF: 0, BHD: 3, KWD: 3, OMR: 3, JOD: 3, TND: 3 };
function canonLocale(tag) {
  if (typeof tag !== 'string' && !(isObject(tag) && typeof tag.toString === 'function')) throw new TypeError('Incorrect locale information provided');
  var s = String(tag);
  if (!/^[A-Za-z]{2,3}([-_][A-Za-z0-9]{1,8})*$/.test(s) && !/^[A-Za-z]{5,8}([-_][A-Za-z0-9]{1,8})*$/.test(s)) throw new RangeError('Incorrect locale information provided');
  var parts = s.replace(/_/g, '-').split('-');
  parts[0] = parts[0].toLowerCase();
  var ext = false;
  for (var i = 1; i < parts.length; i++) {
    if (parts[i].length === 1) ext = true;
    if (ext) parts[i] = parts[i].toLowerCase();
    else if (parts[i].length === 2) parts[i] = parts[i].toUpperCase();
    else if (parts[i].length === 4 && /^[A-Za-z]+$/.test(parts[i])) parts[i] = parts[i].charAt(0).toUpperCase() + parts[i].slice(1).toLowerCase();
    else parts[i] = parts[i].toLowerCase();
  }
  return parts.join('-');
}
function localeList(locales) {
  if (locales === undefined) return [];
  if (typeof locales === 'string') return [canonLocale(locales)];
  var out = [], o = Object(locales), n = toIntegerOrInfinity(o.length);
  for (var i = 0; i < n; i++) if (i in o) { var c = canonLocale(o[i]); if (out.indexOf(c) < 0) out.push(c); }
  return out;
}
// a tag without its extensions (-u-…, -x-…)
function baseTag(tag) {
  var parts = tag.split('-');
  for (var i = 1; i < parts.length; i++) if (parts[i].length === 1) return parts.slice(0, i).join('-');
  return tag;
}
// The row of the tables a tag reads: the tag itself ("en-GB", "pt-BR"),
// language and region without the script, Traditional Chinese for zh-Hant,
// Norwegian Bokmål for no, the language; -1 when none is carried.
function localeIndex(tag) {
  var tags = ld().tags, want = baseTag(tag).toLowerCase(), i = tags.indexOf(want);
  if (i >= 0) return i;
  var parts = want.split('-'), lang = parts[0];
  if (lang === 'no' || lang === 'nn') lang = 'nb';
  if (lang === 'zh' && (parts.indexOf('hant') > 0 || parts.indexOf('tw') > 0 || parts.indexOf('hk') > 0 || parts.indexOf('mo') > 0)) return tags.indexOf('zh-tw');
  for (var k = 1; k < parts.length; k++) if (parts[k].length === 2 && tags.indexOf(lang + '-' + parts[k]) >= 0) return tags.indexOf(lang + '-' + parts[k]);
  return tags.indexOf(lang);
}
function collatorSupports(tag) {
  if (localeIndex(tag) >= 0) return true;
  var want = baseTag(tag).toLowerCase(), t = ld().tailored;
  return t.indexOf(want) >= 0 || t.indexOf(want.split('-')[0]) >= 0;
}
// the first requested tag there are tables for, without extensions, else en-US
function pickLocale(locales, supports) {
  var list = localeList(locales);
  for (var i = 0; i < list.length; i++) if ((supports || function (t) { return localeIndex(t) >= 0; })(list[i])) return baseTag(list[i]);
  return 'en-US';
}
function option(o, name, allowed, dflt) {
  var v = o[name];
  if (v === undefined) return dflt;
  v = typeof dflt === 'boolean' ? Boolean(v) : String(v);
  if (allowed && allowed.indexOf(v) < 0) throw new RangeError('Value ' + v + ' out of range for Intl options property ' + name);
  return v;
}
function numOption(o, name, min, max, dflt) {
  var v = o[name];
  if (v === undefined) return dflt;
  v = Number(v);
  if (v !== v || v < min || v > max) throw new RangeError(name + ' value is out of range.');
  return Math.floor(v);
}
function optionsObject(o) { return o === undefined ? create(null) : Object(o); }
function supportedLocalesOf(locales, supports) { return localeList(locales).filter(supports || function (l) { return localeIndex(l) >= 0; }); }
function intlBrand(o, brand, what) { if (!isObject(o) || o.__intl !== brand) throw new TypeError('Method ' + what + ' called on incompatible receiver'); }
function str22(loc, k) { return ld().num_strings[loc * 22 + k]; }
function int5(loc, k) { return ld().num_ints[loc * 5 + k]; }
// the host-ICU supplement: 0 NaN, 1 H or HH, 2 AM|PM, 3 and 4 the short and
// medium date patterns, 5 the stand-alone month names
function sup(loc, k) { return ld().supplement[loc * 6 + k]; }

// ---- numbers: the digits as a decimal string, rounded half away from zero
// as ICU rounds them -- on the shortest representation of the double, so
// 1.005 to two places is 1.01, where toFixed answers 1.00
function decimalDigits(a) {
  var s = String(a), e = s.indexOf('e'), exp = 0;
  if (e >= 0) { exp = Number(s.slice(e + 1)); s = s.slice(0, e); }
  var dot = s.indexOf('.');
  var digits = dot < 0 ? s : s.slice(0, dot) + s.slice(dot + 1);
  var point = (dot < 0 ? s.length : dot) + exp;
  var lead = 0;
  while (lead < digits.length - 1 && digits.charAt(lead) === '0') lead++;
  return { d: digits.slice(lead), p: point - lead };
}
// keep `keep` digits of x (x.d, x.p): round, carry
function roundDigits(x, keep) {
  var d = x.d, p = x.p;
  if (keep < 0) return { d: '0', p: 1 };
  if (d.length <= keep) return x;
  var up = d.charCodeAt(keep) >= 53;
  d = d.slice(0, keep);
  if (up) {
    var i = d.length - 1;
    while (i >= 0 && d.charAt(i) === '9') i--;
    if (i < 0) { d = '1' + d.replace(/9/g, '0'); p++; }
    else d = d.slice(0, i) + String.fromCharCode(d.charCodeAt(i) + 1) + d.slice(i + 1).replace(/9/g, '0');
  }
  if (d === '') { d = '0'; p = 1; }
  return { d: d, p: p };
}
// integer and fraction strings of |x| under the format's digit options
function numberDigits(o, a, big) {
  var x = big !== undefined ? { d: big, p: big.length } : a === 0 ? { d: '0', p: 1 } : decimalDigits(a);
  var minF = o.minimumFractionDigits, maxF = o.maximumFractionDigits;
  if (o.maximumSignificantDigits !== undefined) {
    x = roundDigits(x, o.maximumSignificantDigits);
    minF = Math.max(0, (o.minimumSignificantDigits || 1) - x.p);
    maxF = 1000;
  } else x = roundDigits(x, x.p + maxF);
  var d = x.d, p = x.p, int, frac;
  if (p <= 0) { int = '0'; frac = new Array(1 - p).join('0') + d; }
  else if (p >= d.length) { int = d + new Array(p - d.length + 1).join('0'); frac = ''; }
  else { int = d.slice(0, p); frac = d.slice(p); }
  if (frac.length > maxF) frac = frac.slice(0, maxF);
  var end = frac.length;
  while (end > minF && frac.charAt(end - 1) === '0') end--;
  frac = frac.slice(0, end);
  while (frac.length < minF) frac += '0';
  int = int.replace(/^0+(?=\d)/, '');
  while (int.length < o.minimumIntegerDigits) int = '0' + int;
  return [int, frac];
}
// Grouping from the right, the first group of its own size: South Asian
// locales group 3 then 2. Spanish and Italian do not group four digits.
function groupInteger(int, loc, mode) {
  var sep = str22(loc, 1);
  if (mode === false || sep === '') return [int];
  var primary = int5(loc, 0), secondary = int5(loc, 1), minGroup = mode === 'min2' ? 2 : mode === 'always' ? 1 : int5(loc, 2);
  if (int.length < primary + minGroup) return [int];
  var groups = [int.slice(int.length - primary)], end = int.length - primary;
  while (end > 0) { groups.unshift(int.slice(Math.max(0, end - secondary), end)); end -= secondary; }
  return groups;
}
function currencyRow(loc, code) {
  var d = ld(), per = d.currency_count[0];
  for (var i = 0; i < per; i++) if (d.currency_strings[(loc * per + i) * 2] === code) return loc * per + i;
  return -1;
}
function currencySymbol(o, loc) {
  if (o.currencyDisplay === 'code' || o.currencyDisplay === 'name') return o.currency;
  if (o.currencyDisplay === 'narrowSymbol' && NARROW_CURRENCY[o.currency]) return NARROW_CURRENCY[o.currency];
  var r = currencyRow(loc, o.currency);
  return r < 0 ? o.currency : ld().currency_strings[r * 2 + 1];
}
// The prefix and suffix around the digits, as the locale records them:
// German puts a space before the percent sign, Dutch the minus after the
// currency symbol. The currency patterns were recorded with U+00A4 in the
// symbol's place, in two variants because the spacing depends on whether
// the symbol is letters.
function numberAffix(o, loc, neg, sym) {
  var off = 4;
  if (o.style === 'percent') off = 8;
  if (o.style === 'currency') {
    var r = currencyRow(loc, o.currency), letters = o.currencyDisplay === 'code' || o.currencyDisplay === 'name' || (r < 0 ? true : ld().currency_ints[r] === 1);
    if (o.currencyDisplay === 'narrowSymbol') letters = /^[A-Za-z]/.test(sym);
    off = letters ? 12 : 16;
  }
  if (neg) off += 2;
  var pre = str22(loc, off), suf = str22(loc, off + 1);
  // CLDR's currency spacing: a symbol of letters next to the digits is set
  // apart by a no-break space (SEK 1,234.50, not SEK1,234.50)
  if (o.style === 'currency' && /[A-Za-z]$/.test(sym) && /¤$/.test(pre)) pre += '\u00a0';
  if (o.style === 'currency' && /^[A-Za-z]/.test(sym) && /^¤/.test(suf)) suf = '\u00a0' + suf;
  return [pre, suf];
}
// an affix split into its signs, symbol and literals
function affixParts(out, affix, o, loc, sym, plus) {
  var marks = [[str22(loc, 2), plus ? 'plusSign' : 'minusSign']];
  if (o.style === 'percent') marks.push([str22(loc, 3), 'percentSign']);
  if (o.style === 'currency') marks.push(['¤', 'currency']);
  var rest = affix;
  while (rest) {
    var at = -1, m = null;
    for (var i = 0; i < marks.length; i++) {
      var k = marks[i][0] ? rest.indexOf(marks[i][0]) : -1;
      if (k >= 0 && (at < 0 || k < at)) { at = k; m = marks[i]; }
    }
    if (!m) { out.push({ type: 'literal', value: rest }); break; }
    if (at > 0) out.push({ type: 'literal', value: rest.slice(0, at) });
    out.push({ type: m[1], value: m[1] === 'currency' ? sym : m[1] === 'plusSign' ? '+' : m[0] });
    rest = rest.slice(at + m[0].length);
  }
}
function NumberFormat(locales, options) {
  if (!(this instanceof NumberFormat) || this.__intl) return new NumberFormat(locales, options);
  var o = optionsObject(options);
  var loc = pickLocale(locales);
  var style = option(o, 'style', ['decimal', 'percent', 'currency', 'unit'], 'decimal');
  var currency = o.currency === undefined ? undefined : String(o.currency);
  if (currency !== undefined && !/^[A-Za-z]{3}$/.test(currency)) throw new RangeError('Invalid currency code : ' + currency);
  if (currency !== undefined) currency = currency.toUpperCase();
  if (style === 'currency' && currency === undefined) throw new TypeError('Currency code is required with currency style.');
  var unit = o.unit === undefined ? undefined : String(o.unit);
  if (style === 'unit' && unit === undefined) throw new TypeError('Unit is required with unit style.');
  var cd = currency !== undefined && CURRENCY_DIGITS[currency] !== undefined ? CURRENCY_DIGITS[currency] : 2;
  var minI = numOption(o, 'minimumIntegerDigits', 1, 21, 1);
  var minF = numOption(o, 'minimumFractionDigits', 0, 100, undefined);
  var maxF = numOption(o, 'maximumFractionDigits', 0, 100, undefined);
  var minS = numOption(o, 'minimumSignificantDigits', 1, 21, undefined);
  var maxS = numOption(o, 'maximumSignificantDigits', minS || 1, 21, undefined);
  if (minS !== undefined && maxS === undefined) maxS = 21;
  var dMin = style === 'currency' ? cd : 0, dMax = style === 'currency' ? cd : style === 'percent' ? 0 : 3;
  if (minF === undefined && maxF === undefined) { minF = dMin; maxF = dMax; }
  else if (minF === undefined) minF = Math.min(dMin, maxF);
  else if (maxF === undefined) maxF = Math.max(dMax, minF);
  else if (minF > maxF) throw new RangeError('maximumFractionDigits value is out of range.');
  var g = o.useGrouping;
  var r = {
    locale: loc, numberingSystem: 'latn', style: style, currency: style === 'currency' ? currency : undefined,
    currencyDisplay: style === 'currency' ? option(o, 'currencyDisplay', ['symbol', 'narrowSymbol', 'code', 'name'], 'symbol') : undefined,
    currencySign: style === 'currency' ? option(o, 'currencySign', ['standard', 'accounting'], 'standard') : undefined,
    unit: style === 'unit' ? unit : undefined, unitDisplay: style === 'unit' ? option(o, 'unitDisplay', ['short', 'narrow', 'long'], 'short') : undefined,
    minimumIntegerDigits: minI
  };
  if (maxS !== undefined) { r.minimumSignificantDigits = minS || 1; r.maximumSignificantDigits = maxS; }
  else { r.minimumFractionDigits = minF; r.maximumFractionDigits = maxF; }
  r.useGrouping = g === undefined || g === true || g === 'true' || g === 'auto' ? 'auto' : g === false || g === 'false' ? false : g === 'min2' ? 'min2' : g === 'always' ? 'always' : 'auto';
  r.notation = option(o, 'notation', ['standard', 'scientific', 'engineering', 'compact'], 'standard');
  r.signDisplay = option(o, 'signDisplay', ['auto', 'never', 'always', 'exceptZero', 'negative'], 'auto');
  r.roundingMode = 'halfExpand';
  hide(this, '__intl', 'NumberFormat');
  hide(this, '__o', r);
  hide(this, '__loc', localeIndex(loc) < 0 ? 0 : localeIndex(loc));
}
function numberParts(nf, x) {
  var o = nf.__o, loc = nf.__loc, parts = [];
  var big;
  if (typeof x === 'bigint') { big = String(x < 0 ? -x : x); x = Number(x); }
  else if (typeof x === 'string' && /^\s*-?\d+\s*$/.test(x) && x.trim().replace('-', '').length > 15) { big = x.trim().replace('-', '').replace(/^0+(?=\d)/, ''); x = Number(x); }
  else x = Number(x);
  var neg = x < 0 || (x === 0 && 1 / x < 0);
  var a = Math.abs(x);
  if (o.style === 'percent') { a = a * 100; if (big !== undefined) big += '00'; }
  var digits = a !== a || a === Infinity ? null : numberDigits(o, a, big);
  var zero = digits !== null && /^0*$/.test(digits[0] + digits[1]);
  // which sign shows: the negative affix carries the minus; a plus sign
  // takes the minus's place in it
  var sd = o.signDisplay, showNeg = false, plus = false;
  if (a !== a) neg = false;
  if (sd === 'auto') showNeg = neg;
  else if (sd === 'always') { showNeg = true; plus = !neg; }
  else if (sd === 'exceptZero') { if (!zero && a === a) { showNeg = true; plus = !neg; } }
  else if (sd === 'negative') showNeg = neg && !zero;
  var sym = o.style === 'currency' ? currencySymbol(o, loc) : '';
  var aff = numberAffix(o, loc, showNeg, sym);
  affixParts(parts, aff[0], o, loc, sym, plus);
  if (a !== a) parts.push({ type: 'nan', value: sup(loc, 0) });
  else if (a === Infinity) parts.push({ type: 'infinity', value: '∞' });
  else {
    var groups = groupInteger(digits[0], loc, o.useGrouping);
    for (var i = 0; i < groups.length; i++) {
      if (i) parts.push({ type: 'group', value: str22(loc, 1) });
      parts.push({ type: 'integer', value: groups[i] });
    }
    if (digits[1]) { parts.push({ type: 'decimal', value: str22(loc, 0) }); parts.push({ type: 'fraction', value: digits[1] }); }
  }
  affixParts(parts, aff[1], o, loc, sym, plus);
  if (o.style === 'unit') parts.push({ type: 'literal', value: o.unitDisplay === 'narrow' ? '' : ' ' }, { type: 'unit', value: o.unit });
  return parts;
}
function joinParts(parts) { var s = ''; for (var i = 0; i < parts.length; i++) s += parts[i].value; return s; }
hide(NumberFormat.prototype, 'formatToParts', function formatToParts(x) { intlBrand(this, 'NumberFormat', 'Intl.NumberFormat.prototype.formatToParts'); return numberParts(this, x); });
defineProperty(NumberFormat.prototype, 'format', { configurable: true, enumerable: false, get: function () {
  intlBrand(this, 'NumberFormat', 'get Intl.NumberFormat.prototype.format');
  var self = this;
  if (!this.__f) hide(this, '__f', function (x) { return joinParts(numberParts(self, x)); });
  return this.__f;
} });
hide(NumberFormat.prototype, 'formatRange', function formatRange(a, b) {
  intlBrand(this, 'NumberFormat', 'Intl.NumberFormat.prototype.formatRange');
  if (a === undefined || b === undefined) throw new TypeError('start or end is undefined');
  var x = joinParts(numberParts(this, a)), y = joinParts(numberParts(this, b));
  if (x === y) return '~' + x;
  return x + (this.__o.style === 'decimal' ? '–' : ' – ') + y;
});
hide(NumberFormat.prototype, 'resolvedOptions', function resolvedOptions() {
  intlBrand(this, 'NumberFormat', 'Intl.NumberFormat.prototype.resolvedOptions');
  var r = {}, o = this.__o;
  for (var k in o) if (o[k] !== undefined) r[k] = o[k];
  return r;
});

// ---- plural rules: CLDR's answers, keyed on just enough of the operands
// (LocalePlural.rgr says which)
function pluralKey(s) {
  var dot = s.indexOf('.'), i = Number(dot < 0 ? s : s.slice(0, dot)), f = dot < 0 ? '' : s.slice(dot + 1);
  if (!f) {
    if (i < 1001) return 's' + i;
    return 'b|' + (i % 100) + '|' + (i % 10) + '|' + (i % 1000000 === 0 ? 1 : 0);
  }
  return 'd|' + Math.min(i, 3) + '|' + Math.min(f.length, 3) + '|' + Math.min(Number(f), 3);
}
var PLURAL_NAMES = ['zero', 'one', 'two', 'few', 'many', 'other'];
function pluralTable(loc, ordinal) {
  var d = ld(), slot = loc * 2 + (ordinal ? 1 : 0);
  if (d.plurals[slot]) return d.plurals[slot];
  var e = d.plural_entries, off = e[slot * 3], cnt = e[slot * 3 + 1], map = create(null), cats = [PLURAL_NAMES[e[slot * 3 + 2]]];
  for (var j = 0; j < cnt; j++) {
    var cat = PLURAL_NAMES[d.plural_pairs[(off + j) * 2 + 1]];
    map[d.plural_pairs[(off + j) * 2]] = cat;
    if (cats.indexOf(cat) < 0) cats.push(cat);
  }
  cats.sort();
  if (cats.indexOf('other') >= 0) { cats.splice(cats.indexOf('other'), 1); cats.push('other'); }
  return (d.plurals[slot] = { map: map, fallback: PLURAL_NAMES[e[slot * 3 + 2]], categories: cats });
}
function pluralOf(loc, ordinal, digits) {
  var t = pluralTable(loc, ordinal), id = ld().keyId[pluralKey(digits)];
  return id !== undefined && t.map[id] ? t.map[id] : t.fallback;
}
function PluralRules(locales, options) {
  if (!(this instanceof PluralRules)) throw new TypeError("Constructor Intl.PluralRules requires 'new'");
  var o = optionsObject(options);
  var loc = pickLocale(locales);
  var type = option(o, 'type', ['cardinal', 'ordinal'], 'cardinal');
  var minF = numOption(o, 'minimumFractionDigits', 0, 100, undefined), maxF = numOption(o, 'maximumFractionDigits', 0, 100, undefined);
  if (minF === undefined && maxF === undefined) { minF = 0; maxF = 3; }
  else if (minF === undefined) minF = Math.min(0, maxF);
  else if (maxF === undefined) maxF = Math.max(3, minF);
  var r = { locale: loc, type: type, minimumIntegerDigits: numOption(o, 'minimumIntegerDigits', 1, 21, 1), minimumFractionDigits: minF, maximumFractionDigits: maxF };
  var minS = numOption(o, 'minimumSignificantDigits', 1, 21, undefined), maxS = numOption(o, 'maximumSignificantDigits', 1, 21, undefined);
  if (minS !== undefined || maxS !== undefined) { delete r.minimumFractionDigits; delete r.maximumFractionDigits; r.minimumSignificantDigits = minS || 1; r.maximumSignificantDigits = maxS || 21; }
  hide(this, '__intl', 'PluralRules');
  hide(this, '__loc', localeIndex(loc) < 0 ? 0 : localeIndex(loc));
  r.pluralCategories = pluralTable(this.__loc, type === 'ordinal').categories;
  r.roundingMode = 'halfExpand';
  hide(this, '__o', r);
}
hide(PluralRules.prototype, 'select', function select(n) {
  intlBrand(this, 'PluralRules', 'Intl.PluralRules.prototype.select');
  n = Number(n);
  if (n !== n || !isFinite(n)) return 'other';
  var o = this.__o, dg = numberDigits(o, Math.abs(n));
  return pluralOf(this.__loc, o.type === 'ordinal', dg[0] + (dg[1] ? '.' + dg[1] : ''));
});
hide(PluralRules.prototype, 'selectRange', function selectRange(a, b) {
  intlBrand(this, 'PluralRules', 'Intl.PluralRules.prototype.selectRange');
  if (a === undefined || b === undefined) throw new TypeError('start or end is undefined');
  return this.select(b);
});
hide(PluralRules.prototype, 'resolvedOptions', function resolvedOptions() { intlBrand(this, 'PluralRules', 'Intl.PluralRules.prototype.resolvedOptions'); var r = {}; for (var k in this.__o) r[k] = this.__o[k]; r.pluralCategories = this.__o.pluralCategories.slice(); return r; });

// ---- dates: six patterns per locale (numeric date, long date, full date,
// short time, time, date and time), each a list of fields and literals in
// the locale's order; the fields asked for are kept, with the literals that
// join them
var DT_FIELDS = ['weekday', 'era', 'year', 'month', 'day', 'hour', 'minute', 'second'];
var DT_CODE = [null, 'year', 'month', 'day', 'weekday', 'hour', 'minute', 'second', 'dayPeriod', 'era', 'timeZoneName'];
function pattern(loc, which) {
  var d = ld(), p = d.patAt[loc * 6 + which], pi = d.pattern_ints, out = [];
  for (var i = 0; i < pi[p]; i++) {
    var f = pi[p + 1 + i * 3];
    if (f === 0) { var lit = d.pattern_strings[pi[p + 2 + i * 3]]; if (lit) out.push({ lit: lit }); }
    else out.push({ f: DT_CODE[f], w: pi[p + 3 + i * 3] });
  }
  return out;
}
// the fields of `pat` that `keep` names, with the literal after each kept
// field that another kept field follows, and a closing literal that is
// letters (年, 日) or ends the pattern (Hungarian "1.")
function keepFields(pat, keep) {
  var out = [], pending = null, firstKept = -1;
  for (var i = 0; i < pat.length; i++) {
    var it = pat[i];
    if (it.lit !== undefined) { if (out.length && pending === null && out[out.length - 1].f) pending = { lit: it.lit, end: i === pat.length - 1 }; continue; }
    if (!keep(it.f)) continue;
    if (firstKept < 0) { firstKept = i; if (i > 0 && pat[i - 1].lit !== undefined && i === 1) out.push(pat[0]); }
    // fields dropped between: the literal after the field before, except
    // before a day period, which keeps its own (3 PM, not 3: PM)
    if (out.length && pending) out.push({ lit: it.f === 'dayPeriod' && pat[i - 1].lit !== undefined ? pat[i - 1].lit : pending.lit });
    pending = null;
    out.push(it);
  }
  if (pending && (pending.end || (!/\s/.test(pending.lit) && /[^\u0000-@]/.test(pending.lit)))) out.push({ lit: pending.lit });
  return out;
}
function DateTimeFormat(locales, options) {
  if (!(this instanceof DateTimeFormat) || this.__intl) return new DateTimeFormat(locales, options);
  var o = optionsObject(options);
  var loc = pickLocale(locales);
  var tz = o.timeZone === undefined ? undefined : String(o.timeZone);
  if (tz !== undefined) {
    var up = tz.toUpperCase();
    if (up === 'UTC' || up === 'GMT' || up === 'ETC/UTC' || up === 'ETC/GMT') tz = 'UTC';
    else if (!/^[A-Za-z_]+(\/[A-Za-z_+\-0-9]+)*$/.test(tz)) throw new RangeError('Invalid time zone specified: ' + tz);
  }
  var li = localeIndex(loc) < 0 ? 0 : localeIndex(loc);
  var r = { locale: loc, calendar: int5(li, 4) ? 'buddhist' : 'gregory', numberingSystem: 'latn', timeZone: tz || 'UTC' };
  var vals = { weekday: ['narrow', 'short', 'long'], era: ['narrow', 'short', 'long'], year: ['2-digit', 'numeric'], month: ['2-digit', 'numeric', 'narrow', 'short', 'long'], day: ['2-digit', 'numeric'], hour: ['2-digit', 'numeric'], minute: ['2-digit', 'numeric'], second: ['2-digit', 'numeric'] };
  var hour12 = o.hour12 === undefined ? undefined : Boolean(o.hour12);
  var hc = option(o, 'hourCycle', ['h11', 'h12', 'h23', 'h24'], undefined);
  var any = false, f = {};
  for (var i = 0; i < DT_FIELDS.length; i++) { var k = DT_FIELDS[i]; var v = option(o, k, vals[k], undefined); if (v !== undefined) { f[k] = v; any = true; } }
  var tzn = option(o, 'timeZoneName', ['short', 'long', 'shortOffset', 'longOffset', 'shortGeneric', 'longGeneric'], undefined);
  if (tzn) any = true;
  var ds = option(o, 'dateStyle', ['full', 'long', 'medium', 'short'], undefined), ts = option(o, 'timeStyle', ['full', 'long', 'medium', 'short'], undefined);
  if ((ds || ts) && any) throw new TypeError("Can't set option " + (ds ? 'dateStyle' : 'timeStyle') + ' when ' + DT_FIELDS.filter(function (k) { return f[k]; }).concat(tzn ? ['timeZoneName'] : [])[0] + ' is used');
  var dflt = o.__dflt || 'date';
  if (!any && !ds && !ts) {
    if (dflt === 'date' || dflt === 'all') { f.year = 'numeric'; f.month = 'numeric'; f.day = 'numeric'; }
    if (dflt === 'time' || dflt === 'all') { f.hour = 'numeric'; f.minute = '2-digit'; f.second = '2-digit'; }
  }
  if (ds) {
    f.year = ds === 'short' ? '2-digit' : 'numeric'; f.month = ds === 'short' ? 'numeric' : ds === 'medium' ? 'short' : 'long'; f.day = 'numeric';
    if (ds === 'full') f.weekday = 'long';
  }
  if (ts) { f.hour = 'numeric'; f.minute = '2-digit'; if (ts !== 'short') f.second = '2-digit'; if (ts === 'long' || ts === 'full') tzn = ts === 'long' ? 'short' : 'long'; }
  var h12 = hour12 !== undefined ? hour12 : hc !== undefined ? (hc === 'h12' || hc === 'h11') : int5(li, 3) === 1;
  for (var q = 0; q < DT_FIELDS.length; q++) if (f[DT_FIELDS[q]]) r[DT_FIELDS[q]] = f[DT_FIELDS[q]];
  if (f.hour) { r.hourCycle = h12 ? (hc === 'h11' ? 'h11' : 'h12') : (hc === 'h24' ? 'h24' : 'h23'); r.hour12 = h12; }
  if (tzn) r.timeZoneName = tzn;
  if (ds) r.dateStyle = ds;
  if (ts) r.timeStyle = ts;
  // the plan: date fields from the long or numeric pattern, time fields from
  // the time pattern, joined as the date-and-time pattern joins them
  var hasDate = f.year || f.month || f.day, hasTime = f.hour || f.minute || f.second;
  var textMonth = f.month === 'long' || f.month === 'short' || f.month === 'narrow';
  var plan = [];
  if (ds === 'short' || ds === 'medium') {
    // dateStyle short and medium: the locale's own pattern
    var src = sup(li, ds === 'short' ? 3 : 4), re = /\{(y+|M+|d+)\}|[^{]+/g, m;
    var OPT = { y: 'numeric', yy: '2-digit', M: 'numeric', MM: '2-digit', MMM: 'short', d: 'numeric', dd: '2-digit' };
    while ((m = re.exec(src))) plan.push(m[1] ? { f: m[1].charAt(0) === 'y' ? 'year' : m[1].charAt(0) === 'M' ? 'month' : 'day', opt: OPT[m[1]] } : { lit: m[0] });
  } else if (hasDate || f.weekday) {
    var dp = pattern(li, textMonth ? (f.weekday ? 2 : 1) : 0);
    if (f.weekday && !textMonth) {
      // a weekday before a numeric date, joined as the full date joins it
      var full = pattern(li, 2), at = -1;
      for (var w = 0; w < full.length; w++) if (full[w].f === 'weekday') at = w;
      var wk = [{ f: 'weekday', w: 0 }], sep = at >= 0 && full[at + 1] && full[at + 1].lit !== undefined ? full[at + 1].lit : ' ';
      dp = at === full.length - 1 ? dp.concat([{ lit: sep === ' ' ? ' ' : sep }], wk) : wk.concat([{ lit: sep }], dp);
    }
    plan = keepFields(dp, function (k) { return !!f[k]; });
  }
  if (hasTime) {
    var tp = keepFields(pattern(li, f.second ? 4 : 3), function (k) { return k === 'dayPeriod' ? !!f.hour && h12 : !!f[k]; });
    var hasPeriod = false;
    for (var z = 0; z < tp.length; z++) if (tp[z].f === 'dayPeriod') hasPeriod = true;
    if (f.hour && h12 && !hasPeriod) tp.push({ lit: ' ' }, { f: 'dayPeriod', w: 0 });
    if (plan.length) {
      var dt = pattern(li, 5), glue = ', ';
      for (var g = 1; g < dt.length; g++) if (dt[g].lit !== undefined && dt[g - 1].f && /year|month|day/.test(dt[g - 1].f) && dt[g + 1] && /hour|minute|second|dayPeriod/.test(dt[g + 1].f)) glue = dt[g].lit;
      // the date-and-time pattern's glue may repeat the date's closing
      // literal (Hungarian "05. 15:04", Bulgarian "г., 15:04")
      var last = plan[plan.length - 1];
      if (last.lit !== undefined && glue.indexOf(last.lit) === 0) glue = glue.slice(last.lit.length);
      plan.push({ lit: glue });
    }
    plan = plan.concat(tp);
  }
  if (tzn) plan.push({ lit: ' ' }, { f: 'timeZoneName', w: 0 });
  hide(this, '__intl', 'DateTimeFormat');
  hide(this, '__o', r);
  hide(this, '__loc', li);
  hide(this, '__plan', plan);
  hide(this, '__dfltHour', !!ts || (!any && !ds && (dflt === 'time' || dflt === 'all')));
}
function padTo(n, w) { var s = String(n); while (s.length < w) s = '0' + s; return s; }
function dateParts(dtf, date) {
  var t = date === undefined ? Date.now() : typeof date === 'number' ? date : DP.getTime.call(date instanceof Date ? date : new Date(date));
  if (t !== t) throw new RangeError('Invalid time value');
  var d = new Date(t), o = dtf.__o, li = dtf.__loc, names = ld().names, base = li * 38;
  var Y = d.getUTCFullYear() + int5(li, 4), M = d.getUTCMonth(), D = d.getUTCDate(), W = d.getUTCDay(), h = d.getUTCHours(), mi = d.getUTCMinutes(), s = d.getUTCSeconds();
  var P = [], plan = dtf.__plan;
  for (var i = 0; i < plan.length; i++) {
    var it = plan[i], v;
    if (it.lit !== undefined) { if (P.length && P[P.length - 1].type === 'literal') P[P.length - 1].value += it.lit; else P.push({ type: 'literal', value: it.lit }); continue; }
    switch (it.f) {
      case 'year': v = (it.opt || o.year) === '2-digit' ? padTo(Y % 100, 2) : String(Y); break;
      case 'month':
        var mo = it.opt || o.month;
        // a month name with no day stands alone (Finnish tammikuu, not tammikuuta)
        if (mo === 'long') v = o.day || o.dateStyle ? names[base + M] : sup(li, 5).split('|')[M];
        else if (mo === 'short') v = names[base + 12 + M];
        else if (mo === 'narrow') v = names[base + M].charAt(0).toUpperCase();
        else v = padTo(M + 1, mo === '2-digit' ? 2 : it.w || 1);
        break;
      case 'day': v = padTo(D, (it.opt || o.day) === '2-digit' ? 2 : it.w || 1); break;
      case 'weekday': v = o.weekday === 'long' ? names[base + 24 + W] : o.weekday === 'short' ? names[base + 31 + W] : names[base + 24 + W].charAt(0).toUpperCase(); break;
      case 'hour':
        var hh = h;
        if (o.hourCycle === 'h12') hh = (h + 11) % 12 + 1;
        else if (o.hourCycle === 'h11') hh = h % 12;
        else if (o.hourCycle === 'h24' && h === 0) hh = 24;
        // two digits when asked; the locale's own width in a default or
        // timeStyle time; else one, except a 24-hour time where the
        // locale's clock is 12-hour (en: 00:30)
        v = padTo(hh, o.hour === '2-digit' ? 2 : o.hour12 ? 1 : dtf.__dfltHour ? sup(li, 1).length : int5(li, 3) === 1 ? 2 : 1);
        break;
      case 'minute': v = padTo(mi, o.hour || o.minute === '2-digit' ? 2 : 1); break;
      case 'second': v = padTo(s, o.hour || o.minute || o.second === '2-digit' ? 2 : 1); break;
      case 'dayPeriod': v = sup(li, 2).split('|')[h < 12 ? 0 : 1]; break;
      case 'timeZoneName': v = o.timeZoneName === 'long' ? 'Coordinated Universal Time' : 'UTC'; break;
      default: v = '';
    }
    P.push({ type: it.f, value: v });
  }
  return P;
}
hide(DateTimeFormat.prototype, 'formatToParts', function formatToParts(d) { intlBrand(this, 'DateTimeFormat', 'Intl.DateTimeFormat.prototype.formatToParts'); return dateParts(this, d); });
defineProperty(DateTimeFormat.prototype, 'format', { configurable: true, enumerable: false, get: function () {
  intlBrand(this, 'DateTimeFormat', 'get Intl.DateTimeFormat.prototype.format');
  var self = this;
  // as V8 does, a plain space where ICU puts U+202F (3:04 PM); the parts keep it
  if (!this.__f) hide(this, '__f', function (d) { return joinParts(dateParts(self, d)).replace(/\u202f/g, ' '); });
  return this.__f;
} });
hide(DateTimeFormat.prototype, 'formatRange', function formatRange(a, b) {
  intlBrand(this, 'DateTimeFormat', 'Intl.DateTimeFormat.prototype.formatRange');
  var x = this.format(a), y = this.format(b);
  return x === y ? x : x + ' – ' + y;
});
hide(DateTimeFormat.prototype, 'resolvedOptions', function resolvedOptions() { intlBrand(this, 'DateTimeFormat', 'Intl.DateTimeFormat.prototype.resolvedOptions'); var r = {}; for (var k in this.__o) r[k] = this.__o[k]; return r; });

// ---- collation: native (intl.rs), the levels the sensitivity asks for
function Collator(locales, options) {
  if (!(this instanceof Collator) || this.__intl) return new Collator(locales, options);
  var o = optionsObject(options);
  var list = localeList(locales), ext = '';
  for (var i = 0; i < list.length; i++) if (collatorSupports(list[i])) { ext = list[i]; break; }
  var loc = ext ? baseTag(ext) : 'en-US';
  var usage = option(o, 'usage', ['sort', 'search'], 'sort');
  var numeric = option(o, 'numeric', null, undefined), caseFirst = option(o, 'caseFirst', ['upper', 'lower', 'false'], undefined);
  var caseFirstOpt = caseFirst;
  // -u-kn and -u-kf in the tag, when the options do not say
  if (numeric === undefined) numeric = /-u(-[a-z0-9]{2,8})*-kn(-true)?(-|$)/.test(ext);
  if (caseFirst === undefined) { var kf = /-u(?:-[a-z0-9]{2,8})*-kf-(upper|lower|false)/.exec(ext); caseFirst = kf ? kf[1] : 'false'; }
  var r = {
    locale: loc, usage: usage, sensitivity: option(o, 'sensitivity', ['base', 'accent', 'case', 'variant'], 'variant'),
    ignorePunctuation: option(o, 'ignorePunctuation', null, loc.split('-')[0] === 'th'), collation: 'default', numeric: numeric, caseFirst: caseFirst
  };
  var flags = { base: 0, accent: 1, case: 2, variant: 3 }[r.sensitivity];
  if (r.numeric) flags |= 4;
  if (r.ignorePunctuation) flags |= 8;
  // Danish sorts capitals first unless told otherwise
  if (r.caseFirst === 'upper' || (r.caseFirst === 'false' && caseFirstOpt === undefined && loc.split('-')[0] === 'da')) flags |= 16;
  if (loc.split('-')[0] === 'tr' || loc.split('-')[0] === 'az') flags |= 32;
  hide(this, '__intl', 'Collator');
  hide(this, '__o', r);
  hide(this, '__flags', flags);
}
defineProperty(Collator.prototype, 'compare', { configurable: true, enumerable: false, get: function () {
  intlBrand(this, 'Collator', 'get Intl.Collator.prototype.compare');
  var tag = this.__o.locale, flags = this.__flags;
  if (!this.__f) hide(this, '__f', function (a, b) { return intlCollate(String(a), String(b), tag, flags); });
  return this.__f;
} });
hide(Collator.prototype, 'resolvedOptions', function resolvedOptions() { intlBrand(this, 'Collator', 'Intl.Collator.prototype.resolvedOptions'); var r = {}; for (var k in this.__o) r[k] = this.__o[k]; return r; });

// ---- relative times (English words, the locale's numbers)
var REL_UNITS = ['year', 'quarter', 'month', 'week', 'day', 'hour', 'minute', 'second'];
function RelativeTimeFormat(locales, options) {
  if (!(this instanceof RelativeTimeFormat)) throw new TypeError("Constructor Intl.RelativeTimeFormat requires 'new'");
  var o = optionsObject(options);
  hide(this, '__intl', 'RelativeTimeFormat');
  hide(this, '__o', { locale: pickLocale(locales), style: option(o, 'style', ['long', 'short', 'narrow'], 'long'), numeric: option(o, 'numeric', ['always', 'auto'], 'always'), numberingSystem: 'latn' });
}
hide(RelativeTimeFormat.prototype, 'format', function format(v, unit) {
  intlBrand(this, 'RelativeTimeFormat', 'Intl.RelativeTimeFormat.prototype.format');
  v = Number(v);
  var u = String(unit).replace(/s$/, '');
  if (REL_UNITS.indexOf(u) < 0) throw new RangeError('Invalid unit argument for format() \'' + unit + '\'');
  if (this.__o.numeric === 'auto') {
    if (u === 'day' && v === 0) return 'today';
    if (u === 'day' && v === 1) return 'tomorrow';
    if (u === 'day' && v === -1) return 'yesterday';
    if (v === 0) return u === 'second' ? 'now' : 'this ' + u;
    if (v === 1 && u !== 'hour' && u !== 'minute' && u !== 'second') return 'next ' + u;
    if (v === -1 && u !== 'hour' && u !== 'minute' && u !== 'second') return 'last ' + u;
  }
  var a = Math.abs(v), n = new NumberFormat(this.__o.locale).format(a) + ' ' + u + (a === 1 ? '' : 's');
  return v < 0 || (v === 0 && 1 / v < 0) ? n + ' ago' : 'in ' + n;
});
hide(RelativeTimeFormat.prototype, 'resolvedOptions', function resolvedOptions() { var r = {}; for (var k in this.__o) r[k] = this.__o[k]; return r; });

// ---- lists: the locale's (pair, middle, end) separators
function ListFormat(locales, options) {
  if (!(this instanceof ListFormat)) throw new TypeError("Constructor Intl.ListFormat requires 'new'");
  var o = optionsObject(options);
  var loc = pickLocale(locales);
  hide(this, '__intl', 'ListFormat');
  hide(this, '__o', { locale: loc, type: option(o, 'type', ['conjunction', 'disjunction', 'unit'], 'conjunction'), style: option(o, 'style', ['long', 'short', 'narrow'], 'long') });
  hide(this, '__loc', localeIndex(loc) < 0 ? 0 : localeIndex(loc));
}
function listParts(lf, list) {
  var items = [];
  if (list !== undefined) for (var x of list) { if (typeof x !== 'string') throw new TypeError('Iterable yielded ' + String(x) + ' which is not a string'); items.push(x); }
  var o = lf.__o, seps = ld().list_separators, b = lf.__loc * 6 + (o.type === 'disjunction' ? 3 : 0), en = lf.__loc < 2;
  var pair = seps[b], mid = seps[b + 1], end = seps[b + 2];
  if (o.type === 'unit') { pair = mid = end = o.style === 'narrow' ? ' ' : ', '; if (o.style === 'long' && en) { pair = ', '; } }
  else if (o.type === 'conjunction' && en && o.style !== 'long') { pair = o.style === 'short' ? ' & ' : ', '; end = o.style === 'short' ? ', & ' : ', '; }
  var P = [];
  for (var i = 0; i < items.length; i++) {
    if (i) P.push({ type: 'literal', value: items.length === 2 ? pair : i === items.length - 1 ? end : mid });
    P.push({ type: 'element', value: items[i] });
  }
  return P;
}
hide(ListFormat.prototype, 'format', function format(list) { intlBrand(this, 'ListFormat', 'Intl.ListFormat.prototype.format'); return joinParts(listParts(this, list)); });
hide(ListFormat.prototype, 'formatToParts', function formatToParts(list) { intlBrand(this, 'ListFormat', 'Intl.ListFormat.prototype.formatToParts'); return listParts(this, list); });
hide(ListFormat.prototype, 'resolvedOptions', function resolvedOptions() { var r = {}; for (var k in this.__o) r[k] = this.__o[k]; return r; });

var Intl = {};
[['NumberFormat', NumberFormat], ['PluralRules', PluralRules], ['DateTimeFormat', DateTimeFormat], ['Collator', Collator], ['RelativeTimeFormat', RelativeTimeFormat], ['ListFormat', ListFormat]].forEach(function (e) {
  var supports = e[0] === 'Collator' ? collatorSupports : undefined;
  hide(e[1], 'supportedLocalesOf', function supportedLocalesOf_(l) { return supportedLocalesOf(l, supports); });
  tag(e[1].prototype, 'Intl.' + e[0]);
  hide(Intl, e[0], e[1]);
});
hide(Intl, 'getCanonicalLocales', function getCanonicalLocales(l) { return localeList(l); });
hide(Intl, 'supportedValuesOf', function supportedValuesOf(k) {
  k = String(k);
  if (k === 'currency') { var d = ld(), out = []; for (var i = 0; i < d.currency_count[0]; i++) out.push(d.currency_strings[i * 2]); return out.sort(); }
  if (k === 'timeZone') return ['UTC'];
  if (k === 'calendar') return ['buddhist', 'gregory'];
  if (k === 'numberingSystem') return ['latn'];
  if (k === 'collation') return ['default'];
  if (k === 'unit') return [];
  throw new RangeError('Invalid key : ' + k);
});
tag(Intl, 'Intl');
// in place of the accessor that loaded this, unless a script has put a value there
var gd = Object.getOwnPropertyDescriptor(globalThis, 'Intl');
if (gd && gd.get) hide(globalThis, 'Intl', Intl);
function dateLocale(d, locales, options, dflt) {
  if (!dateOk(d)) return 'Invalid Date';
  var o = {};
  if (options !== undefined) { var src = Object(options); for (var k in src) o[k] = src[k]; }
  var has = o.dateStyle !== undefined || o.timeStyle !== undefined;
  for (var i = 0; i < DT_FIELDS.length; i++) if (o[DT_FIELDS[i]] !== undefined) has = true;
  if (!has) o.__dflt = dflt;
  else if (dflt === 'date' && o.timeStyle !== undefined) throw new TypeError('Invalid option : timeStyle');
  else if (dflt === 'time' && o.dateStyle !== undefined) throw new TypeError('Invalid option : dateStyle');
  return new DateTimeFormat(locales, o).format(d);
}
return {
  Intl: Intl,
  numberLocale: function (x, locales, options) { return new NumberFormat(locales, options).format(x); },
  collate: function (a, b, locales, options) { return new Collator(locales, options).compare(a, b); },
  dateLocale: dateLocale
};
})"#;
