// SPDX-License-Identifier: AGPL-3.0-or-later
//! Values, heap objects and compiled functions.
//!
//! A value is a primitive or an index into the object arena (`Vm.objs`).
//! A string is a handle into the machine's string table (`Vm.strs`, shared
//! `Rc<String>`s), so a value is plain data: copying one costs nothing and
//! dropping one does nothing. The table is swept by the full collection.
//! Every object -- plain object, array, function, scope, error, iterator --
//! is one `JsObj`; `class` tells which, and the fields a class does not use
//! stay empty.

use ranger::prelude::*;
#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
use std::collections::HashMap;
// Rust: a Map's or Set's index hashes with FxHasher (fxmap.rs)
#[ranger::target(rust)]
use crate::fxmap::HashMap;
use std::rc::Rc;

/// What a value is: `Val::kind` gives it to match on.
#[derive(Clone, Copy)]
pub enum VK {
    Undef,
    Null,
    Bool(bool),
    Num(double),
    /// a handle into Vm.strs (0 is the empty string); in a Proto not yet
    /// linked (compiler, unit), an index into Proto.cstrs
    Str(int),
    Obj(int),
    /// a BigInt whose value is an integer below small_big_limit in
    /// magnitude (bigint.rs); larger ones are C_BIGINT objects
    Big(double),
}

/// A value. Made with Val::Num(x), Val::Undef() and the like, taken apart
/// with `match v.kind()`. Rust: 8 bytes, NaN-boxed. A number is its own
/// bits; the other kinds are tagged in the negative quiet-NaN range above
/// the hardware's default NaN (0xFFF8...), which no number then uses: a
/// NaN made from arbitrary bits is kept as the canonical one (Val::Num).
/// The other targets keep the VK in a struct.
#[ranger::target(rust)]
#[derive(Clone, Copy)]
pub struct Val {
    pub b: u64,
}

/// The first tagged pattern: below it a Val is a number.
#[ranger::target(rust)]
pub const V_TAG: u64 = 0xFFF9_0000_0000_0000;
#[ranger::target(rust)]
pub const V_UNDEF: u64 = 0xFFF9_0000_0000_0000;
#[ranger::target(rust)]
pub const V_NULL: u64 = 0xFFFA_0000_0000_0000;
#[ranger::target(rust)]
pub const V_BOOL: u64 = 0xFFFB_0000_0000_0000;
#[ranger::target(rust)]
pub const V_STR: u64 = 0xFFFC_0000_0000_0000;
#[ranger::target(rust)]
pub const V_OBJ: u64 = 0xFFFD_0000_0000_0000;
#[ranger::target(rust)]
pub const V_BIG: u64 = 0xFFFE_0000_0000_0000;
/// the payload bits of a tagged Val (a handle, or a small BigInt's value)
#[ranger::target(rust)]
pub const V_LOW: u64 = 0x0000_FFFF_FFFF_FFFF;

