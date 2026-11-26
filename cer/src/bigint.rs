// SPDX-License-Identifier: AGPL-3.0-or-later
//! BigInt arithmetic on magnitudes: little-endian limbs of limb_bits()
//! bits (30 in Rust, 15 elsewhere), small enough that a product and its
//! carry fit the int of every Ranger target, and binary, so shifts and
//! bitwise operators work on the limbs.
//! A value is (negative, magnitude); decimal text is made only when a
//! BigInt is written out (show) or read in (parse). The VM keeps a large
//! BigInt's magnitude as its key (key_of): base-32 digits, five bits each.

use ranger::prelude::*;

/// The bits of a limb: 30 where int is 64 bits (the Rust build: a product
/// and its carry stay below 2^62), 15 on the other targets, whose ints may
/// be 32 bits or doubles.
#[ranger::target(rust)]
#[inline]
fn limb_bits() -> int {
    30
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
fn limb_bits() -> int {
    15
}

#[inline]
fn base() -> int {
    1 << limb_bits()
}

#[inline]
fn mask() -> int {
    (1 << limb_bits()) - 1
}

/// `cost` more units of work: true past the deadline, and every time after
/// (time_up answers true once, at the poll that sees it; the halves of a
/// Karatsuba product must all give up).
fn stop(cost: int) -> bool {
    crate::builtins::expired() || crate::builtins::time_up(cost)
}

/// (negative, limbs) of a canonical decimal string: its digits four at a
/// time (base 10000), joined by digits_mag.
pub fn parse(s: &str) -> (bool, Vec<int>) {
    let neg = s.starts_with("-");
    let mut digits: Vec<int> = Vec::new();
    for c in s.chars() {
        if c >= '0' && c <= '9' {
            digits.push((c as int) - 48);
        }
    }
    let n = digits.len() as int;
    let mut chunks: Vec<int> = Vec::new();
    let mut start: int = 0;
    let first = n % 4;
    if first > 0 {
        let mut v: int = 0;
        while start < first {
            v = v * 10 + digits[start as usize];
            start += 1;
        }
        chunks.push(v);
    }
    while start < n {
        let mut v: int = 0;
        let mut k: int = 0;
        while k < 4 {
            v = v * 10 + digits[(start + k) as usize];
            k += 1;
        }
        chunks.push(v);
        start += 4;
    }
    let mut limbs: Vec<int> = if chunks.is_empty() { Vec::new() } else { digits_mag(&chunks, 0, chunks.len(), 10000) };
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
    let cs = mag_chunks(a, 10000);
    let mut s = String::new();
    if neg {
        s.push('-');
    }
    let mut i = (cs.len() as int) - 1;
    s.push_str(format!("{}", cs[i as usize]).as_str());
    i -= 1;
    while i >= 0 {
        let v = cs[i as usize];
        let part = format!("{}", v);
        let mut pad = 4 - (part.as_bytes().len() as int);
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
    let n = if a.len() > b.len() { a.len() } else { b.len() };
    let mut out: Vec<int> = vec![0; n];
    let mut carry: int = 0;
    let mut i: usize = 0;
    while i < n {
        let x = if i < a.len() { a[i] } else { 0 };
        let y = if i < b.len() { b[i] } else { 0 };
        let s = x + y + carry;
        out[i] = s & mask();
        carry = s >> limb_bits();
        i += 1;
    }
    if carry > 0 {
        out.push(carry);
    }
    out
}

/// a - b for |a| >= |b|
fn sub_mag(a: &Vec<int>, b: &Vec<int>) -> Vec<int> {
    let mut out: Vec<int> = vec![0; a.len()];
    let mut borrow: int = 0;
    let mut i: usize = 0;
    while i < a.len() {
        let y = if i < b.len() { b[i] } else { 0 };
        let mut d = a[i] - y - borrow;
        if d < 0 {
            d += base();
            borrow = 1;
        } else {
            borrow = 0;
        }
        out[i] = d;
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

/// acc += x * base()^off, in place.
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
        acc[k] = t & mask();
        carry = t >> limb_bits();
        i += 1;
    }
    trim(acc);
}

/// Schoolbook multiplication (short operands).
fn mul_school(a: &Vec<int>, b: &Vec<int>) -> Vec<int> {
    let mut out: Vec<int> = vec![0; a.len() + b.len()];
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
            out[i + j] = t & mask();
            carry = t >> limb_bits();
            j += 1;
        }
        let mut p = i + b.len();
        while carry > 0 {
            let t = out[p] + carry;
            out[p] = t & mask();
            carry = t >> limb_bits();
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
        out.push(t & mask());
        carry = t >> limb_bits();
    }
    while carry > 0 {
        out.push(carry & mask());
        carry = carry >> limb_bits();
    }
    trim(&mut out);
    out
}

/// (quotient, remainder) of a magnitude by a small d (1 <= d <= 65536:
/// a remainder times base() plus a limb stays below 2^31).
fn div_small(a: &Vec<int>, d: int) -> (Vec<int>, int) {
    let mut q: Vec<int> = vec![0; a.len()];
    let mut r: int = 0;
    let mut i = (a.len() as int) - 1;
    while i >= 0 {
        let t = r * base() + a[i as usize];
        q[i as usize] = t / d;
        r = t % d;
        i -= 1;
    }
    trim(&mut q);
    (q, r)
}

/// The largest power of radix up to base(), and its exponent.
fn radix_chunk(radix: int) -> (int, int) {
    let mut m = radix;
    let mut k = 1;
    while m * radix <= base() {
        m = m * radix;
        k += 1;
    }
    (m, k)
}

/// (quotient, remainder) of magnitudes; b not zero. Knuth's algorithm D
/// in base base(): a quotient limb estimated from the top limbs, at most two
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

/// base()^k as limbs.
fn base_pow(k: usize) -> Vec<int> {
    let mut v: Vec<int> = vec![0; k];
    v.push(1);
    v
}

/// About base()^(2n) / b for b of n limbs (a few units off), by Newton's
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
    // X ~ base()^(n+h) / top ~ base()^(2n) / b
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
        // cur = rem * base()^n + a[off .. off + n]
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
    // normalise: the divisor's top limb at least base() / 2
    let d = base() / (b[b.len() - 1] + 1);
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
        let num = u[ju + n] * base() + u[ju + n - 1];
        let mut qhat = num / vtop;
        let mut rhat = num % vtop;
        while qhat >= base() || qhat * vnext > rhat * base() + u[ju + n - 2] {
            qhat -= 1;
            rhat += vtop;
            if rhat >= base() {
                break;
            }
        }
        // u[j..j+n] -= qhat * v
        let mut carry: int = 0;
        let mut borrow: int = 0;
        let mut i: usize = 0;
        while i < n {
            let p = qhat * v[i] + carry;
            carry = p / base();
            let mut t = u[ju + i] - (p % base()) - borrow;
            if t < 0 {
                t += base();
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
                u[ju + k] = s2 % base();
                c = s2 / base();
                k += 1;
            }
            u[ju + n] = t + base() + c - base();
            if u[ju + n] >= base() {
                u[ju + n] -= base();
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
/// (up to base()), halves joined by a power of radix: O(M(n) log n)
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

/// A magnitude in base `chunk` (2..base()), least significant first:
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
    // chunks in a: about limbs * log(base()) / log(chunk)
    let per = (limb_bits() as double) / log2_of(chunk);
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

/// A value whose magnitude is zero is never negative.
fn canon(neg: bool, m: Vec<int>) -> (bool, Vec<int>) {
    let z = m.is_empty();
    (neg && !z, m)
}

// ---- operators on values (negative, magnitude)

pub fn mag_add(an: bool, a: &Vec<int>, bn: bool, b: &Vec<int>) -> (bool, Vec<int>) {
    signed_add(an, a, bn, b)
}

pub fn mag_sub(an: bool, a: &Vec<int>, bn: bool, b: &Vec<int>) -> (bool, Vec<int>) {
    signed_add(an, a, !bn, b)
}

pub fn mag_mul(an: bool, a: &Vec<int>, bn: bool, b: &Vec<int>) -> (bool, Vec<int>) {
    canon(an != bn, mul_mag(a, b))
}

/// Truncating division; b not zero.
pub fn mag_div(an: bool, a: &Vec<int>, bn: bool, b: &Vec<int>) -> (bool, Vec<int>) {
    let (q, _) = divmod_mag(a, b);
    canon(an != bn, q)
}

/// The remainder takes the dividend's sign; b not zero.
pub fn mag_rem(an: bool, a: &Vec<int>, b: &Vec<int>) -> (bool, Vec<int>) {
    let (_, r) = divmod_mag(a, b);
    canon(an, r)
}

/// x ** e for e >= 0 (a small exponent: the result's size is checked
/// before, big_too_big).
pub fn mag_pow(xn: bool, x: &Vec<int>, e: double) -> (bool, Vec<int>) {
    let odd = (e - 2.0 * (e / 2.0).floor()) == 1.0;
    canon(xn && odd, pow_mag(x, e as int))
}

/// -1, 0, 1
pub fn mag_cmp(an: bool, a: &Vec<int>, bn: bool, b: &Vec<int>) -> int {
    if an != bn {
        return if an { -1 } else { 1 };
    }
    let c = cmp_mag(a, b);
    if an {
        -c
    } else {
        c
    }
}

/// The bits of a magnitude (0 for zero).
pub fn bit_len(a: &Vec<int>) -> double {
    if a.is_empty() {
        return 0.0;
    }
    let mut top = a[a.len() - 1];
    let mut n: int = 0;
    while top > 0 {
        top = top / 2;
        n += 1;
    }
    ((a.len() as int - 1) * limb_bits() + n) as double
}

/// a * 2^k
fn shl_mag(a: &Vec<int>, k: int) -> Vec<int> {
    if a.is_empty() {
        return Vec::new();
    }
    let limbs = k / limb_bits();
    let bits = k % limb_bits();
    let mut out: Vec<int> = vec![0; limbs as usize];
    if bits == 0 {
        for x in a.iter() {
            out.push(*x);
        }
        return out;
    }
    let mul = 1 << bits;
    let mut carry: int = 0;
    for x in a.iter() {
        let t = *x * mul + carry;
        out.push(t & mask());
        carry = t >> limb_bits();
    }
    if carry > 0 {
        out.push(carry);
    }
    out
}

/// floor(a / 2^k)
fn shr_mag(a: &Vec<int>, k: int) -> Vec<int> {
    let limbs = (k / limb_bits()) as usize;
    let bits = k % limb_bits();
    if limbs >= a.len() {
        return Vec::new();
    }
    let mut out: Vec<int> = Vec::new();
    let mut i = limbs;
    while i < a.len() {
        let lo = a[i] >> bits;
        let hi = if i + 1 < a.len() { (a[i + 1] << (limb_bits() - bits)) & mask() } else { 0 };
        out.push(if bits == 0 { a[i] } else { lo | hi });
        i += 1;
    }
    trim(&mut out);
    out
}

/// Whether any of the low k bits of a is set.
fn low_bits_set(a: &Vec<int>, k: int) -> bool {
    let limbs = (k / limb_bits()) as usize;
    let bits = k % limb_bits();
    let mut i: usize = 0;
    while i < limbs && i < a.len() {
        if a[i] != 0 {
            return true;
        }
        i += 1;
    }
    bits > 0 && limbs < a.len() && (a[limbs] & ((1 << bits) - 1)) != 0
}

/// x * 2^k (k >= 0)
pub fn mag_shl(xn: bool, x: &Vec<int>, k: int) -> (bool, Vec<int>) {
    canon(xn, shl_mag(x, k))
}

/// floor(x / 2^k) (k >= 0): a negative x rounds down, as two's complement
/// does.
pub fn mag_shr(xn: bool, x: &Vec<int>, k: int) -> (bool, Vec<int>) {
    let q = shr_mag(x, k);
    if xn && low_bits_set(x, k) {
        return canon(true, add_mag(&q, &vec![1]));
    }
    canon(xn, q)
}

/// The limbs of a value in two's complement, `n` of them.
fn twos(neg: bool, a: &Vec<int>, n: usize) -> Vec<int> {
    let m = if neg { sub_mag(a, &vec![1]) } else { a.clone() };
    let mut out: Vec<int> = Vec::new();
    let mut i: usize = 0;
    while i < n {
        let v = if i < m.len() { m[i] } else { 0 };
        out.push(if neg { mask() - v } else { v });
        i += 1;
    }
    out
}

/// `&` (0), `|` (1) or `^` (2), as two's complement does.
pub fn mag_bitop(op: int, an: bool, a: &Vec<int>, bn: bool, b: &Vec<int>) -> (bool, Vec<int>) {
    let n = (if a.len() > b.len() { a.len() } else { b.len() }) + 1;
    let xa = twos(an, a, n);
    let xb = twos(bn, b, n);
    let neg = if op == 0 { an && bn } else if op == 1 { an || bn } else { an != bn };
    let mut r: Vec<int> = Vec::new();
    let mut i: usize = 0;
    while i < n {
        let (p, q) = (xa[i], xb[i]);
        let v = if op == 0 { p & q } else if op == 1 { p | q } else { p ^ q };
        r.push(if neg { mask() - v } else { v });
        i += 1;
    }
    trim(&mut r);
    if !neg {
        return (false, r);
    }
    canon(true, add_mag(&r, &vec![1]))
}

/// 2^bits
fn pow2_mag(bits: int) -> Vec<int> {
    shl_mag(&vec![1], bits)
}

/// x modulo 2^bits, non-negative.
pub fn mag_as_uint_n(bits: int, xn: bool, x: &Vec<int>) -> (bool, Vec<int>) {
    if !xn && (bits as double) >= bit_len(x) {
        return (false, x.clone());
    }
    // the low `bits` bits of the two's complement
    let limbs = ((bits + limb_bits() - 1) / limb_bits()) as usize;
    let mut t = twos(xn, x, limbs);
    let rest = bits % limb_bits();
    if rest > 0 && limbs > 0 {
        t[limbs - 1] = t[limbs - 1] & ((1 << rest) - 1);
    }
    trim(&mut t);
    (false, t)
}

/// x modulo 2^bits, in [-2^(bits-1), 2^(bits-1)).
pub fn mag_as_int_n(bits: int, xn: bool, x: &Vec<int>) -> (bool, Vec<int>) {
    if bits == 0 {
        return (false, Vec::new());
    }
    if ((bits - 1) as double) >= bit_len(x) {
        return (xn, x.clone());
    }
    let (_, u) = mag_as_uint_n(bits, xn, x);
    if bit_len(&u) >= bits as double {
        // at or past 2^(bits-1): u - 2^bits
        return canon(true, sub_mag(&pow2_mag(bits), &u));
    }
    (false, u)
}

/// The value as a double, rounded to nearest, ties to even.
pub fn mag_to_double(xn: bool, x: &Vec<int>) -> double {
    let n = bit_len(x) as int;
    let mut v: double = 0.0;
    if n <= 53 {
        let mut i = (x.len() as int) - 1;
        while i >= 0 {
            v = v * (base() as double) + (x[i as usize] as double);
            i -= 1;
        }
    } else {
        // the top 54 bits, then round to 53 with the rest as sticky
        let s = n - 54;
        let top = shr_mag(x, s);
        let mut m: double = 0.0;
        let mut i = (top.len() as int) - 1;
        while i >= 0 {
            m = m * (base() as double) + (top[i as usize] as double);
            i -= 1;
        }
        let half = (m / 2.0).floor();
        let low = m - 2.0 * half;
        let mut r = half;
        if low == 1.0 && (low_bits_set(x, s) || (r - 2.0 * (r / 2.0).floor()) == 1.0) {
            r = r + 1.0;
        }
        // r * 2^(s + 1), past 2^1024 Infinity
        let mut e = s + 1;
        v = r;
        while e >= 30 && v < 1.0 / 0.0 {
            v = v * 1073741824.0;
            e -= 30;
        }
        while e > 0 && v < 1.0 / 0.0 {
            v = v * 2.0;
            e -= 1;
        }
    }
    if xn {
        -v
    } else {
        v
    }
}

/// An integral double as a value; None when it is not an integer.
/// (ok, negative, magnitude): ok false when v is not an integer.
pub fn mag_from_double(v: double) -> (bool, bool, Vec<int>) {
    if v != v || v == 1.0 / 0.0 || v == -1.0 / 0.0 || v.floor() != v {
        return (false, false, Vec::new());
    }
    let neg = v < 0.0;
    let mut a = if neg { -v } else { v };
    // a = m * 2^e with m below 2^53, where division by base() is exact
    let mut e: int = 0;
    while a >= 9007199254740992.0 {
        a = a / 2.0;
        e += 1;
    }
    let mut limbs: Vec<int> = Vec::new();
    while a >= 1.0 {
        let q = (a / (base() as double)).floor();
        let r = a - q * (base() as double);
        limbs.push(r as int);
        a = q;
    }
    let (n2, m2) = canon(neg, shl_mag(&limbs, e));
    (true, n2, m2)
}

/// Digits in `radix` (2..36, no sign, no prefix; `_` skipped) as a
/// magnitude: (ok, magnitude), ok false when a digit is out of range or
/// there is none.
pub fn mag_from_radix(digits: &str, radix: int) -> (bool, Vec<int>) {
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
            return (false, Vec::new());
        }
        ds.push(d);
    }
    if ds.is_empty() {
        return (false, Vec::new());
    }
    // k digits a chunk of radix^k (at most base()), most significant first
    let (m, k) = radix_chunk(radix);
    let n = ds.len() as int;
    let mut chunks: Vec<int> = Vec::new();
    let mut start: int = 0;
    let first = n % k;
    if first > 0 {
        let mut v: int = 0;
        while start < first {
            v = v * radix + ds[start as usize];
            start += 1;
        }
        chunks.push(v);
    }
    while start < n {
        let mut v: int = 0;
        let mut j: int = 0;
        while j < k {
            v = v * radix + ds[(start + j) as usize];
            j += 1;
        }
        chunks.push(v);
        start += k;
    }
    let mut acc = digits_mag(&chunks, 0, chunks.len(), m);
    trim(&mut acc);
    (true, acc)
}

/// The value written in `radix` (2..36).
pub fn mag_to_radix(xn: bool, a: &Vec<int>, radix: int) -> String {
    if a.is_empty() {
        return String::from("0");
    }
    if radix == 10 {
        return show(xn, a);
    }
    let mut digits: Vec<char> = Vec::new();
    let chars = "0123456789abcdefghijklmnopqrstuvwxyz".chars().collect::<Vec<char>>();
    let (m, k) = radix_chunk(radix);
    let cs = mag_chunks(a, m);
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
    if xn {
        s.push('-');
    }
    let mut i = (digits.len() as int) - 1;
    while i >= 0 {
        s.push(digits[i as usize]);
        i -= 1;
    }
    s
}

/// A string as StringToBigInt: decimal (signed), 0x / 0o / 0b; None when
/// it is not one: (ok, negative, magnitude).
pub fn mag_from_string(s: &str) -> (bool, bool, Vec<int>) {
    let t = String::from(s.trim());
    if t.is_empty() {
        return (true, false, Vec::new());
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
                return (false, false, Vec::new());
            }
            rest.push(cs[i]);
            i += 1;
        }
        let (ok, m) = mag_from_radix(rest.as_str(), radix);
        return (ok, false, m);
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
        return (false, false, Vec::new());
    }
    for c in body.chars() {
        if !(c >= '0' && c <= '9') {
            return (false, false, Vec::new());
        }
    }
    let (ok, m) = mag_from_radix(body.as_str(), 10);
    let (n2, m2) = canon(neg, m);
    (ok, n2, m2)
}

