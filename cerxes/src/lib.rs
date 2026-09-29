// SPDX-License-Identifier: AGPL-3.0-or-later
//! CErXes: CEr (../cer) with TypeScript and JSX.
//!
//! CEr reads JavaScript; CErXes reads TypeScript and JSX too and builds the
//! same syntax tree, so CEr's compiler, VM and built-ins run the result as
//! they run any script. Those are CEr's own files: `ast`, `compiler`, `vm`,
//! `value`, `builtins`, `builtins2`, `regex`, `num`, `jsstr`, `ops` and
//! `prelude` here are links to ../cer/src (the Ranger compiler reads the
//! modules of one crate, not a crate's dependencies, so the two share
//! sources rather than a Cargo dependency).
//!
//! The front-end is CErXes's own, CEr's lexer and parser grown: `lexer`
//! reads tokens on demand (JSX needs the parser to switch to characters and
//! back), `parser` is CEr's parser with the TypeScript and JSX forms,
//! `types` the type grammar it skips, `ts` the TypeScript statements, `jsx`
//! the elements. `jsx_runtime` holds the default JSX runtime.
//!
//! Like CEr, a strict module: the same source builds with cargo and with
//! `rgrc` into the Ranger targets.

use ranger::prelude::*;

pub mod ast;
pub mod bigint;
pub mod builtins;
pub mod builtins2;
pub mod compiler;
pub mod coroutine;
pub mod dynamic;
pub mod jsstr;
pub mod jsx;
pub mod jsx_runtime;
pub mod lexer;
pub mod num;
pub mod ops;
pub mod parser;
pub mod prelude;
pub mod proxy;
pub mod regex;
pub mod ts;
pub mod typed;
pub mod types;
pub mod value;
pub mod vm;

use parser::Syntax;
use value::*;
use vm::*;

pub struct Engine {
    pub vm: Vm,
    /// the last script's error, empty when it ran to the end
    pub error: String,
    /// how `eval` reads a script; TypeScript and JSX (TSX) by default
    pub syntax: Syntax,
}

impl Engine {
    pub fn new() -> Engine {
        let mut vm = Vm::new();
        vm.setup();
        let mut e = Engine { vm: vm, error: String::new(), syntax: Syntax::new(false, false) };
        // the built-ins written in JavaScript: CEr's, then the JSX runtime
        e.eval(prelude::PRELUDE);
        if !e.error.is_empty() {
            e.vm.out.push(format!("prelude: {}", e.error));
        } else {
            e.eval(jsx_runtime::PRELUDE);
            if !e.error.is_empty() {
                e.vm.out.push(format!("jsx runtime: {}", e.error));
            } else {
                e.vm.out.clear();
            }
        }
        e.syntax = Syntax::tsx();
        e.vm.prelude_protos = e.vm.protos.len() as int;
        e.error = String::new();
        e
    }

    /// Reads scripts as TypeScript (`typescript`) and / or JSX (`jsx`).
    pub fn set_syntax(&mut self, typescript: bool, jsx: bool) {
        self.syntax = Syntax::new(typescript, jsx);
    }

    /// The syntax a file's extension asks for: `.ts` TypeScript, `.tsx`
    /// both, `.js` / `.jsx` / `.mjs` / `.cjs` JSX.
    pub fn set_syntax_for_path(&mut self, path: &str) {
        let p = path.to_lowercase();
        if p.ends_with(".ts") || p.ends_with(".mts") || p.ends_with(".cts") {
            self.set_syntax(true, false);
        } else if p.ends_with(".js") || p.ends_with(".jsx") || p.ends_with(".mjs") || p.ends_with(".cjs") {
            self.set_syntax(false, true);
        } else {
            self.set_syntax(true, true);
        }
    }

    /// Echo `print` / `console.log` lines to stdout as they come.
    pub fn set_echo(&mut self, on: bool) {
        self.vm.echo = on;
    }

