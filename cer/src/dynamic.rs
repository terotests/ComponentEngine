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
/// %ThrowTypeError%, and the Function.prototype caller / arguments getter
pub const NF_THROWER: int = 948;
pub const NF_FN_CALLER: int = 949;
/// __cerxesESMCompile(source, path, async): a module's entries and script
/// as JSON (esm.rs), for the module loader in prelude.rs
pub const NF_ESM_COMPILE: int = 955;

impl Vm {
    pub fn setup_dynamic(&mut self) {
        let g = self.global;
        self.method(g, "eval", NF_EVAL, 1);
        self.method(g, "__cerxesESMCompile", NF_ESM_COMPILE, 3);
        let symp = self.symbol_proto;
        self.method(symp, "valueOf", NF_SYMP_VALUEOF, 0);
        let a_tp = crate::vm::A_TOPRIMITIVE;
        let tpf = self.native_fn("[Symbol.toPrimitive]", NF_SYMP_VALUEOF, 1);
        self.add_prop(symp, a_tp, Val::Obj(tpf), P_HIDDEN | P_READONLY);
        let op = self.object_proto;
        let pg = self.native_fn("get __proto__", NF_PROTO_GET, 0);
        let ps = self.native_fn("set __proto__", NF_PROTO_SET, 1);
        self.define_accessor(op, crate::vm::A_PROTO, Val::Obj(pg), 0, true);
        self.define_accessor(op, crate::vm::A_PROTO, Val::Obj(ps), 1, true);
        let rp = self.regexp_proto;
        self.method(rp, "compile", NF_RE_COMPILE, 2);
        let th = self.native_fn("", NF_THROWER, 0);
        self.roots.push(th);
        self.thrower = th;
        let tz = self.alloc(C_OBJECT, -1);
        self.roots.push(tz);
        self.tdz_obj = tz;
        let fp = self.function_proto;
        let cg = self.native_fn("caller", NF_FN_CALLER, 0);
        let a_caller = self.intern("caller");
        let a_arguments = self.intern("arguments");
        self.define_accessor(fp, a_caller, Val::Obj(cg), 0, true);
        self.define_accessor(fp, a_caller, Val::Obj(th), 1, true);
        self.define_accessor(fp, a_arguments, Val::Obj(cg), 0, true);
        self.define_accessor(fp, a_arguments, Val::Obj(th), 1, true);
        // the constructors of generator and async functions are reached
        // through their prototypes only
        let gfp = self.generator_function_proto;
        let gf = self.native_fn("GeneratorFunction", NF_GENERATOR_FUNCTION, 1);
        self.add_prop(gf, A_PROTOTYPE, Val::Obj(gfp), P_HIDDEN | P_READONLY | P_FIXED);
        self.add_prop(gfp, A_CONSTRUCTOR, Val::Obj(gf), P_HIDDEN | P_READONLY);
        let fc = self.get_obj(self.global, self.atoms["Function"], &Val::Obj(self.global));
        if let Val::Obj(f) = fc {
            self.put_proto(gf, f);
        }
        let agfp = self.async_generator_function_proto;
        let agf = self.native_fn("AsyncGeneratorFunction", NF_ASYNC_GENERATOR_FUNCTION, 1);
        self.add_prop(agf, A_PROTOTYPE, Val::Obj(agfp), P_HIDDEN | P_READONLY | P_FIXED);
        self.add_prop(agfp, A_CONSTRUCTOR, Val::Obj(agf), P_HIDDEN | P_READONLY);
        if let Val::Obj(f) = fc {
            self.put_proto(agf, f);
        }
        let afp = self.async_function_proto;
        let af = self.native_fn("AsyncFunction", NF_ASYNC_FUNCTION, 1);
        self.add_prop(af, A_PROTOTYPE, Val::Obj(afp), P_HIDDEN | P_READONLY | P_FIXED);
        self.add_prop(afp, A_CONSTRUCTOR, Val::Obj(af), P_HIDDEN | P_READONLY);
        if let Val::Obj(f) = fc {
            self.put_proto(af, f);
        }
    }

