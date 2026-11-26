// SPDX-License-Identifier: AGPL-3.0-or-later
//! The native core of ArrayBuffer, the typed arrays and DataView: bytes and
//! the element reads and writes the interpreter's indexing reaches. The
//! constructors and methods are in the prelude, over the helpers here.
//!
//! An ArrayBuffer (C_ARRAYBUFFER) keeps its contents in `nums`; `pos` has
//! bit 1 when detached and bit 2 when shared. `home` says what `nums`
//! holds: the bytes (TK_UINT8), the bytes four to a slot (TK_WORDS), the
//! bytes of a buffer nothing has read or written yet (-1, all zero), or the
//! elements of one kind. The first typed array over the whole of a new
//! buffer makes it hold its elements (`new Float64Array(n)`: one double per
//! element, read and written as they are); anything else that looks at it
//! -- a view of another kind, a DataView, a copy that is not whole
//! elements -- turns it into words for good (bytes when its length is not
//! a multiple of 4, or a BigInt view or a resize needs them: a heap of one
//! buffer under views of every kind, as compiled C keeps it, takes a
//! quarter of the memory as words, and an aligned 32-bit access is one
//! slot). A typed array (C_TYPED) is a view: `env` the buffer, `pos` the
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
/// Not a kind: an ArrayBuffer's `home` when `nums` holds its bytes four to
/// a slot, a little-endian 32-bit word each.
pub const TK_WORDS: int = 12;

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
        if crate::builtins::time_up(n as int) {
            // past the deadline (Vm::check_expired): v is still a
            // permutation of what it was
            return;
        }
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

