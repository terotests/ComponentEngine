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
//! last one in '0'..'O', the others in 'P'..'o'; a string is its length
//! and its text when that is printable ASCII, else its code points.

use ranger::prelude::*;
use std::rc::Rc;
use std::collections::HashMap;

use crate::ast::*;
use crate::compiler::Compiler;
use crate::num::*;
use crate::ops::*;
use crate::value::*;
use crate::vm::*;

/// Bumped when the text, the ops or what they mean change: older texts
/// are then not read.
pub const UNIT_FORMAT: int = 2;

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

/// 1: the op's `a` is an atom, 2: its `b`, 3: `a` and a `b` not
/// negative, 0: neither.
pub fn atom_operand(code: int) -> int {
    if code == OP_GET_LOCAL_PROP {
        return 2;
    }
    if code == OP_GET_PRIVATE || code == OP_SET_PRIVATE || code == OP_HAS_PRIVATE {
        // and b when it is not negative (a brand)
        return 3;
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
        || code == OP_INIT_PROP
        || code == OP_DEFINE_FIELD
        || code == OP_INIT_GETTER
        || code == OP_INIT_SETTER
        || code == OP_DEFINE_METHOD
        || code == OP_GET_SUPER
        || code == OP_SET_SUPER
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
    // a class's private keys and brand ("…\u{2}<id>"): ids of the engine,
    // new at each link, so that classes of two scripts never share one
    let mut ids: HashMap<String, int> = HashMap::new();
    for n in u.atom_names.iter() {
        let cut = crate::jsstr::index_of(n.as_str(), "\u{2}", 0);
        if cut > 0 && n.starts_with("\u{1}") {
            let local = crate::jsstr::slice(n.as_str(), cut + 1, crate::jsstr::len(n.as_str()));
            let id = match ids.get(&local) {
                Some(v) => *v,
                None => {
                    vm.priv_seq += 1;
                    let v = vm.priv_seq;
                    ids.insert(local.clone(), v);
                    v
                }
            };
            let nm = format!("{}\u{2}{}", crate::jsstr::slice(n.as_str(), 0, cut), id);
            map.push(vm.intern(nm.as_str()));
        } else {
            map.push(vm.intern(n.as_str()));
        }
    }
    let base = vm.protos.len() as int;
    for pr in u.protos {
        let mut p = pr;
        // a direct eval's desc names the caller's private keys: the same ids
        let mut ci: usize = 0;
        while ci < p.code.len() {
            if (p.code[ci].code as int) == OP_CALL_EVAL {
                // its desc: a constant, the index of its text before linking
                let kb = p.code[ci].b as usize;
                let mut si: usize = p.cstrs.len();
                if kb < p.consts.len() {
                    if let Val::Str(k) = p.consts[kb] {
                        si = k as usize;
                    }
                }
                if si < p.cstrs.len() && p.cstrs[si].contains(";p|") {
                    let t = remap_priv_ids(p.cstrs[si].as_str(), &mut ids, vm);
                    p.cstrs[si] = Rc::new(t);
                }
            }
            ci += 1;
        }
        vm.link_strs(&mut p);
        if p.fwd_atom >= 0 {
            p.fwd_atom = map[p.fwd_atom as usize];
        }
        let mut i: usize = 0;
        let n = p.code.len();
        while i < n {
            let op = p.code[i];
            let k = atom_operand((op.code as int));
            if k == 1 || k == 3 {
                p.code[i].a = (map[op.a as usize] as i32);
                if k == 3 && op.b >= 0 {
                    p.code[i].b = (map[op.b as usize] as i32);
                }
            } else if k == 2 {
                p.code[i].b = (map[op.b as usize] as i32);
            } else if (op.code as int) == OP_CLOSURE {
                p.code[i].a = op.a + (base as i32);
            }
            i += 1;
        }
        pack_code(&mut p);
        vm.protos.push(p);
    }
    base + u.entry
}

/// A desc's private entries ("…\u{2}<id>~…") with the ids `ids` maps
/// them to (a new engine id for one not seen yet).
fn remap_priv_ids(s: &str, ids: &mut HashMap<String, int>, vm: &mut Vm) -> String {
    let mut out = String::new();
    let mut digits = String::new();
    let mut in_id = false;
    for c in s.chars() {
        if in_id {
            if c >= '0' && c <= '9' {
                digits.push(c);
                continue;
            }
            if c == '~' {
                let id = match ids.get(&digits) {
                    Some(v) => *v,
                    None => {
                        vm.priv_seq += 1;
                        let v = vm.priv_seq;
                        ids.insert(digits.clone(), v);
                        v
                    }
                };
                out.push_str(format!("{}", id).as_str());
            } else {
                out.push_str(digits.as_str());
            }
            digits = String::new();
            in_id = false;
        }
        out.push(c);
        if c == '\u{2}' {
            in_id = true;
        }
    }
    out.push_str(digits.as_str());
    out
}

// ---- the text

pub fn put_num(out: &mut String, v: int) {
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

pub fn char_of(c: int) -> char {
    match char::from_u32(c as u32) {
        Some(ch) => ch,
        None => '0',
    }
}

pub fn put_str(out: &mut String, s: &str) {
    // printable ASCII as it is (the text stays ASCII, so a reader can cut
    // it by position), anything else as code points
    let mut plain = true;
    for c in s.chars() {
        let k = c as int;
        if k < 32 || k > 126 {
            plain = false;
            break;
        }
    }
    let n = s.chars().count() as int;
    if plain {
        put_num(out, n * 2 + 1);
        out.push_str(s);
        return;
    }
    put_num(out, n * 2);
    for c in s.chars() {
        put_num(out, c as int);
    }
}

pub fn put_bool(out: &mut String, b: bool) {
    put_num(out, if b { 1 } else { 0 });
}

pub fn put_double(out: &mut String, x: double) {
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
            Val::Str(k) => {
                let s = &p.cstrs[*k as usize];
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
                Val::Str(k) => {
                    put_num(&mut out, 5);
                    put_str(&mut out, p.cstrs[*k as usize].as_str());
                }
                Val::Big(x) => {
                    put_num(&mut out, 6);
                    put_num(&mut out, *x as int);
                }
                Val::Obj(_) => return String::new(),
            }
        }
        put_num(&mut out, p.code.len() as int);
        for op in p.code.iter() {
            put_num(&mut out, (op.code as int));
            put_num(&mut out, (op.a as int));
            put_num(&mut out, (op.b as int));
        }
    }
    out
}

