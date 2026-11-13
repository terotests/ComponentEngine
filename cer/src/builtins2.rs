// SPDX-License-Identifier: AGPL-3.0-or-later
//! The rest of the built-in functions: Array, String, Number, Boolean, JSON,
//! RegExp, Date, Map / Set, Promise and iterators.

use ranger::prelude::*;

use crate::builtins::*;
use crate::jsstr;
use crate::num::*;
use crate::value::*;
use crate::vm::*;

impl Vm {
    fn this_array(&mut self, this: &Val) -> int {
        match this {
            Val::Obj(o) => *o,
            _ => {
                let o = self.to_object(this);
                o
            }
        }
    }

    /// HasProperty(o, k) for an index: an element in place, else the
    /// generic lookup (element properties, prototypes, proxies).
    pub fn has_elem_or_prop(&mut self, o: int, k: int) -> bool {
        if self.objs[o as usize].class == C_ARRAY && k < (self.evecs[self.objs[o as usize].elems as usize].v.len() as int) && !self.is_hole(o, k) {
            return true;
        }
        let kv = Val::Num(k as double);
        self.has_property(o, &kv)
    }

    /// The elements of an array-like `this` (arrays directly).
    fn elems_of(&mut self, o: int) -> Vec<Val> {
        if o < 0 {
            return Vec::new();
        }
        let class = self.objs[o as usize].class;
        if class == C_ARRAY && !self.colds[self.objs[o as usize].cold as usize].elems2.is_empty() && (self.objs[o as usize].pos == 3 || self.idx_protos) {
            // holes read through element properties and the prototypes
            let n = self.evecs[self.objs[o as usize].elems as usize].v.len() as int;
            let mut out: Vec<Val> = Vec::with_capacity(n as usize);
            let ov = Val::Obj(o);
            let mut k: int = 0;
            while k < n {
                let v = self.get_index(&ov, k);
                out.push(v);
                k += 1;
            }
            return out;
        }
        if class == C_ARRAY || class == C_ARGUMENTS {
            return self.evecs[self.objs[o as usize].elems as usize].v.clone();
        }
        self.array_like_to_vec(&Val::Obj(o))
    }

    fn is_array_obj(&self, o: int) -> bool {
        self.objs[o as usize].class == C_ARRAY
    }

    fn callback(&mut self, f: &Val, name: &str) -> bool {
        if !self.is_callable(f) {
            self.throw_type(format!("{} is not a function", name).as_str());
            return false;
        }
        true
    }

    /// The spec's copy of index `src` to `dst`: Get and Set when `src` is
    /// there, otherwise DeletePropertyOrThrow of `dst`.
    fn move_index(&mut self, o: int, src: int, dst: int) {
        if self.has_index(o, src) {
            let v = self.get_index(&Val::Obj(o), src);
            if self.throwing {
                return;
            }
            self.set_index(o, dst, v);
        } else {
            self.delete(o, &Val::Num(dst as double));
        }
    }

    /// Array.prototype.splice by the spec's steps, for an array whose
    /// length changed while the arguments were converted: `len`, `s` and
    /// `del` are the values taken before.
    fn splice_generic(&mut self, o: int, len: int, s: int, del: int, args: &Vec<Val>) -> Val {
        let mut removed: Vec<Val> = Vec::new();
        let mut holes: Vec<int> = Vec::new();
        let mut k: int = 0;
        while k < del {
            if self.has_index(o, s + k) {
                let v = self.get_index(&Val::Obj(o), s + k);
                if self.throwing {
                    return Val::Undef;
                }
                removed.push(v);
            } else {
                removed.push(Val::Undef);
                holes.push(k);
            }
            k += 1;
        }
        let a = self.new_array(removed);
        for h in holes.iter() {
            self.delete(a, &Val::Num(*h as double));
        }
        let ic: int = if args.len() > 2 { (args.len() as int) - 2 } else { 0 };
        if ic < del {
            let mut k = s;
            while k < len - del {
                self.move_index(o, k + del, k + ic);
                if self.throwing {
                    return Val::Undef;
                }
                k += 1;
            }
            let mut k = len;
            while k > len - del + ic {
                self.delete(o, &Val::Num((k - 1) as double));
                if self.throwing {
                    return Val::Undef;
                }
                k -= 1;
            }
        } else if ic > del {
            let mut k = len - del;
            while k > s {
                self.move_index(o, k + del - 1, k + ic - 1);
                if self.throwing {
                    return Val::Undef;
                }
                k -= 1;
            }
        }
        let mut i: int = 0;
        while i < ic {
            self.set_index(o, s + i, args[(i + 2) as usize].clone());
            if self.throwing {
                return Val::Undef;
            }
            i += 1;
        }
        self.set_length(o, (len - del + ic) as double);
        Val::Obj(a)
    }

    fn flatten_into(&mut self, out: &mut Vec<Val>, items: Vec<Val>, depth: double) {
        if self.stack_exhausted() {
            self.throw_range("Maximum call stack size exceeded");
            return;
        }
        for it in items {
            if depth >= 1.0 {
                if let Val::Obj(o) = &it {
                    if self.objs[*o as usize].class == C_ARRAY {
                        let inner = self.evecs[self.objs[*o as usize].elems as usize].v.clone();
                        self.flatten_into(out, inner, depth - 1.0);
                        if self.throwing {
                            return;
                        }
                        continue;
                    }
                }
            }
            out.push(it);
        }
    }

    fn array_join(&mut self, o: int, sep: &str, seen: &mut Vec<int>) -> String {
        if self.stack_exhausted() {
            self.throw_range("Maximum call stack size exceeded");
            return String::new();
        }
        for s in seen.iter() {
            if *s == o {
                return String::new();
            }
        }
        seen.push(o);
        let items = self.elems_of(o);
        let mut out = String::new();
        let mut i: usize = 0;
        for it in items {
            if i > 0 {
                jsstr::push_js(&mut out, sep);
            }
            if (i & 8191) == 8191 && self.poll(131072) {
                return String::new();
            }
            if (out.as_bytes().len() as int) > MAX_STR_BYTES {
                self.throw_range("Invalid string length");
                return String::new();
            }
            match &it {
                Val::Undef | Val::Null => {}
                Val::Obj(x) => {
                    if self.objs[*x as usize].class == C_ARRAY {
                        let inner = self.array_join(*x, ",", seen);
                        if self.throwing {
                            return String::new();
                        }
                        jsstr::push_js(&mut out, inner.as_str());
                    } else {
                        let s = self.to_str(&it);
                        if self.throwing {
                            return String::new();
                        }
                        jsstr::push_js(&mut out, s.as_str());
                    }
                }
                _ => {
                    let s = self.to_str(&it);
                    jsstr::push_js(&mut out, s.as_str());
                }
            }
            i += 1;
        }
        seen.pop();
        out
    }