    /// Compiles `src` as a script and runs it in the global scope; its
    /// completion value, or a thrown SyntaxError.
    pub fn eval_source(&mut self, src: &str, strict: bool, is_eval: bool) -> Val {
        let code = if strict { format!("'use strict';\n{}", src) } else { String::from(src) };
        // a module's text (cerxes module.rs) goes to the parser, which
        // wraps it
        let module = code.starts_with("/*@cerxesModule ");
        let mut pcomp_error = String::new();
        if self.pcomp && module {
            let ms = parser::module_source(code.as_str());
            if !ms.path.is_empty() {
                let entry = self.pcomp_module(&ms);
                if entry >= 0 {
                    let fp = self.function_proto;
                    let f = self.alloc(C_FUNCTION, fp);
                    self.objs[f as usize].func = entry as i32;
                    let g = Val::Obj(self.global);
                    return self.call_value(Val::Obj(f), g, Vec::new());
                }
                // the parser reads it, and names its file in an error
            }
        }
        if self.pcomp && !module {
            let (entry, error) = self.pcomp_program(code.as_str(), strict, is_eval);
            if entry >= 0 {
                let fp = self.function_proto;
                let f = self.alloc(C_FUNCTION, fp);
                self.objs[f as usize].func = entry as i32;
                let g = Val::Obj(self.global);
                return self.call_value(Val::Obj(f), g, Vec::new());
            }
            pcomp_error = error;
        }
        if self.pcomp && parser::WRITES_JS {
            // TypeScript, JSX or a module: as JavaScript, for pcomp
            let (js, err) = parser::eval_js(code.as_str(), pcomp_error.as_str());
            if !err.is_empty() {
                let msg = err.replace("SyntaxError: ", "");
                self.throw_syntax(msg.as_str());
                return Val::Undef;
            }
            let (entry, error) = self.pcomp_program(js.as_str(), strict, is_eval);
            if entry < 0 {
                let msg = error.replace("SyntaxError: ", "");
                self.throw_syntax(msg.as_str());
                return Val::Undef;
            }
            let fp = self.function_proto;
            let f = self.alloc(C_FUNCTION, fp);
            self.objs[f as usize].func = entry as i32;
            let g = Val::Obj(self.global);
            return self.call_value(Val::Obj(f), g, Vec::new());
        }
        let mut p = parser::Parser::new(code.as_str());
        let root = p.parse_program();
        if p.error.is_empty() && !pcomp_error.is_empty() && !p.beyond_js {
            // plain JavaScript pcomp rejected: its early errors stand
            p.error = pcomp_error;
        }
        if !p.error.is_empty() {
            let msg = p.error.replace("SyntaxError: ", "");
            self.throw_syntax(msg.as_str());
            return Val::Undef;
        }
        let atoms = self.atoms.clone();
        let names = self.atom_names.clone();
        let base = self.protos.len() as int;
        // strict eval code (the caller's or its own "use strict") keeps its
        // declarations to itself
        let own_strict = (p.ast.nodes[root as usize].flags & crate::ast::F_STRICT) != 0;
        let mut c = compiler::Compiler::new(p.ast, atoms, names, base);
        c.local_program = strict || (own_strict && is_eval);
        let entry = c.compile_program(root);
        if !c.error.is_empty() || entry < 0 {
            let msg = if c.error.is_empty() { String::from("invalid code") } else { c.error.replace("SyntaxError: ", "") };
            self.throw_syntax(msg.as_str());
            return Val::Undef;
        }
        self.atoms = c.atoms;
        self.atom_names = c.atom_names;
        for pr in c.protos {
            let mut p2 = pr;
            self.link_strs(&mut p2);
            pack_code(&mut p2);
            self.protos.push(p2);
        }
        let fp = self.function_proto;
        let f = self.alloc(C_FUNCTION, fp);
        self.objs[f as usize].func = entry as i32;
        let g = Val::Obj(self.global);
        self.call_value(Val::Obj(f), g, Vec::new())
    }

