// SPDX-License-Identifier: AGPL-3.0-or-later
//! Compiled code apart from an engine: a script's protos with atoms of its
//! own (a unit), linked into an engine when it runs, and as text that a
//! host keeps on disk, so that a script it ran before is not parsed and
//! compiled again (a code cache).
//!
//! A unit numbers atoms from `first_atoms()` up, and its closures' protos
//! from 0; `link` maps them to the engine's. The ops that name an atom are
//! listed in `atom_operand`: an op that takes a new atom operand must be
//! added there (`tests/cases.txt` runs every case through a unit's text).
//!
//! The text is ASCII: each number a little-endian run of 5-bit digits, the
//! last one in '0'..'O', the others in 'P'..'o', and a string its length
//! and code points.

use ranger::prelude::*;
use std::collections::HashMap;

use crate::ast::*;
use crate::compiler::Compiler;
use crate::num::*;
use crate::ops::*;
use crate::value::*;
use crate::vm::*;

/// Bumped when the text, the ops or what they mean change: older texts
/// are then not read.
pub const UNIT_FORMAT: int = 1;

pub struct Unit {
    pub atom_names: Vec<String>,
    pub protos: Vec<Proto>,
    /// the program's proto
    pub entry: int,
    /// the text the protos' `src` ranges index (Function.prototype.toString)
    pub source: String,
    /// the parser's or compiler's error; the unit is empty then
    pub error: String,
}

impl Unit {
    pub fn new() -> Unit {
        Unit { atom_names: Vec::new(), protos: Vec::new(), entry: -1, source: String::new(), error: String::new() }
    }
}

/// 1: the op's `a` is an atom, 2: its `b`, 0: neither.
pub fn atom_operand(code: int) -> int {
    if code == OP_GET_LOCAL_PROP {
        return 2;
    }
    if code == OP_GET_GLOBAL
        || code == OP_SET_GLOBAL
        || code == OP_TYPEOF_GLOBAL
        || code == OP_DECL_GLOBAL
        || code == OP_GET_PROP
        || code == OP_SET_PROP
        || code == OP_SET_PROP_POP
        || code == OP_DEL_PROP
        || code == OP_GET_METHOD
        || code == OP_GET_THIS_PROP
        || code == OP_GET_PRIVATE
        || code == OP_INIT_PROP
        || code == OP_DEFINE_FIELD
        || code == OP_INIT_GETTER
        || code == OP_INIT_SETTER
        || code == OP_DEFINE_METHOD
        || code == OP_GET_SUPER
        || code == OP_ADD_PROP_POP
    {
        return 1;
    }
    0
}

/// A parsed script compiled as a unit.
pub fn compile(ast: Ast, root: int) -> Unit {
    let names = first_atoms();
    let mut atoms: HashMap<String, int> = HashMap::new();
    let mut i: int = 0;
    for n in names.iter() {
        atoms.insert(n.clone(), i);
        i += 1;
    }
    let source = ast.source.clone();
    let mut c = Compiler::new(ast, atoms, names, 0);
    let entry = c.compile_program(root);
    let mut u = Unit::new();
    if !c.error.is_empty() || entry < 0 {
        u.error = if c.error.is_empty() { String::from("SyntaxError") } else { c.error.clone() };
        return u;
    }
    u.atom_names = c.atom_names;
    u.protos = c.protos;
    u.entry = entry;
    u.source = source;
    u
}

/// Adds the unit's protos to the engine, its atoms and closures renumbered
/// as the engine's; answers the program's proto there.
pub fn link(vm: &mut Vm, u: Unit) -> int {
    let mut map: Vec<int> = Vec::new();
    for n in u.atom_names.iter() {
        map.push(vm.intern(n.as_str()));
    }
    let base = vm.protos.len() as int;
    for pr in u.protos {
        let mut p = pr;
        let mut i: usize = 0;
        let n = p.code.len();
        while i < n {
            let op = p.code[i];
            let k = atom_operand(op.code);
            if k == 1 {
                p.code[i].a = map[op.a as usize];
            } else if k == 2 {
                p.code[i].b = map[op.b as usize];
            } else if op.code == OP_CLOSURE {
                p.code[i].a = op.a + base;
            }
            i += 1;
        }
        vm.protos.push(p);
    }
    base + u.entry
}