    pub fn call_native2(&mut self, id: int, fobj: int, this: Val, args: &Vec<Val>, construct: bool, new_target: Val) -> Val {
        let a0 = arg(&args, 0);
        let a1 = arg(&args, 1);
        if id >= NF_RP_FLAG0 && id < NF_RP_FLAG0 + 8 {
            let ro = obj_of(&this);
            if ro >= 0 && self.objs[ro as usize].class == C_REGEXP {
                let re = &self.regexes[self.objs[ro as usize].func as usize];
                let k = id - NF_RP_FLAG0;
                let b = if k == 0 { re.has_indices } else if k == 1 { re.global } else if k == 2 { re.ignore_case } else if k == 3 { re.multiline } else if k == 4 { re.dot_all } else if k == 5 { re.unicode && !re.unicode_sets } else if k == 6 { re.unicode_sets } else { re.sticky };
                return Val::Bool(b);
            }
            if ro >= 0 && ro == self.regexp_proto {
                return Val::Undef;
            }
            self.throw_type("RegExp.prototype flag getter called on incompatible receiver");
            return Val::Undef;
        }
        match id {
            // ---- Array
            NF_A_ISARRAY => Val::Bool(self.class_of(&a0) == C_ARRAY),
            NF_A_OF => {
                let a = self.new_array(args.clone());
                Val::Obj(a)
            }
            NF_A_FROM => {
                if matches!(a0, Val::Undef) || matches!(a0, Val::Null) {
                    self.throw_type("Array.from requires an array-like object");
                    return Val::Undef;
                }
                let has_iter = {
                    let f = self.get(&a0, A_ITERATOR);
                    self.is_callable(&f) || matches!(a0, Val::Str(_))
                };
                let items = if has_iter { self.iterable_to_vec(&a0) } else { self.array_like_to_vec(&a0) };
                if self.throwing {
                    return Val::Undef;
                }
                if self.is_callable(&a1) {
                    let mut out: Vec<Val> = Vec::new();
                    let mut i: int = 0;
                    for it in items {
                        let r = self.call_fixed(a1.clone(), arg(&args, 2), 2, it, Val::Num(i as double), Val::Undef);
                        if self.throwing {
                            return Val::Undef;
                        }
                        out.push(r);
                        i += 1;
                    }
                    let a = self.new_array(out);
                    return Val::Obj(a);
                }
                let a = self.new_array(items);
                Val::Obj(a)
            }
            NF_AP_PUSH => {
                let o = self.this_array(&this);
                if self.is_array_obj(o) {
                    if !self.objs[o as usize].extensible {
                        let n = self.evecs[self.objs[o as usize].elems as usize].v.len();
                        self.throw_type(format!("Cannot add property {}, object is not extensible", n).as_str());
                        return Val::Undef;
                    }
                    for a in args.iter() {
                        self.push_elem(o, *a);
                    }
                    return Val::Num(self.evecs[self.objs[o as usize].elems as usize].v.len() as double);
                }
                let mut n = self.len_of(&Val::Obj(o));
                for a in args.iter() {
                    self.set_index(o, n, *a);
                    n += 1;
                }
                self.set_obj(o, A_LENGTH, Val::Num(n as double));
                Val::Num(n as double)
            }
            NF_AP_POP => {
                let o = self.this_array(&this);
                if self.is_array_obj(o) {
                    let v = self.pop_elem_val(o);
                    self.dec(&v);
                    return v;
                }
                let n = self.len_of(&Val::Obj(o));
                if n == 0 {
                    self.set_obj(o, A_LENGTH, Val::Num(0.0));
                    return Val::Undef;
                }
                let v = self.get_index(&Val::Obj(o), n - 1);
                let k = Val::Num((n - 1) as double);
                self.delete(o, &k);
                self.set_obj(o, A_LENGTH, Val::Num((n - 1) as double));
                v
            }
            NF_AP_SHIFT => {
                let o = self.this_array(&this);
                if self.evecs[self.objs[o as usize].elems as usize].v.is_empty() {
                    return Val::Undef;
                }
                let v = self.evecs[self.objs[o as usize].elems as usize].v.remove(0);
                self.dec(&v);
                v
            }
            NF_AP_UNSHIFT => {
                let o = self.this_array(&this);
                // the last first, each at 0 (the vector's front room)
                let mut i = args.len();
                while i > 0 {
                    i -= 1;
                    self.insert_elem(o, 0, args[i]);
                }
                Val::Num(self.evecs[self.objs[o as usize].elems as usize].v.len() as double)
            }
            NF_AP_SLICE => {
                let o = self.this_array(&this);
                let items = self.elems_of(o);
                let len = items.len() as int;
                let s = self.rel_index(&a0, len, 0);
                let e = self.rel_index(&a1, len, len);
                let mut out: Vec<Val> = Vec::new();
                let mut i = s;
                while i < e {
                    out.push(items[i as usize].clone());
                    i += 1;
                }
                let a = self.new_array(out);
                Val::Obj(a)
            }
            NF_AP_SPLICE => {
                let o = self.this_array(&this);
                let len = self.evecs[self.objs[o as usize].elems as usize].v.len() as int;
                let s = self.rel_index(&a0, len, 0);
                let del = if args.is_empty() {
                    0
                } else if args.len() == 1 {
                    len - s
                } else {
                    let d = to_integer(self.to_number(&a1)) as int;
                    if d < 0 {
                        0
                    } else if d > len - s {
                        len - s
                    } else {
                        d
                    }
                };
                if self.throwing {
                    return Val::Undef;
                }
                // a valueOf above changed the length: the spec's steps on
                // the array as it is now (holes, growth and all)
                let cur = self.evecs[self.objs[o as usize].elems as usize].v.len() as int;
                if cur != len {
                    return self.splice_generic(o, len, s, del, args);
                }
                let mut removed: Vec<Val> = Vec::new();
                let mut k = 0;
                while k < del {
                    let v = self.evecs[self.objs[o as usize].elems as usize].v.remove(s as usize);
                    self.dec(&v);
                    removed.push(v);
                    k += 1;
                }
                let mut i: usize = 2;
                let mut at = s as usize;
                while i < args.len() {
                    self.insert_elem(o, (at) as int, args[i].clone());
                    at += 1;
                    i += 1;
                }
                let a = self.new_array(removed);
                Val::Obj(a)
            }
            NF_AP_CONCAT => {
                let o = self.this_array(&this);
                // one buffer of the final size, filled once (not a clone
                // grown by pushes): a = a.concat(x) in a loop copies a once
                let mut total: usize = args.len();
                let plain = o >= 0 && (self.objs[o as usize].class == C_ARRAY || self.objs[o as usize].class == C_ARGUMENTS) && self.colds[self.objs[o as usize].cold as usize].elems2.is_empty();
                if plain {
                    total += self.evecs[self.objs[o as usize].elems as usize].v.len();
                }
                for a in args.iter() {
                    if let Val::Obj(x) = &a {
                        if self.objs[*x as usize].class == C_ARRAY {
                            total += self.evecs[self.objs[*x as usize].elems as usize].v.len();
                        }
                    }
                }
                if self.elems_too_many(total as double) {
                    return Val::Undef;
                }
                let mut out: Vec<Val> = Vec::with_capacity(total);
                if plain {
                    let e = self.objs[o as usize].elems as usize;
                    elems_append(&mut out, &self.evecs[e]);
                } else {
                    out = self.elems_of(o);
                }
                for a in args.iter() {
                    if let Val::Obj(x) = &a {
                        if self.objs[*x as usize].class == C_ARRAY {
                            let e = self.objs[*x as usize].elems as usize;
                            elems_append(&mut out, &self.evecs[e]);
                            continue;
                        }
                    }
                    out.push(*a);
                }
                let r = self.new_array(out);
                Val::Obj(r)
            }
            NF_AP_JOIN => {
                let o = self.this_array(&this);
                let sep = if matches!(a0, Val::Undef) { String::from(",") } else { self.to_string(&a0) };
                let mut seen: Vec<int> = Vec::new();
                let s = self.array_join(o, sep.as_str(), &mut seen);
                self.string_val(s)
            }
            NF_AP_TOSTRING => {
                let o = self.this_array(&this);
                if self.objs[o as usize].class != C_ARRAY {
                    let jf = self.get(&this, self.atom_lookup("join"));
                    if self.is_callable(&jf) {
                        return self.call_fixed(jf, this.clone(), 0, Val::Undef, Val::Undef, Val::Undef);
                    }
                    return self.string_val(String::from("[object Object]"));
                }
                let mut seen: Vec<int> = Vec::new();
                let s = self.array_join(o, ",", &mut seen);
                self.string_val(s)
            }
            NF_AP_REVERSE => {
                let o = self.this_array(&this);
                self.evecs[self.objs[o as usize].elems as usize].v.reverse();
                Val::Obj(o)
            }
            NF_AP_TOREVERSED => {
                let o = self.this_array(&this);
                let mut items = self.elems_of(o);
                items.reverse();
                let a = self.new_array(items);
                Val::Obj(a)
            }
            NF_AP_INDEXOF | NF_AP_LASTINDEXOF | NF_AP_INCLUDES => {
                let o = self.this_array(&this);
                if self.throwing {
                    return Val::Undef;
                }
                let ov = Val::Obj(o);
                let len = self.len_of(&ov);
                if self.throwing {
                    return Val::Undef;
                }
                let none = if id == NF_AP_INCLUDES { Val::Bool(false) } else { Val::Num(-1.0) };
                if len == 0 {
                    return none;
                }
                // a plain array: its elements read in place
                let plain = self.objs[o as usize].class == C_ARRAY && self.colds[self.objs[o as usize].cold as usize].elems2.is_empty() && self.objs[o as usize].pos != 3;
                if id == NF_AP_LASTINDEXOF {
                    let mut i = if args.len() > 1 {
                        let n = to_integer(self.to_number(&a1));
                        if self.throwing {
                            return Val::Undef;
                        }
                        if n < 0.0 {
                            if len as double + n < 0.0 { -1 } else { len + (n as int) }
                        } else if n >= len as double {
                            len - 1
                        } else {
                            n as int
                        }
                    } else {
                        len - 1
                    };
                    while i >= 0 {
                        if (i & 0xffff) == 0 && self.poll(0x10000) {
                            return Val::Undef;
                        }
                        let v = if plain && i < (self.evecs[self.objs[o as usize].elems as usize].v.len() as int) {
                            self.evecs[self.objs[o as usize].elems as usize].v[i as usize].clone()
                        } else {
                            if !self.has_elem_or_prop(o, i) {
                                i -= 1;
                                continue;
                            }
                            self.get_index(&ov, i)
                        };
                        if self.throwing {
                            return Val::Undef;
                        }
                        if self.strict_equals(&v, &a0) {
                            return Val::Num(i as double);
                        }
                        i -= 1;
                    }
                    return none;
                }
                let mut i = self.rel_index(&a1, len, 0);
                if self.throwing {
                    return Val::Undef;
                }
                // a number in a plain array: the elements compared in place
                if plain && i >= 0 {
                    if let Val::Num(x) = a0 {
                        if x == x {
                            let e = self.objs[o as usize].elems as usize;
                            let n = self.evecs[e].v.len() as int;
                            let end = if n < len { n } else { len };
                            let mut k = i;
                            while k < end {
                                if (k & 0xffff) == 0 && self.poll(0x10000) {
                                    return Val::Undef;
                                }
                                if let Val::Num(y) = self.evecs[e].v[k as usize] {
                                    if y == x {
                                        return if id == NF_AP_INCLUDES { Val::Bool(true) } else { Val::Num(k as double) };
                                    }
                                }
                                k += 1;
                            }
                            i = if end > i { end } else { i };
                        }
                    }
                    // a string: handles, then lengths, then text
                    if let Val::Str(h) = a0 {
                        let e = self.objs[o as usize].elems as usize;
                        let n = self.evecs[e].v.len() as int;
                        let end = if n < len { n } else { len };
                        let needle = self.sref(h);
                        let nb = needle.as_bytes().len();
                        let mut k = i;
                        while k < end {
                            if (k & 0xffff) == 0 && crate::builtins::time_up(0x10000) {
                                break;
                            }
                            if let Val::Str(h2) = self.evecs[e].v[k as usize] {
                                let st = &self.strs[h2 as usize];
                                let l2 = if st.up < 0 { st.s.as_bytes().len() } else { st.len as usize };
                                if h2 == h || (l2 == nb && self.sref(h2) == needle) {
                                    return if id == NF_AP_INCLUDES { Val::Bool(true) } else { Val::Num(k as double) };
                                }
                            }
                            k += 1;
                        }
                        if self.check_expired() {
                            return Val::Undef;
                        }
                        i = if end > i { end } else { i };
                    }
                }
                while i < len {
                    if (i & 0xffff) == 0 && self.poll(0x10000) {
                        return Val::Undef;
                    }
                    let v = if plain && i < (self.evecs[self.objs[o as usize].elems as usize].v.len() as int) {
                        self.evecs[self.objs[o as usize].elems as usize].v[i as usize].clone()
                    } else {
                        if id != NF_AP_INCLUDES && !self.has_elem_or_prop(o, i) {
                            i += 1;
                            continue;
                        }
                        self.get_index(&ov, i)
                    };
                    if self.throwing {
                        return Val::Undef;
                    }
                    let hit = if id == NF_AP_INCLUDES { self.same_value_zero(&v, &a0) } else { self.strict_equals(&v, &a0) };
                    if hit {
                        return if id == NF_AP_INCLUDES { Val::Bool(true) } else { Val::Num(i as double) };
                    }
                    i += 1;
                }
                none
            }
            NF_AP_FOREACH | NF_AP_MAP | NF_AP_FILTER | NF_AP_SOME | NF_AP_EVERY | NF_AP_FIND | NF_AP_FINDINDEX | NF_AP_FINDLAST | NF_AP_FINDLASTINDEX | NF_AP_FLATMAP => {
                let o = self.this_array(&this);
                if self.throwing {
                    return Val::Undef;
                }
                if !self.callback(&a0, "callback") {
                    return Val::Undef;
                }
                let len = self.len_of(&Val::Obj(o));
                let back = id == NF_AP_FINDLAST || id == NF_AP_FINDLASTINDEX;
                let mut out: Vec<Val> = Vec::new();
                let mut k: int = 0;
                while k < len {
                    let i = if back { len - 1 - k } else { k };
                    let ob = Val::Obj(o);
                    // holes are skipped by forEach / map / filter / some / every
                    let present = if id == NF_AP_FIND || id == NF_AP_FINDINDEX || back {
                        true
                    } else if self.objs[o as usize].class == C_ARRAY && i < (self.evecs[self.objs[o as usize].elems as usize].v.len() as int) && !self.is_hole(o, i) {
                        true
                    } else {
                        let kv = Val::Num(i as double);
                        self.has_property(o, &kv)
                    };
                    if !present {
                        k += 1;
                        continue;
                    }
                    let v = self.get_index(&ob, i);
                    let r = self.call_fixed(a0.clone(), a1.clone(), 3, v.clone(), Val::Num(i as double), ob.clone());
                    if self.throwing {
                        return Val::Undef;
                    }
                    match id {
                        NF_AP_MAP => out.push(r),
                        NF_AP_FLATMAP => {
                            if let Val::Obj(x) = &r {
                                if self.objs[*x as usize].class == C_ARRAY {
                                    let items = self.evecs[self.objs[*x as usize].elems as usize].v.clone();
                                    for it in items {
                                        out.push(it);
                                    }
                                    k += 1;
                                    continue;
                                }
                            }
                            out.push(r);
                        }
                        NF_AP_FILTER => {
                            if self.truthy_v(&r) {
                                out.push(v);
                            }
                        }
                        NF_AP_SOME => {
                            if self.truthy_v(&r) {
                                return Val::Bool(true);
                            }
                        }
                        NF_AP_EVERY => {
                            if !self.truthy_v(&r) {
                                return Val::Bool(false);
                            }
                        }
                        NF_AP_FIND | NF_AP_FINDLAST => {
                            if self.truthy_v(&r) {
                                return v;
                            }
                        }
                        NF_AP_FINDINDEX | NF_AP_FINDLASTINDEX => {
                            if self.truthy_v(&r) {
                                return Val::Num(i as double);
                            }
                        }
                        _ => {}
                    }
                    k += 1;
                }
                match id {
                    NF_AP_MAP | NF_AP_FILTER | NF_AP_FLATMAP => {
                        let a = self.new_array(out);
                        Val::Obj(a)
                    }
                    NF_AP_SOME => Val::Bool(false),
                    NF_AP_EVERY => Val::Bool(true),
                    NF_AP_FINDINDEX | NF_AP_FINDLASTINDEX => Val::Num(-1.0),
                    _ => Val::Undef,
                }
            }
            NF_AP_REDUCE | NF_AP_REDUCERIGHT => {
                let o = self.this_array(&this);
                if self.throwing {
                    return Val::Undef;
                }
                let ov = Val::Obj(o);
                let len = self.len_of(&ov);
                if self.throwing {
                    return Val::Undef;
                }
                if !self.callback(&a0, "reducer") {
                    return Val::Undef;
                }
                let right = id == NF_AP_REDUCERIGHT;
                let mut k: int = if right { len - 1 } else { 0 };
                let mut acc: Val = Val::Undef;
                if args.len() >= 2 {
                    acc = a1.clone();
                } else {
                    let mut found = false;
                    while (right && k >= 0) || (!right && k < len) {
                        let here = self.has_elem_or_prop(o, k);
                        if self.throwing {
                            return Val::Undef;
                        }
                        let i = k;
                        k = if right { k - 1 } else { k + 1 };
                        if here {
                            acc = self.get_index(&ov, i);
                            if self.throwing {
                                return Val::Undef;
                            }
                            found = true;
                            break;
                        }
                    }
                    if !found {
                        self.throw_type("Reduce of empty array with no initial value");
                        return Val::Undef;
                    }
                }
                while (right && k >= 0) || (!right && k < len) {
                    let here = self.has_elem_or_prop(o, k);
                    if self.throwing {
                        return Val::Undef;
                    }
                    if here {
                        let v = self.get_index(&ov, k);
                        if self.throwing {
                            return Val::Undef;
                        }
                        acc = self.call_value(a0.clone(), Val::Undef, vec![acc, v, Val::Num(k as double), ov.clone()]);
                        if self.throwing {
                            return Val::Undef;
                        }
                    }
                    k = if right { k - 1 } else { k + 1 };
                }
                acc
            }
            NF_AP_SORT | NF_AP_TOSORTED => {
                // called from a frame: with the array, the comparator and
                // the items rooted, the comparator's garbage (closures
                // and their scopes) is collected while the sort runs
                let top = self.nat_top;
                let o = self.this_array(&this);
                if !matches!(a0, Val::Undef) && !self.is_callable(&a0) {
                    self.throw_type("The comparison function must be either a function or undefined");
                    return Val::Undef;
                }
                let mut items = self.elems_of(o);
                let nr = self.temp_roots.len();
                self.temp_roots.push(Val::Obj(o));
                let open = top && obj_of(&a0) >= 0 && items.len() > 1;
                if open {
                    self.temp_roots.push(a0.clone());
                    for it in items.iter() {
                        self.temp_roots.push(*it);
                    }
                }
                let g = self.gc_open(open);
                self.merge_sort(&mut items, &a0);
                self.gc_close(g);
                if self.throwing {
                    self.temp_roots.truncate(nr);
                    return Val::Undef;
                }
                let r = if id == NF_AP_TOSORTED {
                    Val::Obj(self.new_array(items))
                } else {
                    if self.objs[o as usize].class == C_ARRAY {
                        self.put_elems(o, items);
                    } else {
                        let mut i: int = 0;
                        for it in items {
                            self.set_index(o, i, it);
                            i += 1;
                        }
                    }
                    Val::Obj(o)
                };
                self.temp_roots.truncate(nr);
                r
            }
            NF_AP_FILL => {
                let o = self.this_array(&this);
                let generic = !self.is_array_obj(o);
                let len = if generic { self.len_of(&Val::Obj(o)) } else { self.evecs[self.objs[o as usize].elems as usize].v.len() as int };
                let s = self.rel_index(&a1, len, 0);
                let e = self.rel_index(&arg(&args, 2), len, len);
                if self.throwing {
                    return Val::Undef;
                }
                // a valueOf above may have shrunk the array: Set grows it back
                let generic = generic || (self.evecs[self.objs[o as usize].elems as usize].v.len() as int) < e;
                let mut i = s;
                while i < e {
                    if generic {
                        self.set_index(o, i, a0.clone());
                        if self.throwing {
                            return Val::Undef;
                        }
                    } else {
                        self.put_elem(o, i, a0.clone());
                        if i < (self.colds[self.objs[o as usize].cold as usize].elems2.len() as int) {
                            self.put_elem2(o, i, Val::Bool(false));
                        }
                    }
                    if (i & 8191) == 8191 && self.poll(131072) {
                        return Val::Undef;
                    }
                    i += 1;
                }
                Val::Obj(o)
            }
            NF_AP_COPYWITHIN => {
                let o = self.this_array(&this);
                let items = self.elems_of(o);
                let len = items.len() as int;
                let t = self.rel_index(&a0, len, 0);
                let s = self.rel_index(&a1, len, 0);
                let e = self.rel_index(&arg(&args, 2), len, len);
                if self.throwing {
                    return Val::Undef;
                }
                let generic = !self.is_array_obj(o);
                if !generic && (self.evecs[self.objs[o as usize].elems as usize].v.len() as int) < len {
                    // a valueOf above shrank the array: read it as it is now,
                    // as the spec's HasProperty / Get / Set / Delete do
                    // the spec's count, and backwards when the ranges
                    // overlap with the target after the source
                    let count = if e - s < len - t { e - s } else { len - t };
                    let back = s < t && t < s + count;
                    let mut j: int = 0;
                    while j < count {
                        let k = if back { count - 1 - j } else { j };
                        self.move_index(o, s + k, t + k);
                        if self.throwing {
                            return Val::Undef;
                        }
                        j += 1;
                    }
                    return Val::Obj(o);
                }
                let mut i: int = 0;
                while s + i < e && t + i < len {
                    if generic {
                        self.set_index(o, t + i, items[(s + i) as usize].clone());
                        if self.throwing {
                            return Val::Undef;
                        }
                    } else {
                        self.put_elem(o, t + i, items[(s + i) as usize].clone());
                    }
                    i += 1;
                }
                Val::Obj(o)
            }
            NF_AP_WITH => {
                let o = self.this_array(&this);
                let mut items = self.elems_of(o);
                let len = items.len() as int;
                let n = to_integer(self.to_number(&a0));
                let i = if n < 0.0 { len + (n as int) } else { n as int };
                if i < 0 || i >= len {
                    self.throw_range("Invalid index");
                    return Val::Undef;
                }
                items[i as usize] = a1;
                let a = self.new_array(items);
                Val::Obj(a)
            }
            NF_AP_AT => {
                let o = self.this_array(&this);
                let len = self.len_of(&Val::Obj(o));
                let n = to_integer(self.to_number(&a0));
                let i = if n < 0.0 { len + (n as int) } else { n as int };
                if i < 0 || i >= len {
                    return Val::Undef;
                }
                self.get_index(&Val::Obj(o), i)
            }
            NF_AP_FLAT => {
                let o = self.this_array(&this);
                let depth = if matches!(a0, Val::Undef) { 1.0 } else { to_integer(self.to_number(&a0)) };
                let items = self.elems_of(o);
                let mut out: Vec<Val> = Vec::new();
                self.flatten_into(&mut out, items, depth);
                let a = self.new_array(out);
                Val::Obj(a)
            }
            NF_AP_KEYS | NF_AP_VALUES | NF_AP_ENTRIES => {
                let o = self.to_object(&this);
                if self.throwing {
                    return Val::Undef;
                }
                let ip = self.iter_proto;
                let it = self.alloc(C_ITER, ip);
                let class = self.objs[o as usize].class;
                self.put_env(it, o);
                self.objs[it as usize].func = 0;
                // another object than an array is read as the iterator
                // goes (its length and elements each step): + 8
                let generic = if class == C_ARRAY || class == C_ARGUMENTS { 0 } else { 8 };
                self.objs[it as usize].pos = generic + if id == NF_AP_KEYS {
                    1
                } else if id == NF_AP_ENTRIES {
                    2
                } else {
                    0
                };
                Val::Obj(it)
            }
            NF_ITER_NEXT => {
                let r = self.new_object();
                match self.iter_next(&this) {
                    Some(v) => {
                        self.add_own(r, A_VALUE, v, 0);
                        self.add_own(r, A_DONE, Val::Bool(false), 0);
                    }
                    None => {
                        self.add_own(r, A_VALUE, Val::Undef, 0);
                        self.add_own(r, A_DONE, Val::Bool(true), 0);
                    }
                }
                Val::Obj(r)
            }
            NF_ITER_SELF => this,
            // ---- String
            NF_S_FROMCHARCODE => {
                let mut u: Vec<int> = Vec::new();
                for a in args.iter() {
                    let n = self.to_number(a);
                    u.push((to_uint32(n) % 65536.0) as int);
                }
                let len = u.len() as int;
                self.string_val(jsstr::from_units(&u, 0, len))
            }
            NF_S_FROMCODEPOINT => {
                let mut s = String::new();
                for a in args.iter() {
                    let n = self.to_number(a);
                    if n != to_integer(n) || n < 0.0 || n > 1114111.0 {
                        self.throw_range(format!("Invalid code point {}", number_to_string(n)).as_str());
                        return Val::Undef;
                    }
                    let mut t = String::new();
                    jsstr::push_code(&mut t, n as int);
                    jsstr::push_js(&mut s, t.as_str());
                }
                self.string_val(s)
            }
            NF_S_RAW => {
                let raw = self.get(&a0, A_RAW);
                let parts = self.array_like_to_vec(&raw);
                let mut s = String::new();
                let mut i: usize = 0;
                while i < parts.len() {
                    let p = self.to_string(&parts[i]);
                    jsstr::push_js(&mut s, p.as_str());
                    if i + 1 < parts.len() && i + 1 < args.len() {
                        let v = self.to_string(&args[i + 1]);
                        jsstr::push_js(&mut s, v.as_str());
                    }
                    i += 1;
                }
                self.string_val(s)
            }
            NF_SP_TOSTRING => match &this {
                Val::Str(_) => this,
                Val::Obj(o) if self.objs[*o as usize].class == C_STRING => self.colds[self.objs[*o as usize].cold as usize].prim.clone(),
                _ => {
                    self.throw_type("String.prototype.toString requires that 'this' be a String");
                    Val::Undef
                }
            },
            NF_SP_CHARAT | NF_SP_CHARCODEAT | NF_SP_CODEPOINTAT | NF_SP_AT => {
                // an ASCII string value: the unit off its handle (no copy
                // of a prefix, no scan of a string that grew in place)
                if let (Val::Str(h), Val::Num(x)) = (this, a0) {
                    let mut i = to_integer(x);
                    if id == NF_SP_AT && i < 0.0 {
                        i += self.hlen(h) as double;
                    }
                    let c = if i < 0.0 || i > 4294967295.0 { -1 } else { self.hunit(h, i as int) };
                    if c != -2 {
                        return match id {
                            NF_SP_CHARAT => {
                                if c < 0 {
                                    self.str_val("")
                                } else {
                                    self.unit_val(c)
                                }
                            }
                            NF_SP_CHARCODEAT => Val::Num(if c < 0 { nan() } else { c as double }),
                            _ => {
                                if c < 0 {
                                    Val::Undef
                                } else if id == NF_SP_AT {
                                    self.unit_val(c)
                                } else {
                                    Val::Num(c as double)
                                }
                            }
                        };
                    }
                }
                let s = self.this_str(&this);
                if self.throwing {
                    return Val::Undef;
                }
                let mut i = to_integer(self.to_number(&a0));
                if id == NF_SP_AT && i < 0.0 {
                    i += self.str_len(&s) as double;
                }
                let c = if i < 0.0 || i > 4294967295.0 { -1 } else { self.str_at(&s, i as int) };
                match id {
                    NF_SP_CHARAT => {
                        if c < 0 {
                            self.str_val("")
                        } else {
                            self.unit_val(c)
                        }
                    }
                    NF_SP_AT => {
                        if c < 0 {
                            Val::Undef
                        } else {
                            self.unit_val(c)
                        }
                    }
                    NF_SP_CHARCODEAT => Val::Num(if c < 0 { nan() } else { c as double }),
                    _ => {
                        if c < 0 {
                            return Val::Undef;
                        }
                        if c >= 0xd800 && c <= 0xdbff {
                            let d = self.str_at(&s, (i as int) + 1);
                            if d >= 0xdc00 && d <= 0xdfff {
                                return Val::Num((0x10000 + ((c - 0xd800) << 10) + (d - 0xdc00)) as double);
                            }
                        }
                        Val::Num(c as double)
                    }
                }
            }
            NF_SP_INDEXOF | NF_SP_LASTINDEXOF | NF_SP_INCLUDES | NF_SP_STARTSWITH | NF_SP_ENDSWITH => {
                let s = self.this_str(&this);
                if self.throwing {
                    return Val::Undef;
                }
                if (id == NF_SP_INCLUDES || id == NF_SP_STARTSWITH || id == NF_SP_ENDSWITH) && is_obj(&a0) {
                    // IsRegExp: Symbol.match when set, else the internal slot
                    let am = self.intern("@@match");
                    let m = self.get(&a0, am);
                    if self.throwing {
                        return Val::Undef;
                    }
                    let is_re = if matches!(m, Val::Undef) { self.class_of(&a0) == C_REGEXP } else { self.truthy_v(&m) };
                    if is_re {
                        let what = if id == NF_SP_INCLUDES { "includes" } else if id == NF_SP_STARTSWITH { "startsWith" } else { "endsWith" };
                        self.throw_type(format!("First argument to String.prototype.{} must not be a regular expression", what).as_str());
                        return Val::Undef;
                    }
                }
                let p = self.to_str(&a0);
                if (id == NF_SP_STARTSWITH || id == NF_SP_ENDSWITH) && matches!(a1, Val::Undef) {
                    // no position: an ASCII text compared as bytes
                    let q = ascii_affix(s.as_str(), p.as_str(), id == NF_SP_ENDSWITH);
                    if q >= 0 {
                        return Val::Bool(q == 1);
                    }
                }
                let len = self.str_len(&s);
                match id {
                    NF_SP_INDEXOF => {
                        let from = self.rel_index_pos(&a1, len, 0);
                        Val::Num(self.str_index_of(&s, p.as_str(), from) as double)
                    }
                    NF_SP_LASTINDEXOF => {
                        let n = self.to_number(&a1);
                        let from = if is_nan(n) { len } else { to_integer(n) as int };
                        // the spec clamps the start to 0..len
                        let from = if from < 0 { 0 } else if from > len { len } else { from };
                        let r = jsstr::last_index_of(s.as_str(), p.as_str(), from);
                        if self.check_expired() {
                            return Val::Undef;
                        }
                        Val::Num(r as double)
                    }
                    NF_SP_INCLUDES => {
                        let from = self.rel_index_pos(&a1, len, 0);
                        Val::Bool(self.str_index_of(&s, p.as_str(), from) >= 0)
                    }
                    NF_SP_STARTSWITH => {
                        let from = self.rel_index_pos(&a1, len, 0);
                        let pl = jsstr::len(p.as_str());
                        Val::Bool(self.str_slice(&s, from, from + pl).as_str() == p.as_str())
                    }
                    _ => {
                        let end = self.rel_index_pos(&a1, len, len);
                        let pl = jsstr::len(p.as_str());
                        if end - pl < 0 {
                            return Val::Bool(false);
                        }
                        Val::Bool(self.str_slice(&s, end - pl, end).as_str() == p.as_str())
                    }
                }
            }
            NF_SP_SLICE => {
                let s = self.this_str(&this);
                if self.throwing {
                    return Val::Undef;
                }
                let len = self.str_len(&s);
                let a = self.rel_index(&a0, len, 0);
                let b = self.rel_index(&a1, len, len);
                if a <= 0 && b >= len {
                    return self.same_str_val(&this, s);
                }
                { let tw2 = self.str_slice(&s, a, b); self.string_val(tw2) }
            }
            NF_SP_SUBSTRING => {
                let s = self.this_str(&this);
                if self.throwing {
                    return Val::Undef;
                }
                let len = self.str_len(&s);
                let a = self.rel_index_pos(&a0, len, 0);
                let b = self.rel_index_pos(&a1, len, len);
                if (a <= 0 && b >= len) || (b <= 0 && a >= len) {
                    return self.same_str_val(&this, s);
                }
                if a < b {
                    { let tw3 = self.str_slice(&s, a, b); self.string_val(tw3) }
                } else {
                    { let tw4 = self.str_slice(&s, b, a); self.string_val(tw4) }
                }
            }
            NF_SP_SUBSTR => {
                let s = self.this_str(&this);
                if self.throwing {
                    return Val::Undef;
                }
                let len = self.str_len(&s);
                let a = self.rel_index(&a0, len, 0);
                let n = if matches!(a1, Val::Undef) { len - a } else { to_integer(self.to_number(&a1)) as int };
                if n <= 0 {
                    return self.str_val("");
                }
                { let tw0 = self.str_slice(&s, a, a + n); self.string_val(tw0) }
            }
            NF_SP_UPPER => {
                let s = self.this_str(&this);
                self.string_val(jsstr::to_upper(s.as_str()))
            }
            NF_SP_LOWER => {
                let s = self.this_str(&this);
                if let Val::Str(_) = this {
                    // already lower case: the same string, nothing made
                    if jsstr::ascii_lower(s.as_str()) {
                        return this;
                    }
                }
                self.string_val(jsstr::to_lower(s.as_str()))
            }
            NF_SP_TRIM => {
                let s = self.this_str(&this);
                if !trim_needed(s.as_str(), true, true) {
                    return self.same_str_val(&this, s);
                }
                self.string_val(js_trim(s.as_str()))
            }
            NF_SP_TRIMSTART => {
                let s = self.this_str(&this);
                if !trim_needed(s.as_str(), true, false) {
                    return self.same_str_val(&this, s);
                }
                self.string_val(js_trim_start(s.as_str()))
            }
            NF_SP_TRIMEND => {
                let s = self.this_str(&this);
                if !trim_needed(s.as_str(), false, true) {
                    return self.same_str_val(&this, s);
                }
                self.string_val(js_trim_end(s.as_str()))
            }
            NF_SP_ISWELLFORMED | NF_SP_TOWELLFORMED => {
                let s = self.this_str(&this);
                if self.throwing {
                    return Val::Undef;
                }
                let ok = jsstr::well_formed(s.as_str());
                if id == NF_SP_ISWELLFORMED {
                    return Val::Bool(ok);
                }
                if ok {
                    return self.same_str_val(&this, s);
                }
                self.string_val(jsstr::to_well_formed(s.as_str(), false))
            }
            NF_SP_NORMALIZE => {
                let s = self.this_str(&this);
                if self.throwing {
                    return Val::Undef;
                }
                let form = if matches!(a0, Val::Undef) { String::from("NFC") } else { self.to_str(&a0).as_str().to_string() };
                if self.throwing {
                    return Val::Undef;
                }
                if form != "NFC" && form != "NFD" && form != "NFKC" && form != "NFKD" {
                    self.throw_range("The normalization form should be one of NFC, NFD, NFKC, NFKD.");
                    return Val::Undef;
                }
                let compose = form == "NFC" || form == "NFKC";
                { let tw1 = crate::normalize::normalize(&mut self.norm, s.as_str(), compose); self.string_val(tw1) }
            }
            NF_SP_SPLIT => {
                let s = self.this_str(&this);
                if self.throwing {
                    return Val::Undef;
                }
                self.string_split(&s, &a0, &a1)
            }
            NF_SP_REPLACE | NF_SP_REPLACEALL => {
                let s = self.this_str(&this);
                if self.throwing {
                    return Val::Undef;
                }
                self.string_replace(&s, &a0, &a1, id == NF_SP_REPLACEALL)
            }
            NF_SP_MATCH | NF_SP_MATCHALL | NF_SP_SEARCH => {
                let s = self.this_str(&this);
                if self.throwing {
                    return Val::Undef;
                }
                let re = if self.class_of(&a0) == C_REGEXP {
                    a0.clone()
                } else {
                    let flags = if id == NF_SP_MATCHALL { self.str_val("g") } else { Val::Undef };
                    let src = if matches!(a0, Val::Undef) {
                        Val::Undef
                    } else {
                        let t = self.to_str(&a0);
                        self.rc_val(t)
                    };
                    self.new_regexp(&src, &flags)
                };
                if self.throwing {
                    return Val::Undef;
                }
                let ro = obj_of(&re);
                let idx = self.objs[ro as usize].func as int;
                let global = self.regexes[idx as usize].global;
                if id == NF_SP_SEARCH {
                    let units = self.rx_take(&s);
                    let caps = self.regexes[idx as usize].exec(&units, 0);
                    self.rx_after();
                    self.rx_put(&s, units);
                    return Val::Num(if caps.is_empty() { -1.0 } else { caps[0] as double });
                }
                if id == NF_SP_MATCHALL {
                    if !global {
                        self.throw_type("String.prototype.matchAll called with a non-global RegExp argument");
                        return Val::Undef;
                    }
                    let mut out: Vec<Val> = Vec::new();
                    let sv2 = self.str_val("g");
                    let copy = self.new_regexp(&self.colds[self.objs[ro as usize].cold as usize].prim.clone(), &sv2);
                    let co = obj_of(&copy);
                    loop {
                        let m = self.regexp_exec(co, &s);
                        if matches!(m, Val::Null) {
                            break;
                        }
                        let m0 = self.get_index(&m, 0);
                        if let Val::Str(ms) = &m0 {
                            let ms = self.sget(ms.clone());
                            if ms.is_empty() {
                                let li = self.get_obj(co, A_LASTINDEX, &copy);
                                let n = self.to_number(&li);
                                let uni = self.regexes[idx as usize].unicode || self.regexes[idx as usize].unicode_sets;
                                let units = self.rx_take(&s);
                                let nx = crate::builtins::adv_index(&units, n as int, uni);
                                self.rx_put(&s, units);
                                self.set_obj(co, A_LASTINDEX, Val::Num(nx as double));
                            }
                        }
                        out.push(m);
                    }
                    let arr = self.new_array(out);
                    let ip = self.iter_proto;
                    let it = self.alloc(C_ITER, ip);
                    self.put_env(it, arr);
                    self.objs[it as usize].func = 0;
                    return Val::Obj(it);
                }
                if !global {
                    let inp = if let Val::Str(_) = this { this } else { Val::Undef };
                    return self.regexp_exec_in(ro, &s, inp, true);
                }
                let units = self.rx_take(&s);
                let mut out: Vec<Val> = Vec::new();
                let mut start: int = 0;
                while start <= units.len() as int {
                    let caps = self.regexes[idx as usize].exec(&units, start);
                    self.rx_after();
                    if caps.is_empty() {
                        break;
                    }
                    out.push(self.string_val(jsstr::from_units(&units, caps[0], caps[1])));
                    let uni = self.regexes[idx as usize].unicode || self.regexes[idx as usize].unicode_sets;
                    start = if caps[1] == caps[0] { crate::builtins::adv_index(&units, caps[1], uni) } else { caps[1] };
                }
                self.rx_put(&s, units);
                self.set_obj(ro, A_LASTINDEX, Val::Num(0.0));
                if out.is_empty() {
                    return Val::Null;
                }
                let a = self.new_array(out);
                Val::Obj(a)
            }
            NF_SP_REPEAT => {
                let s = self.this_str(&this);
                let n = to_integer(self.to_number(&a0));
                if n < 0.0 || !is_finite(n) {
                    self.throw_range("Invalid count value");
                    return Val::Undef;
                }
                if (s.as_bytes().len() as double) * n > 536870888.0 {
                    self.throw_range("Invalid string length");
                    return Val::Undef;
                }
                // a string that ends with a lone high surrogate and starts with
                // a lone low one: each copy joins the next
                if jsstr::joins(s.as_str(), s.as_str()) {
                    let mut out = String::new();
                    let mut k: int = 0;
                    while k < (n as int) {
                        jsstr::push_js(&mut out, s.as_str());
                        k += 1;
                    }
                    return self.string_val(out);
                }
                self.string_val(s.repeat(n as usize))
            }
            NF_SP_PADSTART | NF_SP_PADEND => {
                let s = self.this_str(&this);
                let target_d = to_integer(self.to_number(&a0));
                let fill = if matches!(a1, Val::Undef) { String::from(" ") } else { self.to_string(&a1) };
                let len = jsstr::len(s.as_str());
                if target_d > (len as double) && !fill.is_empty() && self.str_too_long((s.as_bytes().len() as double) + target_d - (len as double)) {
                    return Val::Undef;
                }
                let target = target_d as int;
                if target <= len || fill.is_empty() {
                    return self.same_str_val(&this, s);
                }
                let need = target - len;
                if jsstr::is_ascii(fill.as_str()) {
                    // an ASCII fill: whole copies of it and a head, no units
                    let fl = fill.as_bytes().len() as int;
                    let mut out = String::new();
                    if id != NF_SP_PADSTART {
                        out.push_str(s.as_str());
                    }
                    let mut k = need / fl;
                    while k > 0 {
                        out.push_str(fill.as_str());
                        k -= 1;
                    }
                    if need % fl > 0 {
                        out.push_str(jsstr::slice(fill.as_str(), 0, need % fl).as_str());
                    }
                    if id == NF_SP_PADSTART {
                        out.push_str(s.as_str());
                    }
                    return self.string_val(out);
                }
                let fu = jsstr::units(fill.as_str());
                let mut pad: Vec<int> = Vec::new();
                let mut i: int = 0;
                while i < need {
                    pad.push(fu[(i as usize) % fu.len()]);
                    i += 1;
                }
                let ps = jsstr::from_units(&pad, 0, need);
                // the pad and the string may join a lone high surrogate to a lone low one
                let mut out = String::new();
                if id == NF_SP_PADSTART {
                    out.push_str(ps.as_str());
                    jsstr::push_js(&mut out, s.as_str());
                } else {
                    out.push_str(s.as_str());
                    jsstr::push_js(&mut out, ps.as_str());
                }
                self.string_val(out)
            }
            NF_SP_CONCAT => {
                let s = self.this_str(&this);
                if args.is_empty() {
                    return self.same_str_val(&this, s);
                }
                let mut out = s.as_ref().clone();
                for a in args.iter() {
                    let x = self.to_str(a);
                    jsstr::push_js(&mut out, x.as_str());
                }
                self.string_val(out)
            }
            NF_SP_LOCALECOMPARE => {
                let s = self.this_str(&this);
                let t = self.to_str(&a0);
                Val::Num(jsstr::compare(s.as_str(), t.as_str()) as double)
            }
            NF_SP_ITERATOR => {
                let s = self.this_str(&this);
                let sv = self.rc_val(s);
                let ip = self.string_iter_proto;
                self.string_iter(sv, ip)
            }
            // ---- Number / Boolean
            NF_N_ISINTEGER | NF_N_ISSAFEINTEGER | NF_N_ISFINITE | NF_N_ISNAN => {
                let n = match a0 {
                    Val::Num(x) => x,
                    _ => return Val::Bool(false),
                };
                Val::Bool(match id {
                    NF_N_ISINTEGER => is_finite(n) && n == to_integer(n),
                    NF_N_ISSAFEINTEGER => is_finite(n) && n == to_integer(n) && n.abs() <= 9007199254740991.0,
                    NF_N_ISFINITE => is_finite(n),
                    _ => is_nan(n),
                })
            }
            NF_NP_VALUEOF => {
                let n = self.this_num(&this);
                Val::Num(n)
            }
            NF_NP_TOSTRING => {
                let n = self.this_num(&this);
                if self.throwing {
                    return Val::Undef;
                }
                let radix = if matches!(a0, Val::Undef) { 10 } else { to_integer(self.to_number(&a0)) as int };
                if radix < 2 || radix > 36 {
                    self.throw_range("toString() radix must be between 2 and 36");
                    return Val::Undef;
                }
                self.string_val(radix_string(n, radix))
            }
            NF_NP_TOFIXED => {
                let n = self.this_num(&this);
                let d = to_integer(self.to_number(&a0));
                if d < 0.0 || d > 100.0 {
                    self.throw_range("toFixed() digits argument must be between 0 and 100");
                    return Val::Undef;
                }
                self.string_val(to_fixed(n, d as int))
            }
            NF_NP_TOPRECISION => {
                let n = self.this_num(&this);
                if matches!(a0, Val::Undef) {
                    return self.string_val(number_to_string(n));
                }
                let p = to_integer(self.to_number(&a0)) as int;
                if !is_finite(n) {
                    return self.string_val(number_to_string(n));
                }
                if p < 1 || p > 100 {
                    self.throw_range("toPrecision() argument must be between 1 and 100");
                    return Val::Undef;
                }
                self.string_val(to_precision(n, p))
            }
            NF_NP_TOEXPONENTIAL => {
                let n = self.this_num(&this);
                let fd = if matches!(a0, Val::Undef) { -1.0 } else { to_integer(self.to_number(&a0)) };
                if self.throwing {
                    return Val::Undef;
                }
                if !is_finite(n) {
                    return self.string_val(number_to_string(n));
                }
                if !matches!(a0, Val::Undef) && (fd < 0.0 || fd > 100.0) {
                    self.throw_range("toExponential() argument must be between 0 and 100");
                    return Val::Undef;
                }
                let d = fd as int;
                self.string_val(to_exponential(n, d))
            }
            NF_BP_TOSTRING | NF_BP_VALUEOF => {
                let b = match &this {
                    Val::Bool(b) => *b,
                    Val::Obj(o) if self.objs[*o as usize].class == C_BOOLEAN => self.truthy_v(&self.colds[self.objs[*o as usize].cold as usize].prim.clone()),
                    _ => {
                        self.throw_type("Boolean.prototype method called on incompatible receiver");
                        return Val::Undef;
                    }
                };
                if id == NF_BP_VALUEOF {
                    Val::Bool(b)
                } else {
                    self.str_val(if b { "true" } else { "false" })
                }
            }
            // ---- JSON
            NF_JSON_STRINGIFY => {
                let mut allow: Vec<String> = Vec::new();
                if self.class_of(&a1) == C_ARRAY {
                    let items = self.evecs[self.objs[obj_of(&a1) as usize].elems as usize].v.clone();
                    for it in items {
                        let s = self.to_string(&it);
                        allow.push(s);
                    }
                }
                let sp = arg(&args, 2);
                let gap = match &sp {
                    Val::Num(n) => {
                        let k = if *n > 10.0 { 10 } else { *n as int };
                        " ".repeat(if k > 0 { k as usize } else { 0 })
                    }
                    Val::Str(s) => {
                        let s = self.sget(s.clone());
                        let t = s.as_ref().clone();
                        jsstr::slice(t.as_str(), 0, 10)
                    }
                    _ => String::new(),
                };
                let holder = self.new_object();
                self.add_own(holder, self.atom_lookup(""), a0.clone(), 0);
                let mut out = String::new();
                let mut stack: Vec<int> = Vec::new();
                let sv1 = self.str_val("");
                // plans are dropped only outside every stringify (a toJSON
                // may call it again while an object's plan is in use)
                if self.js_depth == 0 && self.js_slots.len() > 2048 {
                    self.js_plan.clear();
                    self.js_slots.clear();
                    self.js_atoms.clear();
                    self.js_kq.clear();
                }
                self.js_depth += 1;
                let ok = self.json_str(&Val::Obj(holder), &sv1, -1, a0, &a1, &allow, gap.as_str(), "", &mut stack, &mut out);
                self.js_depth -= 1;
                if self.throwing || !ok {
                    return Val::Undef;
                }
                self.string_val(out)
            }
            NF_JSON_PARSE => {
                let s = self.to_string(&a0);
                let mut v = Val::Undef;
                if !json_parse_fast(self, s.as_str(), &mut v) {
                    let cs = s.chars().collect::<Vec<char>>();
                    let mut pos: usize = 0;
                    v = self.json_parse_value(&cs, &mut pos);
                    if self.throwing {
                        return Val::Undef;
                    }
                    while pos < cs.len() && (cs[pos] == ' ' || cs[pos] == '\t' || cs[pos] == '\n' || cs[pos] == '\r') {
                        pos += 1;
                    }
                    if pos < cs.len() {
                        self.throw_syntax("Unexpected non-whitespace character after JSON");
                        return Val::Undef;
                    }
                }
                if self.throwing {
                    return Val::Undef;
                }
                if self.is_callable(&a1) {
                    let holder = self.new_object();
                    self.add_own(holder, self.atom_lookup(""), v, 0);
                    let sv0 = self.str_val("");
                    return self.json_revive(&Val::Obj(holder), &sv0, &a1);
                }
                v
            }
            // ---- RegExp
            NF_REGEXP => {
                // IsRegExp: Symbol.match when set, else the internal slot
                let mut is_re = false;
                if is_obj(&a0) {
                    let am = self.intern("@@match");
                    let m = self.get(&a0, am);
                    if self.throwing {
                        return Val::Undef;
                    }
                    is_re = if matches!(m, Val::Undef) { self.class_of(&a0) == C_REGEXP } else { self.truthy_v(&m) };
                }
                if !construct && is_re && matches!(a1, Val::Undef) {
                    let pc = self.get(&a0, A_CONSTRUCTOR);
                    if self.throwing {
                        return Val::Undef;
                    }
                    if let Val::Obj(c) = pc {
                        if c == fobj {
                            return a0;
                        }
                    }
                }
                let (src, fl) = if self.class_of(&a0) == C_REGEXP {
                    let src = self.colds[self.objs[obj_of(&a0) as usize].cold as usize].prim.clone();
                    let fl = if matches!(a1, Val::Undef) { self.string_val(self.regexp_flags(obj_of(&a0))) } else { a1 };
                    (src, fl)
                } else if is_re {
                    let a_source = self.atom_lookup("source");
                    let src = self.get(&a0, a_source);
                    if self.throwing {
                        return Val::Undef;
                    }
                    let fl = if matches!(a1, Val::Undef) {
                        let a_flags = self.atom_lookup("flags");
                        self.get(&a0, a_flags)
                    } else {
                        a1
                    };
                    (src, fl)
                } else {
                    (a0, a1)
                };
                if self.throwing {
                    return Val::Undef;
                }
                // the pattern's ToString before the flags'
                let src = if is_obj(&src) {
                    let t = self.to_str(&src);
                    if self.throwing {
                        return Val::Undef;
                    }
                    self.str_val(t.as_str())
                } else {
                    src
                };
                let r = self.new_regexp(&src, &fl);
                if construct && !self.throwing {
                    let rp = self.regexp_proto;
                    let proto = self.proto_from(&new_target, rp);
                    if proto != rp {
                        self.put_proto(obj_of(&r), proto);
                    }
                }
                r
            }
            NF_RP_EXEC | NF_RP_TEST => {
                let ro = obj_of(&this);
                if ro < 0 || self.objs[ro as usize].class != C_REGEXP {
                    self.throw_type("RegExp method called on incompatible receiver");
                    return Val::Undef;
                }
                let s = self.to_str(&a0);
                let inp = if let Val::Str(_) = a0 { a0 } else { Val::Undef };
                let m = self.regexp_exec_in(ro, &s, inp, id != NF_RP_TEST);
                if id == NF_RP_TEST {
                    return Val::Bool(!matches!(m, Val::Null));
                }
                m
            }
            NF_RP_TOSTRING => {
                let src = self.get(&this, self.atom_lookup("source"));
                let ss = self.to_string(&src);
                let fl = self.get(&this, self.atom_lookup("flags"));
                let fs = self.to_string(&fl);
                self.string_val(format!("/{}/{}", ss, fs))
            }
            NF_RP_SOURCE => {
                let ro = obj_of(&this);
                if ro >= 0 && self.objs[ro as usize].class == C_REGEXP {
                    let ix = self.objs[ro as usize].func as int;
                    let shown = self.regex_shown_at(ix);
                    if !matches!(shown, Val::Undef) {
                        return shown;
                    }
                    return self.colds[self.objs[ro as usize].cold as usize].prim.clone();
                }
                if ro >= 0 && ro == self.regexp_proto {
                    return self.str_val("(?:)");
                }
                self.throw_type("RegExp.prototype.source getter called on incompatible receiver");
                Val::Undef
            }
            NF_RP_FLAGS => {
                let ro = obj_of(&this);
                if ro < 0 || self.objs[ro as usize].class != C_REGEXP {
                    return self.str_val("");
                }
                self.string_val(self.regexp_flags(ro))
            }
            // ---- Date
            NF_DATE => {
                if !construct {
                    let t = now_ms();
                    return self.string_val(date_string(t));
                }
                let t = if args.is_empty() {
                    now_ms().floor()
                } else if args.len() == 1 {
                    match &a0 {
                        Val::Str(s) => Vm::parse_date(self.sref(*s)),
                        Val::Obj(o) if self.objs[*o as usize].class == C_DATE => match self.colds[self.objs[*o as usize].cold as usize].prim {
                            Val::Num(n) => n,
                            _ => nan(),
                        },
                        _ => {
                            let p = self.to_primitive(&a0, "default");
                            if let Val::Str(s) = &p {
                                let s = self.sget(s.clone());
                                Vm::parse_date(s.as_str())
                            } else {
                                let n = self.to_number(&p);
                                if is_finite(n) && n.abs() <= 8.64e15 {
                                    to_integer(n) + 0.0
                                } else {
                                    nan()
                                }
                            }
                        }
                    }
                } else {
                    let mut f: Vec<double> = vec![0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0];
                    let mut i: usize = 0;
                    while i < 7 && i < args.len() {
                        f[i] = self.to_number(&args[i]);
                        i += 1;
                    }
                    if f[0] >= 0.0 && f[0] <= 99.0 && f[0] == to_integer(f[0]) {
                        f[0] += 1900.0;
                    }
                    Vm::make_time(&f)
                };
                let proto = self.proto_from(&new_target, self.date_proto);
                let d = self.alloc(C_DATE, proto);
                self.put_prim(d, Val::Num(t));
                Val::Obj(d)
            }
            NF_DATE_NOW => Val::Num(now_ms().floor()),
            NF_DATE_UTC => {
                let mut f: Vec<double> = vec![0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0];
                let mut i: usize = 0;
                while i < 7 && i < args.len() {
                    f[i] = self.to_number(&args[i]);
                    i += 1;
                }
                if f[0] >= 0.0 && f[0] <= 99.0 {
                    f[0] += 1900.0;
                }
                Val::Num(Vm::make_time(&f))
            }
            NF_DATE_PARSE => {
                let s = self.to_string(&a0);
                Val::Num(Vm::parse_date(s.as_str()))
            }
            NF_DP_VALUEOF => {
                let t = self.date_value(&this);
                Val::Num(t)
            }
            NF_DP_TOISO | NF_DP_TOJSON => {
                let t = self.date_value(&this);
                if self.throwing {
                    return Val::Undef;
                }
                if is_nan(t) {
                    if id == NF_DP_TOJSON {
                        return Val::Null;
                    }
                    self.throw_range("Invalid time value");
                    return Val::Undef;
                }
                self.string_val(Vm::iso_string(t))
            }
            NF_DP_TOSTRING => {
                let t = self.date_value(&this);
                if is_nan(t) {
                    return self.str_val("Invalid Date");
                }
                self.string_val(date_string(t))
            }
            // ---- Map / Set
            NF_MAP | NF_SET | NF_WEAKMAP | NF_WEAKSET => {
                if !construct {
                    self.throw_type("Constructor requires 'new'");
                    return Val::Undef;
                }
                let is_map = id == NF_MAP || id == NF_WEAKMAP;
                let dflt = if is_map { self.map_proto } else { self.set_proto };
                let proto = self.proto_from(&new_target, dflt);
                let m = self.alloc(if is_map { C_MAP } else { C_SET }, proto);
                if id == NF_WEAKMAP || id == NF_WEAKSET {
                    // weak: keys must be objects
                    self.objs[m as usize].func = 1;
                }
                if !matches!(a0, Val::Undef) && !matches!(a0, Val::Null) {
                    // the adder, as the constructor reads it: a replaced
                    // set / add is called for each entry
                    let adder_atom = self.intern(if is_map { "set" } else { "add" });
                    let adder = self.get_obj(m, adder_atom, &Val::Obj(m));
                    if !self.is_callable(&adder) {
                        self.throw_type("'set' / 'add' of the new collection is not a function");
                        return Val::Undef;
                    }
                    let ao = obj_of(&adder);
                    let native_adder = self.objs[ao as usize].class == C_NATIVE && (self.objs[ao as usize].func == NF_MP_SET as i32 || self.objs[ao as usize].func == NF_SETP_ADD as i32 || self.objs[ao as usize].func == NF_WM_SET as i32 || self.objs[ao as usize].func == NF_WS_ADD as i32);
                    let plain = self.class_of(&a0) == C_ARRAY && obj_of(&self.get(&a0, A_ITERATOR)) == self.array_values_fn;
                    if !native_adder || !plain {
                        self.temp_roots.push(Val::Obj(m));
                        self.fill_collection(m, &a0, adder, is_map);
                        self.temp_roots.pop();
                        if self.throwing {
                            return Val::Undef;
                        }
                        return Val::Obj(m);
                    }
                    self.temp_roots.push(Val::Obj(m));
                    let items = self.iterable_to_vec(&a0);
                    self.temp_roots.pop();
                    if self.throwing {
                        return Val::Undef;
                    }
                    for it in items {
                        if is_map {
                            if !is_obj(&it) {
                                self.throw_type("Iterator value is not an entry object");
                                return Val::Undef;
                            }
                            let k = self.get_index(&it, 0);
                            let v = self.get_index(&it, 1);
                            if id == NF_WEAKMAP && !self.held_weakly(&k) {
                                self.throw_type("Invalid value used as weak map key");
                                return Val::Undef;
                            }
                            self.map_set(m, k, v);
                        } else {
                            if id == NF_WEAKSET && !self.held_weakly(&it) {
                                self.throw_type("Invalid value used in weak set");
                                return Val::Undef;
                            }
                            self.map_set(m, it.clone(), it);
                        }
                    }
                }
                Val::Obj(m)
            }
            NF_MP_GET | NF_MP_SET | NF_MP_HAS | NF_MP_DELETE | NF_MP_CLEAR | NF_MP_SIZE | NF_MP_FOREACH | NF_MP_KEYS | NF_MP_VALUES | NF_MP_ENTRIES | NF_SETP_ADD | NF_SP_HAS | NF_SP_DELETE | NF_SP_CLEAR | NF_SP_FOREACH | NF_SP_SIZE | NF_SP_KEYS | NF_SP_ENTRIES | NF_WM_GET | NF_WM_SET | NF_WM_HAS | NF_WM_DELETE | NF_WS_ADD | NF_WS_HAS | NF_WS_DELETE | NF_MP_GETORINSERT | NF_MP_GETORINSERTC | NF_WM_GETORINSERT | NF_WM_GETORINSERTC => {
                let o = obj_of(&this);
                // brand check: a Map method takes only a Map, a WeakSet
                // method only a WeakSet...
                let want_set = id == NF_SETP_ADD || id == NF_WS_ADD || id == NF_WS_HAS || id == NF_WS_DELETE || (id >= NF_SP_HAS && id <= NF_SP_ENTRIES);
                let want_weak = (id >= NF_WM_GET && id <= NF_WM_SET) || (id >= NF_WM_HAS && id <= NF_WS_DELETE) || id == NF_WM_GETORINSERT || id == NF_WM_GETORINSERTC;
                if o < 0 || self.objs[o as usize].class != (if want_set { C_SET } else { C_MAP }) || (self.objs[o as usize].func == 1) != want_weak {
                    self.throw_type("Map/Set method called on incompatible receiver");
                    return Val::Undef;
                }
                let id = Vm::coll_base(id);
                if want_weak && (id == NF_MP_SET || id == NF_SETP_ADD || id == NF_MP_GETORINSERT || id == NF_MP_GETORINSERTC) && !self.held_weakly(&a0) {
                    self.throw_type("Invalid value used as weak collection key");
                    return Val::Undef;
                }
                match id {
                    NF_MP_GETORINSERT | NF_MP_GETORINSERTC => {
                        if id == NF_MP_GETORINSERTC && !self.callback(&a1, "callbackfn") {
                            return Val::Undef;
                        }
                        // canonical key: -0 is +0
                        let mut k = a0.clone();
                        if let Val::Num(n) = k {
                            if n == 0.0 {
                                k = Val::Num(0.0);
                            }
                        }
                        let i = self.map_find(o, &k);
                        if i >= 0 {
                            return self.colds[self.objs[o as usize].cold as usize].elems2[i as usize].clone();
                        }
                        let v = if id == NF_MP_GETORINSERTC {
                            let r = self.call_fixed(a1.clone(), Val::Undef, 1, k.clone(), Val::Undef, Val::Undef);
                            if self.throwing {
                                return Val::Undef;
                            }
                            r
                        } else {
                            a1.clone()
                        };
                        self.map_set(o, k, v.clone());
                        v
                    }
                    NF_MP_GET => {
                        let i = self.map_find(o, &a0);
                        if i < 0 {
                            Val::Undef
                        } else {
                            self.colds[self.objs[o as usize].cold as usize].elems2[i as usize].clone()
                        }
                    }
                    NF_MP_SET => {
                        if self.objs[o as usize].func == 1 && !is_obj(&a0) {
                            self.throw_type("Invalid value used as weak map key");
                            return Val::Undef;
                        }
                        self.map_set(o, a0, a1);
                        this
                    }
                    NF_SETP_ADD => {
                        if self.objs[o as usize].func == 1 && !is_obj(&a0) {
                            self.throw_type("Invalid value used in weak set");
                            return Val::Undef;
                        }
                        self.map_set(o, a0.clone(), a0);
                        this
                    }
                    NF_MP_HAS => Val::Bool(self.map_find(o, &a0) >= 0),
                    NF_MP_DELETE => Val::Bool(self.map_delete(o, &a0)),
                    NF_MP_CLEAR => {
                        // live iterators start over (coll_gen); func stays
                        // other than 1 (-1, 0, 2, 4...: 1 is a weak one)
                        self.objs[o as usize].func = (self.objs[o as usize].func | 1) + 1;
                        self.put_elems(o, Vec::new());
                        self.put_elems2(o, Vec::new());
                        self.free_cold(o);
                        self.objs[o as usize].pos = 0;
                        Val::Undef
                    }
                    NF_MP_SIZE => Val::Num(self.objs[o as usize].pos as double),
                    NF_MP_FOREACH => {
                        if !self.callback(&a0, "callback") {
                            return Val::Undef;
                        }
                        let is_set = self.objs[o as usize].class == C_SET;
                        let mut i: usize = 0;
                        let mut g = self.coll_gen(o);
                        while i < self.evecs[self.objs[o as usize].elems as usize].v.len() {
                            if self.colds[self.objs[o as usize].cold as usize].marks[i] == 0 {
                                let k = self.evecs[self.objs[o as usize].elems as usize].v[i].clone();
                                let v = if is_set { k.clone() } else { self.colds[self.objs[o as usize].cold as usize].elems2[i].clone() };
                                self.call_fixed(a0.clone(), a1.clone(), 3, v, k, this.clone());
                                if self.throwing {
                                    return Val::Undef;
                                }
                                if self.coll_gen(o) != g {
                                    // cleared by the callback: what is there now
                                    g = self.coll_gen(o);
                                    i = 0;
                                    continue;
                                }
                            }
                            i += 1;
                        }
                        Val::Undef
                    }
                    NF_MP_KEYS => self.collection_iter(o, 0),
                    NF_MP_VALUES => self.collection_iter(o, 1),
                    _ => self.collection_iter(o, 2),
                }
            }
            // ---- Promise
            NF_PROMISE => {
                if !construct {
                    self.throw_type("Promise constructor cannot be invoked without 'new'");
                    return Val::Undef;
                }
                if !self.callback(&a0, "Promise resolver") {
                    return Val::Undef;
                }
                let p = self.new_promise();
                let proto = self.proto_from(&new_target, self.promise_proto);
                self.put_proto(p, proto);
                let (res, rej) = self.resolving_functions(p);
                self.temp_roots.push(Val::Obj(p));
                self.call_fixed(a0, Val::Undef, 2, Val::Obj(res), Val::Obj(rej), Val::Undef);
                self.temp_roots.pop();
                if self.throwing && !self.terminated {
                    self.throwing = false;
                    let e = self.exc.clone();
                    self.exc = Val::Undef;
                    self.settle(p, 2, e);
                }
                Val::Obj(p)
            }
            NF_PR_RESOLVE_FN | NF_PR_REJECT_FN => {
                let p = self.objs[fobj as usize].env as int;
                let cell = self.colds[self.objs[fobj as usize].cold as usize].home;
                if cell >= 0 {
                    if self.objs[cell as usize].pos != 0 {
                        return Val::Undef;
                    }
                    self.objs[cell as usize].pos = 1;
                }
                self.settle(p, if id == NF_PR_RESOLVE_FN { 1 } else { 2 }, a0);
                Val::Undef
            }
            NF_PR_RESOLVE | NF_PR_REJECT => {
                if id == NF_PR_RESOLVE && self.class_of(&a0) == C_PROMISE {
                    return a0;
                }
                let p = self.new_promise();
                self.settle(p, if id == NF_PR_RESOLVE { 1 } else { 2 }, a0);
                Val::Obj(p)
            }
            NF_PR_THEN | NF_PR_CATCH | NF_PR_FINALLY => {
                let p = obj_of(&this);
                if p < 0 || self.objs[p as usize].class != C_PROMISE {
                    self.throw_type("Promise.prototype.then called on incompatible receiver");
                    return Val::Undef;
                }
                match id {
                    NF_PR_THEN => self.promise_then(p, a0, a1),
                    NF_PR_CATCH => self.promise_then(p, Val::Undef, a0),
                    _ => self.promise_then(p, a0.clone(), a0),
                }
            }
            NF_PR_ALL => {
                // settles when every input promise has: a simple form
                let items = self.iterable_to_vec(&a0);
                if self.throwing {
                    return Val::Undef;
                }
                let p = self.new_promise();
                let mut values: Vec<Val> = Vec::new();
                let mut all_done = true;
                for it in items.iter() {
                    if self.class_of(it) == C_PROMISE {
                        let o = obj_of(it);
                        let st = self.objs[o as usize].pos as int;
                        if st == 1 {
                            values.push(self.colds[self.objs[o as usize].cold as usize].prim.clone());
                        } else if st == 2 {
                            let e = self.colds[self.objs[o as usize].cold as usize].prim.clone();
                            self.settle(p, 2, e);
                            return Val::Obj(p);
                        } else {
                            all_done = false;
                        }
                    } else {
                        values.push(it.clone());
                    }
                }
                if all_done {
                    let a = self.new_array(values);
                    self.settle(p, 1, Val::Obj(a));
                }
                Val::Obj(p)
            }
            _ => {
                self.throw_type(format!("built-in {} is not implemented", id).as_str());
                Val::Undef
            }
        }
    }

