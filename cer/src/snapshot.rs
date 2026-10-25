// SPDX-License-Identifier: AGPL-3.0-or-later
//! A heap snapshot: an idle engine's whole state (objects, compiled code,
//! atoms, symbols, pending promise jobs) as text, and an engine made from
//! it. A host that sets up the same realm again and again (the built-ins,
//! a library, a page's document and style sheets) keeps the realm once and
//! starts each later one from it instead of running the setup again.
//!
//! Not kept, as they are caches the engine fills again when asked: the
//! decoded Unicode and Intl tables, the string unit caches. Compiled
//! regular expressions are compiled again from their source and flags.
//!
//! The text is unit.rs's: numbers as 5-bit digits, strings once each in a
//! table that values name by index.

use ranger::prelude::*;
use std::collections::HashMap;
use std::rc::Rc;

use crate::num::*;
use crate::unit::*;
use crate::value::*;
use crate::vm::*;

/// Bumped when the text or what it holds changes.
pub const SNAPSHOT_FORMAT: int = 2;

struct Strings {
    index: HashMap<String, int>,
    list: Vec<String>,
}

impl Strings {
    fn id(&mut self, s: &str) -> int {
        match self.index.get(s) {
            Some(i) => *i,
            None => {
                let i = self.list.len() as int;
                self.list.push(String::from(s));
                self.index.insert(String::from(s), i);
                i
            }
        }
    }
}

fn put_val(out: &mut String, st: &mut Strings, v: &Val) {
    match v {
        Val::Undef => put_num(out, 0),
        Val::Null => put_num(out, 1),
        Val::Bool(b) => put_num(out, if *b { 3 } else { 2 }),
        Val::Num(x) => put_number(out, *x),
        Val::Str(s) => {
            put_num(out, 5);
            put_num(out, st.id(s.as_str()));
        }
        Val::Obj(o) => {
            put_num(out, 6);
            put_num(out, *o);
        }
    }
}

/// A number: 7 and the integer for a small whole one, else 4 and its text.
fn put_number(out: &mut String, x: double) {
    if x == x && x >= -9007199254740991.0 && x <= 9007199254740991.0 && x == x.floor() && !(x == 0.0 && 1.0 / x < 0.0) {
        put_num(out, 7);
        put_num(out, x as int);
    } else {
        put_num(out, 4);
        put_double(out, x);
    }
}

fn get_val(r: &mut Reader, strs: &Vec<Val>) -> Val {
    let t = r.num();
    if t == 0 {
        Val::Undef
    } else if t == 1 {
        Val::Null
    } else if t == 2 {
        Val::Bool(false)
    } else if t == 3 {
        Val::Bool(true)
    } else if t == 4 {
        Val::Num(string_to_number(r.string().as_str()))
    } else if t == 5 {
        let i = r.num();
        if i < 0 || i >= strs.len() as int {
            r.bad = true;
            return Val::Undef;
        }
        strs[i as usize].clone()
    } else if t == 6 {
        Val::Obj(r.num())
    } else if t == 7 {
        Val::Num(r.num() as double)
    } else {
        r.bad = true;
        Val::Undef
    }
}

fn put_ints(out: &mut String, v: &Vec<int>) {
    put_num(out, v.len() as int);
    for x in v.iter() {
        put_num(out, *x);
    }
}

fn get_ints(r: &mut Reader) -> Vec<int> {
    let n = r.num();
    let mut v: Vec<int> = Vec::new();
    let mut i: int = 0;
    while i < n && !r.bad {
        v.push(r.num());
        i += 1;
    }
    v
}

fn put_vals(out: &mut String, st: &mut Strings, v: &Vec<Val>) {
    put_num(out, v.len() as int);
    for x in v.iter() {
        put_val(out, st, x);
    }
}

fn get_vals(r: &mut Reader, strs: &Vec<Val>) -> Vec<Val> {
    let n = r.num();
    let mut v: Vec<Val> = Vec::new();
    let mut i: int = 0;
    while i < n && !r.bad {
        v.push(get_val(r, strs));
        i += 1;
    }
    v
}