    /// Runs a script; answers the value of its last expression statement
    /// as a string, or the uncaught exception (`Uncaught TypeError: …`).
    pub fn eval(&mut self, src: &str) -> String {
        self.error = String::new();
        let mut p = parser::Parser::with_syntax(src, self.syntax.clone());
        let root = p.parse_program();
        if !p.error.is_empty() {
            self.error = p.error.clone();
            return self.error.clone();
        }
        let atoms = self.vm.atoms.clone();
        let names = self.vm.atom_names.clone();
        let base = self.vm.protos.len() as int;
        let mut c = compiler::Compiler::new(p.ast, atoms, names, base);
        let entry = c.compile_program(root);
        if !c.error.is_empty() || entry < 0 {
            self.error = if c.error.is_empty() { String::from("SyntaxError") } else { c.error.clone() };
            return self.error.clone();
        }
        self.vm.atoms = c.atoms;
        self.vm.atom_names = c.atom_names;
        for pr in c.protos {
            self.vm.protos.push(pr);
        }
        let fp = self.vm.function_proto;
        let f = self.vm.alloc(C_FUNCTION, fp);
        self.vm.objs[f as usize].func = entry;
        let g = Val::Obj(self.vm.global);
        let r = self.vm.run_program(Val::Obj(f), g);
        if self.vm.throwing {
            return self.uncaught();
        }
        self.vm.run_jobs();
        if self.vm.throwing {
            return self.uncaught();
        }
        self.vm.display(&r)
    }

    /// Runs a script and answers its completion value with its type, as
    /// the conformance harness compares it: `n:<number>`, `s:<string>`,
    /// `b:true`, `u` (undefined), `l` (null), `o:<class>`, or
    /// `t:<exception>` when it threw.
    pub fn eval_typed(&mut self, src: &str) -> String {
        self.error = String::new();
        let mut p = parser::Parser::with_syntax(src, self.syntax.clone());
        let root = p.parse_program();
        if !p.error.is_empty() {
            return format!("t:{}", p.error);
        }
        let atoms = self.vm.atoms.clone();
        let names = self.vm.atom_names.clone();
        let base = self.vm.protos.len() as int;
        let mut c = compiler::Compiler::new(p.ast, atoms, names, base);
        let entry = c.compile_program(root);
        if !c.error.is_empty() || entry < 0 {
            return format!("t:{}", c.error);
        }
        self.vm.atoms = c.atoms;
        self.vm.atom_names = c.atom_names;
        for pr in c.protos {
            self.vm.protos.push(pr);
        }
        let fp = self.vm.function_proto;
        let f = self.vm.alloc(C_FUNCTION, fp);
        self.vm.objs[f as usize].func = entry;
        let g = Val::Obj(self.vm.global);
        let r = self.vm.run_program(Val::Obj(f), g);
        if self.vm.throwing {
            let u = self.uncaught();
            return format!("t:{}", u);
        }
        self.vm.run_jobs();
        match &r {
            Val::Undef => String::from("u"),
            Val::Null => String::from("l"),
            Val::Bool(b) => format!("b:{}", b),
            Val::Num(n) => {
                if *n == 0.0 && 1.0 / *n < 0.0 {
                    String::from("n:-0")
                } else {
                    format!("n:{}", num::number_to_string(*n))
                }
            }
            Val::Str(s) => format!("s:{}", s),
            Val::Obj(o) => format!("o:{}", self.vm.objs[*o as usize].class),
        }
    }

    fn uncaught(&mut self) -> String {
        let e = self.vm.exc.clone();
        self.vm.throwing = false;
        self.vm.exc = Val::Undef;
        self.vm.stack.clear();
        self.vm.frames.clear();
        self.vm.handlers.clear();
        self.vm.native_depth = 0;
        let s = self.vm.display(&e);
        self.error = format!("Uncaught {}", s);
        self.error.clone()
    }

    /// Calls a global function by name with no arguments; its result as a
    /// string.
    pub fn call(&mut self, name: &str) -> String {
        let a = self.vm.intern(name);
        let g = self.vm.global;
        let f = self.vm.get_obj(g, a, &Val::Obj(g));
        let r = self.vm.call_value(f, Val::Undef, Vec::new());
        if self.vm.throwing {
            return self.uncaught();
        }
        self.vm.run_jobs();
        self.vm.display(&r)
    }

    pub fn output_count(&self) -> int {
        self.vm.out.len() as int
    }

    pub fn output_at(&self, i: int) -> String {
        self.vm.out[i as usize].clone()
    }

    pub fn clear_output(&mut self) {
        self.vm.out = Vec::new();
    }

    /// The collected lines joined with newlines.
    pub fn output(&self) -> String {
        self.vm.out.join("\n")
    }

    pub fn gc_runs(&self) -> int {
        self.vm.gc_runs
    }

    pub fn heap_size(&self) -> int {
        (self.vm.objs.len() - self.vm.free_list.len()) as int
    }
}
