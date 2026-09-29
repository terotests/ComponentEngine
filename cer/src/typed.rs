// SPDX-License-Identifier: AGPL-3.0-or-later
//! The native core of ArrayBuffer, the typed arrays and DataView: bytes and
//! the element reads and writes the interpreter's indexing reaches. The
//! constructors and methods are in the prelude, over the helpers here.
//!
//! An ArrayBuffer (C_ARRAYBUFFER) keeps its bytes as numbers in `elems`;
//! `pos` has bit 1 when detached and bit 2 when shared. A typed array
//! (C_TYPED) is a view: `env` the buffer, `pos` the element kind, `func`
//! the byte offset, `prim` the length. Floats are encoded by arithmetic, so
//! every Ranger target writes the same bytes.

use ranger::prelude::*;

use crate::value::*;
use crate::vm::*;

pub const TK_INT8: int = 0;
pub const TK_UINT8: int = 1;
pub const TK_UINT8C: int = 2;
pub const TK_INT16: int = 3;
pub const TK_UINT16: int = 4;
pub const TK_INT32: int = 5;
pub const TK_UINT32: int = 6;
pub const TK_FLOAT32: int = 7;
pub const TK_FLOAT64: int = 8;
pub const TK_BIGINT64: int = 9;
pub const TK_BIGUINT64: int = 10;

pub const NF_BUF_NEW: int = 920;
pub const NF_BUF_INFO: int = 921;
pub const NF_BUF_GET: int = 922;
pub const NF_BUF_SET: int = 923;
pub const NF_BUF_COPY: int = 924;
pub const NF_BUF_DETACH: int = 925;
pub const NF_TA_MAKE: int = 926;
pub const NF_TA_INFO: int = 927;

pub fn kind_size(kind: int) -> int {
    if kind <= TK_UINT8C {
        1
    } else if kind <= TK_UINT16 {
        2
    } else if kind <= TK_UINT32 || kind == TK_FLOAT32 {
        4
    } else {
        8
    }
}

/// 2^e for any e, exactly.
fn two_pow(e: int) -> double {
    let mut r: double = 1.0;
    let mut i: int = 0;
    if e >= 0 {
        while i < e {
            r = r * 2.0;
            i += 1;
        }
    } else {
        while i > e {
            r = r * 0.5;
            i -= 1;
        }
    }
    r
}

/// The bits of `x` as an IEEE 754 float with `ebits` exponent bits and
/// `mbits` mantissa bits: (sign, biased exponent, mantissa as a double).
fn float_parts(x: double, ebits: int, mbits: int) -> (int, int, double) {
    let bias = (1 << (ebits - 1)) - 1;
    let emax = (1 << ebits) - 1;
    if x != x {
        return (0, emax, two_pow(mbits - 1));
    }
    let neg = x < 0.0 || (x == 0.0 && 1.0 / x < 0.0);
    let sign = if neg { 1 } else { 0 };
    let mut a = if neg { -x } else { x };
    if a == 0.0 {
        return (sign, 0, 0.0);
    }
    if a == 1.0 / 0.0 {
        return (sign, emax, 0.0);
    }
    // a = m * 2^e with m in [1, 2)
    let mut e: int = 0;
    while a >= 2.0 {
        if a >= 1.0e16 {
            a = a / 65536.0;
            e += 16;
        } else {
            a = a / 2.0;
            e += 1;
        }
    }
    while a < 1.0 {
        if a < 1.0e-16 {
            a = a * 65536.0;
            e -= 16;
        } else {
            a = a * 2.0;
            e -= 1;
        }
    }
    let full = two_pow(mbits);
    if e + bias <= 0 {
        // subnormal: the mantissa counts units of 2^(1 - bias - mbits)
        let m = round_even(a * two_pow(e + bias - 1 + mbits));
        if m >= full {
            return (sign, 1, 0.0);
        }
        return (sign, 0, m);
    }
    let mut m = round_even((a - 1.0) * full);
    let mut exp = e + bias;
    if m >= full {
        m = 0.0;
        exp += 1;
    }
    if exp >= emax {
        return (sign, emax, 0.0);
    }
    (sign, exp, m)
}

