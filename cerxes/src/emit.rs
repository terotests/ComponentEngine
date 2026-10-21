// SPDX-License-Identifier: AGPL-3.0-or-later
//! TypeScript and JSX to JavaScript: the tree CErXes's parser builds,
//! written back out as a script any JavaScript engine runs the way CErXes
//! runs the source.
//!
//! The parser has already done the work: types are gone, JSX elements are
//! `__jsx(…)` calls, `enum` and `namespace` are the objects TypeScript
//! emits, parameter properties are `this.x = x`, and a value `import` is a
//! `const` read from `__cerxesImport("m")`. What is left is printing the
//! tree, with the parentheses its operators need.
//!
//! The output keeps the source's lines: a statement starts on the line it
//! started on in the source (when it can; several on one line stay on one
//! line), so an error's line number in the script points at the `.tsx`.
//!
//! The script it writes expects what CErXes gives every script: `__jsx`,
//! `__jsxFragment`, `renderToString` and the host's modules
//! (`defineModule`, `defineSourceModule`, `__cerxesImport`). `runtime()` is
//! that, as JavaScript, to run once before the scripts; `emit_module_js`
//! writes a module the host gives as source.
//!
//! Class fields are written as fields again: the parser turns them into a
//! method run on each new instance and one run once on the class (`c` and
//! `d` of the class node), whose statements are `this.x define v`.

use ranger::prelude::*;

use crate::ast::*;
use crate::parser::Parser;
use crate::parser::Syntax;

// how tightly an expression binds: an operand of an operator that binds
// tighter than the operand is put in parentheses
const P_SEQ: int = 1;
const P_ASSIGN: int = 2;
const P_COND: int = 3;
const P_UNARY: int = 17;
const P_POSTFIX: int = 18;
const P_CALL: int = 19;
const P_PRIMARY: int = 20;

/// The host functions an emitted script calls, as JavaScript: the host's
/// modules, as CErXes defines them (crate::module::REGISTRY). A module given
/// as source there is the function `emit_module_js` wrote.
pub const MODULES: &str = crate::module::REGISTRY;

/// `MODULES` and the JSX runtime: what an emitted script needs defined
/// before it runs. The JSX runtime defines its globals on `this`, which is
/// the global object only in a script's top level: it is called with
/// `globalThis` so the text runs anywhere (a module, Node's CommonJS).
pub fn runtime() -> String {
    format!("(function () {{\n{}\n}}).call(globalThis);\n{}\n", crate::jsx_runtime::PRELUDE, MODULES)
}

/// The JavaScript for `src` read as `syntax`, or the syntax error
/// (`SyntaxError: … (line n)`) as `error`.
pub struct Emitted {
    pub js: String,
    pub error: String,
}

/// `src` as JavaScript; `syntax` as `Engine::set_syntax` takes it.
pub fn emit_js(src: &str, syntax: Syntax) -> Emitted {
    let mut p = Parser::with_syntax(src, syntax);
    let root = p.parse_program();
    if !p.error.is_empty() {
        return Emitted { js: String::new(), error: p.error.clone() };
    }
    let mut e = Emitter::new(p.ast);
    e.program(root);
    Emitted { js: e.out, error: String::new() }
}

/// `src` as JavaScript, read with the syntax a file's extension asks for
/// (as `Engine::set_syntax_for_path`).
pub fn emit_js_for_path(src: &str, path: &str) -> Emitted {
    emit_js(src, Syntax::for_path(path))
}

/// A module the host gives as source (crate::module), as the JavaScript
/// function that runs it and returns its exports: what an emitted host
/// passes `defineSourceModule` as the source.
pub fn emit_module_js(src: &str, path: &str) -> Emitted {
    let m = crate::module::module_text(src, path);
    if !m.error.is_empty() {
        return Emitted { js: String::new(), error: m.error };
    }
    let mut p = crate::module::module_parser(m.text.as_str(), path);
    let root = p.parse_program();
    if !p.error.is_empty() {
        return Emitted { js: String::new(), error: p.error.clone() };
    }
    let mut e = Emitter::new(p.ast);
    e.program(root);
    Emitted { js: e.out, error: String::new() }
}

pub struct Emitter {
    pub ast: Ast,
    pub out: String,
    /// the line the output is on, from 1
    pub line: int,
    /// inside a template's substitution, whose tree has lines of its own:
    /// no lines are added there
    pub no_lines: int,
}

/// A name that may stand after `.` or as a property key unquoted.
fn is_ident_name(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut first = true;
    for c in s.chars() {
        if first {
            if !crate::lexer::is_id_start(c) {
                return false;
            }
            first = false;
        } else if !crate::lexer::is_id_part(c) {
            return false;
        }
    }
    true
}