// ---- the same on canonical decimal text, for the callers outside the
// VM's operators (typed arrays, literals, number formatting)

fn result(neg: bool, m: Vec<int>) -> String {
    let (n, m2) = canon(neg, m);
    show(n, &m2)
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

pub fn negate(x: &str) -> String {
    let (n, a) = parse(x);
    result(!n, a)
}

/// -1, 0, 1
pub fn compare(x: &str, y: &str) -> int {
    let (an, a) = parse(x);
    let (bn, b) = parse(y);
    mag_cmp(an, &a, bn, &b)
}

pub fn is_zero(x: &str) -> bool {
    x == "0"
}

/// The value as a double (rounded to nearest).
pub fn to_double(x: &str) -> double {
    let (n, a) = parse(x);
    mag_to_double(n, &a)
}

/// An integral double as a BigInt; "" when it is not an integer.
pub fn from_double(v: double) -> String {
    let (ok, n, m) = mag_from_double(v);
    if !ok {
        return String::new();
    }
    show(n, &m)
}

/// Digits in `radix` as a canonical decimal; "" when a digit is out of
/// range or there is none.
pub fn from_radix(digits: &str, radix: int) -> String {
    let (ok, m) = mag_from_radix(digits, radix);
    if !ok {
        return String::new();
    }
    show(false, &m)
}

/// The value written in `radix` (2..36).
pub fn to_radix(x: &str, radix: int) -> String {
    let (n, a) = parse(x);
    mag_to_radix(n, &a, radix)
}

/// BigInt.asUintN on decimal text.
pub fn as_uint_n(bits: int, x: &str) -> String {
    let (n, a) = parse(x);
    let (rn, r) = mag_as_uint_n(bits, n, &a);
    show(rn, &r)
}

/// BigInt.asIntN on decimal text.
pub fn as_int_n(bits: int, x: &str) -> String {
    let (n, a) = parse(x);
    let (rn, r) = mag_as_int_n(bits, n, &a);
    show(rn, &r)
}

/// A string as StringToBigInt, as decimal text; "" when it is not one.
pub fn from_string(s: &str) -> String {
    let (ok, n, m) = mag_from_string(s);
    if !ok {
        return String::new();
    }
    show(n, &m)
}

/// V8's limit on a BigInt's size, 2^30 bits: a result past it throws
/// "Maximum BigInt size exceeded" before any work is done.
pub const MAX_BITS: double = 1073741824.0;

/// The digits of a canonical decimal (without its sign); 0 when it is not
/// one.
pub fn digit_count(s: &str) -> int {
    let n = s.as_bytes().len() as int;
    if s.starts_with("-") {
        return n - 1;
    }
    n
}

// ---- a large BigInt's key: its sign and a limb's bits in base-32 digits
// (least significant first), one text a value, so the VM interns it

pub fn key_of(neg: bool, a: &Vec<int>) -> String {
    let per = limb_bits() / 5;
    let mut s = String::new();
    s.push(if neg { '-' } else { '+' });
    for x in a.iter() {
        let mut v = *x;
        let mut k: int = 0;
        while k < per {
            s.push(key_char(v & 31));
            v = v >> 5;
            k += 1;
        }
    }
    s
}

fn key_char(d: int) -> char {
    let c: u8 = if d < 10 { (48 + d) as u8 } else { (87 + d) as u8 };
    c as char
}

fn key_digit(b: int) -> int {
    if b >= 97 {
        b - 87
    } else {
        b - 48
    }
}

/// The value of a key.
pub fn key_mag(s: &str) -> (bool, Vec<int>) {
    let per = (limb_bits() / 5) as usize;
    let n = s.as_bytes().len();
    let neg = n > 0 && s.as_bytes()[0] == 45;
    let count = if n > 0 { (n - 1) / per } else { 0 };
    let mut out: Vec<int> = vec![0; count];
    let mut k: usize = 0;
    while k < count {
        let at = 1 + per * k;
        let mut v: int = 0;
        let mut j = per;
        while j > 0 {
            j -= 1;
            v = (v << 5) | key_digit(s.as_bytes()[at + j] as int);
        }
        out[k] = v;
        k += 1;
    }
    (neg, out)
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
// interned C_BIGINT object whose `prim` is its key (key_of: the magnitude
// in binary, not decimal). Each value has one form, so === and SameValue
// compare Bigs by value and objects by identity. class_of gives C_BIGINT
// for both.

use crate::ops::*;
use crate::value::*;
use crate::vm::*;

pub const NF_BIGINT: int = 950;
pub const NF_BIGINT_ASINTN: int = 951;
pub const NF_BIGINT_ASUINTN: int = 952;
pub const NF_BIGINTP_TOSTRING: int = 953;
pub const NF_BIGINTP_VALUEOF: int = 954;

/// The magnitude of a small value (an integer below small_big_limit).
fn small_mag(n: double) -> (bool, Vec<int>) {
    let neg = n < 0.0;
    let mut a = if neg { -n } else { n };
    let mut out: Vec<int> = Vec::new();
    while a >= 1.0 {
        let q = (a / (base() as double)).floor();
        out.push((a - q * (base() as double)) as int);
        a = q;
    }
    (neg, out)
}

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
        let (neg, m) = parse(s);
        self.bigint_of(neg, &m)
    }

    /// The BigInt (negative, magnitude): a Val::Big when it is small, else
    /// the interned object of its key.
    pub fn bigint_of(&mut self, neg: bool, m: &Vec<int>) -> Val {
        // up to 53 bits the value is exact in a double; small when below
        // small_big_limit
        if bit_len(m) <= 53.0 {
            let mut v: double = 0.0;
            let mut i = (m.len() as int) - 1;
            while i >= 0 {
                v = v * (base() as double) + (m[i as usize] as double);
                i -= 1;
            }
            if v < small_big_limit() {
                return self.bigint_small(if neg { -v } else { v });
            }
        }
        let k = key_of(neg && !m.is_empty(), m);
        if let Some(o) = self.bigints.get(k.as_str()) {
            return Val::Obj(*o);
        }
        let bp = self.bigint_proto;
        let o = self.alloc(C_BIGINT, bp);
        let sv0 = self.str_val(k.as_str());
        self.put_prim(o, sv0);
        self.bigints.insert(k, o);
        // the table is weak: a BigInt nothing holds is freed like any object
        // and leaves the table then (release)
        Val::Obj(o)
    }

    /// The small value of a BigInt, when it has one.
    #[inline]
    pub fn big_small_val(&self, v: &Val) -> Option<double> {
        if let VK::Big(n) = v.kind() {
            return Some(n);
        }
        None
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
        let (an, a) = small_mag(n);
        let (bn, b) = small_mag(d);
        let (rn, m) = mag_add(an, &a, bn, &b);
        self.bigint_of(rn, &m)
    }

    /// A BigInt plus a small integer `d` (++, --, the typed paths).
    pub fn big_add_small(&mut self, v: &Val, d: double) -> Val {
        if let Some(n) = self.big_small_val(v) {
            return self.big_step(n, d);
        }
        let (an, a) = self.big_mag(v);
        let (bn, b) = small_mag(d);
        let (rn, m) = mag_add(an, &a, bn, &b);
        self.bigint_of(rn, &m)
    }

    /// -v of a BigInt.
    pub fn big_negate(&mut self, v: &Val) -> Val {
        if let Some(n) = self.big_small_val(v) {
            return self.bigint_small(-n);
        }
        let (an, a) = self.big_mag(v);
        self.bigint_of(!an, &a)
    }

    /// ~v of a BigInt: -v - 1.
    pub fn big_not(&mut self, v: &Val) -> Val {
        let n = self.big_negate(v);
        self.big_add_small(&n, -1.0)
    }

    /// A BigInt being freed leaves the table (when it is the one there).
    pub fn forget_bigint(&mut self, o: int) {
        let k = self.big_key_text(o);
        let mine = match self.bigints.get(k.as_str()) {
            Some(x) => *x == o,
            None => false,
        };
        if mine {
            self.bigints.remove(k.as_str());
        }
    }

    /// The key a large BigInt object is interned by.
    fn big_key_text(&self, o: int) -> String {
        let pv = self.colds[self.objs[o as usize].cold as usize].prim;
        if let Some(s) = pv.as_str() {
            let s = self.sget(s);
            return s.as_ref().clone();
        }
        String::from("+")
    }

    pub fn is_bigint(&self, v: &Val) -> bool {
        v.is_big() || self.class_of(v) == C_BIGINT
    }

    /// (negative, magnitude) of a BigInt value.
    pub fn big_mag(&self, v: &Val) -> (bool, Vec<int>) {
        if let VK::Big(n) = v.kind() {
            return small_mag(n);
        }
        let o = obj_of(v);
        if o >= 0 {
            let pv = self.colds[self.objs[o as usize].cold as usize].prim;
            if let Some(h) = pv.as_str() {
                return key_mag(self.sref(h));
            }
        }
        (false, Vec::new())
    }

    /// The decimal text of a BigInt value.
    pub fn big_text(&self, v: &Val) -> String {
        if let VK::Big(n) = v.kind() {
            return int_text(n as int);
        }
        let (n, m) = self.big_mag(v);
        show(n, &m)
    }

    /// A BigInt as a double (Number(b), rounded to nearest).
    pub fn big_to_double(&self, v: &Val) -> double {
        if let VK::Big(n) = v.kind() {
            return n;
        }
        let (n, m) = self.big_mag(v);
        mag_to_double(n, &m)
    }

    /// The map key of a BigInt (Map, Set): one text a value.
    pub fn big_map_key(&self, v: &Val) -> String {
        if let VK::Big(n) = v.kind() {
            return format!("g{}", int_text(n as int));
        }
        let o = obj_of(v);
        if o >= 0 {
            return format!("G{}", self.big_key_text(o));
        }
        String::from("g0")
    }

    /// ToBigInt.
    pub fn to_bigint(&mut self, v: &Val) -> Val {
        let p = self.to_primitive(v, "number");
        if self.throwing {
            return Val::Undef();
        }
        if p.is_bool() {
            return self.bigint_small(if p.boolv() { 1.0 } else { 0.0 });
        } else if let Some(s) = (&p).as_str() {
            let s = self.sget(s.clone());
            let (ok, n, m) = mag_from_string(s.as_str());
            if !ok {
                self.throw_syntax(format!("Cannot convert {} to a BigInt", s).as_str());
                return Val::Undef();
            }
            return self.bigint_of(n, &m);
        }
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
        let (an, x) = self.big_mag(a);
        let (bn, y) = self.big_mag(b);
        if self.big_too_big(code, an, &x, bn, &y) {
            self.throw_range("Maximum BigInt size exceeded");
            return Val::Undef();
        }
        let (rn, r) = if code == OP_ADD {
            mag_add(an, &x, bn, &y)
        } else if code == OP_SUB {
            mag_sub(an, &x, bn, &y)
        } else if code == OP_MUL {
            mag_mul(an, &x, bn, &y)
        } else if code == OP_DIV || code == OP_MOD {
            if y.is_empty() {
                self.throw_range("Division by zero");
                return Val::Undef();
            }
            if code == OP_DIV { mag_div(an, &x, bn, &y) } else { mag_rem(an, &x, &y) }
        } else if code == OP_EXP {
            if bn {
                self.throw_range("Exponent must be non-negative");
                return Val::Undef();
            }
            let one = x.len() == 1 && x[0] == 1;
            if y.is_empty() {
                (false, vec![1])
            } else if x.is_empty() {
                (false, Vec::new())
            } else if one {
                // (-1) ** y: the sign of y's parity
                (an && (y[0] & 1) == 1, vec![1])
            } else {
                mag_pow(an, &x, mag_to_double(false, &y))
            }
        } else if code == OP_SHL || code == OP_SHR {
            let k = mag_to_double(false, &y);
            let left = (code == OP_SHL) != bn;
            if x.is_empty() {
                (false, Vec::new())
            } else if left {
                mag_shl(an, &x, k as int)
            } else if k >= bit_len(&x) {
                // every bit shifted out: 0, or -1 for a negative x (floor)
                if an { (true, vec![1]) } else { (false, Vec::new()) }
            } else {
                mag_shr(an, &x, k as int)
            }
        } else if code == OP_BAND {
            mag_bitop(0, an, &x, bn, &y)
        } else if code == OP_BOR {
            mag_bitop(1, an, &x, bn, &y)
        } else if code == OP_BXOR {
            mag_bitop(2, an, &x, bn, &y)
        } else {
            self.throw_type("BigInts have no unsigned right shift, use >> instead");
            return Val::Undef();
        };
        self.bigint_of(rn, &r)
    }

    /// Whether a BigInt operation's result would pass MAX_BITS, from the
    /// operands' sizes alone (lower bounds: a result that fits is never
    /// refused).
    fn big_too_big(&self, code: int, an: bool, x: &Vec<int>, bn: bool, y: &Vec<int>) -> bool {
        if x.is_empty() {
            return false;
        }
        let lx = bit_len(x) - 1.0;
        if code == OP_MUL {
            return !y.is_empty() && lx + (bit_len(y) - 1.0) > MAX_BITS;
        }
        if code == OP_SHL || code == OP_SHR {
            let left = (code == OP_SHL) != bn;
            return left && lx + mag_to_double(false, y) > MAX_BITS;
        }
        if code == OP_EXP {
            let one = x.len() == 1 && x[0] == 1;
            if one || bn {
                return false;
            }
            // |x| >= 2: at least one bit per unit of the exponent
            let per = if lx > 1.0 { lx } else { 1.0 };
            let _ = an;
            return mag_to_double(false, y) * per > MAX_BITS;
        }
        false
    }

    /// `<` with a BigInt on either side: 1, 0, or -1 (undefined).
    pub fn big_less(&mut self, a: &Val, b: &Val) -> int {
        let (oka, an, xa) = self.big_side(a);
        let (okb, bn, xb) = self.big_side(b);
        if oka && okb {
            return if mag_cmp(an, &xa, bn, &xb) < 0 { 1 } else { 0 };
        }
        // a BigInt and a Number: compared exactly, the Number's integer
        // part as a BigInt (an infinity or NaN decides alone)
        if self.is_bigint(a) && b.is_num() {
            return self.big_less_num(a, b.num(), false);
        }
        if self.is_bigint(b) && a.is_num() {
            return self.big_less_num(b, a.num(), true);
        }
        let na = if self.is_bigint(a) { self.big_to_double(a) } else { self.to_number(a) };
        let nb = if self.is_bigint(b) { self.big_to_double(b) } else { self.to_number(b) };
        if na != na || nb != nb {
            -1
        } else if na < nb {
            1
        } else {
            0
        }
    }

    /// big < n (or n < big when `flip`): 1, 0, -1 for NaN.
    fn big_less_num(&mut self, big: &Val, n: double, flip: bool) -> int {
        if n != n {
            return -1;
        }
        if n == 1.0 / 0.0 {
            return if flip { 0 } else { 1 };
        }
        if n == -1.0 / 0.0 {
            return if flip { 1 } else { 0 };
        }
        let (bn, bm) = self.big_mag(big);
        let fl = n.floor();
        let (_, nn, nm) = mag_from_double(fl);
        let c = mag_cmp(bn, &bm, nn, &nm);
        if !flip {
            // big < n: big < floor(n), or big == floor(n) < n
            if c < 0 || (c == 0 && fl < n) { 1 } else { 0 }
        } else {
            // n < big: floor(n) < big (n itself <= its floor + fraction)
            if c > 0 { 1 } else { 0 }
        }
    }

    /// A BigInt's value, or a string's as a BigInt: (ok, negative,
    /// magnitude); ok false for other values (and for a string that is not
    /// one).
    fn big_side(&mut self, v: &Val) -> (bool, bool, Vec<int>) {
        if self.is_bigint(v) {
            let (n, m) = self.big_mag(v);
            return (true, n, m);
        }
        if let Some(s) = v.as_str() {
            let s = self.sget(s.clone());
            return mag_from_string(s.as_str());
        }
        (false, false, Vec::new())
    }

    /// `==` with a BigInt on one side (the other primitive).
    pub fn big_loose_eq(&mut self, a: &Val, b: &Val) -> bool {
        let (big, other) = if self.is_bigint(a) { (a, b) } else { (b, a) };
        let (xn, x) = self.big_mag(big);
        if let Some(n) = other.as_num() {
            let (ok, tn, t) = mag_from_double(n);
            ok && mag_cmp(tn, &t, xn, &x) == 0
        } else if let Some(s) = other.as_str() {
            let s = self.sget(s.clone());
            let (ok, tn, t) = mag_from_string(s.as_str());
            ok && mag_cmp(tn, &t, xn, &x) == 0
        } else if other.is_bool() {
            let want: Vec<int> = if other.boolv() { vec![1] } else { Vec::new() };
            !xn && cmp_mag(&x, &want) == 0
        } else {
            if self.is_bigint(other) {
                let (yn, y) = self.big_mag(other);
                return mag_cmp(xn, &x, yn, &y) == 0;
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
                let (ok, tn, t) = mag_from_double(n);
                if !ok {
                    let ns = self.to_string(&p);
                    self.throw_range(format!("The number {} cannot be converted to a BigInt because it is not an integer", ns).as_str());
                    return Val::Undef();
                }
                return self.bigint_of(tn, &t);
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
            let (xn, m) = self.big_mag(&x);
            let bl = bit_len(&m);
            // x fits in the bits: x itself, decided on the double (an int
            // may be 32 bits on some targets)
            let fits = if id == NF_BIGINT_ASINTN { bits - 1.0 >= bl } else { !xn && bits >= bl };
            if fits {
                return x;
            }
            // a negative x modulo 2^bits needs all of the bits
            if id == NF_BIGINT_ASUINTN && xn && bits > MAX_BITS {
                self.throw_range("Maximum BigInt size exceeded");
                return Val::Undef();
            }
            let (rn, r) = if id == NF_BIGINT_ASINTN { mag_as_int_n(bits as int, xn, &m) } else { mag_as_uint_n(bits as int, xn, &m) };
            return self.bigint_of(rn, &r);
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
        let (n, m) = self.big_mag(&v);
        self.string_val(mag_to_radix(n, &m, radix as int))
    }
}

/// The magnitude of bytes, most significant first.
pub fn mag_from_bytes(be: &Vec<int>) -> Vec<int> {
    let mut acc: Vec<int> = Vec::new();
    for b in be.iter() {
        acc = shl_mag(&acc, 8);
        if *b != 0 {
            acc = add_mag(&acc, &vec![*b]);
        }
        trim(&mut acc);
    }
    acc
}

/// The low `n` bytes of a magnitude, least significant first.
pub fn mag_low_bytes(m: &Vec<int>, n: int) -> Vec<int> {
    let mut out: Vec<int> = Vec::new();
    let mut x = m.clone();
    let mut j: int = 0;
    while j < n {
        let (q, r) = div_small(&x, 256);
        out.push(r);
        x = q;
        j += 1;
    }
    out
}
