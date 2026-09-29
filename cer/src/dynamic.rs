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

impl Vm {
    pub fn setup_dynamic(&mut self) {
        let g = self.global;
        self.method(g, "eval", NF_EVAL, 1);
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
