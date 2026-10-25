// SPDX-License-Identifier: AGPL-3.0-or-later
//! CEr: the JavaScript evaluator of ComponentEngine
//! (../engine) written again as a strict Rust module (Ranger's
//! docs/plans/PLAN_RUST_SYNTAX.md), beside it.
//!
//! An experiment in the manner of lib/evgr: one source, built by cargo into
//! native code and by `rgrc` into every Ranger target, measured against the
//! Ranger engine on the same scripts (bench/).
//!
//! The pipeline: `lexer` → `parser` (a tree in an arena) → `compiler`
//! (scopes, closures captured into scope objects, bytecode for a stack
//! machine) → `vm` (objects in an arena with a mark-and-sweep collector,
//! inline caches for property reads and writes). `builtins` / `builtins2`
//! hold the standard library, `regex` a backtracking regular expression
//! engine.

use ranger::prelude::*;

pub mod ast;
pub mod bigint;
pub mod builtins;
pub mod builtins2;
pub mod compiler;
pub mod coroutine;
pub mod dynamic;
pub mod intl;
pub mod intl_data;
pub mod jsstr;
pub mod lexer;
pub mod normalize;
pub mod num;
pub mod ops;
pub mod parser;
pub mod prelude;
pub mod proxy;
pub mod raster;
pub mod rasterjs;
pub mod regex;
pub mod snapshot;
pub mod typed;
pub mod uninorm;
pub mod uniprops;
pub mod unit;
pub mod value;
pub mod vm;

use value::*;
use vm::*;

pub struct Engine {
    pub vm: Vm,
    /// the last script's error, empty when it ran to the end
    pub error: String,
    /// `eval` runs each script from its unit's text (unit.rs), as a host
    /// with a code cache does: the tests check the text loses nothing
    pub through_text: bool,
}

impl Engine {
    pub fn new() -> Engine {
        let mut vm = Vm::new();
        vm.setup();
        let mut e = Engine { vm: vm, error: String::new(), through_text: false };
        // the built-ins written in JavaScript
        e.eval(prelude::PRELUDE);
        if !e.error.is_empty() {
            e.vm.out.push(format!("prelude: {}", e.error));
        } else {
            e.vm.out.clear();
        }
        e.vm.prelude_protos = e.vm.protos.len() as int;
        e.error = String::new();
        e
    }

    /// The engine's whole state as text (snapshot.rs), for `from_snapshot`;
    /// "" while a script runs.
    pub fn snapshot(&self) -> String {
        snapshot::snapshot(&self.vm)
    }

    /// An engine in the state a snapshot's text holds, as if the scripts
    /// run before it was taken had run in it; when the text is not a
    /// snapshot, a new engine with `error` set.
    pub fn from_snapshot(text: &str) -> Engine {
        let mut vm = Vm::new();
        if !snapshot::restore(&mut vm, text) {
            let mut e = Engine::new();
            e.error = String::from("not a snapshot");
            return e;
        }
        Engine { vm: vm, error: String::new(), through_text: false }
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
        let mut p = parser::Parser::new(src);
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