pub struct Reader {
    pub s: String,
    pub pos: int,
    pub len: int,
    pub bad: bool,
}

impl Reader {
    pub fn new(text: &str) -> Reader {
        Reader { s: String::from(text), pos: 0, len: text.as_bytes().len() as int, bad: false }
    }

    pub fn num(&mut self) -> int {
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

    pub fn string(&mut self) -> String {
        let m = self.num();
        let n = m / 2;
        if m % 2 == 1 {
            if n < 0 || self.pos + n > self.len {
                self.bad = true;
                return String::new();
            }
            let out = String::from(&self.s[(self.pos as usize)..((self.pos + n) as usize)]);
            self.pos += n;
            return out;
        }
        let mut out = String::new();
        let mut i: int = 0;
        while i < n && !self.bad {
            let c = self.num();
            out.push(char_of(c));
            i += 1;
        }
        out
    }

    pub fn flag(&mut self) -> bool {
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
    let src_rc = Rc::new(shared.clone());
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
            p.src = Val::Str(p.cstrs.len() as int);
            p.cstrs.push(src_rc.clone());
        } else if sk == 2 {
            p.src = Val::Str(p.cstrs.len() as int);
            p.cstrs.push(Rc::new(r.string()));
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
                let k = p.cstrs.len() as int;
                p.cstrs.push(Rc::new(r.string()));
                Val::Str(k)
            } else if t == 6 {
                Val::Big((r.num() as double) + 0.0)
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
            p.code.push(Op { code: (code as i32), a: (a as i32), b: (b as i32), c: -1 });
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

/// Every function's code passes code_ok (a damaged text must not index
/// past the tables).
fn valid(u: &Unit) -> bool {
    let na = u.atom_names.len() as int;
    let np = u.protos.len() as int;
    for p in u.protos.iter() {
        if !code_ok(p, na, np) {
            return false;
        }
    }
    true
}

/// A function's code read from a text (a code cache unit, a snapshot) is
/// one the interpreters can run without indexing past their tables: its
/// counts are sane; every atom, closure, constant and local operand is in
/// range; and the stack depth is the same on every path to an op, never
/// below the frame, from known ops only (regs::Tr::flow, as the register
/// translation checks it). `na` atoms, `np` functions.
pub fn code_ok(p: &Proto, na: int, np: int) -> bool {
    let n = p.code.len() as int;
    let ns = p.nslots;
    if ns < 0 || ns > 1000000 || p.nparams < 0 || p.nparams > ns || p.env_size < 0 || p.env_size > 1000000 || p.rest < -1 || p.rest > ns {
        return false;
    }
    let nc = p.consts.len() as int;
    for op in p.code.iter() {
        let c = op.code as int;
        let k = atom_operand(c);
        if ((k == 1 || k == 3) && (op.a < 0 || (op.a as int) >= na)) || (k == 2 && (op.b < 0 || (op.b as int) >= na)) || (k == 3 && (op.b as int) >= na) {
            return false;
        }
        if c == OP_CLOSURE && (op.a < 0 || (op.a as int) >= np) {
            return false;
        }
        if (c == OP_CONST || c == OP_SET_NAME || c == OP_CHECK_TDZ) && (op.a < 0 || (op.a as int) >= nc) {
            return false;
        }
        let local_a = c == OP_GET_LOCAL || c == OP_SET_LOCAL || c == OP_SET_LOCAL_POP || c == OP_INC_LOCAL || c == OP_POSTINC_LOCAL || c == OP_PREINC_LOCAL || c == OP_GET_LOCAL_PROP || c == OP_ADD_LOCAL_POP || c == OP_GET_LOCAL2;
        if local_a && (op.a < 0 || (op.a as int) >= ns) {
            return false;
        }
        if c == OP_GET_LOCAL2 && (op.b < 0 || (op.b as int) >= ns) {
            return false;
        }
        if c == OP_LOCAL_NULL_JF && (op.b < 0 || ((op.b / 256) as int) >= ns) {
            return false;
        }
        if (c == OP_CALL || c == OP_CALL_EVAL || c == OP_NEW || c == OP_SUPER_CALL || c == OP_NEW_ARRAY || c == OP_OBJ_REST) && (op.a < 0 || op.a > 1048576) {
            return false;
        }
        if c == OP_OBJ_REST && (op.b < 0 || op.b > 1048576) {
            return false;
        }
        if (c == OP_GET_ENV || c == OP_SET_ENV || c == OP_GET_ENV_UNDEF || c == OP_ADD_ENV_POP) && (op.a < 0 || op.b < 0) {
            return false;
        }
    }
    if n == 0 {
        return true;
    }
    let mut f = crate::regs::Fl::new(n);
    f.run(&p.code);
    f.err.is_empty()
}