/// A string literal for `s`, in double quotes.
pub fn quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        let cp = c as u32;
        if c == '"' {
            out.push_str("\\\"");
        } else if c == '\\' {
            out.push_str("\\\\");
        } else if c == '\n' {
            out.push_str("\\n");
        } else if c == '\r' {
            out.push_str("\\r");
        } else if c == '\t' {
            out.push_str("\\t");
        } else if cp < 0x20 || cp == 0x7f || cp == 0x2028 || cp == 0x2029 {
            out.push_str(format!("\\u{:04x}", cp).as_str());
        } else {
            out.push(c);
        }
    }
    out.push('"');
    out
}

/// A private name as the parser keeps it (`\u{1}#x`: a character 1 that
/// no source name holds, then the name), as `#x`; else empty.
fn private_name(s: &str) -> String {
    let mut out = String::new();
    let mut i: int = 0;
    for c in s.chars() {
        if i == 0 && (c as u32) != 1 {
            return String::new();
        }
        if i == 1 && c != '#' {
            return String::new();
        }
        if i > 0 {
            out.push(c);
        }
        i += 1;
    }
    out
}

fn binary_prec(op: &str) -> int {
    if op == "??" {
        return 5;
    }
    if op == "||" {
        return 6;
    }
    if op == "&&" {
        return 7;
    }
    if op == "|" {
        return 8;
    }
    if op == "^" {
        return 9;
    }
    if op == "&" {
        return 10;
    }
    if op == "==" || op == "!=" || op == "===" || op == "!==" {
        return 11;
    }
    if op == "<" || op == ">" || op == "<=" || op == ">=" || op == "instanceof" || op == "in" {
        return 12;
    }
    if op == "<<" || op == ">>" || op == ">>>" {
        return 13;
    }
    if op == "+" || op == "-" {
        return 14;
    }
    if op == "*" || op == "/" || op == "%" {
        return 15;
    }
    16
}

impl Emitter {
    pub fn new(ast: Ast) -> Emitter {
        Emitter { ast: ast, out: String::new(), line: 1, no_lines: 0 }
    }

    // ---- output

    fn put(&mut self, s: &str) {
        for c in s.chars() {
            if c == '\n' {
                self.line += 1;
            }
        }
        self.out.push_str(s);
    }

    /// Moves the output down to `line` (a node's line in the source), or
    /// puts a space between two statements on one line.
    fn at_line(&mut self, line: int) {
        if self.no_lines > 0 || line <= 0 {
            self.space();
            return;
        }
        if self.line >= line {
            self.space();
            return;
        }
        while self.line < line {
            self.out.push('\n');
            self.line += 1;
        }
    }

    fn space(&mut self) {
        if !self.out.is_empty() && !self.out.ends_with("\n") && !self.out.ends_with(" ") && !self.out.ends_with("(") {
            self.out.push(' ');
        }
    }

    fn kind(&self, n: int) -> int {
        self.ast.kind(n)
    }

    fn op(&self, n: int) -> String {
        self.ast.nodes[n as usize].op.clone()
    }

    fn s(&self, n: int) -> String {
        self.ast.nodes[n as usize].s.clone()
    }

    // ---- statements

    pub fn program(&mut self, root: int) {
        let body = self.ast.nodes[root as usize].list.clone();
        for st in body.iter() {
            self.statement(*st);
        }
        if !self.out.ends_with("\n") {
            self.out.push('\n');
        }
    }

    fn statements(&mut self, list: &Vec<int>) {
        for st in list.iter() {
            self.statement(*st);
        }
    }

    fn block(&mut self, n: int) {
        let list = self.ast.nodes[n as usize].list.clone();
        self.put("{");
        self.statements(&list);
        self.space();
        self.put("}");
    }

    /// A statement as the body of `if`, a loop or a label.
    fn body(&mut self, n: int) {
        if self.kind(n) == N_BLOCK {
            self.space();
            self.block(n);
        } else {
            self.statement(n);
        }
    }

