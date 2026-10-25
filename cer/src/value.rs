// SPDX-License-Identifier: AGPL-3.0-or-later
//! Values, heap objects and compiled functions.
//!
//! A value is a primitive or an index into the object arena (`Vm.objs`).
//! Strings are shared (`Rc<String>`), so passing one around never copies it.
//! Every object -- plain object, array, function, scope, error, iterator --
//! is one `JsObj`; `class` tells which, and the fields a class does not use
//! stay empty.

use ranger::prelude::*;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Clone)]
pub enum Val {
    Undef,
    Null,
    Bool(bool),
    Num(double),
    Str(Rc<String>),
    Obj(int),
}

pub fn str_val(s: &str) -> Val {
    Val::Str(Rc::new(String::from(s)))
}

pub fn string_val(s: String) -> Val {
    Val::Str(Rc::new(s))
}

// object classes
pub const C_OBJECT: int = 0;
pub const C_ARRAY: int = 1;
pub const C_FUNCTION: int = 2;
pub const C_NATIVE: int = 3;
pub const C_ENV: int = 4;
pub const C_ERROR: int = 5;
pub const C_REGEXP: int = 6;
pub const C_DATE: int = 7;
pub const C_BOUND: int = 8;
pub const C_ARGUMENTS: int = 9;
pub const C_BOOLEAN: int = 10;
pub const C_NUMBER: int = 11;
pub const C_STRING: int = 12;
pub const C_ACCESSOR: int = 13;
pub const C_ITER: int = 14;
pub const C_MAP: int = 15;
pub const C_SET: int = 16;
pub const C_SYMBOL: int = 17;
pub const C_PROMISE: int = 18;
/// a generator object, or the coroutine of an async call
pub const C_GENERATOR: int = 19;
/// an ArrayBuffer (bytes in elems) and a typed array over one (typed.rs)
pub const C_ARRAYBUFFER: int = 20;
pub const C_TYPED: int = 21;
/// a Proxy (proxy.rs)
pub const C_PROXY: int = 22;
/// a BigInt primitive (bigint.rs): its decimal text in prim, interned
pub const C_BIGINT: int = 23;

// property attributes
pub const P_HIDDEN: int = 1;
pub const P_READONLY: int = 2;
pub const P_FIXED: int = 4;
pub const P_ACCESSOR: int = 8;

/// Past this many properties an object keeps a hash index of its keys.
pub const INDEX_AT: int = 32;

pub struct JsObj {
    pub class: int,
    /// the prototype, -1 for none; for a scope, the enclosing scope
    pub proto: int,
    /// the shape (`Vm.shapes`): property keys (atoms) in insertion order
    /// and their attributes; `vals` holds the values, slot by slot
    pub shape: int,
    pub vals: Vec<Val>,
    /// a Map's or Set's entries by map_key, and which are deleted (1)
    pub index: HashMap<int, int>,
    pub marks: Vec<int>,
    /// array elements, scope slots, a bound function's arguments, an
    /// accessor's getter and setter, a map's keys
    pub elems: Vec<Val>,
    /// a map's values
    pub elems2: Vec<Val>,
    /// an ArrayBuffer's contents: its bytes, or the elements of the one
    /// typed array kind that has used it (typed.rs)
    pub nums: Vec<double>,
    /// the compiled function (index into `Vm.protos`) or the native id
    pub func: int,
    /// a closure's scope; a bound function's target; an iterator's source
    pub env: int,
    /// an arrow's `this`, a bound `this`, a boxed primitive, a date's time,
    /// a regexp's source
    pub prim: Val,
    /// the object a method's `super` looks up from
    pub home: int,
    /// an iterator's position; a regexp's flags as bits
    pub pos: int,
    pub extensible: bool,
    pub marked: bool,
    pub free: bool,
    /// a function's own `prototype` object was made
    pub has_proto_obj: bool,
}