/// Rounds to the nearest integer, ties to even.
fn round_even(x: double) -> double {
    let f = floor(x);
    let d = x - f;
    if d > 0.5 || (d == 0.5 && (f - 2.0 * floor(f / 2.0)) == 1.0) {
        f + 1.0
    } else {
        f
    }
}

/// Math.fround: `x` rounded to the nearest float32.
pub fn fround(x: double) -> double {
    let (sign, exp, mant) = float_parts(x, 8, 23);
    float_from_parts(sign, exp, mant, 8, 23)
}

fn float_from_parts(sign: int, exp: int, mant: double, ebits: int, mbits: int) -> double {
    let bias = (1 << (ebits - 1)) - 1;
    let emax = (1 << ebits) - 1;
    let v = if exp == emax {
        if mant != 0.0 {
            0.0 / 0.0
        } else {
            1.0 / 0.0
        }
    } else if exp == 0 {
        mant * two_pow(1 - bias - mbits)
    } else {
        (1.0 + mant / two_pow(mbits)) * two_pow(exp - bias)
    };
    if sign == 1 {
        -v
    } else {
        v
    }
}

/// `x` as the element kind's integer (modulo, clamped for Uint8Clamped).
fn int_bits(kind: int, x: double) -> double {
    if kind == TK_UINT8C {
        if x != x || x <= 0.0 {
            return 0.0;
        }
        if x >= 255.0 {
            return 255.0;
        }
        // round half to even
        let f = floor(x);
        let d = x - f;
        if d > 0.5 || (d == 0.5 && (f - 2.0 * floor(f / 2.0)) == 1.0) {
            return f + 1.0;
        }
        return f;
    }
    let size = kind_size(kind);
    let m = two_pow(size * 8);
    let u = to_uint32(x);
    let v = if size == 4 { u } else { u - m * floor(u / m) };
    v
}

impl Vm {
    /// A BigInt64 / BigUint64 element at byte `at`.
    pub fn buf_read_big(&mut self, b: int, at: int, kind: int, little: bool) -> Val {
        let mut acc = String::from("0");
        let mut k: int = 0;
        while k < 8 {
            let idx = if little { at + 7 - k } else { at + k };
            let byte = self.byte(b, idx);
            acc = crate::bigint::add(crate::bigint::mul(acc.as_str(), "256").as_str(), format!("{}", byte).as_str());
            k += 1;
        }
        if kind == TK_BIGINT64 {
            acc = crate::bigint::as_int_n(64, acc.as_str());
        }
        self.bigint_val(acc.as_str())
    }

    /// Writes a BigInt (converted already) as 64 bits at byte `at`.
    pub fn buf_write_big(&mut self, b: int, at: int, v: &Val, little: bool) {
        let t = self.big_text(v);
        let mut x = crate::bigint::as_uint_n(64, t.as_str());
        let mut low: Vec<int> = Vec::new();
        let mut j = 0;
        while j < 8 {
            let r = crate::bigint::rem(x.as_str(), "256");
            low.push(crate::bigint::to_double(r.as_str()) as int);
            x = crate::bigint::div(x.as_str(), "256");
            j += 1;
        }
        let mut k: int = 0;
        while k < 8 {
            // low[0] is the least significant byte
            let byte = low[(7 - k) as usize];
            let idx = if little { at + 7 - k } else { at + k };
            self.objs[b as usize].elems[idx as usize] = Val::Num(byte as double);
            k += 1;
        }
    }

    pub fn is_typed(&self, v: &Val) -> bool {
        self.class_of(v) == C_TYPED
    }

    pub fn ta_length(&self, o: int) -> int {
        let b = self.objs[o as usize].env;
        if b < 0 || (self.objs[b as usize].pos & 1) != 0 {
            return 0;
        }
        match self.objs[o as usize].prim {
            Val::Num(n) => n as int,
            _ => 0,
        }
    }