    pub fn statement(&mut self, n: int) {
        let k = self.kind(n);
        let line = self.ast.nodes[n as usize].line;
        self.at_line(line);
        let a = self.ast.nodes[n as usize].a;
        let b = self.ast.nodes[n as usize].b;
        let c = self.ast.nodes[n as usize].c;
        let d = self.ast.nodes[n as usize].d;
        if k == N_EXPR {
            self.expr_statement(a);
            self.put(";");
        } else if k == N_VAR {
            self.var_decl(n);
            self.put(";");
        } else if k == N_FUNC {
            self.function(n);
        } else if k == N_CLASS {
            self.class(n);
        } else if k == N_BLOCK {
            self.block(n);
        } else if k == N_EMPTY {
            self.put(";");
        } else if k == N_IF {
            self.put("if (");
            self.expr(a, P_SEQ);
            self.put(")");
            // `if (a) if (b) x; else y` with the else on the outer if
            let dangling = c >= 0 && self.ends_in_open_if(b);
            if dangling {
                self.put(" {");
                self.statement(b);
                self.put(" }");
            } else {
                self.body(b);
            }
            if c >= 0 {
                self.put(" else");
                if self.kind(c) == N_BLOCK {
                    self.put(" ");
                    self.block(c);
                } else {
                    self.put(" ");
                    self.statement(c);
                }
            }
        } else if k == N_FOR {
            self.put("for (");
            if a >= 0 {
                if self.kind(a) == N_VAR {
                    self.var_decl(a);
                } else {
                    let e = self.ast.nodes[a as usize].a;
                    // `in` would end the head: the whole init in parentheses
                    self.put("(");
                    self.expr(e, P_SEQ);
                    self.put(")");
                }
            }
            self.put("; ");
            if b >= 0 {
                self.expr(b, P_SEQ);
            }
            self.put("; ");
            if c >= 0 {
                self.expr(c, P_SEQ);
            }
            self.put(")");
            self.body(d);
        } else if k == N_FORIN {
            self.put("for (");
            if self.kind(a) == N_VAR {
                self.var_decl(a);
            } else {
                self.expr(a, P_CALL);
            }
            let o = self.op(n);
            self.put(" ");
            self.put(o.as_str());
            self.put(" ");
            if o.as_str() == "of" {
                self.expr(b, P_ASSIGN);
            } else {
                self.expr(b, P_SEQ);
            }
            self.put(")");
            self.body(c);
        } else if k == N_WHILE {
            self.put("while (");
            self.expr(a, P_SEQ);
            self.put(")");
            self.body(b);
        } else if k == N_DOWHILE {
            self.put("do");
            if self.kind(a) == N_BLOCK {
                self.put(" ");
                self.block(a);
            } else {
                self.put(" ");
                self.statement(a);
            }
            self.put(" while (");
            self.expr(b, P_SEQ);
            self.put(");");
        } else if k == N_RETURN {
            self.put("return");
            if a >= 0 {
                self.put(" ");
                self.expr(a, P_SEQ);
            }
            self.put(";");
        } else if k == N_THROW {
            self.put("throw ");
            self.expr(a, P_SEQ);
            self.put(";");
        } else if k == N_BREAK || k == N_CONTINUE {
            self.put(if k == N_BREAK { "break" } else { "continue" });
            let label = self.s(n);
            if !label.is_empty() {
                self.put(" ");
                self.put(label.as_str());
            }
            self.put(";");
        } else if k == N_TRY {
            self.put("try ");
            self.block(a);
            if c >= 0 {
                self.put(" catch");
                if b >= 0 {
                    self.put(" (");
                    self.expr(b, P_ASSIGN);
                    self.put(")");
                }
                self.put(" ");
                self.block(c);
            }
            if d >= 0 {
                self.put(" finally ");
                self.block(d);
            }
        } else if k == N_SWITCH {
            self.put("switch (");
            self.expr(a, P_SEQ);
            self.put(") {");
            let cases = self.ast.nodes[n as usize].list.clone();
            for cs in cases.iter() {
                let cl = self.ast.nodes[*cs as usize].line;
                self.at_line(cl);
                let t = self.ast.nodes[*cs as usize].a;
                if t >= 0 {
                    self.put("case ");
                    self.expr(t, P_SEQ);
                    self.put(":");
                } else {
                    self.put("default:");
                }
                let body = self.ast.nodes[*cs as usize].list.clone();
                self.statements(&body);
            }
            self.space();
            self.put("}");
        } else if k == N_LABELED {
            let label = self.s(n);
            self.put(label.as_str());
            self.put(":");
            self.body(a);
        } else if k == N_DEBUGGER {
            self.put("debugger;");
        } else if k == N_WITH {
            self.put("with (");
            self.expr(a, P_SEQ);
            self.put(")");
            self.body(b);
        } else {
            // an expression where a statement stands
            self.expr_statement(n);
            self.put(";");
        }
    }

    /// Whether `if (…) x` written before an `else` would take it: an `if`
    /// without an `else` at the end of the statement.
    fn ends_in_open_if(&self, n: int) -> bool {
        let k = self.kind(n);
        if k == N_IF {
            let c = self.ast.nodes[n as usize].c;
            if c < 0 {
                return true;
            }
            return self.ends_in_open_if(c);
        }
        if k == N_FOR {
            return self.ends_in_open_if(self.ast.nodes[n as usize].d);
        }
        if k == N_FORIN {
            return self.ends_in_open_if(self.ast.nodes[n as usize].c);
        }
        if k == N_WHILE || k == N_WITH {
            return self.ends_in_open_if(self.ast.nodes[n as usize].b);
        }
        if k == N_LABELED {
            return self.ends_in_open_if(self.ast.nodes[n as usize].a);
        }
        false
    }

