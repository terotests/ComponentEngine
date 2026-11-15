// SPDX-License-Identifier: AGPL-3.0-or-later
//! BigInt arithmetic on decimal strings: a value is its canonical decimal
//! text ("-123", "0"); the work is done on magnitudes as little-endian limbs
//! of base 10000, small enough that a product and its carry fit the 32-bit
//! int of every Ranger target.

use ranger::prelude::*;

const BASE: int = 10000;

/// `cost` more units of work: true past the deadline, and every time after
/// (time_up answers true once, at the poll that sees it; the halves of a
/// Karatsuba product must all give up).
fn stop(cost: int) -> bool {
    crate::builtins::expired() || crate::builtins::time_up(cost)
}

/// (negative, limbs) of a canonical decimal string.
pub fn parse(s: &str) -> (bool, Vec<int>) {
    let neg = s.starts_with("-");
    let mut digits: Vec<char> = Vec::new();
    for c in s.chars() {
        if c >= '0' && c <= '9' {
            digits.push(c);
        }
    }
    let mut limbs: Vec<int> = Vec::new();
    let mut end = digits.len() as int;
    while end > 0 {
        let start = if end >= 4 { end - 4 } else { 0 };
        let mut v: int = 0;
        let mut i = start;
        while i < end {
            v = v * 10 + ((digits[i as usize] as int) - 48);
            i += 1;
        }
        limbs.push(v);
        end = start;
    }
    trim(&mut limbs);
    let zero = limbs.is_empty();
    (neg && !zero, limbs)
}

fn trim(a: &mut Vec<int>) {
    while !a.is_empty() && a[a.len() - 1] == 0 {
        a.pop();
    }
}

/// The canonical decimal text.
pub fn show(neg: bool, a: &Vec<int>) -> String {
    if a.is_empty() {
        return String::from("0");
    }
    let mut s = String::new();
    if neg {
        s.push('-');
    }
    let mut i = (a.len() as int) - 1;
    s.push_str(format!("{}", a[i as usize]).as_str());
    i -= 1;
    while i >= 0 {
        let v = a[i as usize];
        let part = format!("{}", v);
        let mut pad = 4 - (part.chars().count() as int);
        while pad > 0 {
            s.push('0');
            pad -= 1;
        }
        s.push_str(part.as_str());
        i -= 1;
    }
    s
}

pub fn cmp_mag(a: &Vec<int>, b: &Vec<int>) -> int {
    if a.len() != b.len() {
        return if a.len() < b.len() { -1 } else { 1 };
    }
    let mut i = (a.len() as int) - 1;
    while i >= 0 {
        let (x, y) = (a[i as usize], b[i as usize]);
        if x != y {
            return if x < y { -1 } else { 1 };
        }
        i -= 1;
    }
    0
}

fn add_mag(a: &Vec<int>, b: &Vec<int>) -> Vec<int> {
    let mut out: Vec<int> = Vec::new();
    let mut carry: int = 0;
    let n = if a.len() > b.len() { a.len() } else { b.len() };
    let mut i: usize = 0;
    while i < n {
        let x = if i < a.len() { a[i] } else { 0 };
        let y = if i < b.len() { b[i] } else { 0 };
        let s = x + y + carry;
        out.push(s % BASE);
        carry = s / BASE;
        i += 1;
    }
    if carry > 0 {
        out.push(carry);
    }
    out
}

/// a - b for |a| >= |b|
fn sub_mag(a: &Vec<int>, b: &Vec<int>) -> Vec<int> {
    let mut out: Vec<int> = Vec::new();
    let mut borrow: int = 0;
    let mut i: usize = 0;
    while i < a.len() {
        let y = if i < b.len() { b[i] } else { 0 };
        let mut d = a[i] - y - borrow;
        if d < 0 {
            d += BASE;
            borrow = 1;
        } else {
            borrow = 0;
        }
        out.push(d);
        i += 1;
    }
    trim(&mut out);
    out
}

/// Operands this long (limbs) or longer multiply by Karatsuba.
const KARA: usize = 40;

fn mul_mag(a: &Vec<int>, b: &Vec<int>) -> Vec<int> {
    if a.is_empty() || b.is_empty() {
        return Vec::new();
    }
    if a.len() < KARA || b.len() < KARA {
        return mul_school(a, b);
    }
    let n = if a.len() > b.len() { a.len() } else { b.len() };
    if stop(n as int) {
        // past the deadline: the result is dropped (Vm::check_expired)
        return Vec::new();
    }
    let m = n / 2;
    if a.len() <= m {
        // a short: a * b1 shifted plus a * b0
        let mut lo = mul_mag(a, &low_limbs(b, m));
        let hi = mul_mag(a, &high_limbs(b, m));
        add_at(&mut lo, &hi, m);
        return lo;
    }
    if b.len() <= m {
        let mut lo = mul_mag(&low_limbs(a, m), b);
        let hi = mul_mag(&high_limbs(a, m), b);
        add_at(&mut lo, &hi, m);
        return lo;
    }
    let a0 = low_limbs(a, m);
    let a1 = high_limbs(a, m);
    let b0 = low_limbs(b, m);
    let b1 = high_limbs(b, m);
    let z0 = mul_mag(&a0, &b0);
    let z2 = mul_mag(&a1, &b1);
    let mid = mul_mag(&add_mag(&a0, &a1), &add_mag(&b0, &b1));
    let mut z1 = sub_mag(&mid, &z0);
    z1 = sub_mag(&z1, &z2);
    trim(&mut z1);
    let mut out = z0;
    add_at(&mut out, &z1, m);
    add_at(&mut out, &z2, 2 * m);
    out
}

/// Limbs 0..m of a (trimmed).
fn low_limbs(a: &Vec<int>, m: usize) -> Vec<int> {
    let mut out: Vec<int> = Vec::new();
    let mut i: usize = 0;
    while i < m && i < a.len() {
        out.push(a[i]);
        i += 1;
    }
    trim(&mut out);
    out
}

/// Limbs m.. of a.
fn high_limbs(a: &Vec<int>, m: usize) -> Vec<int> {
    let mut out: Vec<int> = Vec::new();
    let mut i: usize = m;
    while i < a.len() {
        out.push(a[i]);
        i += 1;
    }
    out
}