fn get_str(r: &mut Reader, strs: &Vec<Val>) -> String {
    let k = r.num();
    if k < 0 || k >= strs.len() as int {
        r.bad = true;
        return String::new();
    }
    match &strs[k as usize] {
        Val::Str(s) => s.as_str().to_string(),
        _ => String::new(),
    }
}

fn in_objs(v: &Val, no: int) -> bool {
    match v {
        Val::Obj(o) => *o >= 0 && *o < no,
        _ => true,
    }
}

/// The engine's state as text; "" when it is not idle (a script running,
/// an exception pending), which a snapshot cannot hold.
pub fn snapshot(vm: &Vm) -> String {
    if !vm.stack.is_empty() || !vm.frames.is_empty() || !vm.handlers.is_empty() || vm.throwing || vm.native_depth != 0 || !vm.temp_roots.is_empty() {
        return String::new();
    }
    let mut st = Strings { index: HashMap::new(), list: Vec::new() };
    let mut b = String::new();
    put_num(&mut b, vm.atom_names.len() as int);
    for a in vm.atom_names.iter() {
        put_num(&mut b, st.id(a.as_str()));
    }
    put_num(&mut b, vm.symbol_atoms.len() as int);
    for (k, v) in vm.symbol_atoms.iter() {
        put_num(&mut b, *k);
        put_num(&mut b, *v);
    }
    // the compiled code
    put_num(&mut b, vm.protos.len() as int);
    for p in vm.protos.iter() {
        put_num(&mut b, st.id(p.name.as_str()));
        put_num(&mut b, p.length);
        put_num(&mut b, p.nparams);
        put_num(&mut b, p.nslots);
        put_num(&mut b, p.env_size);
        put_num(&mut b, p.rest);
        let mut flags: int = 0;
        let fl = vec![p.uses_args, p.arrow, p.strict, p.class_ctor, p.derived, p.method, p.getter_setter, p.generator, p.is_async, p.lexical_ctor];
        let mut bit: int = 1;
        for f in fl.iter() {
            if *f {
                flags += bit;
            }
            bit = bit * 2;
        }
        put_num(&mut b, flags);
        put_val(&mut b, &mut st, &p.src);
        put_num(&mut b, p.src_start);
        put_num(&mut b, p.src_end);
        put_vals(&mut b, &mut st, &p.consts);
        put_num(&mut b, p.code.len() as int);
        for op in p.code.iter() {
            put_num(&mut b, op.code);
            put_num(&mut b, op.a);
            put_num(&mut b, op.b);
        }
    }
    // the objects
    put_num(&mut b, vm.objs.len() as int);
    for o in vm.objs.iter() {
        if o.free {
            put_num(&mut b, -1);
            continue;
        }
        put_num(&mut b, o.class);
        put_num(&mut b, o.proto);
        put_ints(&mut b, &o.keys);
        put_vals(&mut b, &mut st, &o.vals);
        put_ints(&mut b, &o.attrs);
        put_vals(&mut b, &mut st, &o.elems);
        put_vals(&mut b, &mut st, &o.elems2);
        put_num(&mut b, o.nums.len() as int);
        for x in o.nums.iter() {
            put_number(&mut b, *x);
        }
        put_num(&mut b, o.func);
        put_num(&mut b, o.env);
        put_val(&mut b, &mut st, &o.prim);
        put_num(&mut b, o.home);
        put_num(&mut b, o.pos);
        put_num(&mut b, (if o.extensible { 1 } else { 0 }) + (if o.has_proto_obj { 2 } else { 0 }));
        // a Map's or Set's index (by map_key) and entry states are kept;
        // the properties' index is made again from the keys
        if o.class == C_MAP || o.class == C_SET {
            put_num(&mut b, o.mindex.len() as int);
            for (k, v) in o.mindex.iter() {
                put_num(&mut b, *k);
                put_num(&mut b, *v);
            }
            put_ints(&mut b, &o.mattrs);
        }
    }
    put_ints(&mut b, &vm.free_list);
    let ints = vec![
        vm.global,
        vm.object_proto,
        vm.function_proto,
        vm.array_proto,
        vm.string_proto,
        vm.number_proto,
        vm.boolean_proto,
        vm.symbol_proto,
        vm.error_proto,
        vm.type_error_proto,
        vm.range_error_proto,
        vm.reference_error_proto,
        vm.syntax_error_proto,
        vm.regexp_proto,
        vm.date_proto,
        vm.map_proto,
        vm.set_proto,
        vm.iter_proto,
        vm.promise_proto,
        vm.iterator_proto,
        vm.map_iter_proto,
        vm.set_iter_proto,
        vm.string_iter_proto,
        vm.generator_proto,
        vm.generator_function_proto,
        vm.async_function_proto,
        vm.async_generator_proto,
        vm.async_generator_function_proto,
        vm.async_from_sync,
        vm.proxy_hooks,
        vm.thrower,
        vm.tdz_obj,
        vm.bigint_proto,
        vm.array_values_fn,
        vm.alloc_count,
        vm.gc_threshold,
        vm.prelude_protos,
        vm.rng,
        vm.gc_runs,
        if vm.echo { 1 } else { 0 },
        if vm.any_setter { 1 } else { 0 },
    ];
    put_ints(&mut b, &ints);
    put_num(&mut b, vm.bigints.len() as int);
    for (k, v) in vm.bigints.iter() {
        put_num(&mut b, st.id(k.as_str()));
        put_num(&mut b, *v);
    }
    put_ints(&mut b, &vm.roots);
    put_num(&mut b, vm.out.len() as int);
    for l in vm.out.iter() {
        put_num(&mut b, st.id(l.as_str()));
    }
    put_ints(&mut b, &vm.symbols);
    put_num(&mut b, vm.symbol_registry.len() as int);
    for (k, v) in vm.symbol_registry.iter() {
        put_num(&mut b, st.id(k.as_str()));
        put_num(&mut b, *v);
    }
    put_vals(&mut b, &mut st, &vm.jobs);
    // the regular expressions by their cache key ("source/flags"), in order
    let mut keys: Vec<String> = Vec::new();
    let mut i: usize = 0;
    while i < vm.regexes.len() {
        keys.push(String::new());
        i += 1;
    }
    for (k, v) in vm.regex_cache.iter() {
        if *v >= 0 && (*v as usize) < keys.len() {
            keys[*v as usize] = k.clone();
        }
    }
    put_num(&mut b, keys.len() as int);
    for k in keys.iter() {
        if k.is_empty() {
            return String::new();
        }
        put_num(&mut b, st.id(k.as_str()));
    }
    let mut out = String::new();
    put_num(&mut out, SNAPSHOT_FORMAT);
    put_num(&mut out, st.list.len() as int);
    for s in st.list.iter() {
        put_str(&mut out, s.as_str());
    }
    out.push_str(b.as_str());
    out
}