    /// An expression statement: in parentheses when it would start with
    /// `{`, `function`, `class` or `let [`, which a statement reads as
    /// something else.
    fn expr_statement(&mut self, e: int) {
        if self.starts_ambiguous(e) {
            self.put("(");
            self.expr(e, P_SEQ);
            self.put(")");
        } else {
            self.expr(e, P_SEQ);
        }
    }

    /// The expression's first token, as it is written out, is `{`,
    /// `function`, `async function`, `class` or `let`.
    fn starts_ambiguous(&self, e: int) -> bool {
        let k = self.kind(e);
        if k == N_OBJECT || k == N_CLASS {
            return true;
        }
        if k == N_FUNC {
            return (self.ast.nodes[e as usize].flags & F_ARROW) == 0;
        }
        if k == N_IDENT {
            return self.s(e).as_str() == "let";
        }
        let a = self.ast.nodes[e as usize].a;
        if k == N_CALL || k == N_MEMBER || k == N_INDEX || k == N_TAGGED || k == N_OPT_CHAIN || k == N_ASSIGN || k == N_COND || k == N_BINARY || k == N_LOGICAL {
            if self.needs_parens(e, a) {
                return false;
            }
            return self.starts_ambiguous(a);
        }
        if k == N_UPDATE && (self.ast.nodes[e as usize].flags & 1) == 0 {
            return self.starts_ambiguous(a);
        }
        if k == N_SEQ {
            let first = self.ast.nodes[e as usize].list[0];
            return self.starts_ambiguous(first);
        }
        false
    }

    /// Whether the first operand `a` of `e` is written in parentheses.
    fn needs_parens(&self, e: int, a: int) -> bool {
        let k = self.kind(e);
        if k == N_CALL || k == N_MEMBER || k == N_INDEX || k == N_TAGGED {
            return self.object_parens(a);
        }
        if k == N_OPT_CHAIN {
            return false;
        }
        if k == N_ASSIGN {
            return false;
        }
        if k == N_COND {
            return self.prec(a) < P_COND + 1;
        }
        if k == N_BINARY || k == N_LOGICAL {
            let o = self.op(e);
            let p = binary_prec(o.as_str());
            if o.as_str() == "**" {
                return self.prec(a) < P_POSTFIX;
            }
            return self.prec(a) < p || self.mixes_coalesce(e, a);
        }
        false
    }

    fn var_decl(&mut self, n: int) {
        let kw = self.op(n);
        self.put(kw.as_str());
        self.put(" ");
        let decls = self.ast.nodes[n as usize].list.clone();
        let mut first = true;
        for d in decls.iter() {
            if !first {
                self.put(", ");
            }
            first = false;
            let t = self.ast.nodes[*d as usize].a;
            let init = self.ast.nodes[*d as usize].b;
            self.expr(t, P_ASSIGN);
            if init >= 0 {
                self.put(" = ");
                self.expr(init, P_ASSIGN);
            }
        }
    }

    // ---- functions and classes

    fn params(&mut self, f: int) {
        let params = self.ast.nodes[f as usize].list.clone();
        self.put("(");
        let mut first = true;
        for p in params.iter() {
            if !first {
                self.put(", ");
            }
            first = false;
            self.expr(*p, P_ASSIGN);
        }
        self.put(")");
    }

    /// The parameters and body of a function or method.
    fn function_rest(&mut self, f: int) {
        self.params(f);
        self.put(" ");
        let body = self.ast.nodes[f as usize].a;
        if body >= 0 {
            self.block(body);
        } else {
            self.put("{}");
        }
    }

    fn function(&mut self, f: int) {
        let flags = self.ast.nodes[f as usize].flags;
        if (flags & F_ASYNC) != 0 {
            self.put("async ");
        }
        if (flags & F_ARROW) != 0 {
            self.params(f);
            self.put(" => ");
            let body = self.ast.nodes[f as usize].a;
            if (flags & F_EXPR_BODY) != 0 {
                if self.leads_with_object(body) {
                    self.put("(");
                    self.expr(body, P_SEQ);
                    self.put(")");
                } else {
                    self.expr(body, P_ASSIGN);
                }
            } else {
                self.block(body);
            }
            return;
        }
        self.put("function");
        if (flags & F_GENERATOR) != 0 {
            self.put("*");
        }
        let name = self.s(f);
        if !name.is_empty() {
            self.put(" ");
            self.put(name.as_str());
        }
        self.function_rest(f);
    }

    /// An arrow's expression body that would start with `{` (read as a
    /// block): an object literal first.
    fn leads_with_object(&self, e: int) -> bool {
        let k = self.kind(e);
        if k == N_OBJECT {
            return true;
        }
        let a = self.ast.nodes[e as usize].a;
        if k == N_CALL || k == N_MEMBER || k == N_INDEX || k == N_TAGGED || k == N_OPT_CHAIN || k == N_ASSIGN || k == N_COND || k == N_BINARY || k == N_LOGICAL {
            if self.needs_parens(e, a) {
                return false;
            }
            return self.leads_with_object(a);
        }
        if k == N_UPDATE && (self.ast.nodes[e as usize].flags & 1) == 0 {
            return self.leads_with_object(a);
        }
        if k == N_SEQ {
            return true;
        }
        false
    }

