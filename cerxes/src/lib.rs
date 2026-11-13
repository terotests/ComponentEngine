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
pub mod esm;
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
pub mod pcomp;
pub mod prelude;
pub mod proxy;
pub mod regex;
pub mod regs;
pub mod rvm;
pub mod snapshot;
pub mod ts;
pub mod typed;
pub mod uninorm;
pub mod uniprops;
pub mod unit;
pub mod types;
pub mod value;
pub mod vm;
/// a HashMap with a cheaper hash for the Map / Set index (fxmap.rs):
/// Rust only, the other targets keep their maps
#[ranger::target(rust)]
pub mod fxmap {
    include!("fxmap.rs");
}
/// an object's elements with an O(1) shift (evec.rs): Rust only, the other
/// targets keep their arrays
#[ranger::target(rust)]
pub mod evec {
    include!("evec.rs");
}
/// an object's property values past slot 1 as a thin vector (svec.rs):
/// Rust only, the other targets keep their arrays
#[ranger::target(rust)]
pub mod svec {
    include!("svec.rs");
}

/// the baseline JIT (wjit.rs): Rust only
#[ranger::target(rust)]
pub mod wjit {
    include!("wjit.rs");
}

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
    /// An engine; its JavaScript compiles with pcomp.rs (`new_with`).
    pub fn new() -> Engine {
        Engine::new_with(true)
    }

    /// An engine whose JavaScript (the prelude too) compiles with pcomp.rs,
    /// without a syntax tree (`pcomp`); TypeScript and JSX go through the
    /// parser either way.
    pub fn new_with(pcomp: bool) -> Engine {
        if pcomp {
            // the prelude as compiled when the crate was built (Rust), not
            // compiled again: a third of the startup peak
            let mut vm = Vm::new();
            if prelude_snapshot(&mut vm) {
                vm.pcomp = true;
                return Engine { vm: vm, error: String::new(), through_text: false, syntax: Syntax::tsx() };
            }
        }
        Engine::compiled(pcomp)
    }

    /// An engine whose prelude is compiled now, not read from
    /// `prelude.snap`; `cerxes --prelude-snapshot` writes that file from
    /// its snapshot.
    pub fn compiled(pcomp: bool) -> Engine {
        let mut vm = Vm::new();
        vm.pcomp = pcomp;
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
        Engine { vm: vm, error: String::new(), through_text: false, syntax: Syntax::tsx() }
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
        let mut pcomp_error = String::new();
        if self.vm.pcomp && !self.syntax.typescript && !self.syntax.jsx {
            // JavaScript, its `import`s the host's modules; what pcomp does
            // not read goes to the parser
            let u = pcomp::compile_script(src, false, false, "", "");
            if u.error.is_empty() {
                return u;
            }
            pcomp_error = u.error.clone();
        }
        if self.vm.pcomp && (self.syntax.typescript || self.syntax.jsx) {
            // TypeScript and JSX: pcomp skips the types and the lexer
            // writes each element as its factory's call; what pcomp does
            // not read (decorators …) goes to the parser
            let mut sx = self.syntax.clone();
            crate::jsx::read_pragmas(src, &mut sx);
            let u = pcomp::compile_script(src, sx.typescript, sx.jsx, sx.jsx_factory.as_str(), sx.jsx_fragment.as_str());
            if u.error.is_empty() {
                return u;
            }
            if !sx.typescript {
                // JavaScript that has no JSX in it: pcomp's error stands
                // (the parser does not check every early error)
                pcomp_error = u.error.clone();
            }
        }
        if self.vm.pcomp {
            // TypeScript, JSX or a module: the parser writes it as
            // JavaScript, which pcomp compiles; the parser and its tree are
            // gone by then
            let (js, error) = parser::script_js(src, self.syntax.clone(), pcomp_error.as_str());
            if !error.is_empty() {
                self.error = error;
                return unit::Unit::new();
            }
            let u = pcomp::compile_unit(js.as_str());
            if !u.error.is_empty() {
                self.error = u.error.clone();
            }
            return u;
        }
        let mut p = parser::Parser::with_syntax(src, self.syntax.clone());
        let root = p.parse_program();
        if p.error.is_empty() && !pcomp_error.is_empty() && !p.beyond_js {
            // plain JavaScript pcomp rejected: its early errors stand
            p.error = pcomp_error;
        }
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
        let began = self.vm.begin_call(true);
        let r = self.run_val(u);
        if self.vm.suspended {
            self.vm.end_call(began);
            return String::new();
        }
        let out = self.finish_run(r);
        self.vm.end_call(began);
        out
    }

    /// What `run` does once the script has returned: its jobs, then its
    /// completion value or the uncaught exception.
    fn finish_run(&mut self, r: Val) -> String {
        if self.vm.terminated && !self.vm.throwing {
            // a native dropped the time limit's exception: it stands
            self.vm.terminate();
        }
        if self.vm.throwing {
            return self.uncaught();
        }
        // the completion value lives through the jobs: a reclaim there
        // must not free it; a job stopped by its slice keeps it with it
        self.vm.temp_roots.push(r);
        self.vm.run_result = r;
        self.vm.job_slices = true;
        self.vm.run_jobs();
        self.vm.job_slices = false;
        self.vm.run_result = Val::Undef;
        self.vm.temp_roots.pop();
        if self.vm.suspended {
            return String::new();
        }
        if self.vm.terminated && !self.vm.throwing {
            self.vm.terminate();
        }
        if self.vm.throwing {
            return self.uncaught();
        }
        self.vm.display(&r)
    }

    /// The longest one call into the engine (`run`, `eval`, `call`, one
    /// `resume`, the host's Vm::call_top / run_jobs) may run, in ms,
    /// compile time not counted; 0: no limit (the default). Past it the
    /// script stops wherever it is, under native code too (a getter, a
    /// sort's comparator, a promise job, a long regex or BigInt
    /// operation): the call answers `Uncaught Error: script time limit
    /// exceeded (… ms)`, no catch or finally runs, and `timed_out` is true
    /// until the next call. Promise jobs the script queued stay queued.
    /// With slices on (`set_slice`), it bounds how long a slice can
    /// overrun where it cannot stop (under native code).
    pub fn set_time_limit(&mut self, ms: double) {
        self.vm.set_time_limit(ms);
    }

    /// The last call was stopped by the time limit (`set_time_limit`).
    pub fn timed_out(&self) -> bool {
        self.vm.terminated
    }

    /// Scripts run in slices of `ms` milliseconds: when one runs out at a
    /// branch, call or return, `run` / `eval` return "" with `is_suspended`
    /// true, and `resume` goes on from there (a snapshot taken meanwhile
    /// goes on in another engine too). 0: no slices (the default).
    pub fn set_slice(&mut self, ms: double) {
        self.vm.slice_ms = ms;
    }

    /// Limits a script's depth: at most `frames` script frames at once
    /// (10 000 by default) and `stack_bytes` of native stack below the call
    /// into the engine (1 MB, 512 KB in WebAssembly; keep it well under the
    /// thread's stack). Past either a call throws RangeError ("Maximum call
    /// stack size exceeded") instead of exhausting memory or overflowing
    /// the thread's stack.
    pub fn set_depth_limits(&mut self, frames: int, stack_bytes: int) {
        self.vm.max_frames = frames;
        self.vm.stack_budget = stack_bytes;
    }

    /// The script stopped when its slice ran out (`set_slice`).
    pub fn is_suspended(&self) -> bool {
        self.vm.suspended
    }

    /// Goes on with a suspended script for another slice; answers as `run`
    /// does, "" with `is_suspended` true when this slice runs out too.
    pub fn resume(&mut self) -> String {
        if !self.vm.suspended {
            return String::new();
        }
        self.vm.suspended = false;
        self.vm.stack_base = vm::native_sp();
        let began = self.vm.begin_call(true);
        if !self.vm.regs || self.vm.translate_frames() {
            self.vm.run(0);
        }
        if self.vm.suspended {
            self.vm.end_call(began);
            return String::new();
        }
        let r = if self.vm.job_stopped() && !self.vm.terminated {
            // a promise job's frames ran out: the job ends, the rest of
            // the queue runs, and the script's own value is the answer
            self.vm.job_resume()
        } else {
            if self.vm.throwing {
                self.vm.end_call(began);
                return self.uncaught();
            }
            match self.vm.stack.pop() {
                Some(v) => v,
                None => Val::Undef,
            }
        };
        let out = self.finish_run(r);
        self.vm.end_call(began);
        out
    }

    fn run_val(&mut self, u: unit::Unit) -> Val {
        self.vm.stack_base = vm::native_sp();
        let entry = unit::link(&mut self.vm, u);
        let fp = self.vm.function_proto;
        let f = self.vm.alloc(C_FUNCTION, fp);
        self.vm.objs[f as usize].func = entry as i32;
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
        let began = self.vm.begin_call(false);
        let r = self.run_val(u);
        if self.vm.throwing {
            self.vm.end_call(began);
            let u = self.uncaught();
            return format!("t:{}", u);
        }
        self.vm.temp_roots.push(r);
        self.vm.run_jobs();
        self.vm.temp_roots.pop();
        self.vm.end_call(began);
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
            Val::Str(s) => format!("s:{}", self.vm.sref(*s)),
            Val::Obj(o) => format!("o:{}", self.vm.objs[*o as usize].class),
            Val::Big(_) => format!("o:{}", C_BIGINT),
        }
    }

    fn uncaught(&mut self) -> String {
        if self.vm.terminated {
            // no script runs past the time limit, a toString neither
            self.vm.exc = Val::Undef;
            self.vm.throwing = false;
            self.vm.stack.clear();
            self.vm.frames.clear();
            self.vm.handlers.clear();
            self.vm.native_depth = 0;
            self.error = format!("Uncaught Error: script time limit exceeded ({} ms)", self.vm.time_limit);
            return self.error.clone();
        }
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
        self.vm.stack_base = vm::native_sp();
        let a = self.vm.intern(name);
        let g = self.vm.global;
        let f = self.vm.get_obj(g, a, &Val::Obj(g));
        let began = self.vm.begin_call(false);
        let r = self.vm.call_top(f, Val::Undef, Vec::new());
        if self.vm.throwing {
            self.vm.end_call(began);
            return self.uncaught();
        }
        // the jobs' frames are safe points: the result is held through them
        self.vm.temp_roots.push(r.clone());
        self.vm.run_jobs();
        self.vm.temp_roots.pop();
        self.vm.end_call(began);
        if self.vm.terminated {
            if !self.vm.throwing {
                self.vm.terminate();
            }
            return self.uncaught();
        }
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

/// Rust: the engine's state after the prelude, from `src/prelude_snap.rs`
/// (a raw string written by `cerxes --prelude-snapshot`, checked by
/// tests/syntax.rs; a .rs file so builds that copy src/*.rs get it, and
/// not a module, so other targets do not carry it); false when it does
/// not restore, and the prelude is then compiled.
#[ranger::target(rust)]
pub fn prelude_snapshot(vm: &mut Vm) -> bool {
    snapshot::restore_built_in(vm, include!("prelude_snap.rs"))
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn prelude_snapshot(_vm: &mut Vm) -> bool {
    false
}

/// Rust: a snapshot's text as the raw string literal `src/prelude_snap.rs`
/// holds (enough `#` that the text cannot end it).
#[ranger::target(rust)]
pub fn prelude_snapshot_rs(text: &str) -> String {
    let mut hashes = String::from("#");
    while text.contains(format!("\"{}", hashes).as_str()) {
        hashes.push('#');
    }
    format!("// written by `cerxes --prelude-snapshot`; see lib.rs prelude_snapshot\nr{}\"{}\"{}\n", hashes, text, hashes)
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn prelude_snapshot_rs(_text: &str) -> String {
    String::new()
}