/// acc += x * BASE^off, in place.
fn add_at(acc: &mut Vec<int>, x: &Vec<int>, off: usize) {
    while acc.len() < off + x.len() {
        acc.push(0);
    }
    let mut carry: int = 0;
    let mut i: usize = 0;
    while i < x.len() || carry > 0 {
        let k = off + i;
        if k >= acc.len() {
            acc.push(0);
        }
        let xi = if i < x.len() { x[i] } else { 0 };
        let t = acc[k] + xi + carry;
        acc[k] = t % BASE;
        carry = t / BASE;
        i += 1;
    }
    trim(acc);
}

/// Schoolbook multiplication (short operands).
fn mul_school(a: &Vec<int>, b: &Vec<int>) -> Vec<int> {
    let mut out: Vec<int> = Vec::new();
    let mut k: usize = 0;
    while k < a.len() + b.len() {
        out.push(0);
        k += 1;
    }
    let mut i: usize = 0;
    while i < a.len() {
        if stop(b.len() as int) {
            // past the deadline: the result is dropped (Vm::check_expired)
            return Vec::new();
        }
        let mut carry: int = 0;
        let mut j: usize = 0;
        while j < b.len() {
            let t = out[i + j] + a[i] * b[j] + carry;
            out[i + j] = t % BASE;
            carry = t / BASE;
            j += 1;
        }
        let mut p = i + b.len();
        while carry > 0 {
            let t = out[p] + carry;
            out[p] = t % BASE;
            carry = t / BASE;
            p += 1;
        }
        i += 1;
    }
    trim(&mut out);
    out
}

fn mul_small(a: &Vec<int>, m: int) -> Vec<int> {
    let mut out: Vec<int> = Vec::new();
    let mut carry: int = 0;
    for x in a.iter() {
        let t = *x * m + carry;
        out.push(t % BASE);
        carry = t / BASE;
    }
    while carry > 0 {
        out.push(carry % BASE);
        carry = carry / BASE;
    }
    trim(&mut out);
    out
}

/// (quotient, remainder) of a magnitude by a small d (1 <= d <= 100000:
/// a remainder times BASE plus a limb stays below 2^31).
fn div_small(a: &Vec<int>, d: int) -> (Vec<int>, int) {
    let mut q: Vec<int> = vec![0; a.len()];
    let mut r: int = 0;
    let mut i = (a.len() as int) - 1;
    while i >= 0 {
        let t = r * BASE + a[i as usize];
        q[i as usize] = t / d;
        r = t % d;
        i -= 1;
    }
    trim(&mut q);
    (q, r)
}

/// The largest power of radix up to 100000, and its exponent.
fn radix_chunk(radix: int) -> (int, int) {
    let mut m = radix;
    let mut k = 1;
    while m * radix <= 100000 {
        m = m * radix;
        k += 1;
    }
    (m, k)
}

/// (quotient, remainder) of magnitudes; b not zero. Knuth's algorithm D
/// in base BASE: a quotient limb estimated from the top limbs, at most two
/// corrections (was a bisection of 14 trial products a limb: 115 s for a
/// 2^20-bit by 2^19-bit division).
fn divmod_mag(a: &Vec<int>, b: &Vec<int>) -> (Vec<int>, Vec<int>) {
    if b.len() >= NEWTON && a.len() >= b.len() + NEWTON {
        return divmod_newton(a, b);
    }
    divmod_knuth(a, b)
}

/// Divisors this long (limbs) divide by a reciprocal (Newton) when the
/// quotient is as long: two Karatsuba products a block of n limbs.
const NEWTON: usize = 200;

/// BASE^k as limbs.
fn base_pow(k: usize) -> Vec<int> {
    let mut v: Vec<int> = vec![0; k];
    v.push(1);
    v
}

/// About BASE^(2n) / b for b of n limbs (a few units off), by Newton's
/// iteration from the reciprocal of b's top half.
fn recip(b: &Vec<int>) -> Vec<int> {
    let n = b.len();
    if n <= 40 {
        let (q, _) = divmod_knuth(&base_pow(2 * n), b);
        return q;
    }
    let h = n / 2 + 1;
    let top = high_limbs(b, n - h);
    let rt = recip(&top);
    // X ~ BASE^(n+h) / top ~ BASE^(2n) / b
    let mut x: Vec<int> = vec![0; n - h];
    for v in rt.iter() {
        x.push(*v);
    }
    trim(&mut x);
    let p = mul_mag(b, &x);
    let t = base_pow(2 * n);
    if cmp_mag(&p, &t) <= 0 {
        let e = sub_mag(&t, &p);
        let mut e2 = e.clone();
        trim(&mut e2);
        let c = high_limbs(&mul_mag(&x, &e2), 2 * n);
        let mut r = x.clone();
        add_at(&mut r, &c, 0);
        r
    } else {
        let mut e = sub_mag(&p, &t);
        trim(&mut e);
        let mut c = high_limbs(&mul_mag(&x, &e), 2 * n);
        c = add_mag(&c, &vec![1]);
        let mut r = sub_mag(&x, &c);
        trim(&mut r);
        r
    }
}

/// Division by a long divisor with its reciprocal, a block of n limbs at
/// a time from the top; each block's quotient corrected to exact.
fn divmod_newton(a: &Vec<int>, b: &Vec<int>) -> (Vec<int>, Vec<int>) {
    let n = b.len();
    let r = recip(b);
    let blocks = (a.len() + n - 1) / n;
    let mut q_all: Vec<int> = vec![0; blocks * n];
    let mut rem: Vec<int> = Vec::new();
    let mut bi = blocks as int - 1;
    while bi >= 0 {
        if stop(n as int) {
            return (Vec::new(), Vec::new());
        }
        let off = (bi as usize) * n;
        // cur = rem * BASE^n + a[off .. off + n]
        let mut cur: Vec<int> = Vec::new();
        let mut k: usize = 0;
        while k < n {
            cur.push(if off + k < a.len() { a[off + k] } else { 0 });
            k += 1;
        }
        for v in rem.iter() {
            cur.push(*v);
        }
        trim(&mut cur);
        let mut q = high_limbs(&mul_mag(&cur, &r), 2 * n);
        trim(&mut q);
        let mut qb = mul_mag(&q, b);
        let mut steps = 0;
        while cmp_mag(&qb, &cur) > 0 && steps < 8 {
            qb = sub_mag(&qb, b);
            trim(&mut qb);
            q = sub_mag(&q, &vec![1]);
            trim(&mut q);
            steps += 1;
        }
        let mut rr = sub_mag(&cur, &qb);
        trim(&mut rr);
        while cmp_mag(&rr, b) >= 0 && steps < 16 {
            rr = sub_mag(&rr, b);
            trim(&mut rr);
            q = add_mag(&q, &vec![1]);
            steps += 1;
        }
        if steps >= 16 || cmp_mag(&qb, &cur) > 0 {
            // the reciprocal was too far off: the block by Knuth
            let (q2, r2) = divmod_knuth(&cur, b);
            q = q2;
            rr = r2;
        }
        let mut j: usize = 0;
        while j < q.len() && j < n {
            q_all[off + j] = q[j];
            j += 1;
        }
        rem = rr;
        bi -= 1;
    }
    trim(&mut q_all);
    (q_all, rem)
}