/// A float32 from its 32 bits (0 to 2^32 - 1). Rust: the bits as they are.
#[ranger::target(rust)]
fn f32_of_bits(u: double) -> double {
    f32::from_bits(u as u32) as f64
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
fn f32_of_bits(u: double) -> double {
    let ud = u;
    let sign = if ud >= 2147483648.0 { 1 } else { 0 };
    let r = ud - (sign as double) * 2147483648.0;
    let exp = floor(r / 8388608.0);
    float_from_parts(sign, exp as int, r - exp * 8388608.0, 8, 23)
}

/// The 32 bits of `x` as a float32 (a NaN as 0x7fc00000).
#[ranger::target(rust)]
fn f32_bits(x: double) -> double {
    if x != x {
        return 2143289344.0;
    }
    (x as f32).to_bits() as f64
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
fn f32_bits(x: double) -> double {
    let (sign, exp, mant) = float_parts(x, 8, 23);
    (sign as double) * 2147483648.0 + (exp as double) * 8388608.0 + mant
}

/// A float64 from its low and high 32 bits.
#[ranger::target(rust)]
fn f64_of_words(lo: double, hi: double) -> double {
    f64::from_bits(((hi as u64) << 32) | ((lo as u64) & 0xffffffff))
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
fn f64_of_words(lo: double, hi: double) -> double {
    let hd = hi;
    let sign = if hd >= 2147483648.0 { 1 } else { 0 };
    let r = hd - (sign as double) * 2147483648.0;
    let exp = floor(r / 1048576.0);
    let mant = (r - exp * 1048576.0) * 4294967296.0 + lo;
    float_from_parts(sign, exp as int, mant, 11, 52)
}

/// The low and high 32 bits of `x` as a float64 (a NaN as
/// 0x7ff8000000000000).
#[ranger::target(rust)]
fn f64_words(x: double) -> (double, double) {
    if x != x {
        return (0.0, 2146959360.0);
    }
    let b = x.to_bits();
    ((b & 0xffffffff) as f64, (b >> 32) as f64)
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
fn f64_words(x: double) -> (double, double) {
    let (sign, exp, mant) = float_parts(x, 11, 52);
    let top = floor(mant / 4294967296.0);
    let lo = mant - top * 4294967296.0;
    (lo, (sign as double) * 2147483648.0 + (exp as double) * 1048576.0 + top)
}

/// The `size` bytes at `at` of colds[c].nums (bytes, one a slot) as
/// unsigned numbers: the four least significant, then the four above
/// them. Rust: as integers.
#[ranger::target(rust)]
fn get_bytes(colds: &Vec<Cold>, c: usize, at: int, size: int, little: bool, words: bool) -> (double, double) {
    let nums = &colds[c].nums;
    let a = at as usize;
    if words {
        // four bytes a slot: an aligned word (or two) as it is, a byte or
        // two inside one word by a shift
        if little && (a & 3) == 0 {
            if size == 4 {
                return (nums[a >> 2], 0.0);
            }
            if size == 8 {
                return (nums[a >> 2], nums[(a >> 2) + 1]);
            }
        }
        if little && size <= 2 && (a & 3) + (size as usize) <= 4 {
            let m: u32 = if size == 1 { 255 } else { 65535 };
            return ((((nums[a >> 2] as u32) >> ((a & 3) * 8)) & m) as f64, 0.0);
        }
        let mut v: u64 = 0;
        let mut k: int = 0;
        while k < size {
            let idx = (if little { at + k } else { at + size - 1 - k }) as usize;
            v |= ((((nums[idx >> 2] as u32) >> ((idx & 3) * 8)) & 255) as u64) << (8 * k);
            k += 1;
        }
        return ((v & 0xffffffff) as f64, (v >> 32) as f64);
    }
    if little {
        // the common case, unrolled: whole elements, least significant first
        if size == 4 {
            let w = (nums[a] as u32) | ((nums[a + 1] as u32) << 8) | ((nums[a + 2] as u32) << 16) | ((nums[a + 3] as u32) << 24);
            return (w as f64, 0.0);
        }
        if size == 2 {
            return (((nums[a] as u32) | ((nums[a + 1] as u32) << 8)) as f64, 0.0);
        }
        if size == 1 {
            return (nums[a], 0.0);
        }
    }
    let mut v: u64 = 0;
    let mut k: int = 0;
    while k < size {
        let idx = if little { at + k } else { at + size - 1 - k };
        v |= (nums[idx as usize] as u64) << (8 * k);
        k += 1;
    }
    ((v & 0xffffffff) as f64, (v >> 32) as f64)
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
fn get_bytes(colds: &Vec<Cold>, c: usize, at: int, size: int, little: bool, words: bool) -> (double, double) {
    let mut lo: double = 0.0;
    let mut hi: double = 0.0;
    let mut mul: double = 1.0;
    let mut k: int = 0;
    while k < size {
        let idx = if little { at + k } else { at + size - 1 - k };
        let byte = if words { word_byte(colds[c].nums[(idx / 4) as usize], idx % 4) } else { colds[c].nums[idx as usize] };
        if k < 4 {
            lo = lo + byte * mul;
        } else {
            hi = hi + byte * mul;
        }
        mul = if k == 3 { 1.0 } else { mul * 256.0 };
        k += 1;
    }
    (lo, hi)
}

/// Writes `lo` (the four least significant bytes, unsigned) and `hi` (the
/// four above them) as `size` bytes at `at` of colds[c].nums. Rust: as
/// integers.
#[ranger::target(rust)]
fn put_bytes(colds: &mut Vec<Cold>, c: usize, at: int, size: int, lo: double, hi: double, little: bool, words: bool) {
    let v: u64 = (lo as u64) | ((hi as u64) << 32);
    let nums = &mut colds[c].nums;
    let a = at as usize;
    if words {
        if little && (a & 3) == 0 {
            if size == 4 {
                nums[a >> 2] = (v as u32) as f64;
                return;
            }
            if size == 8 {
                nums[a >> 2] = (v as u32) as f64;
                nums[(a >> 2) + 1] = (v >> 32) as f64;
                return;
            }
        }
        if little && size <= 2 && (a & 3) + (size as usize) <= 4 {
            let m: u32 = if size == 1 { 255 } else { 65535 };
            let sh = (a & 3) * 8;
            let w = nums[a >> 2] as u32;
            nums[a >> 2] = ((w & !(m << sh)) | (((v as u32) & m) << sh)) as f64;
            return;
        }
        let mut k: int = 0;
        while k < size {
            let idx = (if little { at + k } else { at + size - 1 - k }) as usize;
            let sh = (idx & 3) * 8;
            let w = nums[idx >> 2] as u32;
            nums[idx >> 2] = ((w & !(255u32 << sh)) | ((((v >> (8 * k)) & 255) as u32) << sh)) as f64;
            k += 1;
        }
        return;
    }
    if little && size <= 4 {
        // the common case: whole elements, least significant first
        let w = v as u32;
        nums[a] = (w & 255) as f64;
        if size >= 2 {
            nums[a + 1] = ((w >> 8) & 255) as f64;
        }
        if size == 4 {
            nums[a + 2] = ((w >> 16) & 255) as f64;
            nums[a + 3] = (w >> 24) as f64;
        }
        return;
    }
    let mut k: int = 0;
    while k < size {
        let idx = if little { at + k } else { at + size - 1 - k };
        nums[idx as usize] = ((v >> (8 * k)) & 255) as f64;
        k += 1;
    }
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
fn put_bytes(colds: &mut Vec<Cold>, c: usize, at: int, size: int, lo: double, hi: double, little: bool, words: bool) {
    let mut l = lo;
    let mut h = hi;
    let mut k: int = 0;
    while k < size {
        let idx = if little { at + k } else { at + size - 1 - k };
        let src = if k < 4 { l } else { h };
        let q = floor(src / 256.0);
        if words {
            let w = colds[c].nums[(idx / 4) as usize];
            colds[c].nums[(idx / 4) as usize] = w + (src - q * 256.0 - word_byte(w, idx % 4)) * word_mul(idx % 4);
        } else {
            colds[c].nums[idx as usize] = src - q * 256.0;
        }
        if k < 4 {
            l = q;
        } else {
            h = q;
        }
        k += 1;
    }
}

/// A view's element at byte `at` (aligned to its size, as views are) of a
/// buffer holding words: (true, the number) where this target reads it
/// straight from the word, else (false, 0) and bytes_read does it. Rust:
/// every kind but the BigInt ones; the rest: none.
#[ranger::target(rust)]
#[inline]
fn words_get(colds: &Vec<Cold>, c: usize, at: int, kind: int) -> (bool, double) {
    let nums = &colds[c].nums;
    if nums.len() == 0 || colds[c].home != TK_WORDS {
        return (false, 0.0);
    }
    let a = at as usize;
    let w = nums[a >> 2] as u32;
    let sh = (a & 3) * 8;
    let x = match kind {
        TK_INT8 => ((w >> sh) as u8 as i8) as f64,
        TK_UINT8 | TK_UINT8C => ((w >> sh) & 255) as f64,
        TK_INT16 => ((w >> sh) as u16 as i16) as f64,
        TK_UINT16 => ((w >> sh) & 65535) as f64,
        TK_INT32 => (w as i32) as f64,
        TK_UINT32 => w as f64,
        TK_FLOAT32 => f32::from_bits(w) as f64,
        TK_FLOAT64 => f64::from_bits((w as u64) | ((nums[(a >> 2) + 1] as u32 as u64) << 32)),
        _ => return (false, 0.0),
    };
    (true, x)
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
fn words_get(colds: &Vec<Cold>, c: usize, at: int, kind: int) -> (bool, double) {
    (false, 0.0)
}

/// Writes `x` (a number) as a view's element at byte `at` (aligned) of a
/// buffer holding words, where this target writes it straight into the
/// word: true then, false when nothing was written (bytes_write does it).
/// Rust: the integer kinds for a number below 2^63 (ToInt32 and the rest
/// are its low bits truncated), Float32 and Float64, NaN as one NaN.
#[ranger::target(rust)]
#[inline]
fn words_put(colds: &mut Vec<Cold>, c: usize, at: int, kind: int, x: double) -> bool {
    if colds[c].home != TK_WORDS {
        return false;
    }
    let a = at as usize;
    let k = a >> 2;
    let nums = &mut colds[c].nums;
    if kind == TK_FLOAT64 {
        let v = if x.is_nan() { 0x7ff8000000000000u64 } else { x.to_bits() };
        nums[k] = (v as u32) as f64;
        nums[k + 1] = (v >> 32) as f64;
        return true;
    }
    let v: u32 = if kind == TK_FLOAT32 {
        if x.is_nan() { 0x7fc00000 } else { (x as f32).to_bits() }
    } else if kind == TK_UINT8C || kind > TK_FLOAT64 || !(x > -9.2e18 && x < 9.2e18) {
        return false;
    } else {
        (x as i64) as u32
    };
    let size = kind_size(kind);
    if size == 4 {
        nums[k] = v as f64;
        return true;
    }
    let sh = (a & 3) * 8;
    let m: u32 = if size == 1 { 255 } else { 65535 };
    let w = nums[k] as u32;
    nums[k] = ((w & !(m << sh)) | ((v & m) << sh)) as f64;
    true
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
fn words_put(colds: &mut Vec<Cold>, c: usize, at: int, kind: int, x: double) -> bool {
    false
}

/// 256 to the power `r` (0..3): where byte `r` of a word starts.
fn word_mul(r: int) -> double {
    if r == 0 {
        1.0
    } else if r == 1 {
        256.0
    } else if r == 2 {
        65536.0
    } else {
        16777216.0
    }
}

/// Byte `r` (0 the least significant) of word `w` (an unsigned 32-bit
/// number).
fn word_byte(w: double, r: int) -> double {
    let q = floor(w / word_mul(r));
    q - floor(q / 256.0) * 256.0
}

/// `x` (a number) as a `kind` integer element's bits, unsigned: an
/// integer the kind holds without a modulo, the rest as int_bits does it.
fn uint_bits(kind: int, x: double) -> double {
    let size = kind_size(kind);
    let m = if size == 1 { 256.0 } else if size == 2 { 65536.0 } else { 4294967296.0 };
    if x == floor(x) {
        if x >= 0.0 && x < m {
            return x + 0.0;
        }
        if x < 0.0 && x >= -m && kind != TK_UINT8C {
            return x + m;
        }
    }
    int_bits(kind, x)
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
    // two bits a kind, TK_INT8 (0) to TK_FLOAT16 (11): 0 0 0 1 1 2 2 2 3 3 3 1
    (8366400 >> (2 * kind)) & 3
}

/// The bytes a slot of a buffer's `nums` holds under `home` = `mode`.
pub fn mode_size(mode: int) -> int {
    if mode == TK_WORDS {
        4
    } else {
        kind_size(mode)
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
        rust: { (x as f32) as f64 },
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

impl Vm {
    /// A BigInt64 / BigUint64 element at byte `at`.
    pub fn buf_read_big(&mut self, b: int, at: int, kind: int, little: bool) -> Val {
        self.buf_to_bytes(b);
        let mut be: Vec<int> = Vec::new();
        let mut k: int = 0;
        while k < 8 {
            let idx = if little { at + 7 - k } else { at + k };
            be.push(self.byte(b, idx));
            k += 1;
        }
        let m = crate::bigint::mag_from_bytes(&be);
        if kind == TK_BIGINT64 {
            let (n, r) = crate::bigint::mag_as_int_n(64, false, &m);
            return self.bigint_of(n, &r);
        }
        self.bigint_of(false, &m)
    }

    /// Writes a BigInt (converted already) as 64 bits at byte `at`.
    pub fn buf_write_big(&mut self, b: int, at: int, v: &Val, little: bool) {
        self.buf_to_bytes(b);
        let (xn, xm) = self.big_mag(v);
        let (_, u) = crate::bigint::mag_as_uint_n(64, xn, &xm);
        let low = crate::bigint::mag_low_bytes(&u, 8);
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
        let fixed = if let Some(n) = (self.colds[self.objs[o as usize].cold as usize].prim).as_num() { n as int } else { -1 };
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

    /// ta_len_plain's answer out of a ta_memo way that fits o: its length,
    /// -1 when the getter is not the native one, -2 when no way fits.
    #[inline]
    pub fn ta_len_hit(&self, o: int) -> int {
        let ob = crate::rvm::obj_at(&self.objs, o as usize);
        // a dictionary's shape stays the same when it gains a key (no
        // epoch moves for an object that is no prototype): an own `length`
        // defined later would not be seen (JSC typedarray-length-dictionary)
        if self.shapes[ob.shape as usize].owned {
            return -2;
        }
        let m = &self.ta_memo;
        let ep = self.shape_epoch as i32;
        // the way for o's shape and prototype (two ways, 7 ints each)
        let w: usize = if m.len() >= 7 && crate::rvm::i32_at(m, 0) == ob.shape && crate::rvm::i32_at(m, 2) == ob.proto && crate::rvm::i32_at(m, 1) == ep {
            0
        } else if m.len() >= 14 && crate::rvm::i32_at(m, 7) == ob.shape && crate::rvm::i32_at(m, 9) == ob.proto && crate::rvm::i32_at(m, 8) == ep {
            7
        } else {
            return -2;
        };
        // ob.proto is m[w + 2], an object while o lives
        let po1 = crate::rvm::obj_at(&self.objs, ob.proto as usize);
        if po1.shape != crate::rvm::i32_at(m, w + 3) || po1.proto != crate::rvm::i32_at(m, w + 4) {
            return -2;
        }
        let po2 = crate::rvm::obj_at(&self.objs, po1.proto as usize);
        if po2.shape != crate::rvm::i32_at(m, w + 5) {
            return -2;
        }
        // the slot exists while p2's shape is the one that had it
        let pair = obj_of(&crate::rvm::slot_at(po2, crate::rvm::i32_at(m, w + 6) as usize));
        if pair < 0 {
            return -1;
        }
        let pob = crate::rvm::obj_at(&self.objs, pair as usize);
        if pob.class != C_ACCESSOR {
            return -1;
        }
        let g = obj_of(crate::rvm::val_at(&crate::rvm::evec_at(&self.evecs, pob.elems as usize).v, 0));
        if g < 0 {
            return -1;
        }
        let gob = crate::rvm::obj_at(&self.objs, g as usize);
        if gob.class != C_NATIVE || (gob.func as int) != NF_TA_LEN {
            return -1;
        }
        // a fixed-length view over a buffer neither detached nor resizable:
        // its length as made (ta_length does the rest)
        let b = ob.env as int;
        if b >= 0 {
            let bo = crate::rvm::obj_at(&self.objs, b as usize);
            if (bo.pos & 1) == 0 && bo.cold > 0 && !(crate::rvm::cold_at(&self.colds, bo.cold as usize).prim).is_num() {
                if let Some(n) = (crate::rvm::cold_at(&self.colds, ob.cold as usize).prim).as_num() {
                    return n as int;
                }
            }
        }
        self.ta_length(o)
    }

    /// `o.length` for typed array `o` when %TypedArray%.prototype's own
    /// getter answers it: no `length` of o's own or on its prototype, the
    /// getter the native one. The lookup is kept (ta_memo, two ways of 7
    /// ints: o's shape, shape_epoch, both prototypes and their shapes, the
    /// slot) while those stay; -1 when it does not hold.
    pub fn ta_len_plain(&mut self, o: int) -> int {
        let n = self.ta_len_hit(o);
        if n != -2 {
            return n;
        }
        let p1 = self.objs[o as usize].proto;
        if p1 < 0 {
            return -1;
        }
        let p2 = self.objs[p1 as usize].proto;
        if p2 < 0 {
            return -1;
        }
        let osh = self.objs[o as usize].shape;
        let ep = self.shape_epoch as i32;
        if self.find(o, A_LENGTH) >= 0 || self.find(p1 as int, A_LENGTH) >= 0 {
            return -1;
        }
        let slot = self.find(p2 as int, A_LENGTH);
        if slot < 0 || (self.attr_at(p2 as int, slot) & P_ACCESSOR) == 0 {
            return -1;
        }
        if self.shapes[osh as usize].owned {
            // a dictionary is not remembered (ta_len_hit): the generic
            // lookup answers at every read
            return -1;
        }
        // a new way: the first two fill, then each replaces the older one
        if self.ta_memo.len() < 14 {
            let mut k = 0;
            while k < 7 {
                self.ta_memo.push(-1);
                k += 1;
            }
        } else {
            let mut k = 0;
            while k < 7 {
                self.ta_memo[k] = self.ta_memo[k + 7];
                k += 1;
            }
        }
        let b = self.ta_memo.len() - 7;
        self.ta_memo[b] = osh;
        self.ta_memo[b + 1] = ep;
        self.ta_memo[b + 2] = p1;
        self.ta_memo[b + 3] = self.objs[p1 as usize].shape;
        self.ta_memo[b + 4] = p2;
        self.ta_memo[b + 5] = self.objs[p2 as usize].shape;
        self.ta_memo[b + 6] = slot as i32;
        // the getter is checked as on a hit
        self.ta_len_plain(o)
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
        if let Some(n) = (self.colds[self.objs[o as usize].cold as usize].prim).as_num() { off + (n as int) * size > bs } else { off > bs }
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
        (self.colds[self.objs[b as usize].cold as usize].prim).is_num()
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
            n * mode_size(mode)
        }
    }

    /// Makes buffer `b` hold its bytes one a slot, if it holds elements or
    /// words.
    pub fn buf_to_bytes(&mut self, b: int) {
        let mode = self.colds[self.objs[b as usize].cold as usize].home;
        if mode < 0 || mode == TK_UINT8 {
            let cid = self.cold_id(b) as usize;
            self.colds[cid].home = TK_UINT8;
            return;
        }
        if mode == TK_WORDS {
            let c = self.cold_id(b) as usize;
            let ws = self.colds[c].nums.clone();
            self.colds[c].nums = zeros((ws.len() as int) * 4);
            self.colds[c].home = TK_UINT8;
            let mut i: usize = 0;
            while i < ws.len() {
                let mut w = ws[i];
                let mut k: usize = 0;
                while w != 0.0 && k < 4 {
                    let q = floor(w / 256.0);
                    self.colds[c].nums[i * 4 + k] = w - q * 256.0;
                    w = q;
                    k += 1;
                }
                i += 1;
            }
            return;
        }
        // zeros, then each element that is not +0 written over them
        let size = kind_size(mode);
        let c = self.cold_id(b) as usize;
        let elems = self.colds[c].nums.clone();
        self.colds[c].nums = zeros((elems.len() as int) * size);
        self.colds[c].home = TK_UINT8;
        let mut i: usize = 0;
        while i < elems.len() {
            let x = elems[i];
            if x != 0.0 || 1.0 / x < 0.0 {
                self.bytes_write(b, (i as int) * size, mode, x, true);
            }
            i += 1;
        }
    }

    /// Makes buffer `b` hold its bytes: as words when its byte length is a
    /// multiple of 4 and nothing made it bytes already, else one a slot.
    pub fn buf_to_mixed(&mut self, b: int) {
        let mode = self.colds[self.objs[b as usize].cold as usize].home;
        if mode == TK_UINT8 || mode == TK_WORDS {
            return;
        }
        let n = self.buf_size(b);
        if n % 4 != 0 || self.buf_resizable(b) {
            self.buf_to_bytes(b);
            return;
        }
        let c = self.cold_id(b) as usize;
        let old = self.colds[c].nums.clone();
        self.colds[c].nums = zeros(n / 4);
        self.colds[c].home = TK_WORDS;
        // zeros, then each element (or byte) that is not +0 written over
        let k = if mode < 0 { TK_UINT8 } else { mode };
        let size = kind_size(k);
        let mut i: usize = 0;
        while i < old.len() {
            let x = old[i];
            if x != 0.0 || 1.0 / x < 0.0 {
                self.bytes_write(b, (i as int) * size, k, x, true);
            }
            i += 1;
        }
    }

    /// Reads a `kind` element at byte `at` of buffer `b`.
    pub fn buf_read(&mut self, b: int, at: int, kind: int, little: bool) -> double {
        self.buf_to_mixed(b);
        self.bytes_read(b, at, kind, little)
    }

    /// buf_read for a buffer that holds bytes (or words) already.
    fn bytes_read(&self, b: int, at: int, kind: int, little: bool) -> double {
        let c = self.objs[b as usize].cold as usize;
        let size = kind_size(kind);
        let (lo, hi) = get_bytes(&self.colds, c, at, size, little, self.colds[c].home == TK_WORDS);
        if kind == TK_FLOAT32 {
            return f32_of_bits(lo);
        }
        if kind == TK_FLOAT64 {
            return f64_of_words(lo, hi);
        }
        if kind == TK_FLOAT16 {
            let sign = if lo >= 32768.0 { 1 } else { 0 };
            let r = lo - (sign as double) * 32768.0;
            let exp = floor(r / 1024.0);
            return float_from_parts(sign, exp as int, r - exp * 1024.0, 5, 10);
        }
        if kind == TK_INT8 || kind == TK_INT16 || kind == TK_INT32 {
            let half = if size == 1 { 128.0 } else if size == 2 { 32768.0 } else { 2147483648.0 };
            if lo >= half {
                return lo - 2.0 * half;
            }
        }
        lo
    }

    /// Writes `x` (a number already) as a `kind` element at byte `at`.
    pub fn buf_write(&mut self, b: int, at: int, kind: int, x: double, little: bool) {
        self.buf_to_mixed(b);
        self.bytes_write(b, at, kind, x, little);
    }

    /// buf_write for a buffer that holds bytes (or words) already.
    fn bytes_write(&mut self, b: int, at: int, kind: int, x: double, little: bool) {
        let size = kind_size(kind);
        let mut hi: double = 0.0;
        let lo: double = if kind == TK_FLOAT32 {
            f32_bits(x)
        } else if kind == TK_FLOAT64 {
            let (l, h) = f64_words(x);
            hi = h;
            l
        } else if kind == TK_FLOAT16 {
            let (sign, exp, mant) = float_parts(x, 5, 10);
            (sign as double) * 32768.0 + (exp as double) * 1024.0 + mant
        } else {
            uint_bits(kind, x)
        };
        let c = self.objs[b as usize].cold as usize;
        let words = self.colds[c].home == TK_WORDS;
        put_bytes(&mut self.colds, c, at, size, lo, hi, little, words);
    }

    /// The byte where `ta[i]` starts when its buffer holds bytes or words (a view
    /// of another kind made it so) and the access needs nothing else: a
    /// fixed-length view, not a BigInt kind, over a buffer neither
    /// detached nor resizable, `i` inside it. -1 otherwise.
    #[inline]
    fn ta_byte_at(&self, o: int, i: int) -> int {
        let ob = crate::rvm::obj_at(&self.objs, o as usize);
        let b = ob.env as int;
        if b < 0 || i < 0 {
            return -1;
        }
        let bo = crate::rvm::obj_at(&self.objs, b as usize);
        if (bo.pos & 1) != 0 || bo.cold <= 0 {
            return -1;
        }
        let inside = if let Some(n) = (crate::rvm::cold_at(&self.colds, ob.cold as usize).prim).as_num() { (i as double) < n } else { false };
        if !inside {
            return -1;
        }
        let bc = crate::rvm::cold_at(&self.colds, bo.cold as usize);
        let kind = ob.pos as int;
        if bc.prim.is_num() || (bc.home != TK_UINT8 && bc.home != TK_WORDS) || is_big(kind) {
            return -1;
        }
        (ob.func as int) + (i << kind_shift(kind))
    }

    /// `ta[i]` over a buffer that holds bytes, as ta_byte_at finds it:
    /// (true, the number), else (false, 0) and ta_get does it.
    pub fn ta_bytes_get(&self, o: int, i: int) -> (bool, double) {
        let at = self.ta_byte_at(o, i);
        if at < 0 {
            return (false, 0.0);
        }
        let ob = crate::rvm::obj_at(&self.objs, o as usize);
        let c = crate::rvm::obj_at(&self.objs, ob.env as usize).cold as usize;
        let (done, x) = words_get(&self.colds, c, at, ob.pos as int);
        if done {
            return (true, x);
        }
        (true, self.bytes_read(ob.env as int, at, ob.pos as int, true))
    }

    /// `ta[i] = x` (a number) over a buffer that holds bytes, as
    /// ta_byte_at finds it; false: nothing written, ta_set does it.
    pub fn ta_bytes_put(&mut self, o: int, i: int, x: double) -> bool {
        let at = self.ta_byte_at(o, i);
        if at < 0 {
            return false;
        }
        let b = self.objs[o as usize].env as int;
        let kind = self.objs[o as usize].pos as int;
        let c = self.objs[b as usize].cold as usize;
        if !words_put(&mut self.colds, c, at, kind, x) {
            self.bytes_write(b, at, kind, x, true);
        }
        true
    }

    /// `ta[i]`: undefined outside the view.
    pub fn ta_get(&mut self, o: int, i: int) -> Val {
        if i < 0 || i >= self.ta_length(o) {
            return Val::Undef();
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
        let inside = if let Some(n) = (crate::rvm::cold_at(&self.colds, ob.cold as usize).prim).as_num() { (i as double) < n } else { false };
        if !inside {
            return -1;
        }
        let bc = crate::rvm::cold_at(&self.colds, bo.cold as usize);
        if bc.prim.is_num() {
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
        // ta_slot's checks, the buffer's Cold kept for the write
        if i < 0 {
            return false;
        }
        let ob = crate::rvm::obj_at(&self.objs, o as usize);
        let b = ob.env as int;
        if b < 0 {
            return false;
        }
        let bo = crate::rvm::obj_at(&self.objs, b as usize);
        if (bo.pos & 1) != 0 || bo.cold <= 0 {
            return false;
        }
        let inside = if let Some(n) = (crate::rvm::cold_at(&self.colds, ob.cold as usize).prim).as_num() { (i as double) < n } else { false };
        if !inside {
            return false;
        }
        let bci = bo.cold as usize;
        let bc = crate::rvm::cold_at(&self.colds, bci);
        if bc.prim.is_num() {
            return false;
        }
        let kind = ob.pos as int;
        let s = if bc.home == kind {
            // a view's byte offset is a multiple of its element size
            ((ob.func as int) >> kind_shift(kind)) + i
        } else if bc.home < 0 && kind == TK_UINT8 {
            (ob.func as int) + i
        } else {
            return false;
        };
        if is_big(kind) {
            return false;
        }
        // an integer the kind holds is stored as it is (as +0 for -0)
        let iv = x as int;
        let v = if kind == TK_FLOAT64 {
            x
        } else if (iv as double) == x
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
            })
        {
            // + 0.0: an int is never -0, but a JavaScript build's x as int
            // keeps x's sign
            (iv as double) + 0.0
        } else {
            element_value(kind, x)
        };
        let c = crate::rvm::cold_mut(&mut self.colds, bci);
        // a Uint8Array over bytes: the buffer holds Uint8 elements from now
        c.home = kind;
        c.nums[s as usize] = v;
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
        if !(self.colds[self.objs[t as usize].cold as usize].prim).is_num() {
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
                v.push(if let Some(d) = x.as_num() { d } else { 0.0 / 0.0 });
                i += 1;
            }
            sort_nums(&mut v);
            if self.check_expired() {
                return Val::Undef();
            }
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
                return Val::Undef();
            }
            let kind = num(&args[0]) as int;
            let n = if let Some(x) = (&args[1]).as_num() {
                    let k = x as int;
                    if (k as double) != x || k < 0 || k > 100000000 {
                        return Val::Undef();
                    }
                    k
                } else {
                    return Val::Undef();
                };
            let proto = obj_of(&args[2]);
            let ab = obj_of(&args[3]);
            let b = self.alloc(C_ARRAYBUFFER, ab);
            // the buffer holds the elements as this kind from the start
            let home = if is_big(kind) { -1 } else { kind };
            let slots = if home >= 0 { n } else { n * kind_size(kind) };
            if self.alloc_refused((slots as double) * 8.0) {
                return Val::Undef();
            }
            let z = zeros(slots);
            self.note_buf_bytes((z.len() as int) * 8);
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
            return Val::Undef();
        }
        let t = obj_of(&args[0]);
        if t < 0 || !self.ta_plain_view(t) {
            return Val::Undef();
        }
        let b = self.objs[t as usize].env as int;
        let n = self.ta_length(t);
        // relIndex: ToIntegerOrInfinity of a number, clamped into [0, n]
        let mut ends: Vec<int> = vec![0, n];
        let mut k: usize = 0;
        while k < 2 {
            if (&args[1 + k]).is_undef() {} else if let Some(x) = (&args[1 + k]).as_num() {
                    let x = x;
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
                } else {
                    return Val::Undef();
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
            return Val::Undef();
        }
        let miss = if id == NF_DV_GET { Val::Undef() } else { Val::Bool(false) };
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
        let x = if let Some(x) = (&args[3]).as_num() { x } else {
                return miss;
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
        let i = if let Some(x) = idx.as_num() {
                let k = x as int;
                if (k as double) != x || k < 0 {
                    return -1;
                }
                k
            } else {
                return -1;
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
        let x = if let Some(x) = (&self.stack[bp + 1]).as_num() { x } else {
                return false;
            };
        let little = argc > 2 && truthy(&self.stack[bp + 2]);
        self.dv_write(b, at, kind, x, little);
        self.stack[fpos as usize] = Val::Undef();
        true
    }

    /// buf_read for a DataView.
    fn dv_read(&mut self, b: int, at: int, kind: int, little: bool) -> double {
        self.buf_read(b, at, kind, little)
    }

    /// buf_write for a DataView.
    fn dv_write(&mut self, b: int, at: int, kind: int, x: double, little: bool) {
        self.buf_write(b, at, kind, x, little);
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
                return Val::Undef();
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
            if (&args[2]).is_undef() { 0 } else if let Some(x) = (&args[2]).as_num() {
                    let i = x as int;
                    if (i as double) != x || i < 0 {
                        return Val::Bool(false);
                    }
                    i
                } else {
                    return Val::Bool(false);
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
            let done = if let Some(x) = v.as_num() { self.ta_put_num(t, off + j, x) } else { false };
            if !done {
                self.ta_set(t, off + j, &v);
                if self.throwing {
                    return Val::Undef();
                }
            }
            j += 1;
        }
        Val::Bool(true)
    }

    pub fn call_native_typed(&mut self, id: int, args: &Vec<Val>) -> Val {
        let mut a: Vec<Val> = args.clone();
        while a.len() < 5 {
            a.push(Val::Undef());
        }
        if id == NF_BUF_NEW {
            // (byteLength, prototype, shared, maxByteLength, kind)
            let n = num(&a[0]) as int;
            if n > 1000000000 {
                self.throw_range("Array buffer allocation failed");
                return Val::Undef();
            }
            let proto = obj_of(&a[1]);
            let b = self.alloc(C_ARRAYBUFFER, proto);
            // (…, kind): the buffer of a new typed array of that kind, made
            // holding its elements at once (zeros, no bytes to convert)
            let mut home = -1;
            if let Some(k) = (&a[4]).as_num() {
                let kind = k as int;
                if !is_big(kind) && n % kind_size(kind) == 0 {
                    home = kind;
                }
            }
            let slots = if home >= 0 { n / kind_size(home) } else { n };
            if self.alloc_refused((slots as double) * 8.0) {
                return Val::Undef();
            }
            let bytes = zeros(slots);
            self.note_buf_bytes((bytes.len() as int) * 8);
            { let c = self.cold_id(b) as usize; self.colds[c].nums = bytes; }
            let cid = self.cold_id(b) as usize;
            self.colds[cid].home = home;
            if truthy(&a[2]) {
                self.objs[b as usize].pos = 2;
            }
            if let Some(m) = (&a[3]).as_num() {
                let mx = m;
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
            return if let Some(m) = (self.colds[self.objs[b as usize].cold as usize].prim).as_num() { Val::Num(m) } else { Val::Num(-1.0) };
        }
        if id == NF_BUF_RESIZE {
            // (buffer, byteLength): new bytes are zero
            let b = self.buffer_of(&a[0]);
            if b < 0 {
                return Val::Undef();
            }
            let n = num(&a[1]) as int;
            if n > 1000000000 {
                self.throw_range("Array buffer allocation failed");
                return Val::Undef();
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
            return Val::Undef();
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
                return Val::Undef();
            }
            let kind = num(&a[2]) as int;
            let at = num(&a[1]) as int;
            if at < 0 || at + kind_size(kind) > self.buf_size(b) {
                self.throw_range("Offset is outside the bounds of the DataView");
                return Val::Undef();
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
                return Val::Undef();
            }
            let kind = num(&a[2]) as int;
            let at = num(&a[1]) as int;
            if at < 0 || at + kind_size(kind) > self.buf_size(b) {
                self.throw_range("Offset is outside the bounds of the DataView");
                return Val::Undef();
            }
            if is_big(kind) {
                let v = a[3].clone();
                self.buf_write_big(b, at, &v, truthy(&a[4]));
                return Val::Undef();
            }
            let x = num(&a[3]);
            self.buf_write(b, at, kind, x, truthy(&a[4]));
            return Val::Undef();
        }
        if id == NF_BUF_COPY {
            // (from, fromByte, to, toByte, count), overlapping allowed
            let s = self.buffer_of(&a[0]);
            let d = self.buffer_of(&a[2]);
            if s < 0 || d < 0 {
                return Val::Undef();
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
                return Val::Undef();
            }
            // whole elements between buffers of one kind are copied as they
            // are; a new buffer copied whole takes the source's kind
            let ms = self.colds[self.objs[s as usize].cold as usize].home;
            let mut md = self.colds[self.objs[d as usize].cold as usize].home;
            if ms > TK_UINT8 || ms == TK_INT8 {
                let size = mode_size(ms);
                if md < 0 && ts == 0 && n == self.buf_size(d) && n % size == 0 {
                    let z = zeros(n / size);
                    self.note_buf_bytes((z.len() as int) * 8);
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
                    return Val::Undef();
                }
            }
            self.buf_to_mixed(s);
            self.buf_to_mixed(d);
            let mut tmp: Vec<double> = Vec::new();
            let mut i = 0;
            if self.colds[self.objs[s as usize].cold as usize].home == TK_UINT8 && self.colds[self.objs[d as usize].cold as usize].home == TK_UINT8 {
                while i < n {
                    tmp.push(self.colds[self.objs[s as usize].cold as usize].nums[(fs + i) as usize]);
                    i += 1;
                }
                i = 0;
                while i < n {
                    { let c = self.cold_id(d) as usize; self.colds[c].nums[(ts + i) as usize] = tmp[i as usize]; }
                    i += 1;
                }
                return Val::Undef();
            }
            // words on either side: byte by byte through them
            while i < n {
                tmp.push(self.bytes_read(s, fs + i, TK_UINT8, true));
                i += 1;
            }
            i = 0;
            while i < n {
                self.bytes_write(d, ts + i, TK_UINT8, tmp[i as usize], true);
                i += 1;
            }
            return Val::Undef();
        }
        if id == NF_BUF_DETACH {
            let b = self.buffer_of(&a[0]);
            if b >= 0 {
                { let c = self.cold_id(b) as usize; self.colds[c].nums = Vec::new(); }
                let cid = self.cold_id(b) as usize;
                self.colds[cid].home = TK_UINT8;
                self.objs[b as usize].pos |= 1;
            }
            return Val::Undef();
        }
        if id == NF_TA_MAKE {
            // (kind, buffer, byteOffset, length, prototype)
            let b = self.buffer_of(&a[1]);
            if b < 0 {
                return Val::Undef();
            }
            let proto = obj_of(&a[4]);
            let kind = num(&a[0]) as int;
            let off = num(&a[2]) as int;
            let len = num(&a[3]) as int;
            let tracking = a[3].is_undef();
            // the first view over all of a new buffer: it holds this kind
            if !tracking && !self.buf_resizable(b) && self.colds[self.objs[b as usize].cold as usize].home < 0 && !is_big(kind) && off == 0 && len * kind_size(kind) == self.buf_size(b) && (kind == TK_UINT8 || kind == TK_UINT8C || (kind == TK_INT8 && self.buf_zero(b))) {
                // one byte per element: the bytes are the elements as they
                // are (an Int8Array's when all are 0)
                let cid = self.cold_id(b) as usize;
                self.colds[cid].home = kind;
            } else if !tracking && !self.buf_resizable(b) && self.colds[self.objs[b as usize].cold as usize].home < 0 && !is_big(kind) && off == 0 && len * kind_size(kind) == self.buf_size(b) {
                if self.alloc_refused((len as double) * 8.0) {
                    return Val::Undef();
                }
                let z = zeros(len);
                self.note_buf_bytes((z.len() as int) * 8);
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
                return Val::Undef();
            }
            let kind = Val::Num(self.objs[t as usize].pos as double);
            let buf = obj_val_of(self.objs[t as usize].env as int);
            let off = Val::Num(self.objs[t as usize].func as double);
            let len = Val::Num(self.ta_length(t) as double);
            let oob = Val::Bool(self.ta_out_of_bounds(t));
            let tracking = Val::Bool(!(self.colds[self.objs[t as usize].cold as usize].prim).is_num());
            let arr = self.new_array(vec![kind, buf, off, len, oob, tracking]);
            return Val::Obj(arr);
        }
        Val::Undef()
    }
}

fn floor(x: double) -> double {
    x.floor()
}

fn to_uint32(x: double) -> double {
    crate::num::to_uint32(x)
}

fn num(v: &Val) -> double {
    if let Some(n) = v.as_num() { n } else { 0.0 }
}

fn obj_val_of(o: int) -> Val {
    if o >= 0 {
        Val::Obj(o)
    } else {
        Val::Undef()
    }
}