    /// A property key: a name, a string, a number, `[computed]` or `#private`.
    fn key(&mut self, key: int, computed: bool) {
        if computed {
            self.put("[");
            self.expr(key, P_ASSIGN);
            self.put("]");
            return;
        }
        if self.kind(key) == N_NUM {
            let v = self.ast.nodes[key as usize].num;
            self.put(crate::num::number_to_string(v).as_str());
            return;
        }
        let s = self.s(key);
        let private = private_name(s.as_str());
        if !private.is_empty() {
            self.put(private.as_str());
        } else if is_ident_name(s.as_str()) {
            self.put(s.as_str());
        } else {
            self.put(quote(s.as_str()).as_str());
        }
    }

    /// A method of a class or an object literal: `key(…) {…}`, with its
    /// `static`, `get` / `set`, `async` and `*`.
    fn method(&mut self, prop: int) {
        let pflags = self.ast.nodes[prop as usize].flags;
        let key = self.ast.nodes[prop as usize].a;
        let f = self.ast.nodes[prop as usize].b;
        let fflags = self.ast.nodes[f as usize].flags;
        if (pflags & F_STATIC) != 0 {
            self.put("static ");
        }
        if (pflags & F_GETTER) != 0 {
            self.put("get ");
        } else if (pflags & F_SETTER) != 0 {
            self.put("set ");
        }
        if (fflags & F_ASYNC) != 0 {
            self.put("async ");
        }
        if (fflags & F_GENERATOR) != 0 {
            self.put("*");
        }
        self.key(key, (pflags & F_COMPUTED) != 0);
        self.function_rest(f);
    }

    fn class(&mut self, n: int) {
        let name = self.s(n);
        let sup = self.ast.nodes[n as usize].a;
        let ctor = self.ast.nodes[n as usize].b;
        let inst = self.ast.nodes[n as usize].c;
        let stat = self.ast.nodes[n as usize].d;
        self.put("class");
        if !name.is_empty() {
            self.put(" ");
            self.put(name.as_str());
        }
        if sup >= 0 {
            self.put(" extends ");
            self.expr(sup, P_CALL);
        }
        self.put(" {");
        // the members in the order of their lines: (what, node, line),
        // what 1 an instance field, 2 the constructor, 3 a method, 4 a
        // static field or block
        let mut whats: Vec<int> = Vec::new();
        let mut nodes: Vec<int> = Vec::new();
        let mut lines: Vec<int> = Vec::new();
        if inst >= 0 {
            self.field_lines(inst, 1, &mut whats, &mut nodes, &mut lines);
        }
        if ctor >= 0 && !self.default_ctor(ctor) {
            whats.push(2);
            nodes.push(ctor);
            lines.push(self.ast.nodes[ctor as usize].line);
        }
        let methods = self.ast.nodes[n as usize].list.clone();
        for m in methods.iter() {
            whats.push(3);
            nodes.push(*m);
            lines.push(self.ast.nodes[*m as usize].line);
        }
        if stat >= 0 {
            self.field_lines(stat, 4, &mut whats, &mut nodes, &mut lines);
        }
        // a stable insertion sort: members on one line keep their order
        let mut i: usize = 1;
        while i < lines.len() {
            let mut j = i;
            while j > 0 && lines[j - 1] > lines[j] {
                lines.swap(j - 1, j);
                whats.swap(j - 1, j);
                nodes.swap(j - 1, j);
                j -= 1;
            }
            i += 1;
        }
        let mut k: usize = 0;
        while k < whats.len() {
            let what = whats[k];
            let m = nodes[k];
            self.at_line(lines[k]);
            if what == 1 || what == 4 {
                self.field(m, what == 4);
            } else if what == 2 {
                self.put("constructor");
                self.function_rest(m);
            } else {
                self.method(m);
            }
            k += 1;
        }
        self.space();
        self.put("}");
    }

    /// The statements of a class's field initializer `f` with the lines
    /// they stand on: an initializer's own line, else (a field with no
    /// initializer, whose node has the class's last line) the one before,
    /// so fields keep their order.
    fn field_lines(&self, f: int, what: int, whats: &mut Vec<int>, nodes: &mut Vec<int>, lines: &mut Vec<int>) {
        let body = self.ast.nodes[f as usize].a;
        let list = self.ast.nodes[body as usize].list.clone();
        let mut prev: int = 0;
        for st in list.iter() {
            let mut line = prev;
            if self.kind(*st) == N_BLOCK {
                line = self.ast.nodes[*st as usize].line;
            } else {
                let asg = self.ast.nodes[*st as usize].a;
                if asg >= 0 && self.kind(asg) == N_ASSIGN {
                    let v = self.ast.nodes[asg as usize].b;
                    if v >= 0 && self.kind(v) != N_UNDEF {
                        line = self.ast.nodes[v as usize].line;
                    }
                }
            }
            if line < prev {
                line = prev;
            }
            prev = line;
            whats.push(what);
            nodes.push(*st);
            lines.push(line);
        }
    }

