// SPDX-License-Identifier: AGPL-3.0-or-later
//! CErXes: CEr (../cer) with TypeScript and JSX.
//!
//! CEr reads JavaScript; CErXes reads TypeScript and JSX too and hands CEr
//! the same syntax tree, so its compiler, VM and built-ins run the result
//! as they run any script. The front-end is CEr's lexer and parser grown:
//! `lexer` reads tokens on demand (JSX needs the parser to switch to
//! characters and back), `parser` is CEr's parser with the TypeScript and
//! JSX forms, `types` the type grammar it skips, `ts` the TypeScript
//! statements, `jsx` the elements. `prelude` holds the default JSX runtime.
//!
//! CEr itself is untouched by any of it: a `.js` script run with CEr takes
//! CEr's own parser.

use ranger::prelude::*;

pub mod jsx;
pub mod lexer;
pub mod parser;
pub mod prelude;
pub mod ts;
pub mod types;

pub use parser::Syntax;

pub struct Engine {
    pub cer: cer::Engine,
    /// how `eval` reads a script; TypeScript and JSX (TSX) by default
    pub syntax: Syntax,
    pub error: String,
}

impl Engine {
    pub fn new() -> Engine {
        let mut c = cer::Engine::new();
        let r = c.eval(prelude::PRELUDE);
        if !c.error.is_empty() {
            c.vm.out.push(format!("cerxes prelude: {}", r));
        }
        // the JSX runtime reports [native code] like CEr's own prelude
        c.vm.prelude_protos = c.vm.protos.len() as int;
        c.error = String::new();
        Engine { cer: c, syntax: Syntax::tsx(), error: String::new() }
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

    pub fn set_echo(&mut self, on: bool) {
        self.cer.set_echo(on);
    }

    /// Runs a script; answers as `cer::Engine::eval`.
    pub fn eval(&mut self, src: &str) -> String {
        self.error = String::new();
        let mut p = parser::Parser::with_syntax(src, self.syntax.clone());
        let root = p.parse_program();
        if !p.error.is_empty() {
            self.error = p.error.clone();
            self.cer.error = p.error.clone();
            return self.error.clone();
        }
        let r = self.cer.run_ast(p.ast, root);
        self.error = self.cer.error.clone();
        r
    }

    /// Runs a script; answers as `cer::Engine::eval_typed`.
    pub fn eval_typed(&mut self, src: &str) -> String {
        self.error = String::new();
        let mut p = parser::Parser::with_syntax(src, self.syntax.clone());
        let root = p.parse_program();
        if !p.error.is_empty() {
            return format!("t:{}", p.error);
        }
        self.cer.run_ast_typed(p.ast, root)
    }

    pub fn call(&mut self, name: &str) -> String {
        self.cer.call(name)
    }

    /// The collected `print` / `console.log` lines joined with newlines.
    pub fn output(&self) -> String {
        self.cer.output()
    }

    pub fn clear_output(&mut self) {
        self.cer.clear_output();
    }
}
