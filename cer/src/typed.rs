// SPDX-License-Identifier: AGPL-3.0-or-later
//! The native core of ArrayBuffer, the typed arrays and DataView: bytes and
//! the element reads and writes the interpreter's indexing reaches. The
//! constructors and methods are in the prelude, over the helpers here.
//!
//! An ArrayBuffer (C_ARRAYBUFFER) keeps its contents in `nums`; `pos` has
//! bit 1 when detached and bit 2 when shared. `home` says what `nums`
//! holds: the bytes (TK_UINT8), the bytes of a buffer nothing has read or
//! written yet (-1, all zero), or the elements of one kind. The first
//! typed array over the whole of a new buffer makes it hold its elements
//! (`new Float64Array(n)`: one double per element, read and written as they
//! are); anything else that looks at it -- a view of another kind, a
//! DataView, a copy that is not whole elements -- turns it into bytes for
//! good. A typed array (C_TYPED) is a view: `env` the buffer, `pos` the
//! element kind, `func` the byte offset, `prim` the length. Floats are
//! encoded by arithmetic, so every Ranger target writes the same bytes.

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
pub const TK_FLOAT16: int = 11;

/// A BigInt64 / BigUint64 kind.
pub fn is_big(kind: int) -> bool {
    kind == TK_BIGINT64 || kind == TK_BIGUINT64
}

pub const NF_BUF_NEW: int = 920;
pub const NF_BUF_INFO: int = 921;
pub const NF_BUF_GET: int = 922;
pub const NF_BUF_SET: int = 923;
pub const NF_BUF_COPY: int = 924;
pub const NF_BUF_DETACH: int = 925;
pub const NF_TA_MAKE: int = 926;
pub const NF_TA_INFO: int = 927;
pub const NF_BUF_MAX: int = 928;
pub const NF_BUF_RESIZE: int = 929;
/// TypedArray.prototype's `length` getter, and `set`'s copy from a typed
/// array or a plain array (call_native_ta)
pub const NF_TA_LEN: int = 970;
pub const NF_TA_SET: int = 971;
/// DataView: __dvMake(view, buffer, byteOffset, byteLength or -1) marks a
/// new view (C_DATAVIEW); __dvGet(view, index, kind, little) and
/// __dvSet(view, index, kind, value, little) do get/set for a number kind,
/// an integer index and a view in bounds (undefined / false otherwise: the
/// prelude's checks and errors then)
pub const NF_DV_MAKE: int = 972;
pub const NF_DV_GET: int = 973;
pub const NF_DV_SET: int = 974;
/// __taKind(o): o's element kind when it is a typed array __taSub can
/// take (fixed length, in bounds, buffer neither detached nor resizable),
/// -1 otherwise.
/// __taSub(view, start, end, prototype): `view.subarray(start, end)` with
/// the default species, for start/end undefined or numbers and a view in
/// bounds over a fixed buffer (undefined otherwise: the prelude's path).
/// __taNew(kind, length, prototype, ArrayBuffer.prototype): `new
/// XArray(length)` for an integer length (undefined otherwise).
pub const NF_TA_KIND: int = 975;
pub const NF_TA_SUB: int = 976;
pub const NF_TA_NEW: int = 977;
/// __taSort(view): sorts a number-kind view as the default comparator
/// does (NaN last, -0 before +0) and gives true; false for anything
/// __taKind refuses or a BigInt kind (the prelude sorts those).
pub const NF_TA_SORT: int = 978;
/// __dvMethod(kind, slow, isSet, name, length): DataView.prototype's
/// get<Kind> / set<Kind> as a native (NF_DVM_GET / NF_DVM_SET, the kind in
/// its pos, `slow` in its env): a number kind, an integer index and a view
/// in bounds are done in place (Vm::native_fast), anything else calls
/// `slow` (the prelude's checks and errors).
pub const NF_DV_METHOD: int = 979;
pub const NF_DVM_GET: int = 980;
pub const NF_DVM_SET: int = 981;
/// a prelude method (its function in env) whose plain cases a native runs
/// (its id in pos): __fastWrap(native, slowFn, name, length), fast_wrap
pub const NF_FAST_WRAP: int = 982;

/// The default typed array order: NaN last, -0 before +0.
pub fn num_before(a: double, b: double) -> bool {
    if a != a {
        return false;
    }
    if b != b {
        return true;
    }
    if a < b {
        return true;
    }
    if a == 0.0 && b == 0.0 {
        return 1.0 / a < 0.0 && 1.0 / b > 0.0;
    }
    false
}

/// `v` sorted by num_before (a merge sort: stable, n log n).
pub fn sort_nums(v: &mut Vec<double>) {
    let n = v.len();
    let mut tmp: Vec<double> = v.clone();
    let mut w: usize = 1;
    while w < n {
        let mut lo: usize = 0;
        while lo < n {
            let mid = if lo + w < n { lo + w } else { n };
            let hi = if lo + 2 * w < n { lo + 2 * w } else { n };
            let mut i = lo;
            let mut j = mid;
            let mut k = lo;
            while k < hi {
                if i < mid && (j >= hi || !num_before(v[j], v[i])) {
                    tmp[k] = v[i];
                    i += 1;
                } else {
                    tmp[k] = v[j];
                    j += 1;
                }
                k += 1;
            }
            lo = hi;
        }
        let mut q: usize = 0;
        while q < n {
            v[q] = tmp[q];
            q += 1;
        }
        w = w * 2;
    }
}