// ---- the text

fn put_num(out: &mut String, v: int) {
    let mut z: int = if v >= 0 { v * 2 } else { -v * 2 - 1 };
    loop {
        let d = z % 32;
        z = z / 32;
        if z == 0 {
            out.push(char_of(48 + d));
            return;
        }
        out.push(char_of(80 + d));
    }
}

fn char_of(c: int) -> char {
    match char::from_u32(c as u32) {
        Some(ch) => ch,
        None => '0',
    }
}

fn put_str(out: &mut String, s: &str) {
    let n = s.chars().count() as int;
    put_num(out, n);
    for c in s.chars() {
        put_num(out, c as int);
    }
}

fn put_bool(out: &mut String, b: bool) {
    put_num(out, if b { 1 } else { 0 });
}

fn put_double(out: &mut String, x: double) {
    if x == 0.0 && 1.0 / x < 0.0 {
        put_str(out, "-0");
    } else {
        put_str(out, number_to_string(x).as_str());
    }
}

/// The unit as text; "" when it holds a value the text cannot (a
/// compile-time object).
pub fn encode(u: &Unit, input: &str) -> String {
    let mut out = String::new();
    put_num(&mut out, UNIT_FORMAT);
    put_num(&mut out, u.entry);
    if u.source == input {
        put_num(&mut out, 0);
    } else {
        put_num(&mut out, 1);
        put_str(&mut out, u.source.as_str());
    }
    put_num(&mut out, u.atom_names.len() as int);
    for a in u.atom_names.iter() {
        put_str(&mut out, a.as_str());
    }
    put_num(&mut out, u.protos.len() as int);
    for p in u.protos.iter() {
        put_str(&mut out, p.name.as_str());
        put_num(&mut out, p.length);
        put_num(&mut out, p.nparams);
        put_num(&mut out, p.nslots);
        put_num(&mut out, p.env_size);
        put_num(&mut out, p.rest);
        put_bool(&mut out, p.uses_args);
        put_bool(&mut out, p.arrow);
        put_bool(&mut out, p.strict);
        put_bool(&mut out, p.class_ctor);
        put_bool(&mut out, p.derived);
        put_bool(&mut out, p.method);
        put_bool(&mut out, p.getter_setter);
        put_bool(&mut out, p.generator);
        put_bool(&mut out, p.is_async);
        put_bool(&mut out, p.lexical_ctor);
        // where the function's text is: none, in the source, its own
        match &p.src {
            Val::Undef => put_num(&mut out, 0),
            Val::Str(s) => {
                if s.as_str() == u.source.as_str() {
                    put_num(&mut out, 1);
                } else {
                    put_num(&mut out, 2);
                    put_str(&mut out, s.as_str());
                }
            }
            _ => return String::new(),
        }
        put_num(&mut out, p.src_start);
        put_num(&mut out, p.src_end);
        put_num(&mut out, p.consts.len() as int);
        for c in p.consts.iter() {
            match c {
                Val::Undef => put_num(&mut out, 0),
                Val::Null => put_num(&mut out, 1),
                Val::Bool(b) => put_num(&mut out, if *b { 3 } else { 2 }),
                Val::Num(x) => {
                    put_num(&mut out, 4);
                    put_double(&mut out, *x);
                }
                Val::Str(s) => {
                    put_num(&mut out, 5);
                    put_str(&mut out, s.as_str());
                }
                Val::Obj(_) => return String::new(),
            }
        }
        put_num(&mut out, p.code.len() as int);
        for op in p.code.iter() {
            put_num(&mut out, op.code);
            put_num(&mut out, op.a);
            put_num(&mut out, op.b);
        }
    }
    out
}

struct Reader {
    s: String,
    pos: int,
    len: int,
    bad: bool,
}

