// SPDX-License-Identifier: AGPL-3.0-or-later
//! Code made at run time: `eval` (always as global code: the evaluator
//! keeps no scope a direct eval could see) and the Function,
//! GeneratorFunction and AsyncFunction constructors, which compile their
//! source as a script in the global scope.

use ranger::prelude::*;

use crate::compiler;
use crate::parser;
use crate::value::*;
use crate::vm::*;

pub const NF_EVAL: int = 940;
pub const NF_GENERATOR_FUNCTION: int = 941;
pub const NF_ASYNC_FUNCTION: int = 942;
pub const NF_ASYNC_GENERATOR_FUNCTION: int = 943;
pub const NF_SYMP_VALUEOF: int = 944;
pub const NF_PROTO_GET: int = 945;
pub const NF_PROTO_SET: int = 946;
pub const NF_RE_COMPILE: int = 947;

impl Vm {
    pub fn setup_dynamic(&mut self) {
        let g = self.global;
        self.method(g, "eval", NF_EVAL, 1);
        let symp = self.symbol_proto;
        self.method(symp, "valueOf", NF_SYMP_VALUEOF, 0);
        let a_tp = crate::vm::A_TOPRIMITIVE;
        let tpf = self.native_fn("[Symbol.toPrimitive]", NF_SYMP_VALUEOF, 1);
        self.objs[symp as usize].add(a_tp, Val::Obj(tpf), P_HIDDEN | P_READONLY);
        let op = self.object_proto;
        let pg = self.native_fn("get __proto__", NF_PROTO_GET, 0);
        let ps = self.native_fn("set __proto__", NF_PROTO_SET, 1);
        self.define_accessor(op, crate::vm::A_PROTO, Val::Obj(pg), 0, true);
        self.define_accessor(op, crate::vm::A_PROTO, Val::Obj(ps), 1, true);
        let rp = self.regexp_proto;
        self.method(rp, "compile", NF_RE_COMPILE, 2);
        // the constructors of generator and async functions are reached
        // through their prototypes only
        let gfp = self.generator_function_proto;
        let gf = self.native_fn("GeneratorFunction", NF_GENERATOR_FUNCTION, 1);
        self.objs[gf as usize].add(A_PROTOTYPE, Val::Obj(gfp), P_HIDDEN | P_READONLY | P_FIXED);
        self.objs[gf as usize].has_proto_obj = true;
        self.objs[gfp as usize].add(A_CONSTRUCTOR, Val::Obj(gf), P_HIDDEN | P_READONLY);
        let fc = self.get_obj(self.global, self.atoms["Function"], &Val::Obj(self.global));
        if let Val::Obj(f) = fc {
            self.objs[gf as usize].proto = f;
        }
        let agfp = self.async_generator_function_proto;
        let agf = self.native_fn("AsyncGeneratorFunction", NF_ASYNC_GENERATOR_FUNCTION, 1);
        self.objs[agf as usize].add(A_PROTOTYPE, Val::Obj(agfp), P_HIDDEN | P_READONLY | P_FIXED);
        self.objs[agf as usize].has_proto_obj = true;
        self.objs[agfp as usize].add(A_CONSTRUCTOR, Val::Obj(agf), P_HIDDEN | P_READONLY);
        if let Val::Obj(f) = fc {
            self.objs[agf as usize].proto = f;
        }
        let afp = self.async_function_proto;
        let af = self.native_fn("AsyncFunction", NF_ASYNC_FUNCTION, 1);
        self.objs[af as usize].add(A_PROTOTYPE, Val::Obj(afp), P_HIDDEN | P_READONLY | P_FIXED);
        self.objs[af as usize].has_proto_obj = true;
        self.objs[afp as usize].add(A_CONSTRUCTOR, Val::Obj(af), P_HIDDEN | P_READONLY);
        if let Val::Obj(f) = fc {
            self.objs[af as usize].proto = f;
        }
    }

    /// Compiles `src` as a script and runs it in the global scope; its
    /// completion value, or a thrown SyntaxError.
    pub fn eval_source(&mut self, src: &str, strict: bool) -> Val {
        let code = if strict { format!("'use strict';\n{}", src) } else { String::from(src) };
        let mut p = parser::Parser::new(code.as_str());
        let root = p.parse_program();
        if !p.error.is_empty() {
            let msg = p.error.replace("SyntaxError: ", "");
            self.throw_syntax(msg.as_str());
            return Val::Undef;
        }
        let atoms = self.atoms.clone();
        let names = self.atom_names.clone();
        let base = self.protos.len() as int;
        let mut c = compiler::Compiler::new(p.ast, atoms, names, base);
        c.local_program = strict;
        let entry = c.compile_program(root);
        if !c.error.is_empty() || entry < 0 {
            let msg = if c.error.is_empty() { String::from("invalid code") } else { c.error.replace("SyntaxError: ", "") };
            self.throw_syntax(msg.as_str());
            return Val::Undef;
        }
        self.atoms = c.atoms;
        self.atom_names = c.atom_names;
        for pr in c.protos {
            self.protos.push(pr);
        }
        let fp = self.function_proto;
        let f = self.alloc(C_FUNCTION, fp);
        self.objs[f as usize].func = entry;
        let g = Val::Obj(self.global);
        self.call_value(Val::Obj(f), g, Vec::new())
    }

    fn caller_strict(&self) -> bool {
        if self.frames.is_empty() {
            return false;
        }
        let f = &self.frames[self.frames.len() - 1];
        self.protos[f.proto as usize].strict
    }