    fn rel_index_pos(&mut self, v: &Val, len: int, dflt: int) -> int {
        if matches!(v, Val::Undef) {
            return dflt;
        }
        let n = to_integer(self.to_number(v));
        if n < 0.0 {
            0
        } else if n > len as double {
            len
        } else {
            n as int
        }
    }
}

fn pow10(n: i32) -> double {
    let ten: double = 10.0;
    ten.powf(n as double)
}

pub fn date_string(t: double) -> String {
    let days = vec!["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    let months = vec!["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let f = Vm::time_fields(t);
    format!(
        "{} {} {:02} {} {:02}:{:02}:{:02} GMT+0000 (Coordinated Universal Time)",
        days[f[3] as usize],
        months[f[1] as usize],
        f[2] as int,
        f[0] as int,
        f[4] as int,
        f[5] as int,
        f[6] as int
    )
}

/// The p significant digits of v > 0 (rounded half up on its exact
/// value, as the spec asks) and the exponent of the first: Rust from the
/// exact decimal expansion, so 100 digits and denormals come out right.
#[ranger::target(rust)]
fn precision_digits(v: double, p: int) -> (String, int) {
    // every double's exact expansion fits in 800 significant digits
    let t = format!("{:.*e}", 800, v);
    let (m, ex) = match t.find('e') {
        Some(i) => (&t[..i], t[i + 1..].parse::<int>().unwrap_or(0)),
        None => (t.as_str(), 0),
    };
    let all: Vec<u8> = m.bytes().filter(|c| *c != b'.').map(|c| c - b'0').collect();
    let n = p as usize;
    let mut d: Vec<u8> = all[..n].to_vec();
    let mut e = ex;
    if all[n] >= 5 {
        // half up: carry through the nines
        let mut i = n;
        loop {
            if i == 0 {
                d.insert(0, 1);
                d.pop();
                e += 1;
                break;
            }
            i -= 1;
            if d[i] == 9 {
                d[i] = 0;
            } else {
                d[i] += 1;
                break;
            }
        }
    }
    (d.iter().map(|c| (b'0' + c) as char).collect(), e)
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
fn precision_digits(v: double, p: int) -> (String, int) {
    let mut e = v.log10().floor() as int;
    let mut scale = pow10((p - 1 - e) as i32);
    let mut n = (v * scale).round();
    if n >= pow10(p as i32) {
        e += 1;
        scale = pow10((p - 1 - e) as i32);
        n = (v * scale).round();
    }
    (format!("{}", n as int), e)
}

/// Number.prototype.toPrecision
pub fn to_precision(x: double, p: int) -> String {
    if x == 0.0 {
        if p == 1 {
            return String::from("0");
        }
        return format!("0.{}", "0".repeat((p - 1) as usize));
    }
    let neg = x < 0.0;
    let v = if neg { -x } else { x };
    let (digits, e) = precision_digits(v, p);
    let body: String;
    if e < -6 || e >= p {
        let mut s = String::new();
        s.push_str(&digits[0..1]);
        if p > 1 {
            s.push('.');
            s.push_str(&digits[1..]);
        }
        body = format!("{}e{}{}", s, if e >= 0 { "+" } else { "-" }, e.abs());
    } else if e >= p - 1 {
        body = digits;
    } else if e >= 0 {
        body = format!("{}.{}", &digits[0..((e + 1) as usize)], &digits[((e + 1) as usize)..]);
    } else {
        body = format!("0.{}{}", "0".repeat((-e - 1) as usize), digits);
    }
    if neg {
        format!("-{}", body)
    } else {
        body
    }
}

/// Number.prototype.toExponential; `d` -1 for as many digits as needed
pub fn to_exponential(x: double, d: int) -> String {
    let neg = x < 0.0;
    let v = if neg { -x } else { x };
    let s: String;
    if d < 0 {
        let t = number_to_string(v);
        // shortest digits
        let mut digits = String::new();
        let mut e: int = 0;
        let mut seen_point = false;
        let mut lead = true;
        let mut before: int = 0;
        let mut in_exp = false;
        let mut exp_text = String::new();
        for c in t.chars() {
            if c == 'e' {
                in_exp = true;
                continue;
            }
            if in_exp {
                exp_text.push(c);
                continue;
            }
            if c == '.' {
                seen_point = true;
                continue;
            }
            if lead && c == '0' {
                if seen_point {
                    before -= 1;
                }
                continue;
            }
            lead = false;
            digits.push(c);
            if !seen_point {
                before += 1;
            }
        }
        if in_exp {
            e = exp_text.parse::<i64>().unwrap_or(0);
        }
        let exp = if v == 0.0 { 0 } else { before - 1 + e };
        let mut dg = digits.clone();
        while dg.ends_with("0") {
            dg = String::from(&dg[0..(dg.as_bytes().len() - 1)]);
        }
        let dg2 = if dg.is_empty() { String::from("0") } else { dg };
        let mant = if dg2.as_bytes().len() > 1 { format!("{}.{}", &dg2[0..1], &dg2[1..]) } else { dg2 };
        s = format!("{}e{}{}", mant, if exp >= 0 { "+" } else { "-" }, exp.abs());
    } else {
        let mut e = if v == 0.0 { 0 } else { v.log10().floor() as int };
        let mut n = (v / pow10((e - d) as i32)).round();
        if n >= pow10((d + 1) as i32) {
            e += 1;
            n = (v / pow10((e - d) as i32)).round();
        }
        let mut digits = format!("{}", n as int);
        while (digits.as_bytes().len() as int) < d + 1 {
            digits.push('0');
        }
        let mant = if d > 0 { format!("{}.{}", &digits[0..1], &digits[1..]) } else { digits };
        s = format!("{}e{}{}", mant, if e >= 0 { "+" } else { "-" }, e.abs());
    }
    if neg {
        format!("-{}", s)
    } else {
        s
    }
}

/// Rust: whether `s` starts (ends, `end`) with an ASCII `p`, 1 or 0: an
/// ASCII byte is a whole UTF-16 unit, so the bytes answer it. -1 when `p`
/// is not ASCII, and on the other targets.
#[ranger::target(rust)]
pub fn ascii_affix(s: &str, p: &str, end: bool) -> int {
    if !p.is_ascii() {
        return -1;
    }
    let r = if end { s.as_bytes().ends_with(p.as_bytes()) } else { s.as_bytes().starts_with(p.as_bytes()) };
    if r {
        1
    } else {
        0
    }
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn ascii_affix(_s: &str, _p: &str, _end: bool) -> int {
    -1
}