/// An engine's state from a snapshot's text, into `vm` (a new Vm); false
/// when the text is not one, and `vm` is then not to be used.
pub fn restore(vm: &mut Vm, text: &str) -> bool {
    let mut r = Reader::new(text);
    if r.num() != SNAPSHOT_FORMAT {
        return false;
    }
    let ns = r.num();
    let mut strs: Vec<Val> = Vec::new();
    let mut i: int = 0;
    while i < ns && !r.bad {
        strs.push(Val::Str(Rc::new(r.string())));
        i += 1;
    }
    vm.atoms = HashMap::new();
    vm.atom_names = Vec::new();
    let na = r.num();
    i = 0;
    while i < na && !r.bad {
        let a = get_str(&mut r, &strs);
        vm.atoms.insert(a.clone(), i);
        vm.atom_names.push(a);
        i += 1;
    }
    vm.symbol_atoms = HashMap::new();
    let nsym = r.num();
    i = 0;
    while i < nsym && !r.bad {
        let k = r.num();
        let v = r.num();
        vm.symbol_atoms.insert(k, v);
        i += 1;
    }
    vm.protos = Vec::new();
    let np = r.num();
    i = 0;
    while i < np && !r.bad {
        let name = get_str(&mut r, &strs);
        let mut p = Proto::new(name.as_str());
        p.length = r.num();
        p.nparams = r.num();
        p.nslots = r.num();
        p.env_size = r.num();
        p.rest = r.num();
        let flags = r.num();
        p.uses_args = flags % 2 == 1;
        p.arrow = (flags / 2) % 2 == 1;
        p.strict = (flags / 4) % 2 == 1;
        p.class_ctor = (flags / 8) % 2 == 1;
        p.derived = (flags / 16) % 2 == 1;
        p.method = (flags / 32) % 2 == 1;
        p.getter_setter = (flags / 64) % 2 == 1;
        p.generator = (flags / 128) % 2 == 1;
        p.is_async = (flags / 256) % 2 == 1;
        p.lexical_ctor = (flags / 512) % 2 == 1;
        p.src = get_val(&mut r, &strs);
        p.src_start = r.num();
        p.src_end = r.num();
        p.consts = get_vals(&mut r, &strs);
        let nc = r.num();
        let mut k: int = 0;
        while k < nc && !r.bad {
            let code = r.num();
            let a = r.num();
            let b = r.num();
            p.code.push(Op { code: code, a: a, b: b, c: -1 });
            k += 1;
        }
        vm.protos.push(p);
        i += 1;
    }
    vm.objs = Vec::new();
    let no = r.num();
    i = 0;
    while i < no && !r.bad {
        let class = r.num();
        if class == -1 {
            let mut o = JsObj::new(C_OBJECT, -1);
            o.free = true;
            vm.objs.push(o);
            i += 1;
            continue;
        }
        let proto = r.num();
        let mut o = JsObj::new(class, proto);
        o.keys = get_ints(&mut r);
        o.vals = get_vals(&mut r, &strs);
        o.attrs = get_ints(&mut r);
        o.elems = get_vals(&mut r, &strs);
        o.elems2 = get_vals(&mut r, &strs);
        let nn = r.num();
        let mut k: int = 0;
        while k < nn && !r.bad {
            let x = get_val(&mut r, &strs);
            o.nums.push(match x {
                Val::Num(d) => d,
                _ => 0.0,
            });
            k += 1;
        }
        o.func = r.num();
        o.env = r.num();
        o.prim = get_val(&mut r, &strs);
        o.home = r.num();
        o.pos = r.num();
        let bits = r.num();
        o.extensible = bits % 2 == 1;
        o.has_proto_obj = (bits / 2) % 2 == 1;
        let n = o.keys.len() as int;
        if class == C_MAP || class == C_SET {
            let ni = r.num();
            let mut j: int = 0;
            while j < ni && !r.bad {
                let k = r.num();
                let v = r.num();
                o.mindex.insert(k, v);
                j += 1;
            }
            o.mattrs = get_ints(&mut r);
        }
        if n > INDEX_AT {
            let mut j: int = 0;
            while j < n {
                o.index.insert(o.keys[j as usize], j);
                j += 1;
            }
        }
        if o.keys.len() != o.vals.len() {
            r.bad = true;
        }
        vm.objs.push(o);
        i += 1;
    }
    vm.free_list = get_ints(&mut r);
    let ints = get_ints(&mut r);
    if ints.len() != 41 {
        return false;
    }
    vm.global = ints[0];
    vm.object_proto = ints[1];
    vm.function_proto = ints[2];
    vm.array_proto = ints[3];
    vm.string_proto = ints[4];
    vm.number_proto = ints[5];
    vm.boolean_proto = ints[6];
    vm.symbol_proto = ints[7];
    vm.error_proto = ints[8];
    vm.type_error_proto = ints[9];
    vm.range_error_proto = ints[10];
    vm.reference_error_proto = ints[11];
    vm.syntax_error_proto = ints[12];
    vm.regexp_proto = ints[13];
    vm.date_proto = ints[14];
    vm.map_proto = ints[15];
    vm.set_proto = ints[16];
    vm.iter_proto = ints[17];
    vm.promise_proto = ints[18];
    vm.iterator_proto = ints[19];
    vm.map_iter_proto = ints[20];
    vm.set_iter_proto = ints[21];
    vm.string_iter_proto = ints[22];
    vm.generator_proto = ints[23];
    vm.generator_function_proto = ints[24];
    vm.async_function_proto = ints[25];
    vm.async_generator_proto = ints[26];
    vm.async_generator_function_proto = ints[27];
    vm.async_from_sync = ints[28];
    vm.proxy_hooks = ints[29];
    vm.thrower = ints[30];
    vm.tdz_obj = ints[31];
    vm.bigint_proto = ints[32];
    vm.array_values_fn = ints[33];
    vm.alloc_count = ints[34];
    vm.gc_threshold = ints[35];
    vm.prelude_protos = ints[36];
    vm.rng = ints[37];
    vm.gc_runs = ints[38];
    vm.echo = ints[39] == 1;
    vm.any_setter = ints[40] == 1;
    vm.bigints = HashMap::new();
    let nb = r.num();
    i = 0;
    while i < nb && !r.bad {
        let k = get_str(&mut r, &strs);
        let v = r.num();
        vm.bigints.insert(k, v);
        i += 1;
    }
    vm.roots = get_ints(&mut r);
    vm.out = Vec::new();
    let nout = r.num();
    i = 0;
    while i < nout && !r.bad {
        let l = get_str(&mut r, &strs);
        vm.out.push(l);
        i += 1;
    }
    vm.symbols = get_ints(&mut r);
    vm.symbol_registry = HashMap::new();
    let nr = r.num();
    i = 0;
    while i < nr && !r.bad {
        let k = get_str(&mut r, &strs);
        let v = r.num();
        vm.symbol_registry.insert(k, v);
        i += 1;
    }
    vm.jobs = get_vals(&mut r, &strs);
    vm.regexes = Vec::new();
    vm.regex_cache = HashMap::new();
    let nx = r.num();
    i = 0;
    while i < nx && !r.bad {
        let key = get_str(&mut r, &strs);
        // "source/flags": the flags are the letters after the last '/'
        let cs: Vec<char> = key.chars().collect();
        let mut cut = cs.len();
        while cut > 0 && cs[cut - 1] != '/' {
            cut -= 1;
        }
        if cut == 0 {
            r.bad = true;
            break;
        }
        let mut source = String::new();
        let mut flags = String::new();
        let mut j: usize = 0;
        while j < cs.len() {
            if j + 1 < cut {
                source.push(cs[j]);
            } else if j >= cut {
                flags.push(cs[j]);
            }
            j += 1;
        }
        let re = crate::regex::compile(source.as_str(), flags.as_str());
        vm.regexes.push(re);
        vm.regex_cache.insert(key, i);
        i += 1;
    }
    if r.bad || r.pos != r.len || !valid_heap(vm) {
        return false;
    }
    vm.find_setters();
    true
}

/// Every object and proto index in range: a damaged text must not index
/// past the tables.
fn valid_heap(vm: &Vm) -> bool {
    let no = vm.objs.len() as int;
    let np = vm.protos.len() as int;
    let na = vm.atom_names.len() as int;
    for o in vm.objs.iter() {
        if o.free {
            continue;
        }
        if o.proto < -1 || o.proto >= no || o.env < -1 || o.env >= no || o.home < -1 || o.home >= no || !in_objs(&o.prim, no) {
            return false;
        }
        if o.class == C_FUNCTION && (o.func < 0 || o.func >= np) {
            return false;
        }
        for k in o.keys.iter() {
            if *k < 0 || *k >= na {
                return false;
            }
        }
        for v in o.vals.iter() {
            if !in_objs(v, no) {
                return false;
            }
        }
        for v in o.elems.iter() {
            if !in_objs(v, no) {
                return false;
            }
        }
        for v in o.elems2.iter() {
            if !in_objs(v, no) {
                return false;
            }
        }
    }
    for p in vm.protos.iter() {
        for c in p.consts.iter() {
            if !in_objs(c, no) {
                return false;
            }
        }
    }
    vm.global >= 0 && vm.global < no
}