#[ranger::target(rust)]
#[allow(non_snake_case)]
impl Val {
    #[inline(always)]
    pub fn Undef() -> Val {
        Val { b: V_UNDEF }
    }
    #[inline(always)]
    pub fn Null() -> Val {
        Val { b: V_NULL }
    }
    #[inline(always)]
    pub fn Bool(x: bool) -> Val {
        Val { b: V_BOOL | (x as u64) }
    }
    #[inline(always)]
    pub fn Num(x: double) -> Val {
        let b = x.to_bits();
        Val { b: if b >= V_TAG { 0x7FF8_0000_0000_0000 } else { b } }
    }
    /// Val::Num of a number that cannot be a NaN with other bits than the
    /// hardware's (an arithmetic result of numbers, an integer)
    #[inline(always)]
    pub fn NumC(x: double) -> Val {
        debug_assert!(x.to_bits() < V_TAG);
        Val { b: x.to_bits() }
    }
    #[inline(always)]
    pub fn Str(h: int) -> Val {
        debug_assert!(h >= 0 && h <= 0xFFFF_FFFF, "string handle out of range");
        Val { b: V_STR | ((h as u64) & 0xFFFF_FFFF) }
    }
    #[inline(always)]
    pub fn Obj(o: int) -> Val {
        debug_assert!(o >= 0 && o <= 0xFFFF_FFFF, "object index out of range");
        Val { b: V_OBJ | ((o as u64) & 0xFFFF_FFFF) }
    }
    #[inline(always)]
    pub fn Big(n: double) -> Val {
        debug_assert!(n.abs() < crate::bigint::small_big_limit());
        Val { b: V_BIG | ((n as i64 as u64) & V_LOW) }
    }
    #[inline(always)]
    pub fn as_num(&self) -> Option<double> {
        if self.b < V_TAG { Some(f64::from_bits(self.b)) } else { None }
    }
    #[inline(always)]
    pub fn as_obj(&self) -> Option<int> {
        // an object's index and a string's handle are below 2^32: one
        // subtraction both tests the tag and gives the payload
        let d = self.b.wrapping_sub(V_OBJ);
        if d <= 0xFFFF_FFFF { Some(d as int) } else { None }
    }
    #[inline(always)]
    pub fn as_str(&self) -> Option<int> {
        let d = self.b.wrapping_sub(V_STR);
        if d <= 0xFFFF_FFFF { Some(d as int) } else { None }
    }
    #[inline(always)]
    pub fn as_bool(&self) -> Option<bool> {
        if (self.b >> 48) == 0xFFFB { Some((self.b & 1) != 0) } else { None }
    }
    #[inline(always)]
    pub fn is_num(&self) -> bool {
        self.b < V_TAG
    }
    #[inline(always)]
    pub fn is_obj(&self) -> bool {
        self.b.wrapping_sub(V_OBJ) <= 0xFFFF_FFFF
    }
    #[inline(always)]
    pub fn is_str(&self) -> bool {
        self.b.wrapping_sub(V_STR) <= 0xFFFF_FFFF
    }
    #[inline(always)]
    pub fn is_bool(&self) -> bool {
        (self.b >> 48) == 0xFFFB
    }
    #[inline(always)]
    pub fn is_big(&self) -> bool {
        (self.b >> 48) == 0xFFFE
    }
    /// an object or a string (what reference counts track)
    #[inline(always)]
    pub fn is_heap(&self) -> bool {
        (self.b >> 49) == 0x7FFE
    }
    #[inline(always)]
    pub fn is_undef(&self) -> bool {
        self.b == V_UNDEF
    }
    #[inline(always)]
    pub fn is_null(&self) -> bool {
        self.b == V_NULL
    }
    #[inline(always)]
    pub fn is_nullish(&self) -> bool {
        self.b == V_UNDEF || self.b == V_NULL
    }
    #[inline(always)]
    pub fn kind(&self) -> VK {
        let b = self.b;
        if b < V_TAG {
            return VK::Num(f64::from_bits(b));
        }
        // the payload sign-extended from 48 bits
        let x = ((b << 16) as i64) >> 16;
        match b >> 48 {
            0xFFF9 => VK::Undef,
            0xFFFA => VK::Null,
            0xFFFB => VK::Bool(x != 0),
            0xFFFC => VK::Str((b as u32) as int),
            0xFFFD => VK::Obj((b as u32) as int),
            _ => VK::Big(x as double),
        }
    }
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
#[derive(Clone, Copy)]
pub struct Val {
    pub k: VK,
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
#[allow(non_snake_case)]
impl Val {
    #[inline(always)]
    pub fn Undef() -> Val {
        Val { k: VK::Undef }
    }
    #[inline(always)]
    pub fn Null() -> Val {
        Val { k: VK::Null }
    }
    #[inline(always)]
    pub fn Bool(b: bool) -> Val {
        Val { k: VK::Bool(b) }
    }
    #[inline(always)]
    pub fn Num(x: double) -> Val {
        Val { k: VK::Num(x) }
    }
    pub fn NumC(x: double) -> Val {
        Val { k: VK::Num(x) }
    }
    #[inline(always)]
    pub fn Str(h: int) -> Val {
        Val { k: VK::Str(h) }
    }
    #[inline(always)]
    pub fn Obj(o: int) -> Val {
        Val { k: VK::Obj(o) }
    }
    #[inline(always)]
    pub fn Big(n: double) -> Val {
        Val { k: VK::Big(n) }
    }
    pub fn kind(&self) -> VK {
        self.k
    }
    pub fn as_num(&self) -> Option<double> {
        if let VK::Num(x) = self.k { Some(x) } else { None }
    }
    pub fn as_obj(&self) -> Option<int> {
        if let VK::Obj(x) = self.k { Some(x) } else { None }
    }
    pub fn as_str(&self) -> Option<int> {
        if let VK::Str(x) = self.k { Some(x) } else { None }
    }
    pub fn as_bool(&self) -> Option<bool> {
        if let VK::Bool(x) = self.k { Some(x) } else { None }
    }
    pub fn is_num(&self) -> bool {
        matches!(self.k, VK::Num(_))
    }
    pub fn is_obj(&self) -> bool {
        matches!(self.k, VK::Obj(_))
    }
    pub fn is_str(&self) -> bool {
        matches!(self.k, VK::Str(_))
    }
    pub fn is_bool(&self) -> bool {
        matches!(self.k, VK::Bool(_))
    }
    pub fn is_big(&self) -> bool {
        matches!(self.k, VK::Big(_))
    }
    pub fn is_heap(&self) -> bool {
        matches!(self.k, VK::Obj(_) | VK::Str(_))
    }
    pub fn is_undef(&self) -> bool {
        matches!(self.k, VK::Undef)
    }
    pub fn is_null(&self) -> bool {
        matches!(self.k, VK::Null)
    }
    pub fn is_nullish(&self) -> bool {
        matches!(self.k, VK::Undef | VK::Null)
    }
}

/// `===` of two values as far as their bits tell: 1 or 0, 2 when both
/// are strings with different handles (their text decides).
#[ranger::target(rust)]
#[inline(always)]
pub fn seq_quick(a: &Val, b: &Val) -> int {
    if a.b == b.b {
        // the same bits: equal, unless NaN
        if a.b < V_TAG {
            let x = f64::from_bits(a.b);
            return if x == x { 1 } else { 0 };
        }
        return 1;
    }
    if a.b < V_TAG && b.b < V_TAG {
        // 0 and -0
        return if f64::from_bits(a.b) == f64::from_bits(b.b) { 1 } else { 0 };
    }
    if a.is_str() && b.is_str() {
        return 2;
    }
    0
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn seq_quick(a: &Val, b: &Val) -> int {
    match (a.kind(), b.kind()) {
        (VK::Num(x), VK::Num(y)) => if x == y { 1 } else { 0 },
        (VK::Obj(x), VK::Obj(y)) => if x == y { 1 } else { 0 },
        (VK::Big(x), VK::Big(y)) => if x == y { 1 } else { 0 },
        (VK::Undef, VK::Undef) | (VK::Null, VK::Null) => 1,
        (VK::Bool(x), VK::Bool(y)) => if x == y { 1 } else { 0 },
        (VK::Str(x), VK::Str(y)) => if x == y { 1 } else { 2 },
        _ => 0,
    }
}

// object classes
pub const C_OBJECT: i32 = 0;
pub const C_ARRAY: i32 = 1;
pub const C_FUNCTION: i32 = 2;
pub const C_NATIVE: i32 = 3;
pub const C_ENV: i32 = 4;
pub const C_ERROR: i32 = 5;
pub const C_REGEXP: i32 = 6;
pub const C_DATE: i32 = 7;
pub const C_BOUND: i32 = 8;
pub const C_ARGUMENTS: i32 = 9;
pub const C_BOOLEAN: i32 = 10;
pub const C_NUMBER: i32 = 11;
pub const C_STRING: i32 = 12;
pub const C_ACCESSOR: i32 = 13;
pub const C_ITER: i32 = 14;
pub const C_MAP: i32 = 15;
pub const C_SET: i32 = 16;
pub const C_SYMBOL: i32 = 17;
pub const C_PROMISE: i32 = 18;
/// a generator object, or the coroutine of an async call
pub const C_GENERATOR: i32 = 19;
/// an ArrayBuffer (bytes in elems) and a typed array over one (typed.rs)
pub const C_ARRAYBUFFER: i32 = 20;
pub const C_TYPED: i32 = 21;
/// a Proxy (proxy.rs)
pub const C_PROXY: i32 = 22;
/// a BigInt primitive (bigint.rs): its decimal text in prim, interned
pub const C_BIGINT: i32 = 23;
/// an arguments object whose elements are own index properties: a
/// mapped one (sloppy, simple parameters), or one that got attributes,
/// deletes or a length write (vm.rs slow_args)
pub const C_SARGS: i32 = 24;
/// a DataView (typed.rs): its buffer in env, byte offset in func, byte
/// length in pos (-1: tracks a resizable buffer's length)
pub const C_DATAVIEW: i32 = 25;

// property attributes
pub const P_HIDDEN: int = 1;
pub const P_READONLY: int = 2;
pub const P_FIXED: int = 4;
pub const P_ACCESSOR: int = 8;
/// a mapped arguments element: its value is the parameter's (Cold marks)
pub const P_MAPPED: int = 16;

/// Past this many properties an object keeps a hash index of its keys.
pub const INDEX_AT: int = 32;
/// a shape with more keys than this finds them by its table (Shape.itab)
pub const SHAPE_HASH_AT: int = 8;

/// The rarely used parts of an object (a Map's or Set's index, an
/// ArrayBuffer's contents), in `Vm.colds` by `JsObj.cold`: most objects
/// have none, and a JsObj stays small.
pub struct Cold {
    /// a Map's or Set's entries by map_key, and which are deleted (1)
    pub index: HashMap<int, int>,
    /// a Map's or Set's string keys by their text (no atom or String copy
    /// per key): open addressing, pairs (entry + 1, text hash), entry 0
    /// empty and -1 deleted; `sused` counts the pairs not empty
    pub stab: Vec<int>,
    pub sused: int,
    pub marks: Vec<int>,
    /// an ArrayBuffer's contents: its bytes, or the elements of the one
    /// typed array kind that has used it (typed.rs)
    pub nums: Vec<double>,
    /// a map's values, an array's hole markers, a generator's state, a
    /// function's fields (an arrow's new.target, a class's initializer)
    pub elems2: Vec<Val>,
    /// an arrow's `this`, a bound `this`, a boxed primitive, a date's time,
    /// a regexp's source
    pub prim: Val,
    /// the object a method's `super` looks up from; a proxy's handler; an
    /// ArrayBuffer's typed-array kind (a number, not an object)
    pub home: int,
}

/// A Cold's Map / Set index tables freed (Rust: reset_cold in vm.rs).
#[ranger::target(rust)]
pub fn drop_indexes(c: &mut Cold) {
    if c.index.capacity() > 0 {
        c.index = HashMap::new();
    }
    if c.stab.capacity() > 0 {
        c.stab = Vec::new();
    }
    c.sused = 0;
}

impl Cold {
    pub fn new() -> Cold {
        Cold {
            index: HashMap::new(),
            stab: Vec::new(),
            sused: 0,
            marks: Vec::new(),
            nums: Vec::new(),
            elems2: Vec::new(),
            prim: Val::Undef(),
            home: -1,
        }
    }
}

/// An object's elements (`Vm.evecs`, by `JsObj.elems`): a struct, so each
/// target writes them where they are, not into a copy.
/// Rust: a Vec<Val> that drops its first value in O(1) (evec.rs, shift).
#[ranger::target(rust)]
pub struct Elems {
    pub v: crate::evec::EVec,
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub struct Elems {
    pub v: Vec<Val>,
}

/// Elements holding `v`.
#[ranger::target(rust)]
pub fn elems_of(v: Vec<Val>) -> Elems {
    Elems { v: crate::evec::EVec::from_vec(v) }
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn elems_of(v: Vec<Val>) -> Elems {
    Elems { v: v }
}

/// `e`'s elements pushed onto `out` (Rust: one copy of the slice).
#[ranger::target(rust)]
pub fn elems_append(out: &mut Vec<Val>, e: &Elems) {
    out.extend_from_slice(&e.v[..]);
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn elems_append(out: &mut Vec<Val>, e: &Elems) {
    let n = e.v.len();
    let mut i: usize = 0;
    while i < n {
        out.push(e.v[i]);
        i += 1;
    }
}

/// `es[src]`'s elements pushed onto `es[dst]` (Rust: one slice copy).
#[ranger::target(rust)]
pub fn evecs_append(es: &mut Vec<Elems>, dst: usize, src: usize) {
    // room for a few more pushes too ([...a, x]): an exact fit would be
    // copied again by the next push
    let n = es[src].v.len();
    es[dst].v.reserve(n + 8);
    if dst == src {
        let c: Vec<Val> = es[src].v[..].to_vec();
        es[dst].v.extend_from_slice(&c);
    } else if dst < src {
        let (a, b) = es.split_at_mut(src);
        a[dst].v.extend_from_slice(&b[0].v[..]);
    } else {
        let (a, b) = es.split_at_mut(dst);
        b[0].v.extend_from_slice(&a[src].v[..]);
    }
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn evecs_append(es: &mut Vec<Elems>, dst: usize, src: usize) {
    let n = es[src].v.len();
    let mut i: usize = 0;
    while i < n {
        let x = es[src].v[i];
        es[dst].v.push(x);
        i += 1;
    }
}

/// `e.v[i] = x` (Rust: in place, the start kept).
#[ranger::target(rust)]
#[inline(always)]
pub fn elem_put(e: &mut Elems, i: usize, x: Val) {
    e.v.put(i, x);
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn elem_put(e: &mut Elems, i: usize, x: Val) {
    e.v[i] = x;
}

/// `e.v[i] = x` for an `i` the caller has checked (Rust: unchecked).
#[ranger::target(rust)]
#[inline(always)]
pub fn elem_put_in(e: &mut Elems, i: usize, x: Val) {
    e.v.put_unchecked(i, x);
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn elem_put_in(e: &mut Elems, i: usize, x: Val) {
    e.v[i] = x;
}

/// `e`'s values become `v`.
#[ranger::target(rust)]
pub fn elems_set(e: &mut Elems, v: Vec<Val>) {
    e.v.set(v);
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn elems_set(e: &mut Elems, v: Vec<Val>) {
    e.v = v;
}

/// An object's property values from slot 2 on (JsObj.vals): a struct, so
/// each target writes them where they are. Rust: a thin vector, one
/// pointer in the object (svec.rs).
#[ranger::target(rust)]
pub struct Slots {
    pub v: crate::svec::SVec,
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub struct Slots {
    pub v: Vec<Val>,
}

#[ranger::target(rust)]
#[inline(always)]
pub fn slots_new() -> Slots {
    Slots { v: crate::svec::SVec::new() }
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn slots_new() -> Slots {
    Slots { v: Vec::new() }
}

/// The values and their buffer go.
#[ranger::target(rust)]
pub fn slots_free(s: &mut Slots) {
    s.v.free();
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn slots_free(s: &mut Slots) {
    s.v = Vec::new();
}

/// Value `i` for an `i` the caller has checked (Rust: unchecked).
#[ranger::target(rust)]
#[inline(always)]
pub fn slots_at(s: &Slots, i: usize) -> Val {
    *s.v.get_unchecked(i)
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn slots_at(s: &Slots, i: usize) -> Val {
    s.v[i]
}

/// `s.v[i] = x` for an `i` the caller has checked (Rust: unchecked).
#[ranger::target(rust)]
#[inline(always)]
pub fn slots_put(s: &mut Slots, i: usize, x: Val) {
    *s.v.get_unchecked_mut(i) = x;
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn slots_put(s: &mut Slots, i: usize, x: Val) {
    s.v[i] = x;
}

pub struct JsObj {
    pub class: i32,
    /// the prototype, -1 for none; for a scope, the enclosing scope
    pub proto: i32,
    /// the shape (`Vm.shapes`): property keys (atoms) in insertion order
    /// and their attributes; the values are slot by slot in inl0, inl1 and
    /// then vals (get_slot / set_slot), nv of them
    pub shape: i32,
    /// the first two property values, in the object itself: most objects
    /// have no more and need no buffer of their own
    pub inl0: Val,
    pub inl1: Val,
    /// the values of slots 2 on
    pub vals: Slots,
    /// the property count
    pub nv: i32,
    /// the object's Cold (`Vm.colds`), 0 for none
    pub cold: i32,
    /// array elements, scope slots, a bound function's arguments, an
    /// accessor's getter and setter, a map's keys: its index in Vm.evecs,
    /// 0 for none yet
    pub elems: i32,
    /// the compiled function (index into `Vm.protos`) or the native id
    pub func: i32,
    /// a closure's scope; a bound function's target; an iterator's source
    pub env: i32,
    /// an iterator's position; a regexp's flags as bits
    pub pos: i32,
    /// references to the object from other objects (the heap): stack slots
    /// and the machine's roots are not counted (Vm::reclaim)
    pub rc: i32,
    /// in `Vm.zct`, the objects whose count is 0
    pub zct: bool,
    pub extensible: bool,
    pub marked: bool,
    pub free: bool,
    /// some object has had this one as its prototype: a property added
    /// to it while its shape is its own (a dictionary) must stale the
    /// chain caches (Vm.shape_epoch); other dictionaries need not
    pub is_proto: bool,
}

impl JsObj {
    pub fn new(class: i32, proto: i32) -> JsObj {
        JsObj {
            class: class,
            proto: proto,
            shape: 0,
            inl0: Val::Undef(),
            inl1: Val::Undef(),
            vals: slots_new(),
            nv: 0,
            cold: 0,
            elems: 0,
            func: -1,
            env: -1,
            pos: 0,
            rc: 0,
            zct: false,
            extensible: true,
            marked: false,
            free: false,
            is_proto: false,
        }
    }

    pub fn clear(&mut self) {
        // small vectors stay allocated for the next object in the slot;
        // the shape and the Cold are the caller's (Vm::free_shape,
        // Vm::free_cold)
        self.shape = 0;
        self.inl0 = Val::Undef();
        self.inl1 = Val::Undef();
        self.nv = 0;
        if self.vals.v.len() as int <= INDEX_AT {
            self.vals.v.truncate(0);
        } else {
            slots_free(&mut self.vals);
        }
        self.env = -1;
        self.func = -1;
        self.proto = -1;
        self.pos = 0;
        self.rc = 0;
        self.extensible = true;
        self.is_proto = false;
    }

    /// Property slot `i`'s value.
    #[inline(always)]
    pub fn get_slot(&self, i: usize) -> Val {
        if i == 0 {
            self.inl0
        } else if i == 1 {
            self.inl1
        } else {
            // nv is in the object: no read of the values' header
            assert!(i < self.nv as usize, "slot past the object's values");
            slots_at(&self.vals, i - 2)
        }
    }

    /// Writes property slot `i` (counts are the caller's).
    #[inline(always)]
    pub fn set_slot(&mut self, i: usize, v: Val) {
        if i == 0 {
            self.inl0 = v;
        } else if i == 1 {
            self.inl1 = v;
        } else {
            assert!(i < self.nv as usize, "slot past the object's values");
            slots_put(&mut self.vals, i - 2, v);
        }
    }

    /// A new last slot (counts are the caller's).
    pub fn push_slot(&mut self, v: Val) {
        let n = self.nv;
        if n == 0 {
            self.inl0 = v;
        } else if n == 1 {
            self.inl1 = v;
        } else {
            self.vals.v.push(v);
        }
        self.nv = n + 1;
    }

    /// Keeps the slots `keep` says, in order (Vm::compact_dict; the
    /// others hold nothing counted).
    pub fn retain_slots(&mut self, keep: &Vec<bool>) {
        let all = self.slots();
        self.inl0 = Val::Undef();
        self.inl1 = Val::Undef();
        self.vals.v.truncate(0);
        self.nv = 0;
        let mut i: usize = 0;
        while i < all.len() {
            if keep[i] {
                self.push_slot(all[i]);
            }
            i += 1;
        }
    }

    /// Takes slot `i` out, the later ones moving down one.
    pub fn remove_slot(&mut self, i: usize) -> Val {
        let v = self.get_slot(i);
        let n = self.nv as usize;
        // vals holds slots 2.. (one move of the rest, not a slot at a time)
        if i >= 2 {
            self.vals.v.remove(i - 2);
        } else {
            if i == 0 {
                self.inl0 = self.inl1;
            }
            if n > 2 {
                self.inl1 = self.vals.v.remove(0);
            } else {
                self.inl1 = Val::Undef();
                if n == 1 {
                    self.inl0 = Val::Undef();
                }
            }
        }
        self.nv = (n - 1) as i32;
        v
    }

    /// All the property values, in slot order.
    pub fn slots(&self) -> Vec<Val> {
        let n = self.nv as usize;
        let mut out: Vec<Val> = Vec::with_capacity(n);
        let mut i: usize = 0;
        while i < n {
            out.push(self.get_slot(i));
            i += 1;
        }
        out
    }
}

/// Past this many properties an object gets a shape of its own (a
/// dictionary).
pub const SHAPE_MAX: int = 64;

/// Past this many transitions from a shape, the rest are hashed.
pub const T_LINEAR: int = 8;

/// The keys and attributes objects share: objects that got the same
/// properties in the same order with the same attributes have one shape.
/// A shared shape never changes; adding a property moves an object to the
/// next shape (a transition). An owned shape belongs to one object, which
/// changes it in place.
pub struct Shape {
    /// the shape's property count: its keys are the first nkeys of its
    /// holder's (kref)
    pub nkeys: int,
    /// the shape that holds the keys, itself for a holder (the shape at
    /// index 0 for a new one: Vm sets it as it places the shape). A transition from
    /// the newest shape of a chain appends to the chain's holder instead of
    /// copying its keys, so a chain of shapes made one property at a time
    /// shares one key list (a shape tree, without a copy per transition).
    pub kref: int,
    /// the holder's keys and attributes, the whole chain's
    pub keys: Vec<int>,
    pub attrs: Vec<int>,
    /// past SHAPE_HASH_AT keys, slot + 1 by key: an open-addressing table
    /// of a power-of-two size at least twice the keys, 0 for empty
    pub itab: Vec<i32>,
    /// transitions: adding t_atom[i] with t_attr[i] leads to t_to[i];
    /// past T_LINEAR of them, t_map by atom * 32 + attr
    pub t_atom: Vec<int>,
    pub t_attr: Vec<int>,
    pub t_to: Vec<int>,
    pub t_map: HashMap<int, int>,
    pub owned: bool,
    /// for-in (Vm::for_in_fast): 0 not asked yet, 1 the keys can be
    /// walked in place, 2 an index-like key comes first
    pub for_in: i32,
    /// an owned dictionary's deleted keys still in place (Vm::remove_own):
    /// the tombstone atom (TOMB_NAME, a private name), hidden
    pub ndead: int,
}

/// A deleted key's place in a big dictionary (Vm::remove_own): a private
/// name, hidden, so no key list shows it; compacted once they are half.
pub const TOMB_NAME: &str = "\u{1}deleted";

/// Past this many keys a dictionary's delete leaves a tombstone instead of
/// moving every later key and value down.
pub const TOMB_AT: int = 64;

impl Shape {
    pub fn new() -> Shape {
        Shape {
            nkeys: 0,
            kref: 0,
            keys: Vec::new(),
            attrs: Vec::new(),
            itab: Vec::new(),
            t_atom: Vec::new(),
            t_attr: Vec::new(),
            t_to: Vec::new(),
            t_map: HashMap::new(),
            owned: false,
            for_in: 0,
            ndead: 0,
        }
    }

    /// In a holder: the slot of `atom` among its first `n` keys, -1 when
    /// absent (a chain has each key once, so a slot past n is absent).
    #[inline]
    pub fn find(&self, atom: int, n: int) -> int {
        // a short prefix of a long chain: the scan is shorter than the
        // table's probes
        if n > SHAPE_HASH_AT && self.itab.len() > 0 {
            let mask = (self.itab.len() as int) - 1;
            let mut h = key_hash(atom) & mask;
            loop {
                let e = self.itab[h as usize] as int;
                if e == 0 {
                    return -1;
                }
                if self.keys[(e - 1) as usize] == atom {
                    return if e - 1 < n { e - 1 } else { -1 };
                }
                h = (h + 1) & mask;
            }
        }
        let mut i: int = 0;
        while i < n {
            if self.keys[i as usize] == atom {
                return i;
            }
            i += 1;
        }
        -1
    }

    /// In a holder: appends a key; its slot.
    pub fn push(&mut self, atom: int, attr: int) -> int {
        let slot = self.keys.len() as int;
        self.keys.push(atom);
        self.attrs.push(attr);
        if slot + 1 > SHAPE_HASH_AT {
            if 2 * (slot + 1) > self.itab.len() as int {
                self.rehash();
            } else {
                self.tab_put(atom, slot);
            }
        }
        slot
    }

    fn tab_put(&mut self, atom: int, slot: int) {
        let mask = (self.itab.len() as int) - 1;
        let mut h = key_hash(atom) & mask;
        while self.itab[h as usize] != 0 {
            h = (h + 1) & mask;
        }
        self.itab[h as usize] = (slot + 1) as i32;
    }

    /// The table again for the keys there are now.
    fn rehash(&mut self) {
        let n = self.keys.len() as int;
        self.itab = Vec::new();
        if n <= SHAPE_HASH_AT {
            return;
        }
        let mut size: int = 16;
        while size < 2 * n {
            size = size * 2;
        }
        let mut i: int = 0;
        while i < size {
            self.itab.push(0);
            i += 1;
        }
        i = 0;
        while i < n {
            let a = self.keys[i as usize];
            self.tab_put(a, i);
            i += 1;
        }
    }

    /// An owned dictionary's key at `slot` goes, its place kept by the
    /// tombstone atom `tomb` (slots stay where they are).
    pub fn bury(&mut self, slot: int, tomb: int) {
        if self.itab.len() > 0 {
            self.tab_remove(self.keys[slot as usize]);
        }
        self.keys[slot as usize] = tomb;
        self.attrs[slot as usize] = P_HIDDEN | P_READONLY | P_FIXED;
        self.ndead += 1;
    }

    /// The tombstones out: `keep` says which slots stay.
    pub fn compact(&mut self, keep: &Vec<bool>) {
        let mut keys: Vec<int> = Vec::new();
        let mut attrs: Vec<int> = Vec::new();
        let mut i: usize = 0;
        while i < self.keys.len() {
            if keep[i] {
                keys.push(self.keys[i]);
                attrs.push(self.attrs[i]);
            }
            i += 1;
        }
        self.nkeys = keys.len() as int;
        self.keys = keys;
        self.attrs = attrs;
        self.ndead = 0;
        self.rehash();
    }

    /// An owned shape's key at `slot` goes.
    pub fn remove(&mut self, slot: int) {
        if self.itab.len() > 0 && self.keys.len() as int - 1 > SHAPE_HASH_AT {
            // the table kept: the key's entry out, later slots down one
            self.tab_remove(self.keys[slot as usize]);
            dec_above(&mut self.itab, (slot + 1) as i32);
            self.keys.remove(slot as usize);
            self.attrs.remove(slot as usize);
            self.nkeys -= 1;
            return;
        }
        self.keys.remove(slot as usize);
        self.attrs.remove(slot as usize);
        self.nkeys -= 1;
        self.rehash();
    }

    /// Takes `atom`'s entry out of the table (linear probing: the entries
    /// after it in its run move back where their probe allows).
    fn tab_remove(&mut self, atom: int) {
        let mask = (self.itab.len() as int) - 1;
        let mut i = key_hash(atom) & mask;
        loop {
            let e = self.itab[i as usize] as int;
            if e == 0 {
                return;
            }
            if self.keys[(e - 1) as usize] == atom {
                break;
            }
            i = (i + 1) & mask;
        }
        let mut j = i;
        loop {
            j = (j + 1) & mask;
            let e = self.itab[j as usize] as int;
            if e == 0 {
                break;
            }
            let k = key_hash(self.keys[(e - 1) as usize]) & mask;
            // the entry at j may fill i when its home k is not in (i, j]
            let movable = if i <= j { k <= i || k > j } else { k <= i && k > j };
            if movable {
                self.itab[i as usize] = e as i32;
                i = j;
            }
        }
        self.itab[i as usize] = 0;
    }

    /// In a holder: a holder of its own with the first `n` keys and
    /// attributes, without transitions (owned).
    pub fn copy(&self, n: int) -> Shape {
        let mut s = Shape::new();
        reserve_ints(&mut s.keys, (n + 1) as usize);
        reserve_ints(&mut s.attrs, (n + 1) as usize);
        let mut i: usize = 0;
        while (i as int) < n {
            s.push(self.keys[i], self.attrs[i]);
            i += 1;
        }
        s.nkeys = n;
        s.owned = true;
        s
    }
}

/// Rust: packs a function's code into `p.packed` (code, a and b of each op
/// as zigzag varints, seven bytes to an int, after the op count) and frees
/// `code`: most functions a program defines never run. The other targets
/// keep `code`.
#[ranger::target(rust)]
pub fn pack_code(p: &mut Proto) {
    if p.code.is_empty() || !p.packed.is_empty() {
        return;
    }
    let mut w = Packer { words: Vec::new(), cur: 0, nb: 0 };
    w.put(p.code.len() as int);
    let mut i: usize = 0;
    while i < p.code.len() {
        let op = p.code[i];
        w.put((op.code as int));
        w.put((op.a as int));
        w.put((op.b as int));
        i += 1;
    }
    if w.nb > 0 {
        w.words.push(w.cur);
    }
    w.words.shrink_to_fit();
    p.packed = w.words;
    p.code = Vec::new();
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn pack_code(_p: &mut Proto) {}

/// `p.code` again from `p.packed` (Rust; nothing to do elsewhere).
#[ranger::target(rust)]
pub fn unpack_code(p: &mut Proto) {
    if p.packed.is_empty() {
        return;
    }
    p.code = packed_ops(p);
    p.packed = Vec::new();
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn unpack_code(_p: &mut Proto) {}

/// The function's ops, packed or not (a snapshot of a function that has
/// not run).
#[ranger::target(rust)]
pub fn proto_ops(p: &Proto) -> Vec<Op> {
    if p.packed.is_empty() {
        p.code.clone()
    } else {
        packed_ops(p)
    }
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn proto_ops(p: &Proto) -> Vec<Op> {
    p.code.clone()
}

#[ranger::target(rust)]
fn packed_ops(p: &Proto) -> Vec<Op> {
    let mut r = Unpacker { pos: 0, nb: 0, cur: 0 };
    let n = r.get(&p.packed);
    let mut out: Vec<Op> = Vec::new();
    out.reserve_exact(n as usize);
    let mut i: int = 0;
    while i < n {
        let code = r.get(&p.packed);
        let a = r.get(&p.packed);
        let b = r.get(&p.packed);
        out.push(Op { code: (code as i32), a: (a as i32), b: (b as i32), c: -1 });
        i += 1;
    }
    out
}

/// pack_code's writer: bytes of 7 bits and a continuation bit, seven to a
/// word (Rust only, where an int has 64 bits).
#[ranger::target(rust)]
struct Packer {
    words: Vec<int>,
    cur: int,
    nb: int,
}

#[ranger::target(rust)]
impl Packer {
    fn byte(&mut self, x: int) {
        self.cur |= x << (8 * self.nb);
        self.nb += 1;
        if self.nb == 7 {
            self.words.push(self.cur);
            self.cur = 0;
            self.nb = 0;
        }
    }

    fn put(&mut self, v: int) {
        let mut z: int = if v >= 0 { v * 2 } else { -v * 2 - 1 };
        loop {
            if z < 128 {
                self.byte(z);
                return;
            }
            self.byte((z & 127) | 128);
            z = z >> 7;
        }
    }
}

#[ranger::target(rust)]
struct Unpacker {
    pos: usize,
    nb: int,
    cur: int,
}

#[ranger::target(rust)]
impl Unpacker {
    fn byte(&mut self, w: &Vec<int>) -> int {
        if self.nb == 0 {
            self.cur = w[self.pos];
            self.pos += 1;
            self.nb = 7;
        }
        let x = self.cur & 255;
        self.cur = self.cur >> 8;
        self.nb -= 1;
        x
    }

    fn get(&mut self, w: &Vec<int>) -> int {
        let mut z: int = 0;
        let mut shift: int = 0;
        loop {
            let x = self.byte(w);
            z |= (x & 127) << shift;
            if x < 128 {
                break;
            }
            shift += 7;
        }
        if z % 2 == 0 {
            z / 2
        } else {
            -(z + 1) / 2
        }
    }
}

/// `n` zeros.
#[ranger::target(rust)]
pub fn zero_ints(n: usize) -> Vec<int> {
    vec![0; n]
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn zero_ints(n: usize) -> Vec<int> {
    let mut v: Vec<int> = Vec::new();
    while v.len() < n {
        v.push(0);
    }
    v
}

/// Room for `n` ints (Rust; the other targets grow as they push).
#[ranger::target(rust)]
pub fn reserve_ints(v: &mut Vec<int>, n: usize) {
    v.reserve_exact(n);
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn reserve_ints(_v: &mut Vec<int>, _n: usize) {}

#[derive(Clone, Copy)]
pub struct Op {
    pub code: i32,
    pub a: i32,
    pub b: i32,
    /// an inline cache: the slot the property was found at last time
    pub c: i32,
}

pub struct Proto {
    /// the stack bytecode; empty while `packed` holds it (pack_code)
    pub code: Vec<Op>,
    /// Rust: the code of a function that has not run yet, as varints seven
    /// to an int (a few bytes an op, not an Op's 32); unpack_code makes
    /// `code` again when the function first runs
    pub packed: Vec<int>,
    pub consts: Vec<Val>,
    /// the strings of consts and src until the proto is linked
    /// (Vm::link_strs)
    pub cstrs: Vec<Rc<String>>,
    pub name: String,
    /// the function's `length`
    pub length: int,
    /// parameters that arrive in slots 0..nparams
    pub nparams: int,
    pub nslots: int,
    /// slots of the function's own scope object; 0 when it has none
    pub env_size: int,
    /// a rest parameter's slot, -1 for none
    pub rest: int,
    /// `function() { this.m.apply(this, arguments) }` (pcomp): m's atom,
    /// -1 for any other function; `new` of it enters m itself
    /// -2: a class's default derived constructor,
    /// `constructor(...args) { super(...args); }` (Vm::ctor_target)
    pub fwd_atom: int,
    /// no `return` with a value (pcomp; false when not known)
    pub void_ret: bool,
    pub uses_args: bool,
    pub arrow: bool,
    pub strict: bool,
    pub class_ctor: bool,
    pub derived: bool,
    pub method: bool,
    pub getter_setter: bool,
    pub generator: bool,
    pub is_async: bool,
    /// an arrow that reads new.target or calls super(): its closures keep
    /// new.target and the constructor they were made in
    pub lexical_ctor: bool,
    /// the source text the function came from (a shared string, or
    /// undefined) and where its own text lies in it (chars)
    pub src: Val,
    pub src_start: int,
    pub src_end: int,
    /// the register code (regs.rs), made when the function first runs on
    /// the register machine, lies in Vm.rcode from rbase; rstate 0 before,
    /// 1 made
    pub rbase: int,
    /// the frame's slots: locals, then the stack's depths
    pub fsize: int,
    /// where a call starts in rcode
    pub rentry: int,
    pub rstate: int,
}

impl Proto {
    /// `new` may call it
    pub fn constructible(&self) -> bool {
        !self.arrow && !(self.method && !self.class_ctor) && !self.getter_setter && !self.generator && !self.is_async
    }

    pub fn new(name: &str) -> Proto {
        Proto {
            code: Vec::new(),
            packed: Vec::new(),
            consts: Vec::new(),
            cstrs: Vec::new(),
            name: String::from(name),
            length: 0,
            nparams: 0,
            nslots: 0,
            env_size: 0,
            rest: -1,
            fwd_atom: -1,
            void_ret: false,
            uses_args: false,
            arrow: false,
            strict: false,
            class_ctor: false,
            derived: false,
            method: false,
            getter_setter: false,
            generator: false,
            is_async: false,
            lexical_ctor: false,
            src: Val::Undef(),
            src_start: 0,
            src_end: 0,
            rbase: 0,
            fsize: 0,
            rentry: 0,
            rstate: 0,
        }
    }
}

pub struct Frame {
    pub proto: int,
    /// the function object running
    pub fobj: int,
    pub pc: int,
    /// where slot 0 is on the stack
    pub bp: int,
    pub env: int,
    pub this_val: Val,
    pub new_target: Val,
    /// the arguments object, built when the function mentions `arguments`
    pub args_obj: int,
    /// a constructor call: an object result replaces `this`
    pub construct: bool,
    /// the coroutine this frame runs for, -1 for none
    pub gen: int,
    /// register code: the stack slot its result goes to (-1: the caller's
    /// slot of f, or the stack's top when nothing in this run called it)
    pub ret: int,
    /// register code: a value to move from slot mv_from to mv_to when this
    /// frame goes on (a call's result that came back without a return)
    pub mv_from: int,
    pub mv_to: int,
}

pub struct Handler {
    pub frame: int,
    pub catch_pc: int,
    pub sp: int,
    pub env: int,
}

/// Each entry of `v` above `x` one less. Rust: a loop the compiler
/// vectorizes.
#[ranger::target(rust)]
pub fn dec_above(v: &mut Vec<i32>, x: i32) {
    for e in v.iter_mut() {
        *e -= (*e > x) as i32;
    }
}

/// Where a key's probe starts in a shape's table. Atoms made one after
/// another (k0, k1, ...) are consecutive: as they are they would fill one
/// run, and a delete walks the run to its end; spread they leave gaps.
/// (Kept within 31 bits for targets with narrower ints.)
#[inline(always)]
pub fn key_hash(atom: int) -> int {
    (atom & 65535) * 25173 + (atom >> 16)
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn dec_above(v: &mut Vec<i32>, x: i32) {
    let m = v.len();
    let mut h: usize = 0;
    while h < m {
        if v[h] > x {
            v[h] -= 1;
        }
        h += 1;
    }
}