    /// eval_source's code compiled with pcomp.rs and linked: the program's
    /// proto, or -1 (nothing changed) and the error when pcomp cannot
    /// compile it.
    fn pcomp_program(&mut self, code: &str, strict: bool, is_eval: bool) -> (int, String) {
        let atoms = self.atoms.clone();
        let names = self.atom_names.clone();
        let base = self.protos.len() as int;
        let mut c = crate::pcomp::PComp::new(code, atoms, names, base);
        c.local_program = strict;
        c.local_if_strict = is_eval;
        // its classes' private keys: ids of the engine (as unit::link's)
        c.priv_next = self.priv_seq + 1;
        let entry = c.compile_program();
        self.priv_seq = c.priv_next;
        if !c.error.is_empty() || entry < 0 {
            let e = if c.error.is_empty() { String::from("SyntaxError") } else { c.error.clone() };
            return (-1, e);
        }
        self.atoms = c.atoms;
        self.atom_names = c.atom_names;
        for pr in c.protos {
            let mut p2 = pr;
            self.link_strs(&mut p2);
            pack_code(&mut p2);
            self.protos.push(p2);
        }
        (entry, String::new())
    }

    /// A direct eval of `src` from the running frame: its code sees the
    /// caller's bindings (`desc`, pcomp `x_eval_desc`), `this`, and
    /// strictness.
    pub fn direct_eval(&mut self, src: &str, desc: &str) -> Val {
        let strict = self.caller_strict();
        if !self.pcomp || src.starts_with("/*@cerxesModule ") {
            return self.eval_source(src, strict, true);
        }
        let atoms = self.atoms.clone();
        let names = self.atom_names.clone();
        let base = self.protos.len() as int;
        let mut c = crate::pcomp::PComp::new(src, atoms, names, base);
        c.force_strict = strict;
        c.local_if_strict = true;
        c.outer_desc = String::from(desc);
        c.priv_next = self.priv_seq + 1;
        let entry = c.compile_program();
        self.priv_seq = c.priv_next;
        if !c.error.is_empty() || entry < 0 {
            let e = if c.error.is_empty() { String::from("invalid code") } else { c.error.replace("SyntaxError: ", "") };
            self.throw_syntax(e.as_str());
            return Val::Undef;
        }
        self.atoms = c.atoms;
        self.atom_names = c.atom_names;
        for pr in c.protos {
            let mut p2 = pr;
            self.link_strs(&mut p2);
            pack_code(&mut p2);
            self.protos.push(p2);
        }
        // an arrow over the caller's environment: its `this` and new.target
        self.protos[entry as usize].arrow = true;
        self.protos[entry as usize].lexical_ctor = true;
        let f = self.closure(entry);
        self.call_value(Val::Obj(f), Val::Undef, Vec::new())
    }

    /// A module's function (`parser::module_source`) compiled with pcomp
    /// and linked: its proto, or -1 (nothing changed).
    fn pcomp_module(&mut self, ms: &parser::ModuleSource) -> int {
        let atoms = self.atoms.clone();
        let names = self.atom_names.clone();
        let base = self.protos.len() as int;
        let mut c = crate::pcomp::PComp::new_syntax(ms.body.as_str(), atoms, names, base, ms.jsx, ms.jsx_factory.as_str(), ms.jsx_fragment.as_str());
        c.ts = ms.typescript;
        c.module_path = ms.path.clone();
        let entry = c.compile_program();
        if !c.error.is_empty() || entry < 0 {
            return -1;
        }
        self.atoms = c.atoms;
        self.atom_names = c.atom_names;
        for pr in c.protos {
            let mut p2 = pr;
            self.link_strs(&mut p2);
            pack_code(&mut p2);
            self.protos.push(p2);
        }
        entry
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
        if o >= 0 && self.objs[o as usize].class == C_OBJECT && self.class_of(&self.colds[self.objs[o as usize].cold as usize].prim.clone()) == C_SYMBOL {
            return self.colds[self.objs[o as usize].cold as usize].prim.clone();
        }
        self.throw_type("Symbol.prototype method called on incompatible receiver");
        Val::Undef
    }