    /// The byte at `i` of buffer `b`.
    fn byte(&self, b: int, i: int) -> int {
        match self.objs[b as usize].elems[i as usize] {
            Val::Num(n) => n as int,
            _ => 0,
        }
    }

    /// Reads a `kind` element at byte `at` of buffer `b`.
    pub fn buf_read(&self, b: int, at: int, kind: int, little: bool) -> double {
        let size = kind_size(kind);
        // the bytes as an unsigned big-endian number, in pieces for floats
        let mut bytes: Vec<int> = Vec::new();
        let mut k: int = 0;
        while k < size {
            let idx = if little { at + size - 1 - k } else { at + k };
            bytes.push(self.byte(b, idx));
            k += 1;
        }
        if kind == TK_FLOAT32 {
            let sign = bytes[0] >> 7;
            let exp = ((bytes[0] & 127) << 1) | (bytes[1] >> 7);
            let mant = ((((bytes[1] & 127) * 256) + bytes[2]) * 256 + bytes[3]) as double;
            return float_from_parts(sign, exp, mant, 8, 23);
        }
        if kind == TK_FLOAT64 {
            let sign = bytes[0] >> 7;
            let exp = ((bytes[0] & 127) << 4) | (bytes[1] >> 4);
            let mut mant: double = (bytes[1] & 15) as double;
            let mut j: usize = 2;
            while j < 8 {
                mant = mant * 256.0 + (bytes[j] as double);
                j += 1;
            }
            return float_from_parts(sign, exp, mant, 11, 52);
        }
        let mut u: double = 0.0;
        for x in bytes.iter() {
            u = u * 256.0 + (*x as double);
        }
        if kind == TK_INT8 || kind == TK_INT16 || kind == TK_INT32 {
            let half = two_pow(size * 8 - 1);
            if u >= half {
                u = u - 2.0 * half;
            }
        }
        u
    }

    /// Writes `x` (a number already) as a `kind` element at byte `at`.
    pub fn buf_write(&mut self, b: int, at: int, kind: int, x: double, little: bool) {
        let size = kind_size(kind);
        let mut bytes: Vec<int> = Vec::new();
        if kind == TK_FLOAT32 {
            let (sign, exp, mant) = float_parts(x, 8, 23);
            let m = mant as int;
            bytes.push((sign << 7) | (exp >> 1));
            bytes.push(((exp & 1) << 7) | ((m >> 16) & 127));
            bytes.push((m >> 8) & 255);
            bytes.push(m & 255);
        } else if kind == TK_FLOAT64 {
            let (sign, exp, mant) = float_parts(x, 11, 52);
            // the 52 mantissa bits: the top 4, then six bytes
            let hi = floor(mant / two_pow(48));
            let mut rest = mant - hi * two_pow(48);
            bytes.push((sign << 7) | (exp >> 4));
            bytes.push(((exp & 15) << 4) | (hi as int));
            let mut low: Vec<int> = Vec::new();
            let mut j = 0;
            while j < 6 {
                let q = floor(rest / 256.0);
                low.push((rest - q * 256.0) as int);
                rest = q;
                j += 1;
            }
            let mut k = 5;
            while k >= 0 {
                bytes.push(low[k as usize]);
                k -= 1;
            }
        } else {
            let mut u = int_bits(kind, x);
            let mut low: Vec<int> = Vec::new();
            let mut j = 0;
            while j < size {
                let q = floor(u / 256.0);
                low.push((u - q * 256.0) as int);
                u = q;
                j += 1;
            }
            let mut k = size - 1;
            while k >= 0 {
                bytes.push(low[k as usize]);
                k -= 1;
            }
        }
        let mut k: int = 0;
        while k < size {
            let idx = if little { at + size - 1 - k } else { at + k };
            self.objs[b as usize].elems[idx as usize] = Val::Num(bytes[k as usize] as double);
            k += 1;
        }
    }

