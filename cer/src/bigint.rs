// SPDX-License-Identifier: AGPL-3.0-or-later
//! BigInt arithmetic on decimal strings: a value is its canonical decimal
//! text ("-123", "0"); the work is done on magnitudes as little-endian limbs
//! of base 10000, small enough that a product and its carry fit the 32-bit
//! int of every Ranger target.

use ranger::prelude::*;

const BASE: int = 10000;

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

fn mul_mag(a: &Vec<int>, b: &Vec<int>) -> Vec<int> {
    if a.is_empty() || b.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<int> = Vec::new();
    let mut k: usize = 0;
    while k < a.len() + b.len() {
        out.push(0);
        k += 1;
    }
    let mut i: usize = 0;
    while i < a.len() {
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

/// (quotient, remainder) of magnitudes; b not zero.
fn divmod_mag(a: &Vec<int>, b: &Vec<int>) -> (Vec<int>, Vec<int>) {
    if cmp_mag(a, b) < 0 {
        return (Vec::new(), a.clone());
    }
    let mut q: Vec<int> = Vec::new();
    let mut k: usize = 0;
    while k < a.len() {
        q.push(0);
        k += 1;
    }
    let mut r: Vec<int> = Vec::new();
    let mut i = (a.len() as int) - 1;
    while i >= 0 {
        // r = r * BASE + a[i]
        r.insert(0, a[i as usize]);
        trim(&mut r);
        // the largest d with b * d <= r, by bisection
        let (mut lo, mut hi) = (0, BASE - 1);
        while lo < hi {
            let mid = (lo + hi + 1) / 2;
            if cmp_mag(&mul_small(b, mid), &r) <= 0 {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        if lo > 0 {
            r = sub_mag(&r, &mul_small(b, lo));
        }
        q[i as usize] = lo;
        i -= 1;
    }
    trim(&mut q);
    (q, r)
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
    let mut acc: Vec<int> = Vec::new();
    let mut any = false;
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
        acc = add_mag(&mul_small(&acc, radix), &vec![d]);
        trim(&mut acc);
        any = true;
    }
    if !any {
        return String::new();
    }
    result(false, acc)
}

/// The value written in `radix` (2..36).
pub fn to_radix(x: &str, radix: int) -> String {
    if radix == 10 {
        return String::from(x);
    }
    let (n, mut a) = parse(x);
    if a.is_empty() {
        return String::from("0");
    }
    let mut digits: Vec<char> = Vec::new();
    let chars = "0123456789abcdefghijklmnopqrstuvwxyz".chars().collect::<Vec<char>>();
    while !a.is_empty() {
        let (q, r) = divmod_mag(&a, &vec![radix]);
        let d = if r.is_empty() { 0 } else { r[0] };
        digits.push(chars[d as usize]);
        a = q;
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

/// 2^bits as a decimal string.
pub fn pow2(bits: int) -> String {
    let mut acc: Vec<int> = vec![1];
    let mut i = 0;
    while i < bits {
        acc = mul_small(&acc, 2);
        i += 1;
    }
    result(false, acc)
}

/// BigInt.asUintN: x modulo 2^bits, non-negative.
pub fn as_uint_n(bits: int, x: &str) -> String {
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

// ---- the VM's side: a BigInt is an interned C_BIGINT object whose `prim`
// is its decimal text, so === and SameValue compare them by identity

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
        self.objs[bp as usize].add(a_tag, str_val("BigInt"), P_HIDDEN | P_READONLY);
    }

    /// The BigInt with decimal text `s` (canonical), made once.
    pub fn bigint_val(&mut self, s: &str) -> Val {
        if let Some(o) = self.bigints.get(s) {
            return Val::Obj(*o);
        }
        let bp = self.bigint_proto;
        let o = self.alloc(C_BIGINT, bp);
        self.objs[o as usize].prim = str_val(s);
        self.bigints.insert(String::from(s), o);
        self.symbols.push(o);
        Val::Obj(o)
    }

    pub fn is_bigint(&self, v: &Val) -> bool {
        self.class_of(v) == C_BIGINT
    }

    /// The decimal text of a BigInt value.
    pub fn big_text(&self, v: &Val) -> String {
        let o = obj_of(v);
        if o >= 0 {
            if let Val::Str(s) = &self.objs[o as usize].prim {
                return s.as_ref().clone();
            }
        }
        String::from("0")
    }

    /// ToBigInt.
    pub fn to_bigint(&mut self, v: &Val) -> Val {
        let p = self.to_primitive(v, "number");
        if self.throwing {
            return Val::Undef;
        }
        match &p {
            Val::Bool(b) => return self.bigint_val(if *b { "1" } else { "0" }),
            Val::Str(s) => {
                let t = from_string(s.as_str());
                if t.is_empty() {
                    self.throw_syntax(format!("Cannot convert {} to a BigInt", s).as_str());
                    return Val::Undef;
                }
                return self.bigint_val(t.as_str());
            }
            _ => {}
        }
        if self.is_bigint(&p) {
            return p;
        }
        let what = self.type_of(&p);
        self.throw_type(format!("Cannot convert a {} to a BigInt", what).as_str());
        Val::Undef
    }

    /// thisBigIntValue: a BigInt, or the one a BigInt object holds.
    pub fn this_bigint(&mut self, v: &Val) -> Val {
        if self.is_bigint(v) {
            return v.clone();
        }
        let o = obj_of(v);
        if o >= 0 && self.objs[o as usize].class == C_OBJECT && self.is_bigint(&self.objs[o as usize].prim.clone()) {
            return self.objs[o as usize].prim.clone();
        }
        self.throw_type("BigInt.prototype method called on incompatible receiver");
        Val::Undef
    }

    /// A binary operator with a BigInt operand (after ToPrimitive).
    pub fn big_arith(&mut self, code: int, a: &Val, b: &Val) -> Val {
        if !self.is_bigint(a) || !self.is_bigint(b) {
            self.throw_type("Cannot mix BigInt and other types, use explicit conversions");
            return Val::Undef;
        }
        let x = self.big_text(a);
        let y = self.big_text(b);
        let r = match code {
            OP_ADD => add(x.as_str(), y.as_str()),
            OP_SUB => sub(x.as_str(), y.as_str()),
            OP_MUL => mul(x.as_str(), y.as_str()),
            OP_DIV => div(x.as_str(), y.as_str()),
            OP_MOD => rem(x.as_str(), y.as_str()),
            OP_EXP => pow(x.as_str(), y.as_str()),
            OP_SHL | OP_SHR => {
                let n = to_double(y.as_str());
                let left = (code == OP_SHL) == (n >= 0.0);
                let k = if n < 0.0 { -n } else { n };
                let p = pow2(k as int);
                if left {
                    mul(x.as_str(), p.as_str())
                } else {
                    // floor division
                    let q = div(x.as_str(), p.as_str());
                    let r2 = rem(x.as_str(), p.as_str());
                    if r2.starts_with("-") {
                        sub(q.as_str(), "1")
                    } else {
                        q
                    }
                }
            }
            _ => {
                self.throw_type("BigInts have no unsigned right shift, use >> instead");
                return Val::Undef;
            }
        };
        if r.is_empty() {
            if code == OP_EXP {
                self.throw_range("Exponent must be non-negative");
            } else {
                self.throw_range("Division by zero");
            }
            return Val::Undef;
        }
        self.bigint_val(r.as_str())
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
        if let Val::Str(s) = v {
            return from_string(s.as_str());
        }
        String::new()
    }

    /// `==` with a BigInt on one side (the other primitive).
    pub fn big_loose_eq(&mut self, a: &Val, b: &Val) -> bool {
        let (big, other) = if self.is_bigint(a) { (a, b) } else { (b, a) };
        let x = self.big_text(big);
        match other {
            Val::Num(n) => {
                let t = from_double(*n);
                !t.is_empty() && t == x
            }
            Val::Str(s) => {
                let t = from_string(s.as_str());
                !t.is_empty() && t == x
            }
            Val::Bool(bv) => x.as_str() == (if *bv { "1" } else { "0" }),
            _ => {
                if self.is_bigint(other) {
                    return self.big_text(other) == x;
                }
                false
            }
        }
    }

    pub fn call_native_bigint(&mut self, id: int, this: Val, args: Vec<Val>, construct: bool) -> Val {
        let a0 = if args.is_empty() { Val::Undef } else { args[0].clone() };
        let a1 = if args.len() < 2 { Val::Undef } else { args[1].clone() };
        if id == NF_BIGINT {
            if construct {
                self.throw_type("BigInt is not a constructor");
                return Val::Undef;
            }
            let p = self.to_primitive(&a0, "number");
            if self.throwing {
                return Val::Undef;
            }
            if let Val::Num(n) = p {
                let t = from_double(n);
                if t.is_empty() {
                    let ns = self.to_string(&p);
                    self.throw_range(format!("The number {} cannot be converted to a BigInt because it is not an integer", ns).as_str());
                    return Val::Undef;
                }
                return self.bigint_val(t.as_str());
            }
            return self.to_bigint(&p);
        }
        if id == NF_BIGINT_ASINTN || id == NF_BIGINT_ASUINTN {
            let bits = self.to_number(&a0);
            if self.throwing {
                return Val::Undef;
            }
            if bits != bits || bits < 0.0 || bits > 9007199254740991.0 {
                self.throw_range("Invalid value: not (convertible to) a safe integer");
                return Val::Undef;
            }
            let x = self.to_bigint(&a1);
            if self.throwing {
                return Val::Undef;
            }
            let t = self.big_text(&x);
            let r = if id == NF_BIGINT_ASINTN { as_int_n(bits as int, t.as_str()) } else { as_uint_n(bits as int, t.as_str()) };
            return self.bigint_val(r.as_str());
        }
        let v = self.this_bigint(&this);
        if self.throwing {
            return Val::Undef;
        }
        if id == NF_BIGINTP_VALUEOF {
            return v;
        }
        let radix = if matches!(a0, Val::Undef) { 10.0 } else { self.to_number(&a0) };
        if radix < 2.0 || radix > 36.0 || radix.floor() != radix {
            self.throw_range("toString() radix must be between 2 and 36");
            return Val::Undef;
        }
        let t = self.big_text(&v);
        string_val(to_radix(t.as_str(), radix as int))
    }
}