/// Knuth's algorithm D (see divmod_mag).
fn divmod_knuth(a: &Vec<int>, b: &Vec<int>) -> (Vec<int>, Vec<int>) {
    if cmp_mag(a, b) < 0 {
        return (Vec::new(), a.clone());
    }
    if b.len() == 1 {
        let (q, r) = div_small(a, b[0]);
        let mut rv: Vec<int> = Vec::new();
        if r != 0 {
            rv.push(r);
        }
        return (q, rv);
    }
    // normalise: the divisor's top limb at least BASE / 2
    let d = BASE / (b[b.len() - 1] + 1);
    let mut u = mul_small(a, d);
    while u.len() < a.len() + 1 {
        u.push(0);
    }
    let v = mul_small(b, d);
    let n = v.len();
    let m = u.len() - n - 1;
    let mut q: Vec<int> = vec![0; m + 1];
    let vtop = v[n - 1];
    let vnext = v[n - 2];
    let mut j = m as int;
    while j >= 0 {
        if stop(n as int + 1) {
            return (Vec::new(), Vec::new());
        }
        let ju = j as usize;
        let num = u[ju + n] * BASE + u[ju + n - 1];
        let mut qhat = num / vtop;
        let mut rhat = num % vtop;
        while qhat >= BASE || qhat * vnext > rhat * BASE + u[ju + n - 2] {
            qhat -= 1;
            rhat += vtop;
            if rhat >= BASE {
                break;
            }
        }
        // u[j..j+n] -= qhat * v
        let mut carry: int = 0;
        let mut borrow: int = 0;
        let mut i: usize = 0;
        while i < n {
            let p = qhat * v[i] + carry;
            carry = p / BASE;
            let mut t = u[ju + i] - (p % BASE) - borrow;
            if t < 0 {
                t += BASE;
                borrow = 1;
            } else {
                borrow = 0;
            }
            u[ju + i] = t;
            i += 1;
        }
        let t = u[ju + n] - carry - borrow;
        if t < 0 {
            // qhat was one too big: add v back
            qhat -= 1;
            let mut c: int = 0;
            let mut k: usize = 0;
            while k < n {
                let s2 = u[ju + k] + v[k] + c;
                u[ju + k] = s2 % BASE;
                c = s2 / BASE;
                k += 1;
            }
            u[ju + n] = t + BASE + c - BASE;
            if u[ju + n] >= BASE {
                u[ju + n] -= BASE;
            }
        } else {
            u[ju + n] = t;
        }
        q[ju] = qhat;
        j -= 1;
    }
    trim(&mut q);
    let mut r: Vec<int> = Vec::new();
    let mut k: usize = 0;
    while k < n {
        r.push(u[k]);
        k += 1;
    }
    trim(&mut r);
    let (r2, _) = div_small(&r, d);
    (q, r2)
}

/// base^e (e >= 0) by squaring.
fn pow_mag(base: &Vec<int>, e: int) -> Vec<int> {
    let mut acc: Vec<int> = vec![1];
    let mut b = base.clone();
    let mut k = e;
    while k > 0 {
        if (k & 1) == 1 {
            acc = mul_mag(&acc, &b);
        }
        k = k / 2;
        if k > 0 {
            b = mul_mag(&b, &b);
        }
    }
    acc
}

/// The magnitude of digits d[lo..hi] (most significant first) in `radix`
/// (up to 65536), halves joined by a power of radix: O(M(n) log n)
/// instead of a multiply-and-add per digit.
fn digits_mag(d: &Vec<int>, lo: usize, hi: usize, radix: int) -> Vec<int> {
    if hi - lo <= 48 {
        let mut acc: Vec<int> = Vec::new();
        let mut i = lo;
        while i < hi {
            acc = mul_small(&acc, radix);
            if d[i] != 0 {
                acc = add_mag(&acc, &vec![d[i]]);
                trim(&mut acc);
            }
            i += 1;
        }
        return acc;
    }
    if stop((hi - lo) as int) {
        return Vec::new();
    }
    let mid = lo + (hi - lo) / 2;
    let top = digits_mag(d, lo, mid, radix);
    let low = digits_mag(d, mid, hi, radix);
    let p = pow_mag(&mul_small(&vec![1], radix), (hi - mid) as int);
    let mut out = mul_mag(&top, &p);
    add_at(&mut out, &low, 0);
    out
}

/// A magnitude in base `chunk` (2..100000), least significant first:
/// halves split by a division by chunk^k, each side converted on its own
/// (short division a chunk at a time was O(n^2) with a division a pass).
fn mag_chunks(a: &Vec<int>, chunk: int) -> Vec<int> {
    let mut out: Vec<int> = Vec::new();
    if a.len() <= 64 {
        let mut m = a.clone();
        while !m.is_empty() {
            let (q, r) = div_small(&m, chunk);
            out.push(r);
            m = q;
        }
        return out;
    }
    if stop(a.len() as int) {
        return out;
    }
    // chunks in a: about limbs * log(BASE) / log(chunk)
    let per = (4.0 * 3.3219280949) / log2_of(chunk);
    let k = (((a.len() as double) * per) / 2.0) as int;
    let p = pow_mag(&mul_small(&vec![1], chunk), k);
    let (q, r) = divmod_mag(a, &p);
    let mut low = mag_chunks(&r, chunk);
    while (low.len() as int) < k {
        low.push(0);
    }
    let high = mag_chunks(&q, chunk);
    for h in high.iter() {
        low.push(*h);
    }
    while !low.is_empty() && low[low.len() - 1] == 0 {
        low.pop();
    }
    low
}