    pub fn call_native_dynamic_this(&mut self, id: int, this: Val, args: &Vec<Val>) -> Val {
        let a0 = if args.is_empty() { Val::Undef } else { args[0].clone() };
        if id == NF_SYMP_VALUEOF {
            return self.this_symbol(&this);
        }
        if id == NF_THROWER {
            self.throw_type("'caller', 'callee', and 'arguments' properties may not be accessed on strict mode functions or the arguments objects for calls to them");
            return Val::Undef;
        }
        if id == NF_FN_CALLER {
            // a sloppy function's caller / arguments: null here; a strict,
            // arrow, class or method function has none
            let f = obj_of(&this);
            if f >= 0 && self.objs[f as usize].class == C_FUNCTION {
                let pi = self.objs[f as usize].func as usize;
                let p = &self.protos[pi];
                if p.strict || p.arrow || p.method || p.class_ctor || p.generator || p.is_async {
                    self.throw_type("'caller', 'callee', and 'arguments' properties may not be accessed on strict mode functions or the arguments objects for calls to them");
                    return Val::Undef;
                }
            }
            return Val::Null;
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
                c = self.objs[c as usize].proto as int;
            }
            if !self.objs[o as usize].extensible && self.objs[o as usize].proto != p as i32 {
                self.throw_type("#<Object> is not extensible");
                return Val::Undef;
            }
            self.put_proto(o, p);
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
                (if matches!(a0, Val::Undef) { self.str_val("") } else { a0.clone() }, a1)
            };
            let fresh = self.new_regexp(&src, &fl);
            if self.throwing {
                return Val::Undef;
            }
            let n = obj_of(&fresh);
            let func = self.objs[n as usize].func as int;
            let prim = self.colds[self.objs[n as usize].cold as usize].prim.clone();
            let pos = self.objs[n as usize].pos as int;
            self.compact_dict(n);
            let keys = self.keys_of(n);
            let vals = self.objs[n as usize].vals.clone();
            self.objs[o as usize].func = func as i32;
            self.put_prim(o, prim);
            self.objs[o as usize].pos = pos as i32;
            let mut i: usize = 0;
            while i < keys.len() {
                let slot = self.find(o, keys[i]);
                if slot >= 0 {
                    self.put_slot(o, slot, vals[i].clone());
                } else {
                    let attr = self.attr_at(n, i as int);
                    self.add_prop(o, keys[i], vals[i].clone(), attr);
                }
                i += 1;
            }
            let li = self.find(o, crate::vm::A_LASTINDEX);
            if li >= 0 {
                self.put_slot(o, li, Val::Num(0.0));
            }
            return this;
        }
        Val::Undef
    }

    pub fn call_native_esm(&mut self, args: &Vec<Val>) -> Val {
        let a0 = if args.is_empty() { Val::Undef } else { args[0].clone() };
        let a1 = if args.len() < 2 { Val::Undef } else { args[1].clone() };
        let src = self.to_str(&a0);
        let path = self.to_str(&a1);
        let is_async = args.len() > 2 && self.truthy_v(&args[2]);
        let m = crate::esm::esm_compile(src.as_str(), path.as_str(), is_async);
        if !m.error.is_empty() {
            let msg = m.error.clone();
            let t = if msg.starts_with("SyntaxError: ") { String::from(&msg[13..]) } else { msg };
            self.throw_syntax(t.as_str());
            return Val::Undef;
        }
        self.str_val(m.json.as_str())
    }

    pub fn call_native_dynamic(&mut self, id: int, args: &Vec<Val>, new_target: Val) -> Val {
        if id == NF_EVAL {
            if args.is_empty() {
                return Val::Undef;
            }
            if let Val::Str(s) = &args[0] {
                let s = self.sget(s.clone());
                let src = s.as_ref().clone();
                // an indirect eval is sloppy global code (pcomp calls a
                // direct one with OP_CALL_EVAL); the AST compiler's are
                // taken as direct
                let strict = if self.pcomp { false } else { self.caller_strict() };
                return self.eval_source(src.as_str(), strict, true);
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
        let f = self.eval_source(src.as_str(), false, false);
        if self.throwing {
            return Val::Undef;
        }
        // a subclass's instance
        let fo = obj_of(&f);
        if fo >= 0 && is_obj(&new_target) {
            let dflt = self.objs[fo as usize].proto as int;
            let p = self.proto_from(&new_target, dflt);
            self.put_proto(fo, p);
        }
        f
    }
}
