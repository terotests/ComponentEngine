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
//! the elements. `jsx_runtime` holds the default JSX runtime. `emit` writes
//! the tree back out as plain JavaScript, for an engine of the host's own.
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
pub mod emit;
pub mod intl;
pub mod intl_data;
pub mod jsstr;
pub mod jsx;
pub mod jsx_runtime;
pub mod lexer;
pub mod module;
pub mod normalize;
pub mod num;
pub mod ops;
pub mod parser;
pub mod prelude;
pub mod proxy;
pub mod regex;
pub mod ts;
pub mod typed;
pub mod uninorm;
pub mod uniprops;
pub mod unit;
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
    /// `eval` runs each script from its unit's text (unit.rs), as a host
    /// with a code cache does: the tests check the text loses nothing
    pub through_text: bool,
    /// how `eval` reads a script; TypeScript and JSX (TSX) by default
    pub syntax: Syntax,
}

impl Engine {
    pub fn new() -> Engine {
        let mut vm = Vm::new();
        vm.setup();
        let mut e = Engine { vm: vm, error: String::new(), through_text: false, syntax: Syntax::new(false, false) };
        // the built-ins written in JavaScript: CEr's, then the JSX runtime
        e.eval(prelude::PRELUDE);
        if e.error.is_empty() {
            e.eval(module::REGISTRY);
        }
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

    /// The syntax a file's extension asks for (`Syntax::for_path`).
    pub fn set_syntax_for_path(&mut self, path: &str) {
        self.syntax = parser::Syntax::for_path(path);
    }

    /// Echo `print` / `console.log` lines to stdout as they come.
    pub fn set_echo(&mut self, on: bool) {
        self.vm.echo = on;
    }

    /// Runs a script; answers the value of its last expression statement
    /// as a string, or the uncaught exception (`Uncaught TypeError: …`).
    pub fn eval(&mut self, src: &str) -> String {
        let u = self.unit_of(src);
        if !self.error.is_empty() {
            return self.error.clone();
        }
        self.run(u)
    }

    /// `compile`, and with `through_text` the unit as a host would keep
    /// and read it.
    fn unit_of(&mut self, src: &str) -> unit::Unit {
        let u = self.compile(src);
        if !self.error.is_empty() || !self.through_text {
            return u;
        }
        let t = unit::encode(&u, src);
        if t.is_empty() {
            return u;
        }
        let back = unit::decode(t.as_str(), src);
        if back.entry < 0 {
            self.error = String::from("internal: a unit's text did not read back");
        }
        back
    }

    /// A script parsed and compiled, not run: a unit (unit.rs) the host may
    /// keep as text (unit::encode) and run later (`run`), in this engine
    /// or another. Sets `error` when the script is not one.
    pub fn compile(&mut self, src: &str) -> unit::Unit {
        self.error = String::new();
        let mut p = parser::Parser::with_syntax(src, self.syntax.clone());
        let root = p.parse_program();
        if !p.error.is_empty() {
            self.error = p.error.clone();
            return unit::Unit::new();
        }
        let u = unit::compile(p.ast, root);
        if !u.error.is_empty() {
            self.error = u.error.clone();
        }
        u
    }

    /// Runs a compiled script, as `eval` runs one.
    pub fn run(&mut self, u: unit::Unit) -> String {
        self.error = String::new();
        if u.entry < 0 {
            self.error = String::from("SyntaxError");
            return self.error.clone();
        }
        let r = self.run_val(u);
        if self.vm.throwing {
            return self.uncaught();
        }
        self.vm.run_jobs();
        if self.vm.throwing {
            return self.uncaught();
        }
        self.vm.display(&r)
    }

    fn run_val(&mut self, u: unit::Unit) -> Val {
        let entry = unit::link(&mut self.vm, u);
        let fp = self.vm.function_proto;
        let f = self.vm.alloc(C_FUNCTION, fp);
        self.vm.objs[f as usize].func = entry;
        let g = Val::Obj(self.vm.global);
        self.vm.run_program(Val::Obj(f), g)
    }

    /// Runs a script and answers its completion value with its type, as
    /// the conformance harness compares it: `n:<number>`, `s:<string>`,
    /// `b:true`, `u` (undefined), `l` (null), `o:<class>`, or
    /// `t:<exception>` when it threw.
    pub fn eval_typed(&mut self, src: &str) -> String {
        let u = self.unit_of(src);
        if !self.error.is_empty() {
            return format!("t:{}", self.error);
        }
        let r = self.run_val(u);
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