    /// `ta[i]`: undefined outside the view.
    pub fn ta_get(&mut self, o: int, i: int) -> Val {
        if i < 0 || i >= self.ta_length(o) {
            return Val::Undef;
        }
        let kind = self.objs[o as usize].pos;
        let at = self.objs[o as usize].func + i * kind_size(kind);
        let b = self.objs[o as usize].env;
        if kind >= TK_BIGINT64 {
            return self.buf_read_big(b, at, kind, true);
        }
        Val::Num(self.buf_read(b, at, kind, true))
    }

    /// `ta[i] = v`: the value is converted even when `i` is outside.
    pub fn ta_set(&mut self, o: int, i: int, v: &Val) {
        if self.objs[o as usize].pos >= TK_BIGINT64 {
            let big = self.to_bigint(v);
            if self.throwing || i < 0 || i >= self.ta_length(o) {
                return;
            }
            let at = self.objs[o as usize].func + i * 8;
            let b = self.objs[o as usize].env;
            self.buf_write_big(b, at, &big, true);
            return;
        }
        let x = self.to_number(v);
        if self.throwing || i < 0 || i >= self.ta_length(o) {
            return;
        }
        let kind = self.objs[o as usize].pos;
        let at = self.objs[o as usize].func + i * kind_size(kind);
        let b = self.objs[o as usize].env;
        self.buf_write(b, at, kind, x, true);
    }

    fn buffer_of(&mut self, v: &Val) -> int {
        let b = obj_of(v);
        if b < 0 || self.objs[b as usize].class != C_ARRAYBUFFER {
            self.throw_type("not an ArrayBuffer");
            return -1;
        }
        b
    }

    /// The prelude's helpers (`__buf*`, `__ta*`), taken off the global
    /// object by the prelude.
    pub fn setup_typed(&mut self) {
        let g = self.global;
        self.method(g, "__bufNew", NF_BUF_NEW, 3);
        self.method(g, "__bufInfo", NF_BUF_INFO, 1);
        self.method(g, "__bufGet", NF_BUF_GET, 4);
        self.method(g, "__bufSet", NF_BUF_SET, 5);
        self.method(g, "__bufCopy", NF_BUF_COPY, 5);
        self.method(g, "__bufDetach", NF_BUF_DETACH, 1);
        self.method(g, "__taMake", NF_TA_MAKE, 5);
        self.method(g, "__taInfo", NF_TA_INFO, 1);
    }