    /// The constructor the parser makes for a class without one:
    /// `constructor() {}` or `constructor(...args) { super(...args); }`.
    fn default_ctor(&self, f: int) -> bool {
        let flags = self.ast.nodes[f as usize].flags;
        if (flags & F_STRICT) == 0 {
            return false;
        }
        let body = self.ast.nodes[f as usize].a;
        let list = self.ast.nodes[body as usize].list.clone();
        let params = self.ast.nodes[f as usize].list.clone();
        if list.is_empty() {
            return params.is_empty();
        }
        if list.len() != 1 || params.len() != 1 || self.kind(params[0]) != N_REST {
            return false;
        }
        let e = self.ast.nodes[list[0] as usize].a;
        self.kind(list[0]) == N_EXPR && self.kind(e) == N_SUPER_CALL
    }

    /// One statement of a class's field initializer: `this.x define v` as
    /// the field `x = v`, a static block's body as `static {…}`. The parser
    /// makes no other statements there.
    fn field(&mut self, st: int, is_static: bool) {
        if self.kind(st) == N_BLOCK {
            self.put("static ");
            self.block(st);
            return;
        }
        let asg = self.ast.nodes[st as usize].a;
        if is_static {
            self.put("static ");
        }
        let target = self.ast.nodes[asg as usize].a;
        let value = self.ast.nodes[asg as usize].b;
        if self.kind(target) == N_INDEX {
            let key = self.ast.nodes[target as usize].b;
            self.put("[");
            self.expr(key, P_ASSIGN);
            self.put("]");
        } else {
            let s = self.s(target);
            let private = private_name(s.as_str());
            if !private.is_empty() {
                self.put(private.as_str());
            } else if is_ident_name(s.as_str()) {
                self.put(s.as_str());
            } else {
                self.put(quote(s.as_str()).as_str());
            }
        }
        if self.kind(value) != N_UNDEF {
            self.put(" = ");
            self.expr(value, P_ASSIGN);
        }
        self.put(";");
    }

    // ---- expressions

    /// How tightly `e` binds as written.
    fn prec(&self, e: int) -> int {
        let k = self.kind(e);
        if k == N_SEQ {
            return P_SEQ;
        }
        if k == N_ASSIGN || k == N_YIELD || k == N_SPREAD || k == N_REST || k == N_PAT_DEFAULT {
            return P_ASSIGN;
        }
        if k == N_FUNC && (self.ast.nodes[e as usize].flags & F_ARROW) != 0 {
            return P_ASSIGN;
        }
        if k == N_COND {
            return P_COND;
        }
        if k == N_BINARY || k == N_LOGICAL {
            return binary_prec(self.op(e).as_str());
        }
        if k == N_UNARY || k == N_UNDEF {
            return P_UNARY;
        }
        if k == N_UPDATE {
            if (self.ast.nodes[e as usize].flags & 1) != 0 {
                return P_UNARY;
            }
            return P_POSTFIX;
        }
        if k == N_CALL || k == N_NEW || k == N_MEMBER || k == N_INDEX || k == N_TAGGED || k == N_SUPER_CALL || k == N_SUPER_MEMBER || k == N_OPT_CHAIN {
            return P_CALL;
        }
        P_PRIMARY
    }

    /// `a ?? b` mixed with `||` / `&&` without parentheses is a syntax
    /// error: the operand `x` of `e` needs them.
    fn mixes_coalesce(&self, e: int, x: int) -> bool {
        if self.kind(x) != N_LOGICAL {
            return false;
        }
        let o = self.op(e);
        let xo = self.op(x);
        (o.as_str() == "??") != (xo.as_str() == "??")
    }

    /// The object of `.x`, `[x]`, a call or a tag needs parentheses: it
    /// binds looser, is a number (`1.x`), or is an optional chain that was
    /// in parentheses (`(a?.b).c`).
    fn object_parens(&self, a: int) -> bool {
        let k = self.kind(a);
        if k == N_OPT_CHAIN || k == N_NUM {
            return true;
        }
        self.prec(a) < P_CALL
    }

    /// The callee of `new` holds a call (`new (f())()`), which would take
    /// the arguments.
    fn callee_has_call(&self, a: int) -> bool {
        let k = self.kind(a);
        if k == N_CALL || k == N_OPT_CHAIN || k == N_SUPER_CALL {
            return true;
        }
        if k == N_MEMBER || k == N_INDEX || k == N_TAGGED {
            return self.callee_has_call(self.ast.nodes[a as usize].a);
        }
        false
    }