impl Reader {
    fn num(&mut self) -> int {
        let mut v: int = 0;
        let mut scale: int = 1;
        loop {
            if self.pos >= self.len {
                self.bad = true;
                return 0;
            }
            let c = self.s.as_bytes()[self.pos as usize] as int;
            self.pos += 1;
            if c >= 48 && c < 80 {
                v += (c - 48) * scale;
                break;
            }
            if c >= 80 && c < 112 {
                v += (c - 80) * scale;
                scale = scale * 32;
                continue;
            }
            self.bad = true;
            return 0;
        }
        if v % 2 == 0 {
            v / 2
        } else {
            -((v + 1) / 2)
        }
    }

    fn string(&mut self) -> String {
        let n = self.num();
        let mut out = String::new();
        let mut i: int = 0;
        while i < n && !self.bad {
            let c = self.num();
            out.push(char_of(c));
            i += 1;
        }
        out
    }

    fn flag(&mut self) -> bool {
        self.num() != 0
    }
}

/// A unit from its text, for the script `input` it was compiled from;
/// entry -1 when the text is not one (another format, damaged).
pub fn decode(text: &str, input: &str) -> Unit {
    let mut u = Unit::new();
    let mut r = Reader { s: String::from(text), pos: 0, len: text.as_bytes().len() as int, bad: false };
    if r.num() != UNIT_FORMAT {
        return u;
    }
    let entry = r.num();
    let shared = if r.num() == 0 { String::from(input) } else { r.string() };
    let src_val = string_val(shared.clone());
    let na = r.num();
    let mut i: int = 0;
    while i < na && !r.bad {
        let a = r.string();
        u.atom_names.push(a);
        i += 1;
    }
    let np = r.num();
    let mut k: int = 0;
    while k < np && !r.bad {
        let name = r.string();
        let mut p = Proto::new(name.as_str());
        p.length = r.num();
        p.nparams = r.num();
        p.nslots = r.num();
        p.env_size = r.num();
        p.rest = r.num();
        p.uses_args = r.flag();
        p.arrow = r.flag();
        p.strict = r.flag();
        p.class_ctor = r.flag();
        p.derived = r.flag();
        p.method = r.flag();
        p.getter_setter = r.flag();
        p.generator = r.flag();
        p.is_async = r.flag();
        p.lexical_ctor = r.flag();
        let sk = r.num();
        if sk == 1 {
            p.src = src_val.clone();
        } else if sk == 2 {
            p.src = string_val(r.string());
        }
        p.src_start = r.num();
        p.src_end = r.num();
        let nc = r.num();
        let mut j: int = 0;
        while j < nc && !r.bad {
            let t = r.num();
            let v = if t == 1 {
                Val::Null
            } else if t == 2 {
                Val::Bool(false)
            } else if t == 3 {
                Val::Bool(true)
            } else if t == 4 {
                Val::Num(string_to_number(r.string().as_str()))
            } else if t == 5 {
                string_val(r.string())
            } else {
                Val::Undef
            };
            p.consts.push(v);
            j += 1;
        }
        let no = r.num();
        let mut m: int = 0;
        while m < no && !r.bad {
            let code = r.num();
            let a = r.num();
            let b = r.num();
            p.code.push(Op { code: code, a: a, b: b, c: -1 });
            m += 1;
        }
        u.protos.push(p);
        k += 1;
    }
    if r.bad || r.pos != r.len || entry < 0 || entry >= u.protos.len() as int || !valid(&u) {
        return Unit::new();
    }
    u.entry = entry;
    u.source = shared;
    u
}

/// Every atom and closure operand in range (a damaged text must not index
/// past the tables).
fn valid(u: &Unit) -> bool {
    let na = u.atom_names.len() as int;
    let np = u.protos.len() as int;
    for p in u.protos.iter() {
        let nc = p.consts.len() as int;
        for op in p.code.iter() {
            let k = atom_operand(op.code);
            if (k == 1 && (op.a < 0 || op.a >= na)) || (k == 2 && (op.b < 0 || op.b >= na)) {
                return false;
            }
            if op.code == OP_CLOSURE && (op.a < 0 || op.a >= np) {
                return false;
            }
            if (op.code == OP_CONST || op.code == OP_SET_NAME || op.code == OP_CHECK_TDZ) && (op.a < 0 || op.a >= nc) {
                return false;
            }
        }
    }
    true
}
