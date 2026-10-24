// SPDX-License-Identifier: AGPL-3.0-or-later
//! String.prototype.normalize: canonical decomposition (NFD) and
//! composition (NFC) over the tables of `uninorm`, decoded on first use.
//! NFKD and NFKC decompose canonically only (the compatibility mappings
//! are not in the tables).

use ranger::prelude::*;
use std::collections::HashMap;

const SBASE: int = 0xac00;
const LBASE: int = 0x1100;
const VBASE: int = 0x1161;
const TBASE: int = 0x11a7;
const LCOUNT: int = 19;
const VCOUNT: int = 21;
const TCOUNT: int = 28;
const NCOUNT: int = 588;
const SCOUNT: int = 11172;

pub struct Norm {
    pub loaded: bool,
    /// the decomposition table as decoded, and where each code point's
    /// entry (its count) is in it
    pub decomp: Vec<int>,
    pub decomp_at: HashMap<int, int>,
    /// (first, last, class) runs
    pub ccc: Vec<int>,
    /// starter * 0x200000 + mark → composite
    pub compose: HashMap<int, int>,
}

impl Norm {
    pub fn new() -> Norm {
        Norm { loaded: false, decomp: Vec::new(), decomp_at: HashMap::new(), ccc: Vec::new(), compose: HashMap::new() }
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

fn load(n: &mut Norm) {
    if n.loaded {
        return;
    }
    n.loaded = true;
    n.decomp = numbers(crate::uninorm::decomp().as_str());
    let mut i: usize = 0;
    while i + 1 < n.decomp.len() {
        let cp = n.decomp[i];
        let count = n.decomp[i + 1];
        n.decomp_at.insert(cp, (i + 1) as int);
        i += 2 + (count as usize);
    }
    n.ccc = numbers(crate::uninorm::ccc_runs().as_str());
    let c = numbers(crate::uninorm::compose().as_str());
    let mut k: usize = 0;
    while k + 2 < c.len() {
        n.compose.insert(c[k] * 0x200000 + c[k + 1], c[k + 2]);
        k += 3;
    }
}

fn ccc(n: &Norm, cp: int) -> int {
    if cp < 0x300 {
        return 0;
    }
    // binary search of the runs
    let runs = n.ccc.len() / 3;
    let mut lo: usize = 0;
    let mut hi: usize = runs;
    while lo < hi {
        let mid = (lo + hi) / 2;
        let first = n.ccc[mid * 3];
        let last = n.ccc[mid * 3 + 1];
        if cp < first {
            hi = mid;
        } else if cp > last {
            lo = mid + 1;
        } else {
            return n.ccc[mid * 3 + 2];
        }
    }
    0
}

fn decompose_into(n: &Norm, cp: int, out: &mut Vec<int>) {
    if cp >= SBASE && cp < SBASE + SCOUNT {
        let si = cp - SBASE;
        out.push(LBASE + si / NCOUNT);
        out.push(VBASE + (si % NCOUNT) / TCOUNT);
        let t = si % TCOUNT;
        if t != 0 {
            out.push(TBASE + t);
        }
        return;
    }
    match n.decomp_at.get(&cp) {
        Some(at) => {
            let count = n.decomp[*at as usize];
            let mut k: int = 0;
            while k < count {
                out.push(n.decomp[(*at + 1 + k) as usize]);
                k += 1;
            }
        }
        None => out.push(cp),
    }
}

fn composite(n: &Norm, a: int, b: int) -> int {
    if a >= LBASE && a < LBASE + LCOUNT && b >= VBASE && b < VBASE + VCOUNT {
        return SBASE + ((a - LBASE) * VCOUNT + (b - VBASE)) * TCOUNT;
    }
    if a >= SBASE && a < SBASE + SCOUNT && (a - SBASE) % TCOUNT == 0 && b > TBASE && b < TBASE + TCOUNT {
        return a + (b - TBASE);
    }
    match n.compose.get(&(a * 0x200000 + b)) {
        Some(c) => *c,
        None => -1,
    }
}

/// `s` in the form NFC, NFD, NFKC or NFKD.
pub fn normalize(n: &mut Norm, s: &str, compose: bool) -> String {
    let mut ascii = true;
    for b in s.bytes() {
        if b >= 128 {
            ascii = false;
            break;
        }
    }
    if ascii {
        return String::from(s);
    }
    load(n);
    let mut d: Vec<int> = Vec::new();
    for c in s.chars() {
        decompose_into(n, c as int, &mut d);
    }
    // canonical order: marks sorted by class, stably, between starters
    let len = d.len();
    let mut i: usize = 1;
    while i < len {
        let c = ccc(n, d[i]);
        if c != 0 {
            let mut j = i;
            while j > 0 {
                let p = ccc(n, d[j - 1]);
                if p <= c {
                    break;
                }
                let t = d[j - 1];
                d[j - 1] = d[j];
                d[j] = t;
                j -= 1;
            }
        }
        i += 1;
    }
    let mut out: Vec<int> = Vec::new();
    if compose && len > 0 {
        let mut starter: usize = 0;
        out.push(d[0]);
        let mut last = ccc(n, d[0]);
        if last != 0 {
            last = 256;
        }
        let mut k: usize = 1;
        while k < len {
            let ch = d[k];
            let cc = ccc(n, ch);
            let comp = composite(n, out[starter], ch);
            if comp >= 0 && (last < cc || last == 0) {
                out[starter] = comp;
            } else {
                if cc == 0 {
                    starter = out.len();
                }
                last = cc;
                out.push(ch);
            }
            k += 1;
        }
    } else {
        out = d;
    }
    let mut r = String::new();
    for cp in out.iter() {
        crate::jsstr::push_cp(&mut r, *cp);
    }
    r
}