    /// `e` where an expression binding at least `min` is wanted.
    pub fn expr(&mut self, e: int, min: int) {
        if self.prec(e) < min {
            self.put("(");
            self.expr_bare(e);
            self.put(")");
        } else {
            self.expr_bare(e);
        }
    }

    fn operand(&mut self, e: int, x: int, min: int) {
        if self.mixes_coalesce(e, x) {
            self.put("(");
            self.expr_bare(x);
            self.put(")");
        } else {
            self.expr(x, min);
        }
    }

    fn object(&mut self, a: int) {
        if self.object_parens(a) {
            self.put("(");
            self.expr_bare(a);
            self.put(")");
        } else {
            self.expr_bare(a);
        }
    }

    fn args(&mut self, list: &Vec<int>) {
        self.put("(");
        let mut first = true;
        for x in list.iter() {
            if !first {
                self.put(", ");
            }
            first = false;
            self.expr(*x, P_ASSIGN);
        }
        self.put(")");
    }

    fn expr_bare(&mut self, e: int) {
        let k = self.kind(e);
        let a = self.ast.nodes[e as usize].a;
        let b = self.ast.nodes[e as usize].b;
        let c = self.ast.nodes[e as usize].c;
        let d = self.ast.nodes[e as usize].d;
        let flags = self.ast.nodes[e as usize].flags;
        if k == N_NUM {
            let v = self.ast.nodes[e as usize].num;
            self.put(crate::num::number_to_string(v).as_str());
        } else if k == N_BIGINT {
            let s = self.s(e);
            self.put(s.as_str());
            self.put("n");
        } else if k == N_STR {
            let s = self.s(e);
            let private = private_name(s.as_str());
            if !private.is_empty() {
                self.put(private.as_str());
            } else {
                self.put(quote(s.as_str()).as_str());
            }
        } else if k == N_TEMPLATE {
            self.template(e);
        } else if k == N_IDENT {
            let s = self.s(e);
            self.put(s.as_str());
        } else if k == N_THIS {
            self.put("this");
        } else if k == N_NULL {
            self.put("null");
        } else if k == N_TRUE {
            self.put("true");
        } else if k == N_FALSE {
            self.put("false");
        } else if k == N_UNDEF {
            self.put("void 0");
        } else if k == N_HOLE {
            // an array's hole: nothing between its commas
        } else if k == N_ARRAY {
            let items = self.ast.nodes[e as usize].list.clone();
            self.put("[");
            let mut first = true;
            for x in items.iter() {
                if !first {
                    self.put(", ");
                }
                first = false;
                self.expr(*x, P_ASSIGN);
            }
            // a hole at the end needs its own comma: `[a, ,]`
            if !items.is_empty() && self.kind(items[items.len() - 1]) == N_HOLE {
                self.put(",");
            }
            self.put("]");
        } else if k == N_OBJECT {
            self.object_literal(e);
        } else if k == N_FUNC {
            self.function(e);
        } else if k == N_CLASS {
            self.class(e);
        } else if k == N_REGEX {
            let s = self.s(e);
            let f = self.op(e);
            self.put("/");
            self.put(s.as_str());
            self.put("/");
            self.put(f.as_str());
        } else if k == N_UNARY {
            let o = self.op(e);
            self.put(o.as_str());
            let word = o.as_str() == "typeof" || o.as_str() == "void" || o.as_str() == "delete" || o.as_str() == "await";
            let same = (o.as_str() == "-" || o.as_str() == "+") && self.starts_with_sign(a, o.as_str());
            if word || same {
                self.put(" ");
            }
            self.expr(a, P_UNARY);
        } else if k == N_UPDATE {
            let o = self.op(e);
            if (flags & 1) != 0 {
                self.put(o.as_str());
                self.expr(a, P_UNARY);
            } else {
                self.expr(a, P_CALL);
                self.put(o.as_str());
            }
        } else if k == N_BINARY || k == N_LOGICAL {
            let o = self.op(e);
            let p = binary_prec(o.as_str());
            if o.as_str() == "**" {
                self.operand(e, a, P_POSTFIX);
                self.put(" ** ");
                self.operand(e, b, p);
            } else {
                self.operand(e, a, p);
                self.put(" ");
                self.put(o.as_str());
                self.put(" ");
                self.operand(e, b, p + 1);
            }
        } else if k == N_ASSIGN {
            let o = self.op(e);
            self.expr(a, P_CALL);
            if o.as_str() == "define" {
                self.put(" = ");
            } else {
                self.put(" ");
                self.put(o.as_str());
                self.put(" ");
            }
            self.expr(b, P_ASSIGN);
        } else if k == N_COND {
            self.expr(a, P_COND + 1);
            self.put(" ? ");
            self.expr(b, P_ASSIGN);
            self.put(" : ");
            self.expr(c, P_ASSIGN);
        } else if k == N_CALL {
            self.object(a);
            if d == 1 {
                self.put("?.");
            }
            let list = self.ast.nodes[e as usize].list.clone();
            self.args(&list);
        } else if k == N_NEW {
            self.put("new ");
            if self.callee_has_call(a) || self.prec(a) < P_CALL {
                self.put("(");
                self.expr_bare(a);
                self.put(")");
            } else {
                self.expr_bare(a);
            }
            let list = self.ast.nodes[e as usize].list.clone();
            self.args(&list);
        } else if k == N_MEMBER {
            self.object(a);
            let s = self.s(e);
            let private = private_name(s.as_str());
            self.put(if d == 1 { "?." } else { "." });
            if !private.is_empty() {
                self.put(private.as_str());
            } else {
                self.put(s.as_str());
            }
        } else if k == N_INDEX {
            self.object(a);
            if d == 1 {
                self.put("?.");
            }
            self.put("[");
            self.expr(b, P_SEQ);
            self.put("]");
        } else if k == N_SEQ {
            let items = self.ast.nodes[e as usize].list.clone();
            let mut first = true;
            for x in items.iter() {
                if !first {
                    self.put(", ");
                }
                first = false;
                self.expr(*x, P_ASSIGN);
            }
        } else if k == N_SPREAD || k == N_REST {
            self.put("...");
            self.expr(a, P_ASSIGN);
        } else if k == N_PAT_DEFAULT {
            self.expr(a, P_CALL);
            self.put(" = ");
            self.expr(b, P_ASSIGN);
        } else if k == N_SUPER_CALL {
            self.put("super");
            let list = self.ast.nodes[e as usize].list.clone();
            self.args(&list);
        } else if k == N_SUPER_MEMBER {
            if c == 1 {
                self.put("super[");
                self.expr(a, P_SEQ);
                self.put("]");
            } else {
                let s = self.s(a);
                self.put("super.");
                self.put(s.as_str());
            }
        } else if k == N_NEW_TARGET {
            self.put("new.target");
        } else if k == N_OPT_CHAIN {
            self.expr_bare(a);
        } else if k == N_TAGGED {
            self.object(a);
            self.template(b);
        } else if k == N_YIELD {
            self.put(if (flags & 1) != 0 { "yield*" } else { "yield" });
            if a >= 0 {
                self.put(" ");
                self.expr(a, P_ASSIGN);
            }
        } else {
            // a node that is not an expression: what it was is lost
            self.put("void 0");
        }
    }