    /// thisSymbolValue: a symbol, or the one a Symbol object holds.
    pub fn this_symbol(&mut self, v: &Val) -> Val {
        if self.class_of(v) == C_SYMBOL {
            return v.clone();
        }
        let o = obj_of(v);
        if o >= 0 && self.objs[o as usize].class == C_OBJECT && self.class_of(&self.objs[o as usize].prim.clone()) == C_SYMBOL {
            return self.objs[o as usize].prim.clone();
        }
        self.throw_type("Symbol.prototype method called on incompatible receiver");
        Val::Undef
    }

    pub fn call_native_dynamic_this(&mut self, id: int, this: Val, args: Vec<Val>) -> Val {
        let a0 = if args.is_empty() { Val::Undef } else { args[0].clone() };
        if id == NF_SYMP_VALUEOF {
            return self.this_symbol(&this);
        }
        if id == NF_PROTO_GET {
            let o = self.to_object(&this);
            if self.throwing {
                return Val::Undef;
            }
            let p = self.proto_of(o);
            return if p >= 0 { Val::Obj(p) } else { Val::Null };
        }
        if id == NF_PROTO_SET {
            if matches!(this, Val::Undef) || matches!(this, Val::Null) {
                self.throw_type("Object.prototype.__proto__ called on null or undefined");
                return Val::Undef;
            }
            let o = obj_of(&this);
            if o < 0 || !(is_obj(&a0) || matches!(a0, Val::Null)) {
                return Val::Undef;
            }
            let p = obj_of(&a0);
            // no cycles
            let mut c = p;
            while c >= 0 {
                if c == o {
                    self.throw_type("Cyclic __proto__ value");
                    return Val::Undef;
                }
                c = self.objs[c as usize].proto;
            }
            if !self.objs[o as usize].extensible && self.objs[o as usize].proto != p {
                self.throw_type("#<Object> is not extensible");
                return Val::Undef;
            }
            self.objs[o as usize].proto = p;
            return Val::Undef;
        }
        if id == NF_RE_COMPILE {
            // Annex B: the regexp takes a new pattern and flags
            let o = obj_of(&this);
            if o < 0 || self.objs[o as usize].class != C_REGEXP {
                self.throw_type("RegExp.prototype.compile called on incompatible receiver");
                return Val::Undef;
            }
            let a1 = if args.len() < 2 { Val::Undef } else { args[1].clone() };
            let (src, fl) = if self.class_of(&a0) == C_REGEXP {
                let s = self.get(&a0, self.atoms["source"]);
                let f = self.get(&a0, self.atoms["flags"]);
                (s, f)
            } else {
                (if matches!(a0, Val::Undef) { str_val("") } else { a0.clone() }, a1)
            };
            let fresh = self.new_regexp(&src, &fl);
            if self.throwing {
                return Val::Undef;
            }
            let n = obj_of(&fresh);
            let func = self.objs[n as usize].func;
            let prim = self.objs[n as usize].prim.clone();
            let pos = self.objs[n as usize].pos;
            let keys = self.objs[n as usize].keys.clone();
            let vals = self.objs[n as usize].vals.clone();
            self.objs[o as usize].func = func;
            self.objs[o as usize].prim = prim;
            self.objs[o as usize].pos = pos;
            let mut i: usize = 0;
            while i < keys.len() {
                let slot = self.objs[o as usize].find(keys[i]);
                if slot >= 0 {
                    self.objs[o as usize].vals[slot as usize] = vals[i].clone();
                } else {
                    let attr = self.objs[n as usize].attrs[i];
                    self.objs[o as usize].add(keys[i], vals[i].clone(), attr);
                }
                i += 1;
            }
            let li = self.objs[o as usize].find(crate::vm::A_LASTINDEX);
            if li >= 0 {
                self.objs[o as usize].vals[li as usize] = Val::Num(0.0);
            }
            return this;
        }
        Val::Undef
    }

    pub fn call_native_dynamic(&mut self, id: int, args: Vec<Val>, new_target: Val) -> Val {
        if id == NF_EVAL {
            if args.is_empty() {
                return Val::Undef;
            }
            if let Val::Str(s) = &args[0] {
                let src = s.as_ref().clone();
                // the caller's strictness (a direct eval's; an indirect one
                // from strict code is taken as direct)
                let strict = self.caller_strict();
                return self.eval_source(src.as_str(), strict);
            }
            return args[0].clone();
        }
        // new Function(p1, …, pn, body)
        let n = args.len();
        let mut params = String::new();
        let mut i: usize = 0;
        while i + 1 < n {
            if i > 0 {
                params.push(',');
            }
            let s = self.to_string(&args[i]);
            params.push_str(s.as_str());
            i += 1;
        }
        let body = if n > 0 { self.to_string(&args[n - 1]) } else { String::new() };
        if self.throwing {
            return Val::Undef;
        }
        let head = if id == NF_GENERATOR_FUNCTION {
            "function*"
        } else if id == NF_ASYNC_FUNCTION {
            "async function"
        } else if id == NF_ASYNC_GENERATOR_FUNCTION {
            "async function*"
        } else {
            "function"
        };
        let src = format!("({} anonymous({}\n) {{\n{}\n}})", head, params, body);
        let f = self.eval_source(src.as_str(), false);
        if self.throwing {
            return Val::Undef;
        }
        // a subclass's instance
        let fo = obj_of(&f);
        if fo >= 0 && is_obj(&new_target) {
            let dflt = self.objs[fo as usize].proto;
            let p = self.proto_from(&new_target, dflt);
            self.objs[fo as usize].proto = p;
        }
        f
    }
}