fn log2_of(x: int) -> double {
    let mut v = x as double;
    let mut n = 0.0;
    while v >= 2.0 {
        v = v / 2.0;
        n += 1.0;
    }
    n + (v - 1.0)
}

fn signed_add(an: bool, a: &Vec<int>, bn: bool, b: &Vec<int>) -> (bool, Vec<int>) {
    if an == bn {
        return (an, add_mag(a, b));
    }
    let c = cmp_mag(a, b);
    if c == 0 {
        return (false, Vec::new());
    }
    if c > 0 {
        (an, sub_mag(a, b))
    } else {
        (bn, sub_mag(b, a))
    }
}

fn result(neg: bool, m: Vec<int>) -> String {
    let z = m.is_empty();
    show(neg && !z, &m)
}

pub fn add(x: &str, y: &str) -> String {
    let (an, a) = parse(x);
    let (bn, b) = parse(y);
    let (n, m) = signed_add(an, &a, bn, &b);
    result(n, m)
}

pub fn sub(x: &str, y: &str) -> String {
    let (an, a) = parse(x);
    let (bn, b) = parse(y);
    let (n, m) = signed_add(an, &a, !bn, &b);
    result(n, m)
}

pub fn mul(x: &str, y: &str) -> String {
    let (an, a) = parse(x);
    let (bn, b) = parse(y);
    result(an != bn, mul_mag(&a, &b))
}

/// Truncating division; "" when dividing by zero.
pub fn div(x: &str, y: &str) -> String {
    let (an, a) = parse(x);
    let (bn, b) = parse(y);
    if b.is_empty() {
        return String::new();
    }
    let (q, _) = divmod_mag(&a, &b);
    result(an != bn, q)
}

/// The remainder takes the dividend's sign; "" when dividing by zero.
pub fn rem(x: &str, y: &str) -> String {
    let (an, a) = parse(x);
    let (_, b) = parse(y);
    if b.is_empty() {
        return String::new();
    }
    let (_, r) = divmod_mag(&a, &b);
    result(an, r)
}

/// x ** y for y >= 0; "" for a negative exponent.
pub fn pow(x: &str, y: &str) -> String {
    let (yn, mut e) = parse(y);
    if yn {
        return String::new();
    }
    let (xn, base) = parse(x);
    let mut acc: Vec<int> = vec![1];
    let mut b = base.clone();
    let mut odd_total = false;
    while !e.is_empty() {
        let (q, r) = divmod_mag(&e, &vec![2]);
        if !r.is_empty() {
            acc = mul_mag(&acc, &b);
            odd_total = !odd_total;
        }
        e = q;
        if !e.is_empty() {
            b = mul_mag(&b, &b);
        }
    }
    // the sign: negative base to an odd power
    let (_, ye) = parse(y);
    let (_, r2) = divmod_mag(&ye, &vec![2]);
    let _ = odd_total;
    result(xn && !r2.is_empty(), acc)
}

pub fn negate(x: &str) -> String {
    let (n, a) = parse(x);
    result(!n, a)
}

/// -1, 0, 1
pub fn compare(x: &str, y: &str) -> int {
    let (an, a) = parse(x);
    let (bn, b) = parse(y);
    if an != bn {
        return if an { -1 } else { 1 };
    }
    let c = cmp_mag(&a, &b);
    if an {
        -c
    } else {
        c
    }
}

pub fn is_zero(x: &str) -> bool {
    x == "0"
}

/// The value as a double (rounded as the decimal text parses).
pub fn to_double(x: &str) -> double {
    x.parse::<f64>().unwrap_or(0.0)
}

/// An integral double as a BigInt; "" when it is not an integer.
pub fn from_double(v: double) -> String {
    if v != v || v == 1.0 / 0.0 || v == -1.0 / 0.0 || v.floor() != v {
        return String::new();
    }
    let neg = v < 0.0;
    let mut a = if neg { -v } else { v };
    // a = m * 2^e with m below 2^53, where division by 10000 is exact
    let mut e: int = 0;
    while a >= 9007199254740992.0 {
        a = a / 2.0;
        e += 1;
    }
    let mut limbs: Vec<int> = Vec::new();
    while a >= 1.0 {
        let q = (a / 10000.0).floor();
        let r = a - q * 10000.0;
        limbs.push(r as int);
        a = q;
    }
    let mut m = limbs;
    let mut i = 0;
    while i < e {
        m = mul_small(&m, 2);
        i += 1;
    }
    result(neg, m)
}

/// Digits in `radix` (no sign, no prefix) as a canonical decimal; "" when
/// a digit is out of range or there is none.
pub fn from_radix(digits: &str, radix: int) -> String {
    let mut ds: Vec<int> = Vec::new();
    for c in digits.chars() {
        if c == '_' {
            continue;
        }
        let d = if c >= '0' && c <= '9' {
            (c as int) - 48
        } else if c >= 'a' && c <= 'z' {
            (c as int) - 87
        } else if c >= 'A' && c <= 'Z' {
            (c as int) - 55
        } else {
            99
        };
        if d >= radix {
            return String::new();
        }
        ds.push(d);
    }
    if ds.is_empty() {
        return String::new();
    }
    let n = ds.len();
    let mut acc = digits_mag(&ds, 0, n, radix);
    trim(&mut acc);
    result(false, acc)
}

/// The value written in `radix` (2..36).
pub fn to_radix(x: &str, radix: int) -> String {
    if radix == 10 {
        return String::from(x);
    }
    let (n, a) = parse(x);
    if a.is_empty() {
        return String::from("0");
    }
    let mut digits: Vec<char> = Vec::new();
    let chars = "0123456789abcdefghijklmnopqrstuvwxyz".chars().collect::<Vec<char>>();
    // k digits a chunk of radix^k
    let (m, k) = radix_chunk(radix);
    let cs = mag_chunks(&a, m);
    let mut ci: usize = 0;
    while ci < cs.len() {
        let mut v = cs[ci];
        let last = ci + 1 == cs.len();
        let mut j = 0;
        while j < k && (v > 0 || !last) {
            digits.push(chars[(v % radix) as usize]);
            v = v / radix;
            j += 1;
        }
        ci += 1;
    }
    let mut s = String::new();
    if n {
        s.push('-');
    }
    let mut i = (digits.len() as int) - 1;
    while i >= 0 {
        s.push(digits[i as usize]);
        i -= 1;
    }
    s
}