/// n zeros (a new buffer's contents). Rust: one zeroed allocation.
#[ranger::target(rust)]
pub fn zeros(n: int) -> Vec<double> {
    vec![0.0; n.max(0) as usize]
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn zeros(n: int) -> Vec<double> {
    let mut z: Vec<double> = Vec::new();
    let mut i = 0;
    while i < n {
        z.push(0.0);
        i += 1;
    }
    z
}

/// colds[tc].nums[d0..d0 + n] = colds[sc].nums[s0..s0 + n] (the two may be
/// one buffer and overlap). Rust: one memmove.
#[ranger::target(rust)]
pub fn copy_nums(colds: &mut Vec<Cold>, sc: usize, s0: usize, tc: usize, d0: usize, n: usize) {
    if sc == tc {
        colds[sc].nums.copy_within(s0..s0 + n, d0);
    } else if sc < tc {
        let (lo, hi) = colds.split_at_mut(tc);
        hi[0].nums[d0..d0 + n].copy_from_slice(&lo[sc].nums[s0..s0 + n]);
    } else {
        let (lo, hi) = colds.split_at_mut(sc);
        lo[tc].nums[d0..d0 + n].copy_from_slice(&hi[0].nums[s0..s0 + n]);
    }
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn copy_nums(colds: &mut Vec<Cold>, sc: usize, s0: usize, tc: usize, d0: usize, n: usize) {
    let mut tmp: Vec<double> = Vec::new();
    let mut k: usize = 0;
    while k < n {
        tmp.push(colds[sc].nums[s0 + k]);
        k += 1;
    }
    let mut k2: usize = 0;
    while k2 < n {
        colds[tc].nums[d0 + k2] = tmp[k2];
        k2 += 1;
    }
}

/// log2 of kind_size.
pub fn kind_shift(kind: int) -> int {
    if kind == TK_FLOAT16 {
        return 1;
    }
    if kind <= TK_UINT8C {
        0
    } else if kind <= TK_UINT16 {
        1
    } else if kind <= TK_UINT32 || kind == TK_FLOAT32 {
        2
    } else {
        3
    }
}

pub fn kind_size(kind: int) -> int {
    if kind == TK_FLOAT16 {
        return 2;
    }
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
    let r: double = ranger::native!(
        rust: { fround_parts(x) },
        es6: "Math.fround({x})",
        go: "float64(float32({x}))",
        cpp: "(double)(float)({x})",
    );
    r
}

/// Math.f16round: `x` rounded to the nearest float16.
pub fn f16round(x: double) -> double {
    let (sign, exp, mant) = float_parts(x, 5, 10);
    float_from_parts(sign, exp, mant, 5, 10)
}

fn fround_parts(x: double) -> double {
    let (sign, exp, mant) = float_parts(x, 8, 23);
    float_from_parts(sign, exp, mant, 8, 23)
}

/// What a `kind` element reads back after `x` (a number) is written.
fn element_value(kind: int, x: double) -> double {
    if kind == TK_FLOAT64 {
        return x;
    }
    if kind == TK_FLOAT32 {
        return fround(x);
    }
    if kind == TK_FLOAT16 {
        return f16round(x);
    }
    // an integer the kind holds as it is (the common case): no modulo
    if x == floor(x) {
        if x == 0.0 {
            return 0.0;
        }
        let lo = if kind == TK_INT8 {
            -128.0
        } else if kind == TK_INT16 {
            -32768.0
        } else if kind == TK_INT32 {
            -2147483648.0
        } else {
            0.0
        };
        let hi = if kind == TK_INT8 {
            127.0
        } else if kind == TK_UINT8 || kind == TK_UINT8C {
            255.0
        } else if kind == TK_INT16 {
            32767.0
        } else if kind == TK_UINT16 {
            65535.0
        } else if kind == TK_INT32 {
            2147483647.0
        } else {
            4294967295.0
        };
        if x >= lo && x <= hi {
            return x;
        }
    }
    let u = int_bits(kind, x);
    if kind == TK_INT8 || kind == TK_INT16 || kind == TK_INT32 {
        let half = two_pow(kind_size(kind) * 8 - 1);
        if u >= half {
            return u - 2.0 * half;
        }
    }
    u
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

/// The bytes of `x` (a number already) as a `kind` element, most
/// significant first.
fn encode(kind: int, x: double) -> Vec<int> {
    let size = kind_size(kind);
    let mut bytes: Vec<int> = Vec::new();
    if kind == TK_FLOAT16 {
        let (sign, exp, mant) = float_parts(x, 5, 10);
        let m = mant as int;
        bytes.push((sign << 7) | (exp << 2) | ((m >> 8) & 3));
        bytes.push(m & 255);
    } else if kind == TK_FLOAT32 {
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
    bytes
}

impl Vm {
    /// A BigInt64 / BigUint64 element at byte `at`.
    pub fn buf_read_big(&mut self, b: int, at: int, kind: int, little: bool) -> Val {
        self.buf_to_bytes(b);
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
        self.buf_to_bytes(b);
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
            { let c = self.cold_id(b) as usize; self.colds[c].nums[idx as usize] = byte as double; }
            k += 1;
        }
    }

    pub fn is_typed(&self, v: &Val) -> bool {
        self.class_of(v) == C_TYPED
    }

    pub fn ta_length(&self, o: int) -> int {
        let b = self.objs[o as usize].env as int;
        if b < 0 || (self.objs[b as usize].pos & 1) != 0 {
            return 0;
        }
        let fixed = match self.colds[self.objs[o as usize].cold as usize].prim {
            Val::Num(n) => n as int,
            _ => -1,
        };
        if fixed >= 0 && !self.buf_resizable(b) {
            return fixed;
        }
        // over a resizable buffer: out of bounds is length 0; a length-
        // tracking view (no length) covers the rest of the buffer
        let size = kind_size(self.objs[o as usize].pos as int);
        let off = self.objs[o as usize].func as int;
        let bs = self.buf_size(b);
        if fixed >= 0 {
            return if off + fixed * size > bs { 0 } else { fixed };
        }
        if off > bs {
            0
        } else {
            (bs - off) / size
        }
    }

    /// Whether typed array `o` is out of bounds (or its buffer detached).
    pub fn ta_out_of_bounds(&self, o: int) -> bool {
        let b = self.objs[o as usize].env as int;
        if b < 0 || (self.objs[b as usize].pos & 1) != 0 {
            return true;
        }
        if !self.buf_resizable(b) {
            return false;
        }
        let size = kind_size(self.objs[o as usize].pos as int);
        let off = self.objs[o as usize].func as int;
        let bs = self.buf_size(b);
        match self.colds[self.objs[o as usize].cold as usize].prim {
            Val::Num(n) => off + (n as int) * size > bs,
            _ => off > bs,
        }
    }

    /// A resizable ArrayBuffer (or growable SharedArrayBuffer) keeps its
    /// maximum byte length in `prim`.
    /// Every byte of buffer `b` (holding bytes) is 0.
    fn buf_zero(&self, b: int) -> bool {
        let c = self.objs[b as usize].cold as usize;
        let n = self.colds[c].nums.len();
        let mut i: usize = 0;
        while i < n {
            if self.colds[c].nums[i] != 0.0 {
                return false;
            }
            i += 1;
        }
        true
    }

    pub fn buf_resizable(&self, b: int) -> bool {
        matches!(self.colds[self.objs[b as usize].cold as usize].prim, Val::Num(_))
    }

    /// The byte at `i` of buffer `b`, which holds bytes.
    fn byte(&self, b: int, i: int) -> int {
        self.colds[self.objs[b as usize].cold as usize].nums[i as usize] as int
    }

    /// The byte length of buffer `b`.
    pub fn buf_size(&self, b: int) -> int {
        let mode = self.colds[self.objs[b as usize].cold as usize].home;
        let n = self.colds[self.objs[b as usize].cold as usize].nums.len() as int;
        if mode < 0 {
            n
        } else {
            n * kind_size(mode)
        }
    }

    /// Makes buffer `b` hold its bytes, if it holds elements.
    pub fn buf_to_bytes(&mut self, b: int) {
        let mode = self.colds[self.objs[b as usize].cold as usize].home;
        if mode < 0 || mode == TK_UINT8 {
            let cid = self.cold_id(b) as usize;
            self.colds[cid].home = TK_UINT8;
            return;
        }
        let size = kind_size(mode);
        let elems = self.colds[self.objs[b as usize].cold as usize].nums.clone();
        let mut bytes: Vec<double> = Vec::with_capacity(elems.len() * (size as usize));
        for x in elems.iter() {
            let be = encode(mode, *x);
            let mut k: int = size - 1;
            while k >= 0 {
                bytes.push(be[k as usize] as double);
                k -= 1;
            }
        }
        { let c = self.cold_id(b) as usize; self.colds[c].nums = bytes; }
        let cid = self.cold_id(b) as usize;
        self.colds[cid].home = TK_UINT8;
    }

    /// Reads a `kind` element at byte `at` of buffer `b`.
    pub fn buf_read(&mut self, b: int, at: int, kind: int, little: bool) -> double {
        self.buf_to_bytes(b);
        let size = kind_size(kind);
        // the bytes as an unsigned big-endian number, in pieces for floats
        let mut bytes: Vec<int> = Vec::new();
        let mut k: int = 0;
        while k < size {
            let idx = if little { at + size - 1 - k } else { at + k };
            bytes.push(self.byte(b, idx));
            k += 1;
        }
        if kind == TK_FLOAT16 {
            let sign = bytes[0] >> 7;
            let exp = (bytes[0] >> 2) & 31;
            let mant = (((bytes[0] & 3) * 256) + bytes[1]) as double;
            return float_from_parts(sign, exp, mant, 5, 10);
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
        self.buf_to_bytes(b);
        let size = kind_size(kind);
        let bytes = encode(kind, x);
        let mut k: int = 0;
        while k < size {
            let idx = if little { at + size - 1 - k } else { at + k };
            { let c = self.cold_id(b) as usize; self.colds[c].nums[idx as usize] = bytes[k as usize] as double; }
            k += 1;
        }
    }

    /// `ta[i]`: undefined outside the view.
    pub fn ta_get(&mut self, o: int, i: int) -> Val {
        if i < 0 || i >= self.ta_length(o) {
            return Val::Undef;
        }
        let kind = self.objs[o as usize].pos as int;
        let size = kind_size(kind);
        let at = self.objs[o as usize].func + (i * size) as i32;
        let b = self.objs[o as usize].env as int;
        let mode = self.colds[self.objs[b as usize].cold as usize].home;
        if mode == kind {
            return Val::Num(self.colds[self.objs[b as usize].cold as usize].nums[(at / size as i32) as usize]);
        }
        if mode < 0 && kind == TK_UINT8 {
            return Val::Num(self.colds[self.objs[b as usize].cold as usize].nums[at as usize]);
        }
        if is_big(kind) {
            return self.buf_read_big(b, at as int, kind, true);
        }
        Val::Num(self.buf_read(b, at as int, kind, true))
    }

    /// Where `ta[i]` is in its buffer's `nums` when the read or write needs
    /// nothing else: a fixed-length view over a buffer that is neither
    /// detached nor resizable, `i` inside it, and the buffer holding its
    /// elements as this view's kind (or as bytes, for a Uint8Array). -1
    /// otherwise; ta_get / ta_set do the rest. The register machine's
    /// R_GET_ELEM / R_SET_ELEM and get_index / set_index take it first.
    #[inline]
    pub fn ta_slot(&self, o: int, i: int) -> int {
        let ob = crate::rvm::obj_at(&self.objs, o as usize);
        let b = ob.env as int;
        if b < 0 || i < 0 {
            return -1;
        }
        let bo = crate::rvm::obj_at(&self.objs, b as usize);
        if (bo.pos & 1) != 0 || bo.cold <= 0 {
            return -1;
        }
        // the fixed length as a number: i (an int) compared without a
        // conversion back from double
        let inside = match crate::rvm::cold_at(&self.colds, ob.cold as usize).prim {
            Val::Num(n) => (i as double) < n,
            _ => false,
        };
        if !inside {
            return -1;
        }
        let bc = crate::rvm::cold_at(&self.colds, bo.cold as usize);
        if matches!(bc.prim, Val::Num(_)) {
            return -1;
        }
        let kind = ob.pos as int;
        if bc.home == kind {
            // a view's byte offset is a multiple of its element size
            return ((ob.func as int) >> kind_shift(kind)) + i;
        }
        if bc.home < 0 && kind == TK_UINT8 {
            return (ob.func as int) + i;
        }
        -1
    }

    /// `ta[i]` as ta_slot finds it (a buffer's Cold and the slot in its
    /// nums): the number there, for an s >= 0 from ta_slot.
    #[inline(always)]
    pub fn ta_num_at(&self, o: int, s: int) -> double {
        let bo = crate::rvm::obj_at(&self.objs, crate::rvm::obj_at(&self.objs, o as usize).env as usize);
        crate::rvm::cold_at(&self.colds, bo.cold as usize).nums[s as usize]
    }

    /// `ta[i]` when ta_slot finds it, else ta_get.
    #[inline]
    pub fn ta_get_fast(&mut self, o: int, i: int) -> Val {
        let s = self.ta_slot(o, i);
        if s >= 0 {
            return Val::Num(self.ta_num_at(o, s));
        }
        self.ta_get(o, i)
    }

    /// `ta[i] = x` for a number when ta_slot finds the element: an integer
    /// the kind holds is stored as it is (as +0 for -0), anything else as
    /// element_value converts it. false: nothing written, ta_set does it.
    #[inline]
    pub fn ta_put_num(&mut self, o: int, i: int, x: double) -> bool {
        let s = self.ta_slot(o, i);
        if s < 0 {
            return false;
        }
        let ob = crate::rvm::obj_at(&self.objs, o as usize);
        let kind = ob.pos as int;
        if is_big(kind) {
            return false;
        }
        let bc = crate::rvm::obj_at(&self.objs, ob.env as usize).cold as usize;
        // a Uint8Array over bytes: the buffer holds Uint8 elements from now
        self.colds[bc].home = kind;
        if kind == TK_FLOAT64 {
            self.colds[bc].nums[s as usize] = x;
            return true;
        }
        let iv = x as int;
        let fits = (iv as double) == x
            && (if kind == TK_UINT8 || kind == TK_UINT8C {
                iv >= 0 && iv <= 255
            } else if kind == TK_INT32 {
                iv >= -2147483648 && iv <= 2147483647
            } else if kind == TK_UINT16 {
                iv >= 0 && iv <= 65535
            } else if kind == TK_INT16 {
                iv >= -32768 && iv <= 32767
            } else if kind == TK_UINT32 {
                iv >= 0 && iv <= 4294967295
            } else {
                kind == TK_INT8 && iv >= -128 && iv <= 127
            });
        self.colds[bc].nums[s as usize] = if fits { iv as double } else { element_value(kind, x) };
        true
    }

    /// `ta[i] = v`: the value is converted even when `i` is outside.
    pub fn ta_set(&mut self, o: int, i: int, v: &Val) {
        if is_big(self.objs[o as usize].pos as int) {
            let big = self.to_bigint(v);
            if self.throwing || i < 0 || i >= self.ta_length(o) {
                return;
            }
            let at = self.objs[o as usize].func + (i * 8) as i32;
            let b = self.objs[o as usize].env as int;
            self.buf_write_big(b, at as int, &big, true);
            return;
        }
        let x = self.to_number(v);
        if self.throwing || i < 0 || i >= self.ta_length(o) {
            return;
        }
        let kind = self.objs[o as usize].pos as int;
        let size = kind_size(kind);
        let at = self.objs[o as usize].func + (i * size) as i32;
        let b = self.objs[o as usize].env as int;
        let mode = self.colds[self.objs[b as usize].cold as usize].home;
        if mode == kind || (mode < 0 && kind == TK_UINT8) {
            let cid = self.cold_id(b) as usize;
            self.colds[cid].home = kind;
            { let c = self.cold_id(b) as usize; self.colds[c].nums[(at / size as i32) as usize] = element_value(kind, x); }
            return;
        }
        self.buf_write(b, at as int, kind, x, true);
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
        self.method(g, "__taLength", NF_TA_LEN, 0);
        self.method(g, "__taSetFast", NF_TA_SET, 3);
        self.method(g, "__dvMake", NF_DV_MAKE, 4);
        self.method(g, "__dvGet", NF_DV_GET, 4);
        self.method(g, "__dvSet", NF_DV_SET, 5);
        self.method(g, "__dvMethod", NF_DV_METHOD, 5);
        self.method(g, "__fastWrap", NF_FAST_WRAP, 4);
        self.method(g, "__taKind", NF_TA_KIND, 1);
        self.method(g, "__taSub", NF_TA_SUB, 4);
        self.method(g, "__taNew", NF_TA_NEW, 4);
        self.method(g, "__taSort", NF_TA_SORT, 1);
        self.method(g, "__bufMax", NF_BUF_MAX, 1);
        self.method(g, "__bufResize", NF_BUF_RESIZE, 2);
    }

    /// A typed array of fixed length over a buffer that is neither
    /// detached nor resizable, in bounds: what NF_TA_SUB handles.
    fn ta_plain_view(&self, t: int) -> bool {
        if self.objs[t as usize].class != C_TYPED || self.ta_out_of_bounds(t) {
            return false;
        }
        if !matches!(self.colds[self.objs[t as usize].cold as usize].prim, Val::Num(_)) {
            return false;
        }
        let b = self.objs[t as usize].env as int;
        b >= 0 && (self.objs[b as usize].pos & 1) == 0 && !self.buf_resizable(b)
    }

    /// NF_TA_KIND / NF_TA_SUB / NF_TA_NEW (see there).
    pub fn call_native_ta2(&mut self, id: int, args: &Vec<Val>) -> Val {
        if id == NF_TA_SORT {
            let t = if args.is_empty() { -1 } else { obj_of(&args[0]) };
            if t < 0 || !self.ta_plain_view(t) || is_big(self.objs[t as usize].pos as int) {
                return Val::Bool(false);
            }
            let n = self.ta_length(t);
            let mut v: Vec<double> = Vec::new();
            let mut i: int = 0;
            while i < n {
                let x = self.ta_get_fast(t, i);
                v.push(match x {
                    Val::Num(d) => d,
                    _ => 0.0 / 0.0,
                });
                i += 1;
            }
            sort_nums(&mut v);
            i = 0;
            while i < n {
                if !self.ta_put_num(t, i, v[i as usize]) {
                    self.ta_set(t, i, &Val::Num(v[i as usize]));
                }
                i += 1;
            }
            return Val::Bool(true);
        }
        if id == NF_TA_KIND {
            let t = if args.is_empty() { -1 } else { obj_of(&args[0]) };
            if t < 0 || !self.ta_plain_view(t) {
                return Val::Num(-1.0);
            }
            return Val::Num(self.objs[t as usize].pos as double);
        }
        if id == NF_TA_NEW {
            if args.len() < 4 {
                return Val::Undef;
            }
            let kind = num(&args[0]) as int;
            let n = match &args[1] {
                Val::Num(x) => {
                    let k = *x as int;
                    if (k as double) != *x || k < 0 || k > 100000000 {
                        return Val::Undef;
                    }
                    k
                }
                _ => {
                    return Val::Undef;
                }
            };
            let proto = obj_of(&args[2]);
            let ab = obj_of(&args[3]);
            let b = self.alloc(C_ARRAYBUFFER, ab);
            // the buffer holds the elements as this kind from the start
            let home = if is_big(kind) { -1 } else { kind };
            let z = zeros(if home >= 0 { n } else { n * kind_size(kind) });
            { let c = self.cold_id(b) as usize; self.colds[c].nums = z; }
            let cid = self.cold_id(b) as usize;
            self.colds[cid].home = home;
            let t = self.alloc(C_TYPED, proto);
            self.objs[t as usize].pos = kind as i32;
            self.put_env(t, b);
            self.objs[t as usize].func = 0;
            self.put_prim(t, Val::Num(n as double));
            return Val::Obj(t);
        }
        // NF_TA_SUB
        if args.len() < 4 {
            return Val::Undef;
        }
        let t = obj_of(&args[0]);
        if t < 0 || !self.ta_plain_view(t) {
            return Val::Undef;
        }
        let b = self.objs[t as usize].env as int;
        let n = self.ta_length(t);
        // relIndex: ToIntegerOrInfinity of a number, clamped into [0, n]
        let mut ends: Vec<int> = vec![0, n];
        let mut k: usize = 0;
        while k < 2 {
            match &args[1 + k] {
                Val::Undef => {}
                Val::Num(x) => {
                    let x = *x;
                    let t = if x < 0.0 { x.ceil() } else { x.floor() };
                    let r: int = if x != x {
                        0
                    } else if t < 0.0 {
                        let m = (n as double) + t;
                        if m <= 0.0 { 0 } else { m as int }
                    } else if t >= (n as double) {
                        n
                    } else {
                        t as int
                    };
                    ends[k] = r;
                }
                _ => {
                    return Val::Undef;
                }
            }
            k += 1;
        }
        let kind = self.objs[t as usize].pos as int;
        let start = ends[0];
        let len = if ends[1] > start { ends[1] - start } else { 0 };
        let off = (self.objs[t as usize].func as int) + start * kind_size(kind);
        let proto = obj_of(&args[3]);
        let v = self.alloc(C_TYPED, proto);
        self.objs[v as usize].pos = kind as i32;
        self.put_env(v, b);
        self.objs[v as usize].func = off as i32;
        self.put_prim(v, Val::Num(len as double));
        Val::Obj(v)
    }

    /// NF_DV_MAKE / NF_DV_GET / NF_DV_SET (see there).
    pub fn call_native_dv(&mut self, id: int, args: &Vec<Val>) -> Val {
        let v = if args.is_empty() { -1 } else { obj_of(&args[0]) };
        if id == NF_DV_MAKE {
            if v >= 0 && args.len() >= 4 {
                let b = obj_of(&args[1]);
                if b >= 0 && self.objs[b as usize].class == C_ARRAYBUFFER {
                    self.objs[v as usize].class = C_DATAVIEW;
                    self.put_env(v, b);
                    self.objs[v as usize].func = num(&args[2]) as i32;
                    self.objs[v as usize].pos = num(&args[3]) as i32;
                }
            }
            return Val::Undef;
        }
        let miss = if id == NF_DV_GET { Val::Undef } else { Val::Bool(false) };
        if args.len() < 3 {
            return miss;
        }
        let kind = num(&args[2]) as int;
        let at = self.dv_at(v, &args[1], kind);
        if at < 0 {
            return miss;
        }
        let b = self.objs[v as usize].env as int;
        if id == NF_DV_GET {
            let little = args.len() > 3 && truthy(&args[3]);
            return Val::Num(self.dv_read(b, at, kind, little));
        }
        if args.len() < 4 {
            return miss;
        }
        let x = match &args[3] {
            Val::Num(x) => *x,
            _ => {
                return miss;
            }
        };
        let little = args.len() > 4 && truthy(&args[4]);
        self.dv_write(b, at, kind, x, little);
        Val::Bool(true)
    }

    /// The byte position in its buffer of view `v`'s `idx` for a number
    /// `kind`: -1 unless v is a DataView, idx a non-negative integer and
    /// the view in bounds over a buffer that is not detached.
    pub fn dv_at(&mut self, v: int, idx: &Val, kind: int) -> int {
        if v < 0 || self.objs[v as usize].class != C_DATAVIEW || is_big(kind) {
            return -1;
        }
        let i = match idx {
            Val::Num(x) => {
                let k = *x as int;
                if (k as double) != *x || k < 0 {
                    return -1;
                }
                k
            }
            _ => {
                return -1;
            }
        };
        let b = self.objs[v as usize].env as int;
        if b < 0 || (self.objs[b as usize].pos & 1) != 0 {
            return -1;
        }
        // the view's byte length as the prelude's dvLength
        let blen = self.buf_size(b);
        let off = self.objs[v as usize].func as int;
        let fixed = self.objs[v as usize].pos as int;
        if off > blen {
            return -1;
        }
        let n = if fixed < 0 { blen - off } else if off + fixed > blen { -1 } else { fixed };
        if n < 0 || i + kind_size(kind) > n {
            return -1;
        }
        off + i
    }

    /// NF_DVM_GET / NF_DVM_SET of function `f` on the stack frame at fpos
    /// (this at fpos + 1, the arguments after): true when done in place,
    /// the result in stack[fpos].
    pub fn dvm_fast(&mut self, id: int, f: int, fpos: int, argc: int) -> bool {
        let kind = self.objs[f as usize].pos as int;
        let bp = (fpos + 2) as usize;
        let v = obj_of(&self.stack[bp - 1]);
        if argc < 1 {
            return false;
        }
        let idx = self.stack[bp].clone();
        let at = self.dv_at(v, &idx, kind);
        if at < 0 {
            return false;
        }
        let b = self.objs[v as usize].env as int;
        if id == NF_DVM_GET {
            let little = argc > 1 && truthy(&self.stack[bp + 1]);
            let r = self.dv_read(b, at, kind, little);
            self.stack[fpos as usize] = Val::Num(r);
            return true;
        }
        if argc < 2 {
            return false;
        }
        let x = match &self.stack[bp + 1] {
            Val::Num(x) => *x,
            _ => {
                return false;
            }
        };
        let little = argc > 2 && truthy(&self.stack[bp + 2]);
        self.dv_write(b, at, kind, x, little);
        self.stack[fpos as usize] = Val::Undef;
        true
    }

    /// buf_read for a DataView: integer kinds straight from the bytes.
    fn dv_read(&mut self, b: int, at: int, kind: int, little: bool) -> double {
        if kind == TK_FLOAT32 || kind == TK_FLOAT64 || kind == TK_FLOAT16 {
            return self.buf_read(b, at, kind, little);
        }
        self.buf_to_bytes(b);
        let size = kind_size(kind);
        let c = self.objs[b as usize].cold as usize;
        // in doubles: a 32-bit int (C++) would overflow at 4 bytes
        let mut u: double = 0.0;
        let mut k: int = 0;
        while k < size {
            let idx = if little { at + size - 1 - k } else { at + k };
            u = u * 256.0 + self.colds[c].nums[idx as usize];
            k += 1;
        }
        if kind == TK_INT8 && u >= 128.0 {
            u -= 256.0;
        } else if kind == TK_INT16 && u >= 32768.0 {
            u -= 65536.0;
        } else if kind == TK_INT32 && u >= 2147483648.0 {
            u -= 4294967296.0;
        }
        u
    }

    /// buf_write for a DataView: integer kinds straight to the bytes.
    fn dv_write(&mut self, b: int, at: int, kind: int, x: double, little: bool) {
        if kind == TK_FLOAT32 || kind == TK_FLOAT64 || kind == TK_FLOAT16 || !(x > -9007199254740992.0 && x < 9007199254740992.0) {
            self.buf_write(b, at, kind, x, little);
            return;
        }
        self.buf_to_bytes(b);
        let size = kind_size(kind);
        // ToInt8 … ToUint32: the low bits of the truncated number, in
        // doubles (exact below 2^53; a 32-bit int would overflow)
        let tr = if x < 0.0 { -((-x).floor()) } else { x.floor() };
        let mut u = tr - (tr / 4294967296.0).floor() * 4294967296.0;
        let c = self.objs[b as usize].cold as usize;
        let mut k: int = 0;
        while k < size {
            let idx = if little { at + k } else { at + size - 1 - k };
            let q = (u / 256.0).floor();
            self.colds[c].nums[idx as usize] = u - q * 256.0;
            u = q;
            k += 1;
        }
    }

    /// NF_TA_LEN: `this.length` for a typed array (0 when detached or out
    /// of bounds), a TypeError otherwise. NF_TA_SET (target, source,
    /// offset): `target.set(source, offset)` done here when the source is a
    /// typed array of the same family (numbers or BigInts) or a plain array
    /// without holes, the offset a non-negative integer and everything fits;
    /// true then, false when the prelude's `set` has to do it.
    pub fn call_native_ta(&mut self, id: int, this: &Val, args: &Vec<Val>) -> Val {
        if id == NF_DV_MAKE || id == NF_DV_GET || id == NF_DV_SET {
            return self.call_native_dv(id, args);
        }
        if id == NF_DV_METHOD {
            let kind = if args.is_empty() { 0 } else { num(&args[0]) as int };
            let slow = if args.len() > 1 { obj_of(&args[1]) } else { -1 };
            let set = args.len() > 2 && truthy(&args[2]);
            let name = if args.len() > 3 { self.to_string(&args[3]) } else { String::new() };
            let len = if args.len() > 4 { num(&args[4]) as int } else { 0 };
            let f = self.native_fn(name.as_str(), if set { NF_DVM_SET } else { NF_DVM_GET }, len);
            self.objs[f as usize].pos = kind as i32;
            if slow >= 0 {
                self.put_env(f, slow);
            }
            return Val::Obj(f);
        }
        if id == NF_TA_KIND || id == NF_TA_SUB || id == NF_TA_NEW || id == NF_TA_SORT {
            return self.call_native_ta2(id, args);
        }
        if id == NF_TA_LEN {
            let t = obj_of(this);
            if t < 0 || self.objs[t as usize].class != C_TYPED {
                self.throw_type("TypedArray.prototype.length: this is not a typed array");
                return Val::Undef;
            }
            return Val::Num(self.ta_length(t) as double);
        }
        if args.len() < 2 {
            return Val::Bool(false);
        }
        let t = obj_of(&args[0]);
        let src = obj_of(&args[1]);
        if t < 0 || src < 0 || self.objs[t as usize].class != C_TYPED || self.ta_out_of_bounds(t) {
            return Val::Bool(false);
        }
        let off = if args.len() > 2 {
            match &args[2] {
                Val::Undef => 0,
                Val::Num(x) => {
                    let i = *x as int;
                    if (i as double) != *x || i < 0 {
                        return Val::Bool(false);
                    }
                    i
                }
                _ => {
                    return Val::Bool(false);
                }
            }
        } else {
            0
        };
        let tb = self.objs[t as usize].env as int;
        if tb < 0 || (self.objs[tb as usize].pos & 1) != 0 {
            return Val::Bool(false);
        }
        let tlen = self.ta_length(t);
        let tkind = self.objs[t as usize].pos as int;
        let sc = self.objs[src as usize].class;
        let mut vals: Vec<Val> = Vec::new();
        if sc == C_TYPED {
            let sb = self.objs[src as usize].env as int;
            if sb < 0 || (self.objs[sb as usize].pos & 1) != 0 || self.ta_out_of_bounds(src) {
                return Val::Bool(false);
            }
            let skind = self.objs[src as usize].pos as int;
            if is_big(skind) != is_big(tkind) {
                return Val::Bool(false);
            }
            let n = self.ta_length(src);
            if off + n > tlen {
                return Val::Bool(false);
            }
            if n == 0 {
                return Val::Bool(true);
            }
            // the same element kind, both buffers holding it: the numbers
            // as they are (through a copy when the buffers are one)
            let s0 = self.ta_slot(src, 0);
            let d0 = self.ta_slot(t, off);
            if s0 >= 0 && d0 >= 0 && skind == tkind && self.ta_slot(src, n - 1) == s0 + n - 1 && self.ta_slot(t, off + n - 1) == d0 + n - 1 {
                let sbc = self.objs[sb as usize].cold as usize;
                let tbc = self.objs[tb as usize].cold as usize;
                self.colds[tbc].home = tkind;
                copy_nums(&mut self.colds, sbc, s0 as usize, tbc, d0 as usize, n as usize);
                return Val::Bool(true);
            }
            let mut i: int = 0;
            while i < n {
                vals.push(self.ta_get_fast(src, i));
                i += 1;
            }
        } else if sc == C_ARRAY {
            let ob = &self.objs[src as usize];
            if ob.pos == 3 || self.idx_protos || !self.colds[ob.cold as usize].elems2.is_empty() {
                return Val::Bool(false);
            }
            let ev = ob.elems as usize;
            let n = self.evecs[ev].v.len() as int;
            if off + n > tlen {
                return Val::Bool(false);
            }
            let mut i: int = 0;
            while i < n {
                vals.push(self.evecs[ev].v[i as usize].clone());
                i += 1;
            }
        } else {
            return Val::Bool(false);
        }
        let mut j: int = 0;
        while j < (vals.len() as int) {
            let v = vals[j as usize].clone();
            let done = match v {
                Val::Num(x) => self.ta_put_num(t, off + j, x),
                _ => false,
            };
            if !done {
                self.ta_set(t, off + j, &v);
                if self.throwing {
                    return Val::Undef;
                }
            }
            j += 1;
        }
        Val::Bool(true)
    }

    pub fn call_native_typed(&mut self, id: int, args: &Vec<Val>) -> Val {
        let mut a: Vec<Val> = args.clone();
        while a.len() < 5 {
            a.push(Val::Undef);
        }
        if id == NF_BUF_NEW {
            // (byteLength, prototype, shared, maxByteLength, kind)
            let n = num(&a[0]) as int;
            if n > 1000000000 {
                self.throw_range("Array buffer allocation failed");
                return Val::Undef;
            }
            let proto = obj_of(&a[1]);
            let b = self.alloc(C_ARRAYBUFFER, proto);
            // (…, kind): the buffer of a new typed array of that kind, made
            // holding its elements at once (zeros, no bytes to convert)
            let mut home = -1;
            if let Val::Num(k) = &a[4] {
                let kind = *k as int;
                if !is_big(kind) && n % kind_size(kind) == 0 {
                    home = kind;
                }
            }
            let bytes = zeros(if home >= 0 { n / kind_size(home) } else { n });
            { let c = self.cold_id(b) as usize; self.colds[c].nums = bytes; }
            let cid = self.cold_id(b) as usize;
            self.colds[cid].home = home;
            if truthy(&a[2]) {
                self.objs[b as usize].pos = 2;
            }
            if let Val::Num(m) = &a[3] {
                let mx = *m;
                self.put_prim(b, Val::Num(mx));
            }
            return Val::Obj(b);
        }
        if id == NF_BUF_MAX {
            // the maximum byte length, -1 when the buffer is fixed-length
            let b = obj_of(&a[0]);
            if b < 0 || self.objs[b as usize].class != C_ARRAYBUFFER {
                return Val::Num(-1.0);
            }
            return match self.colds[self.objs[b as usize].cold as usize].prim {
                Val::Num(m) => Val::Num(m),
                _ => Val::Num(-1.0),
            };
        }
        if id == NF_BUF_RESIZE {
            // (buffer, byteLength): new bytes are zero
            let b = self.buffer_of(&a[0]);
            if b < 0 {
                return Val::Undef;
            }
            let n = num(&a[1]) as int;
            if n > 1000000000 {
                self.throw_range("Array buffer allocation failed");
                return Val::Undef;
            }
            self.buf_to_bytes(b);
            let c = self.cold_id(b) as usize;
            let cur = self.colds[c].nums.len() as int;
            if n < cur {
                self.colds[c].nums.truncate(n as usize);
            } else {
                let mut k = cur;
                while k < n {
                    self.colds[c].nums.push(0.0);
                    k += 1;
                }
            }
            return Val::Undef;
        }
        if id == NF_BUF_INFO {
            // byteLength; -1 not a buffer, -2 detached; +0.5 when shared
            let b = obj_of(&a[0]);
            if b < 0 || self.objs[b as usize].class != C_ARRAYBUFFER {
                return Val::Num(-1.0);
            }
            let pos = self.objs[b as usize].pos as int;
            if (pos & 1) != 0 {
                return Val::Num(-2.0);
            }
            let n = self.buf_size(b) as double;
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
            if at < 0 || at + kind_size(kind) > self.buf_size(b) {
                self.throw_range("Offset is outside the bounds of the DataView");
                return Val::Undef;
            }
            if is_big(kind) {
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
            if at < 0 || at + kind_size(kind) > self.buf_size(b) {
                self.throw_range("Offset is outside the bounds of the DataView");
                return Val::Undef;
            }
            if is_big(kind) {
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
            let (fs, ts, n0) = (num(&a[1]) as int, num(&a[3]) as int, num(&a[4]) as int);
            // never past either buffer's end (a resize can have shrunk it)
            let mut n = n0;
            if self.buf_size(s) - fs < n {
                n = self.buf_size(s) - fs;
            }
            if self.buf_size(d) - ts < n {
                n = self.buf_size(d) - ts;
            }
            if fs < 0 || ts < 0 || n <= 0 {
                return Val::Undef;
            }
            // whole elements between buffers of one kind are copied as they
            // are; a new buffer copied whole takes the source's kind
            let ms = self.colds[self.objs[s as usize].cold as usize].home;
            let mut md = self.colds[self.objs[d as usize].cold as usize].home;
            if ms > TK_UINT8 || ms == TK_INT8 {
                let size = kind_size(ms);
                if md < 0 && ts == 0 && n == self.buf_size(d) && n % size == 0 {
                    let z = zeros(n / size);
                    { let c = self.cold_id(d) as usize; self.colds[c].nums = z; }
                    let cid = self.cold_id(d) as usize;
                    self.colds[cid].home = ms;
                    md = ms;
                }
                if md == ms && fs % size == 0 && ts % size == 0 && n % size == 0 {
                    let mut tmp: Vec<double> = Vec::new();
                    let mut i = 0;
                    while i < n / size {
                        tmp.push(self.colds[self.objs[s as usize].cold as usize].nums[(fs / size + i) as usize]);
                        i += 1;
                    }
                    i = 0;
                    while i < n / size {
                        { let c = self.cold_id(d) as usize; self.colds[c].nums[(ts / size + i) as usize] = tmp[i as usize]; }
                        i += 1;
                    }
                    return Val::Undef;
                }
            }
            self.buf_to_bytes(s);
            self.buf_to_bytes(d);
            let mut tmp: Vec<double> = Vec::new();
            let mut i = 0;
            while i < n {
                tmp.push(self.colds[self.objs[s as usize].cold as usize].nums[(fs + i) as usize]);
                i += 1;
            }
            i = 0;
            while i < n {
                { let c = self.cold_id(d) as usize; self.colds[c].nums[(ts + i) as usize] = tmp[i as usize]; }
                i += 1;
            }
            return Val::Undef;
        }
        if id == NF_BUF_DETACH {
            let b = self.buffer_of(&a[0]);
            if b >= 0 {
                { let c = self.cold_id(b) as usize; self.colds[c].nums = Vec::new(); }
                let cid = self.cold_id(b) as usize;
                self.colds[cid].home = TK_UINT8;
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
            let kind = num(&a[0]) as int;
            let off = num(&a[2]) as int;
            let len = num(&a[3]) as int;
            let tracking = matches!(a[3], Val::Undef);
            // the first view over all of a new buffer: it holds this kind
            if !tracking && !self.buf_resizable(b) && self.colds[self.objs[b as usize].cold as usize].home < 0 && !is_big(kind) && off == 0 && len * kind_size(kind) == self.buf_size(b) && (kind == TK_UINT8 || kind == TK_UINT8C || (kind == TK_INT8 && self.buf_zero(b))) {
                // one byte per element: the bytes are the elements as they
                // are (an Int8Array's when all are 0)
                let cid = self.cold_id(b) as usize;
                self.colds[cid].home = kind;
            } else if !tracking && !self.buf_resizable(b) && self.colds[self.objs[b as usize].cold as usize].home < 0 && !is_big(kind) && off == 0 && len * kind_size(kind) == self.buf_size(b) {
                let z = zeros(len);
                { let c = self.cold_id(b) as usize; self.colds[c].nums = z; }
                let cid = self.cold_id(b) as usize;
                self.colds[cid].home = kind;
            }
            let t = self.alloc(C_TYPED, proto);
            self.objs[t as usize].pos = kind as i32;
            self.put_env(t, b);
            self.objs[t as usize].func = (num(&a[2]) as int) as i32;
            if !tracking {
                self.put_prim(t, Val::Num(num(&a[3])));
            }
            return Val::Obj(t);
        }
        if id == NF_TA_INFO {
            // [kind, buffer, byteOffset, length], or undefined
            let t = obj_of(&a[0]);
            if t < 0 || self.objs[t as usize].class != C_TYPED {
                return Val::Undef;
            }
            let kind = Val::Num(self.objs[t as usize].pos as double);
            let buf = obj_val_of(self.objs[t as usize].env as int);
            let off = Val::Num(self.objs[t as usize].func as double);
            let len = Val::Num(self.ta_length(t) as double);
            let oob = Val::Bool(self.ta_out_of_bounds(t));
            let tracking = Val::Bool(!matches!(self.colds[self.objs[t as usize].cold as usize].prim, Val::Num(_)));
            let arr = self.new_array(vec![kind, buf, off, len, oob, tracking]);
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
