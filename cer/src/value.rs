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
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Clone, Copy)]
pub enum Val {
    Undef,
    Null,
    Bool(bool),
    Num(double),
    /// a handle into Vm.strs (0 is the empty string); in a Proto not yet
    /// linked (compiler, unit), an index into Proto.cstrs
    Str(int),
    Obj(int),
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

// property attributes
pub const P_HIDDEN: int = 1;
pub const P_READONLY: int = 2;
pub const P_FIXED: int = 4;
pub const P_ACCESSOR: int = 8;

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

impl Cold {
    pub fn new() -> Cold {
        Cold {
            index: HashMap::new(),
            marks: Vec::new(),
            nums: Vec::new(),
            elems2: Vec::new(),
            prim: Val::Undef,
            home: -1,
        }
    }
}

pub struct JsObj {
    pub class: i32,
    /// the prototype, -1 for none; for a scope, the enclosing scope
    pub proto: i32,
    /// the shape (`Vm.shapes`): property keys (atoms) in insertion order
    /// and their attributes; `vals` holds the values, slot by slot
    pub shape: i32,
    pub vals: Vec<Val>,
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
    /// a function's own `prototype` object was made
    pub has_proto_obj: bool,
}

impl JsObj {
    pub fn new(class: i32, proto: i32) -> JsObj {
        JsObj {
            class: class,
            proto: proto,
            shape: 0,
            vals: Vec::new(),
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
            has_proto_obj: false,
        }
    }

    pub fn clear(&mut self) {
        // small vectors stay allocated for the next object in the slot;
        // the shape and the Cold are the caller's (Vm::free_shape,
        // Vm::free_cold)
        self.shape = 0;
        if self.vals.len() as int <= INDEX_AT {
            self.vals.truncate(0);
        } else {
            self.vals = Vec::new();
        }
        self.env = -1;
        self.func = -1;
        self.proto = -1;
        self.pos = 0;
        self.rc = 0;
        self.extensible = true;
        self.has_proto_obj = false;
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
    pub itab: Vec<int>,
    /// transitions: adding t_atom[i] with t_attr[i] leads to t_to[i];
    /// past T_LINEAR of them, t_map by atom * 16 + attr
    pub t_atom: Vec<int>,
    pub t_attr: Vec<int>,
    pub t_to: Vec<int>,
    pub t_map: HashMap<int, int>,
    pub owned: bool,
}

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
            let mut h = atom & mask;
            loop {
                let e = self.itab[h as usize];
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
        let mut h = atom & mask;
        while self.itab[h as usize] != 0 {
            h = (h + 1) & mask;
        }
        self.itab[h as usize] = slot + 1;
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

    /// An owned shape's key at `slot` goes.
    pub fn remove(&mut self, slot: int) {
        self.keys.remove(slot as usize);
        self.attrs.remove(slot as usize);
        self.nkeys -= 1;
        self.rehash();
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
        w.put(op.code);
        w.put(op.a);
        w.put(op.b);
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
        out.push(Op { code: code, a: a, b: b, c: -1 });
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

/// Room for `n` ints (Rust; the other targets grow as they push).
#[ranger::target(rust)]
pub fn reserve_ints(v: &mut Vec<int>, n: usize) {
    v.reserve_exact(n);
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn reserve_ints(_v: &mut Vec<int>, _n: usize) {}

#[derive(Clone, Copy)]
pub struct Op {
    pub code: int,
    pub a: int,
    pub b: int,
    /// an inline cache: the slot the property was found at last time
    pub c: int,
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
            src: Val::Undef,
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