/// A magnitude's bytes, least significant first.
fn mag_bytes(a: &Vec<int>) -> Vec<int> {
    let mut out: Vec<int> = Vec::new();
    for c in mag_chunks(a, 65536).iter() {
        out.push(*c % 256);
        out.push(*c / 256);
    }
    while !out.is_empty() && out[out.len() - 1] == 0 {
        out.pop();
    }
    out
}

fn bytes_mag(b: &Vec<int>) -> Vec<int> {
    // two bytes a digit, most significant first
    let mut ds: Vec<int> = Vec::new();
    let mut i = (b.len() as int) - 1;
    if (b.len() & 1) == 1 {
        ds.push(b[i as usize]);
        i -= 1;
    }
    while i >= 1 {
        ds.push(b[i as usize] * 256 + b[(i - 1) as usize]);
        i -= 2;
    }
    if ds.is_empty() {
        return Vec::new();
    }
    let n = ds.len();
    let mut acc = digits_mag(&ds, 0, n, 65536);
    trim(&mut acc);
    acc
}

/// Two's complement bytes, `n` of them, from the bytes of |x| (or of
/// |x| - 1 for a negative x).
fn twos_of(neg: bool, bytes: Vec<int>, n: usize) -> Vec<int> {
    let mut b = bytes;
    while b.len() < n {
        b.push(0);
    }
    if neg {
        let mut i: usize = 0;
        while i < b.len() {
            b[i] = 255 - b[i];
            i += 1;
        }
    }
    b
}

/// The two's complement bytes of a value, `n` of them.
fn twos(neg: bool, a: &Vec<int>, n: usize) -> Vec<int> {
    let m = if neg { sub_mag(a, &vec![1]) } else { a.clone() };
    let mut b = mag_bytes(&m);
    while b.len() < n {
        b.push(0);
    }
    if neg {
        let mut i: usize = 0;
        while i < b.len() {
            b[i] = 255 - b[i];
            i += 1;
        }
    }
    b
}

/// `&` (0), `|` (1) or `^` (2) of two BigInts, as two's complement does.
pub fn bitop(op: int, x: &str, y: &str) -> String {
    let (an, a) = parse(x);
    let (bn, b) = parse(y);
    let ma = if an { sub_mag(&a, &vec![1]) } else { a.clone() };
    let mb = if bn { sub_mag(&b, &vec![1]) } else { b.clone() };
    let ba = mag_bytes(&ma);
    let bb = mag_bytes(&mb);
    let n = (if ba.len() > bb.len() { ba.len() } else { bb.len() }) + 1;
    let xa = twos_of(an, ba, n);
    let xb = twos_of(bn, bb, n);
    let mut r: Vec<int> = Vec::new();
    let mut i: usize = 0;
    while i < n {
        let (p, q) = (xa[i], xb[i]);
        r.push(if op == 0 { p & q } else if op == 1 { p | q } else { p ^ q });
        i += 1;
    }
    let neg = if op == 0 { an && bn } else if op == 1 { an || bn } else { an != bn };
    if !neg {
        return result(false, bytes_mag(&r));
    }
    let mut j: usize = 0;
    while j < r.len() {
        r[j] = 255 - r[j];
        j += 1;
    }
    let m = add_mag(&bytes_mag(&r), &vec![1]);
    result(true, m)
}

/// 2^bits as a decimal string, 13 bits a pass (a limb times 8192 plus
/// its carry stays below 2^31). Past the host's deadline it stops with a
/// wrong value, which the caller's check_expired turns into the time
/// limit's Error.
pub fn pow2(bits: int) -> String {
    // (2^13)^q by squaring, then the rest by a small multiplication
    let q = bits / 13;
    let r = bits - q * 13;
    let mut acc = pow_mag(&vec![8192], q);
    acc = mul_small(&acc, 1 << r);
    result(false, acc)
}

/// V8's limit on a BigInt's size, 2^30 bits: a result past it throws
/// "Maximum BigInt size exceeded" before any work is done.
pub const MAX_BITS: double = 1073741824.0;

/// An upper bound of the bits of a canonical decimal's magnitude: each
/// digit is at most log2(10) = 3.3219... bits; "0" has none.
pub fn bits_above(s: &str) -> double {
    if s == "0" {
        return 0.0;
    }
    (digit_count(s) as double) * 3.3219280949 + 1.0
}

/// A lower bound of the same: 10^(digits-1) <= |x|.
pub fn bits_below(s: &str) -> double {
    if s == "0" {
        return 0.0;
    }
    ((digit_count(s) - 1) as double) * 3.3219280948
}

/// BigInt.asUintN: x modulo 2^bits, non-negative.
pub fn as_uint_n(bits: int, x: &str) -> String {
    // a non-negative x below 2^bits is its own result: 2^bits is never made
    if !x.starts_with("-") && (bits as double) >= bits_above(x) {
        return String::from(x);
    }
    let m = pow2(bits);
    let r = rem(x, m.as_str());
    if r.starts_with("-") {
        return add(r.as_str(), m.as_str());
    }
    r
}

/// BigInt.asIntN: x modulo 2^bits, in [-2^(bits-1), 2^(bits-1)).
pub fn as_int_n(bits: int, x: &str) -> String {
    if bits == 0 {
        return String::from("0");
    }
    // -2^(bits-1) <= x < 2^(bits-1): x itself
    if ((bits - 1) as double) >= bits_above(x) {
        return String::from(x);
    }
    let u = as_uint_n(bits, x);
    let half = pow2(bits - 1);
    if compare(u.as_str(), half.as_str()) >= 0 {
        return sub(u.as_str(), pow2(bits).as_str());
    }
    u
}

/// A string as StringToBigInt: decimal (signed), 0x / 0o / 0b; "" when
/// it is not one.
pub fn from_string(s: &str) -> String {
    let t = String::from(s.trim());
    if t.is_empty() {
        return String::from("0");
    }
    let cs = t.chars().collect::<Vec<char>>();
    let prefixed = cs.len() >= 2 && cs[0] == '0' && (cs[1] == 'x' || cs[1] == 'X' || cs[1] == 'o' || cs[1] == 'O' || cs[1] == 'b' || cs[1] == 'B');
    if prefixed {
        let radix = if cs[1] == 'x' || cs[1] == 'X' {
            16
        } else if cs[1] == 'o' || cs[1] == 'O' {
            8
        } else {
            2
        };
        let mut rest = String::new();
        let mut i: usize = 2;
        while i < cs.len() {
            if cs[i] == '_' {
                return String::new();
            }
            rest.push(cs[i]);
            i += 1;
        }
        return from_radix(rest.as_str(), radix);
    }
    let mut neg = false;
    let mut body = String::new();
    let mut i: usize = 0;
    if cs[0] == '-' || cs[0] == '+' {
        neg = cs[0] == '-';
        i = 1;
    }
    while i < cs.len() {
        body.push(cs[i]);
        i += 1;
    }
    if body.is_empty() || body.contains('_') {
        return String::new();
    }
    for c in body.chars() {
        if !(c >= '0' && c <= '9') {
            return String::new();
        }
    }
    let v = from_radix(body.as_str(), 10);
    if neg {
        return negate(v.as_str());
    }
    v
}