impl JsObj {
    pub fn new(class: int, proto: int) -> JsObj {
        JsObj {
            class: class,
            proto: proto,
            shape: 0,
            vals: Vec::new(),
            index: HashMap::new(),
            marks: Vec::new(),
            elems: Vec::new(),
            elems2: Vec::new(),
            nums: Vec::new(),
            func: -1,
            env: -1,
            prim: Val::Undef,
            home: -1,
            pos: 0,
            extensible: true,
            marked: false,
            free: false,
            has_proto_obj: false,
        }
    }

    pub fn clear(&mut self) {
        // a small object's values stay allocated for the next one; the
        // shape is the caller's (Vm::free_shape)
        self.shape = 0;
        if self.vals.len() as int <= INDEX_AT {
            self.vals.truncate(0);
        } else {
            self.vals = Vec::new();
        }
        if !self.index.is_empty() {
            self.index = HashMap::new();
        }
        if !self.marks.is_empty() {
            self.marks = Vec::new();
        }
        if self.elems.len() <= 16 {
            self.elems.truncate(0);
        } else {
            self.elems = Vec::new();
        }
        self.elems2 = Vec::new();
        self.nums = Vec::new();
        self.prim = Val::Undef;
        self.env = -1;
        self.home = -1;
        self.func = -1;
        self.proto = -1;
        self.pos = 0;
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
    pub keys: Vec<int>,
    pub attrs: Vec<int>,
    /// past INDEX_AT keys, slot by key
    pub index: HashMap<int, int>,
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
            keys: Vec::new(),
            attrs: Vec::new(),
            index: HashMap::new(),
            t_atom: Vec::new(),
            t_attr: Vec::new(),
            t_to: Vec::new(),
            t_map: HashMap::new(),
            owned: false,
        }
    }

    /// The slot of `atom`, -1 when absent.
    #[inline]
    pub fn find(&self, atom: int) -> int {
        let n = self.keys.len() as int;
        if n > INDEX_AT {
            return match self.index.get(&atom) {
                Some(i) => *i,
                None => -1,
            };
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

    pub fn push(&mut self, atom: int, attr: int) -> int {
        let slot = self.keys.len() as int;
        self.keys.push(atom);
        self.attrs.push(attr);
        if slot + 1 > INDEX_AT {
            if slot == INDEX_AT {
                let mut i: int = 0;
                while i < slot {
                    self.index.insert(self.keys[i as usize], i);
                    i += 1;
                }
            }
            self.index.insert(atom, slot);
        }
        slot
    }

    pub fn remove(&mut self, slot: int) {
        self.keys.remove(slot as usize);
        self.attrs.remove(slot as usize);
        self.index = HashMap::new();
        let n = self.keys.len() as int;
        if n > INDEX_AT {
            let mut i: int = 0;
            while i < n {
                self.index.insert(self.keys[i as usize], i);
                i += 1;
            }
        }
    }

    /// An owned copy of the keys and attributes, without transitions.
    pub fn copy(&self) -> Shape {
        let mut s = Shape::new();
        let mut i: usize = 0;
        while i < self.keys.len() {
            s.push(self.keys[i], self.attrs[i]);
            i += 1;
        }
        s.owned = true;
        s
    }
}

#[derive(Clone, Copy)]
pub struct Op {
    pub code: int,
    pub a: int,
    pub b: int,
    /// an inline cache: the slot the property was found at last time
    pub c: int,
}

pub struct Proto {
    pub code: Vec<Op>,
    pub consts: Vec<Val>,
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
}

impl Proto {
    /// `new` may call it
    pub fn constructible(&self) -> bool {
        !self.arrow && !(self.method && !self.class_ctor) && !self.getter_setter && !self.generator && !self.is_async
    }

    pub fn new(name: &str) -> Proto {
        Proto {
            code: Vec::new(),
            consts: Vec::new(),
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
}

pub struct Handler {
    pub frame: int,
    pub catch_pc: int,
    pub sp: int,
    pub env: int,
}