    pub fn call_native_typed(&mut self, id: int, args: Vec<Val>) -> Val {
        let mut a: Vec<Val> = args;
        while a.len() < 5 {
            a.push(Val::Undef);
        }
        if id == NF_BUF_NEW {
            // (byteLength, prototype, shared)
            let n = num(&a[0]) as int;
            if n > 1000000000 {
                self.throw_range("Array buffer allocation failed");
                return Val::Undef;
            }
            let proto = obj_of(&a[1]);
            let b = self.alloc(C_ARRAYBUFFER, proto);
            let mut bytes: Vec<Val> = Vec::with_capacity(n as usize);
            let mut i = 0;
            while i < n {
                bytes.push(Val::Num(0.0));
                i += 1;
            }
            self.objs[b as usize].elems = bytes;
            if truthy(&a[2]) {
                self.objs[b as usize].pos = 2;
            }
            return Val::Obj(b);
        }
        if id == NF_BUF_INFO {
            // byteLength; -1 not a buffer, -2 detached; +0.5 when shared
            let b = obj_of(&a[0]);
            if b < 0 || self.objs[b as usize].class != C_ARRAYBUFFER {
                return Val::Num(-1.0);
            }
            let pos = self.objs[b as usize].pos;
            if (pos & 1) != 0 {
                return Val::Num(-2.0);
            }
            let n = self.objs[b as usize].elems.len() as double;
            return Val::Num(if (pos & 2) != 0 { n + 0.5 } else { n });
        }
        if id == NF_BUF_GET {
            // (buffer, byteIndex, kind, littleEndian)
            let b = self.buffer_of(&a[0]);
            if b < 0 {
                return Val::Undef;
            }
            let kind = num(&a[2]) as int;
            let at = num(&a[1]) as int;
            if at < 0 || at + kind_size(kind) > self.objs[b as usize].elems.len() as int {
                self.throw_range("Offset is outside the bounds of the DataView");
                return Val::Undef;
            }
            if kind >= TK_BIGINT64 {
                return self.buf_read_big(b, at, kind, truthy(&a[3]));
            }
            return Val::Num(self.buf_read(b, at, kind, truthy(&a[3])));
        }
        if id == NF_BUF_SET {
            // (buffer, byteIndex, kind, number, littleEndian)
            let b = self.buffer_of(&a[0]);
            if b < 0 {
                return Val::Undef;
            }
            let kind = num(&a[2]) as int;
            let at = num(&a[1]) as int;
            if at < 0 || at + kind_size(kind) > self.objs[b as usize].elems.len() as int {
                self.throw_range("Offset is outside the bounds of the DataView");
                return Val::Undef;
            }
            if kind >= TK_BIGINT64 {
                let v = a[3].clone();
                self.buf_write_big(b, at, &v, truthy(&a[4]));
                return Val::Undef;
            }
            let x = num(&a[3]);
            self.buf_write(b, at, kind, x, truthy(&a[4]));
            return Val::Undef;
        }
        if id == NF_BUF_COPY {
            // (from, fromByte, to, toByte, count), overlapping allowed
            let s = self.buffer_of(&a[0]);
            let d = self.buffer_of(&a[2]);
            if s < 0 || d < 0 {
                return Val::Undef;
            }
            let (fs, ts, n) = (num(&a[1]) as int, num(&a[3]) as int, num(&a[4]) as int);
            let mut tmp: Vec<Val> = Vec::new();
            let mut i = 0;
            while i < n {
                tmp.push(self.objs[s as usize].elems[(fs + i) as usize].clone());
                i += 1;
            }
            i = 0;
            while i < n {
                self.objs[d as usize].elems[(ts + i) as usize] = tmp[i as usize].clone();
                i += 1;
            }
            return Val::Undef;
        }
        if id == NF_BUF_DETACH {
            let b = self.buffer_of(&a[0]);
            if b >= 0 {
                self.objs[b as usize].elems = Vec::new();
                self.objs[b as usize].pos |= 1;
            }
            return Val::Undef;
        }
        if id == NF_TA_MAKE {
            // (kind, buffer, byteOffset, length, prototype)
            let b = self.buffer_of(&a[1]);
            if b < 0 {
                return Val::Undef;
            }
            let proto = obj_of(&a[4]);
            let t = self.alloc(C_TYPED, proto);
            self.objs[t as usize].pos = num(&a[0]) as int;
            self.objs[t as usize].env = b;
            self.objs[t as usize].func = num(&a[2]) as int;
            self.objs[t as usize].prim = Val::Num(num(&a[3]));
            return Val::Obj(t);
        }
        if id == NF_TA_INFO {
            // [kind, buffer, byteOffset, length], or undefined
            let t = obj_of(&a[0]);
            if t < 0 || self.objs[t as usize].class != C_TYPED {
                return Val::Undef;
            }
            let kind = Val::Num(self.objs[t as usize].pos as double);
            let buf = obj_val_of(self.objs[t as usize].env);
            let off = Val::Num(self.objs[t as usize].func as double);
            let len = Val::Num(self.ta_length(t) as double);
            let arr = self.new_array(vec![kind, buf, off, len]);
            return Val::Obj(arr);
        }
        Val::Undef
    }
}

fn floor(x: double) -> double {
    x.floor()
}

fn to_uint32(x: double) -> double {
    crate::num::to_uint32(x)
}

fn num(v: &Val) -> double {
    match v {
        Val::Num(n) => *n,
        _ => 0.0,
    }
}

fn obj_val_of(o: int) -> Val {
    if o >= 0 {
        Val::Obj(o)
    } else {
        Val::Undef
    }
}