/// The digits of a canonical decimal (without its sign); 0 when it is not
/// one.
pub fn digit_count(s: &str) -> int {
    let n = s.as_bytes().len() as int;
    if s.starts_with("-") {
        return n - 1;
    }
    n
}

/// How many digits big_arith does in ints: 18 where int is 64 bits (the
/// Rust build); the other targets' ints may be narrower, so none there.
#[ranger::target(rust)]
pub fn small_digits() -> int {
    18
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn small_digits() -> int {
    0
}

/// A canonical decimal of up to 18 digits as an int.
pub fn small_int(s: &str) -> int {
    let mut v: int = 0;
    let mut neg = false;
    for c in s.chars() {
        if c == '-' {
            neg = true;
        } else {
            v = v * 10 + ((c as int) - 48);
        }
    }
    if neg {
        -v
    } else {
        v
    }
}

/// The bound below which a BigInt keeps its value as a Num (exact) instead
/// of decimal text: 2^47 in Rust, where a Val keeps it in its 48 payload
/// bits (value.rs), 2^31 on the other targets so the value is also an int
/// there.
#[ranger::target(rust)]
pub fn small_big_limit() -> double {
    140737488355328.0
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn small_big_limit() -> double {
    2147483648.0
}

/// A canonical decimal's value as a double when it is below
/// small_big_limit in magnitude, else small_big_limit itself.
pub fn small_big_of(s: &str) -> double {
    let n = digit_count(s);
    if n <= 0 || n > 16 {
        return small_big_limit();
    }
    let len = s.as_bytes().len() as int;
    let mut v: double = 0.0;
    let neg = crate::jsstr::byte_at(s, 0) == 45;
    let mut i: int = if neg { 1 } else { 0 };
    while i < len {
        v = v * 10.0 + ((crate::jsstr::byte_at(s, i) - 48) as double);
        i += 1;
    }
    if v >= small_big_limit() {
        return small_big_limit();
    }
    if neg {
        -v
    } else {
        v
    }
}

/// An int's decimal text.
#[ranger::target(rust)]
pub fn int_text(v: int) -> String {
    v.to_string()
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn int_text(v: int) -> String {
    format!("{}", v)
}

// ---- the VM's side: a BigInt below small_big_limit in magnitude is a
// Val::Big holding its value (exact in a double); a larger one is an
// interned C_BIGINT object whose `prim` is its decimal text. Each value has
// one form, so === and SameValue compare Bigs by value and objects by
// identity. class_of gives C_BIGINT for both.

use crate::ops::*;
use crate::value::*;
use crate::vm::*;

pub const NF_BIGINT: int = 950;
pub const NF_BIGINT_ASINTN: int = 951;
pub const NF_BIGINT_ASUINTN: int = 952;
pub const NF_BIGINTP_TOSTRING: int = 953;
pub const NF_BIGINTP_VALUEOF: int = 954;

impl Vm {
    pub fn setup_bigint(&mut self) {
        let op = self.object_proto;
        let bp = self.alloc(C_OBJECT, op);
        self.roots.push(bp);
        self.bigint_proto = bp;
        let c = self.ctor("BigInt", NF_BIGINT, 1, bp);
        self.method(c, "asIntN", NF_BIGINT_ASINTN, 2);
        self.method(c, "asUintN", NF_BIGINT_ASUINTN, 2);
        self.method(bp, "toString", NF_BIGINTP_TOSTRING, 0);
        self.method(bp, "toLocaleString", NF_BIGINTP_TOSTRING, 0);
        self.method(bp, "valueOf", NF_BIGINTP_VALUEOF, 0);
        let a_tag = self.intern("@@toStringTag");
        let sv1 = self.str_val("BigInt");
        self.add_prop(bp, a_tag, sv1, P_HIDDEN | P_READONLY);
    }

    /// The BigInt with decimal text `s` (canonical), made once.
    pub fn bigint_val(&mut self, s: &str) -> Val {
        let n = small_big_of(s);
        if n < small_big_limit() {
            return self.bigint_small(n);
        }
        if let Some(o) = self.bigints.get(s) {
            return Val::Obj(*o);
        }
        let bp = self.bigint_proto;
        let o = self.alloc(C_BIGINT, bp);
        let sv0 = self.str_val(s);
        self.put_prim(o, sv0);
        self.bigints.insert(String::from(s), o);
        // the table is weak: a BigInt nothing holds is freed like any object
        // and leaves the table then (release)
        Val::Obj(o)
    }

    /// The BigInt with a small value `n` (an integer below
    /// small_big_limit in magnitude): a Val::Big, no object.
    #[inline]
    pub fn bigint_small(&mut self, n: double) -> Val {
        Val::Big(n + 0.0)
    }

    /// The small BigInt `n` plus `d` (a small integer).
    pub fn big_step(&mut self, n: double, d: double) -> Val {
        let r = n + d;
        let lim = small_big_limit();
        if r < lim && r > -lim {
            return Val::Big(r + 0.0);
        }
        self.bigint_val(crate::bigint::add(int_text(n as int).as_str(), int_text(d as int).as_str()).as_str())
    }

    /// The small value of a BigInt, when it has one.
    #[inline]
    pub fn big_small_val(&self, v: &Val) -> Option<double> {
        if let VK::Big(n) = v.kind() {
            return Some(n);
        }
        None
    }

    /// A BigInt being freed leaves the table (when it is the one there).
    pub fn forget_bigint(&mut self, o: int) {
        let t = self.big_text(&Val::Obj(o));
        let mine = match self.bigints.get(t.as_str()) {
            Some(x) => *x == o,
            None => false,
        };
        if mine {
            self.bigints.remove(t.as_str());
        }
    }

    pub fn is_bigint(&self, v: &Val) -> bool {
        v.is_big() || self.class_of(v) == C_BIGINT
    }

    /// The decimal text of a BigInt value.
    pub fn big_text(&self, v: &Val) -> String {
        if let VK::Big(n) = v.kind() {
            return int_text(n as int);
        }
        let o = obj_of(v);
        if o >= 0 {
            let pv = self.colds[self.objs[o as usize].cold as usize].prim;
            if let Some(s) = pv.as_str() {
                let s = self.sget(s);
                return s.as_ref().clone();
            }
        }
        String::from("0")
    }

    /// ToBigInt.
    pub fn to_bigint(&mut self, v: &Val) -> Val {
        let p = self.to_primitive(v, "number");
        if self.throwing {
            return Val::Undef();
        }
        if p.is_bool() { return self.bigint_val(if p.boolv() { "1" } else { "0" }) } else if let Some(s) = (&p).as_str() {
                let s = self.sget(s.clone());
                let t = from_string(s.as_str());
                if t.is_empty() {
                    self.throw_syntax(format!("Cannot convert {} to a BigInt", s).as_str());
                    return Val::Undef();
                }
                return self.bigint_val(t.as_str());
            } else {}
        if self.is_bigint(&p) {
            return p;
        }
        let what = self.type_of(&p);
        self.throw_type(format!("Cannot convert a {} to a BigInt", what).as_str());
        Val::Undef()
    }

    /// thisBigIntValue: a BigInt, or the one a BigInt object holds.
    pub fn this_bigint(&mut self, v: &Val) -> Val {
        if self.is_bigint(v) {
            return v.clone();
        }
        let o = obj_of(v);
        if o >= 0 && self.objs[o as usize].class == C_OBJECT && self.is_bigint(&self.colds[self.objs[o as usize].cold as usize].prim.clone()) {
            return self.colds[self.objs[o as usize].cold as usize].prim.clone();
        }
        self.throw_type("BigInt.prototype method called on incompatible receiver");
        Val::Undef()
    }

    /// A binary operator with a BigInt operand (after ToPrimitive).
    /// big_arith_now; past the deadline (the limb loops stop early) the
    /// script stops instead.
    pub fn big_arith(&mut self, code: int, a: &Val, b: &Val) -> Val {
        let r = self.big_arith_now(code, a, b);
        if self.check_expired() {
            return Val::Undef();
        }
        r
    }

    fn big_arith_now(&mut self, code: int, a: &Val, b: &Val) -> Val {
        if !self.is_bigint(a) || !self.is_bigint(b) {
            self.throw_type("Cannot mix BigInt and other types, use explicit conversions");
            return Val::Undef();
        }
        // both small: in doubles, exact while the result stays below
        // small_big_limit (rounding is monotonic, so a true result past it
        // never comes out below it)
        if let Some(p) = self.big_small_val(a) {
            if let Some(q) = self.big_small_val(b) {
                let mut ok = true;
                let v = if code == OP_ADD {
                    p + q
                } else if code == OP_SUB {
                    p - q
                } else if code == OP_MUL {
                    p * q
                } else if code == OP_DIV && q != 0.0 {
                    // both below small_big_limit: exact in int
                    ((p as int) / (q as int)) as double
                } else if code == OP_MOD && q != 0.0 {
                    ((p as int) % (q as int)) as double
                } else {
                    ok = false;
                    0.0
                };
                let lim = small_big_limit();
                if ok && v < lim && v > -lim {
                    return self.bigint_small(v + 0.0);
                }
            }
        }
        let x = self.big_text(a);
        let y = self.big_text(b);
        // small operands in machine integers: up to 9 digits each nothing
        // overflows; add, sub, div and rem stay inside up to 18
        let nx = digit_count(x.as_str());
        let ny = digit_count(y.as_str());
        let lim = small_digits();
        if nx > 0 && ny > 0 && nx <= lim && ny <= lim {
            let small = match code {
                OP_ADD | OP_SUB => true,
                OP_MUL => nx + ny <= 18,
                OP_DIV | OP_MOD => y != "0",
                _ => false,
            };
            if small {
                let p = small_int(x.as_str());
                let q = small_int(y.as_str());
                let v = if code == OP_ADD {
                    p + q
                } else if code == OP_SUB {
                    p - q
                } else if code == OP_MUL {
                    p * q
                } else if code == OP_DIV {
                    p / q
                } else {
                    p % q
                };
                return self.bigint_val(int_text(v).as_str());
            }
        }
        if self.big_too_big(code, x.as_str(), y.as_str()) {
            self.throw_range("Maximum BigInt size exceeded");
            return Val::Undef();
        }
        let r = match code {
            OP_ADD => add(x.as_str(), y.as_str()),
            OP_SUB => sub(x.as_str(), y.as_str()),
            OP_MUL => mul(x.as_str(), y.as_str()),
            OP_DIV => div(x.as_str(), y.as_str()),
            OP_MOD => rem(x.as_str(), y.as_str()),
            OP_EXP => {
                if x == "0" || x == "1" || y == "0" {
                    // 0 ** 0 is 1; a negative exponent is left to pow
                    if y.starts_with("-") { String::new() } else if y == "0" { String::from("1") } else { x.clone() }
                } else if x == "-1" && !y.starts_with("-") {
                    let last = y.as_bytes()[y.as_bytes().len() - 1] as int;
                    if (last & 1) == 1 { String::from("-1") } else { String::from("1") }
                } else {
                    pow(x.as_str(), y.as_str())
                }
            }
            OP_SHL | OP_SHR => {
                let n = to_double(y.as_str());
                let left = (code == OP_SHL) == (n >= 0.0);
                let k = if n < 0.0 { -n } else { n };
                if x == "0" {
                    String::from("0")
                } else if !left && k >= bits_above(x.as_str()) {
                    // every bit shifted out: 0, or -1 for a negative x (floor)
                    if x.starts_with("-") { String::from("-1") } else { String::from("0") }
                } else if left {
                    let p = pow2(k as int);
                    mul(x.as_str(), p.as_str())
                } else {
                    // floor division
                    let p = pow2(k as int);
                    let q = div(x.as_str(), p.as_str());
                    let r2 = rem(x.as_str(), p.as_str());
                    if r2.starts_with("-") {
                        sub(q.as_str(), "1")
                    } else {
                        q
                    }
                }
            }
            OP_BAND => bitop(0, x.as_str(), y.as_str()),
            OP_BOR => bitop(1, x.as_str(), y.as_str()),
            OP_BXOR => bitop(2, x.as_str(), y.as_str()),
            _ => {
                self.throw_type("BigInts have no unsigned right shift, use >> instead");
                return Val::Undef();
            }
        };
        if r.is_empty() {
            if code == OP_EXP {
                self.throw_range("Exponent must be non-negative");
            } else {
                self.throw_range("Division by zero");
            }
            return Val::Undef();
        }
        self.bigint_val(r.as_str())
    }

    /// Whether a BigInt operation's result would pass MAX_BITS, from the
    /// operands' sizes alone (lower bounds: a result that fits is never
    /// refused).
    fn big_too_big(&self, code: int, x: &str, y: &str) -> bool {
        if x == "0" {
            return false;
        }
        if code == OP_MUL {
            return y != "0" && bits_below(x) + bits_below(y) > MAX_BITS;
        }
        if code == OP_SHL || code == OP_SHR {
            let n = to_double(y);
            let left = (code == OP_SHL) == (n >= 0.0);
            let k = if n < 0.0 { -n } else { n };
            return left && bits_below(x) + k > MAX_BITS;
        }
        if code == OP_EXP {
            if x == "1" || x == "-1" || y.starts_with("-") {
                return false;
            }
            // |x| >= 2: at least one bit per unit of the exponent
            let lx = bits_below(x);
            let per = if lx > 1.0 { lx } else { 1.0 };
            return to_double(y) * per > MAX_BITS;
        }
        false
    }

    /// `<` with a BigInt on either side: 1, 0, or -1 (undefined).
    pub fn big_less(&mut self, a: &Val, b: &Val) -> int {
        let xa = self.big_side(a);
        let xb = self.big_side(b);
        if !xa.is_empty() && !xb.is_empty() {
            return if compare(xa.as_str(), xb.as_str()) < 0 { 1 } else { 0 };
        }
        let na = if self.is_bigint(a) { to_double(self.big_text(a).as_str()) } else { self.to_number(a) };
        let nb = if self.is_bigint(b) { to_double(self.big_text(b).as_str()) } else { self.to_number(b) };
        if na != na || nb != nb {
            -1
        } else if na < nb {
            1
        } else {
            0
        }
    }

    /// A BigInt's text, or a string's as a BigInt; "" for other values
    /// (and for a string that is not one).
    fn big_side(&mut self, v: &Val) -> String {
        if self.is_bigint(v) {
            return self.big_text(v);
        }
        if let Some(s) = v.as_str() {
            let s = self.sget(s.clone());
            return from_string(s.as_str());
        }
        String::new()
    }

    /// `==` with a BigInt on one side (the other primitive).
    pub fn big_loose_eq(&mut self, a: &Val, b: &Val) -> bool {
        let (big, other) = if self.is_bigint(a) { (a, b) } else { (b, a) };
        let x = self.big_text(big);
        if let Some(n) = other.as_num() {
                let t = from_double(n);
                !t.is_empty() && t == x
            } else if let Some(s) = other.as_str() {
                let s = self.sget(s.clone());
                let t = from_string(s.as_str());
                !t.is_empty() && t == x
            } else if other.is_bool() { x.as_str() == (if other.boolv() { "1" } else { "0" }) } else {
                if self.is_bigint(other) {
                    return self.big_text(other) == x;
                }
                false
            }
    }

    pub fn call_native_bigint(&mut self, id: int, this: Val, args: &Vec<Val>, construct: bool) -> Val {
        let r = self.call_native_bigint_now(id, this, args, construct);
        if self.check_expired() {
            return Val::Undef();
        }
        r
    }

    fn call_native_bigint_now(&mut self, id: int, this: Val, args: &Vec<Val>, construct: bool) -> Val {
        let a0 = if args.is_empty() { Val::Undef() } else { args[0].clone() };
        let a1 = if args.len() < 2 { Val::Undef() } else { args[1].clone() };
        if id == NF_BIGINT {
            if construct {
                self.throw_type("BigInt is not a constructor");
                return Val::Undef();
            }
            let p = self.to_primitive(&a0, "number");
            if self.throwing {
                return Val::Undef();
            }
            if let Some(n) = p.as_num() {
                let t = from_double(n);
                if t.is_empty() {
                    let ns = self.to_string(&p);
                    self.throw_range(format!("The number {} cannot be converted to a BigInt because it is not an integer", ns).as_str());
                    return Val::Undef();
                }
                return self.bigint_val(t.as_str());
            }
            return self.to_bigint(&p);
        }
        if id == NF_BIGINT_ASINTN || id == NF_BIGINT_ASUINTN {
            let bits = self.to_number(&a0);
            if self.throwing {
                return Val::Undef();
            }
            if bits != bits || bits < 0.0 || bits > 9007199254740991.0 {
                self.throw_range("Invalid value: not (convertible to) a safe integer");
                return Val::Undef();
            }
            let x = self.to_bigint(&a1);
            if self.throwing {
                return Val::Undef();
            }
            let t = self.big_text(&x);
            // x fits in the bits: x itself, decided on the double (an int
            // may be 32 bits on some targets)
            let fits = if id == NF_BIGINT_ASINTN { bits - 1.0 >= bits_above(t.as_str()) } else { !t.starts_with("-") && bits >= bits_above(t.as_str()) };
            if fits {
                return x;
            }
            // a negative x modulo 2^bits needs all of the bits
            if id == NF_BIGINT_ASUINTN && t.starts_with("-") && bits > MAX_BITS {
                self.throw_range("Maximum BigInt size exceeded");
                return Val::Undef();
            }
            let r = if id == NF_BIGINT_ASINTN { as_int_n(bits as int, t.as_str()) } else { as_uint_n(bits as int, t.as_str()) };
            return self.bigint_val(r.as_str());
        }
        let v = self.this_bigint(&this);
        if self.throwing {
            return Val::Undef();
        }
        if id == NF_BIGINTP_VALUEOF {
            return v;
        }
        let radix = if a0.is_undef() { 10.0 } else { self.to_number(&a0) };
        if radix < 2.0 || radix > 36.0 || radix.floor() != radix {
            self.throw_range("toString() radix must be between 2 and 36");
            return Val::Undef();
        }
        let t = self.big_text(&v);
        self.string_val(to_radix(t.as_str(), radix as int))
    }
}