    /// `+x` / `-x` written after the same sign would read as `++` / `--`.
    fn starts_with_sign(&self, a: int, sign: &str) -> bool {
        let k = self.kind(a);
        if k == N_UNARY {
            return self.op(a).as_str() == sign;
        }
        if k == N_UPDATE && (self.ast.nodes[a as usize].flags & 1) != 0 {
            return self.op(a).starts_with(sign);
        }
        false
    }

    fn template(&mut self, t: int) {
        let strs = self.ast.nodes[t as usize].list.clone();
        let exprs = self.ast.nodes[t as usize].list2.clone();
        self.put("`");
        let mut i: usize = 0;
        while i < strs.len() {
            let raw = self.op(strs[i]);
            self.put(raw.as_str());
            if i < exprs.len() {
                self.put("${");
                self.no_lines += 1;
                self.expr(exprs[i], P_SEQ);
                self.no_lines -= 1;
                self.put("}");
            }
            i += 1;
        }
        self.put("`");
    }

    fn object_literal(&mut self, e: int) {
        let props = self.ast.nodes[e as usize].list.clone();
        if props.is_empty() {
            self.put("{}");
            return;
        }
        self.put("{");
        let mut first = true;
        for p in props.iter() {
            if !first {
                self.put(",");
            }
            first = false;
            let line = self.ast.nodes[*p as usize].line;
            self.at_line(line);
            let flags = self.ast.nodes[*p as usize].flags;
            let key = self.ast.nodes[*p as usize].a;
            let v = self.ast.nodes[*p as usize].b;
            if (flags & F_SPREAD) != 0 {
                self.put("...");
                self.expr(v, P_ASSIGN);
            } else if v >= 0 && self.kind(v) == N_FUNC && (self.ast.nodes[v as usize].flags & F_METHOD) != 0 {
                self.method(*p);
            } else if (flags & F_SHORTHAND) != 0 {
                // `{ a }`, or as a pattern `{ a = 1 }`
                self.expr(v, P_ASSIGN);
            } else {
                self.key(key, (flags & F_COMPUTED) != 0);
                self.put(": ");
                self.expr(v, P_ASSIGN);
            }
        }
        self.space();
        self.put("}");
    }
}
