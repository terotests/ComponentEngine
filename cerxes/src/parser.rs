// SPDX-License-Identifier: AGPL-3.0-or-later
//! CEr's parser (../cer/src/parser.rs) with TypeScript and JSX: a
//! recursive-descent parser from tokens to CEr's arena tree (`ast`),
//! so CEr's compiler and VM run the result unchanged.
//!
//! TypeScript is erased as it is read: annotations, type parameters and
//! arguments, `as` / `satisfies` / `!`, `interface`, `type`, `declare`,
//! overload and abstract signatures and `import type` leave no nodes.
//! What has a runtime meaning becomes plain JavaScript: `enum` and
//! `namespace` the objects TypeScript emits for them, constructor parameter
//! properties `this.x = x` after `super(...)`. (`types.rs`: the type
//! grammar, skipped.)
//!
//! JSX becomes calls, the classic transform: `<a b={1}>t</a>` is
//! `__jsx("a", {b: 1}, "t")`, a capitalised or dotted tag a value
//! (`__jsx(Foo, …)`), `<>…</>` `__jsx(__jsxFragment, null, …)`. `@jsx` /
//! `@jsxFrag` comments name other functions. (`jsx.rs`.)

use ranger::prelude::*;

use crate::ast::*;
use crate::lexer::*;

/// Which syntax a script is read with.
#[derive(Clone)]
pub struct Syntax {
    pub typescript: bool,
    pub jsx: bool,
    /// the function a JSX element calls (`@jsx`), dotted names allowed
    pub jsx_factory: String,
    /// the value `<>…</>` passes as the type (`@jsxFrag`)
    pub jsx_fragment: String,
}

impl Syntax {
    /// TypeScript and JSX, as in a `.tsx` file.
    pub fn tsx() -> Syntax {
        Syntax { typescript: true, jsx: true, jsx_factory: String::from("__jsx"), jsx_fragment: String::from("__jsxFragment") }
    }

    pub fn new(typescript: bool, jsx: bool) -> Syntax {
        let mut s = Syntax::tsx();
        s.typescript = typescript;
        s.jsx = jsx;
        s
    }
}

pub struct Parser {
    pub(crate) lx: Lexer,
    /// the tokens read so far; `pos` indexes them, more are read on demand
    pub(crate) toks: Vec<Tok>,
    pub(crate) pos: int,
    pub ast: Ast,
    pub error: String,
    pub syntax: Syntax,
    /// `in` is not an operator in the head of a `for`
    pub(crate) no_in: bool,
    pub(crate) in_function: bool,
    pub(crate) in_class: bool,
    /// tokens a `>` was split off while reading type arguments: (index,
    /// text before), undone when a speculative read is abandoned
    pub(crate) splits: Vec<(int, String)>,
    /// the function just read had no body (an overload, `abstract`,
    /// `declare`)
    pub(crate) no_body: bool,
    /// constructor parameter properties of the function just read
    pub(crate) param_props: Vec<String>,
}

fn binary_prec(op: &str) -> int {
    if op == "??" {
        return 1;
    }
    if op == "||" {
        return 2;
    }
    if op == "&&" {
        return 3;
    }
    if op == "|" {
        return 4;
    }
    if op == "^" {
        return 5;
    }
    if op == "&" {
        return 6;
    }
    if op == "==" || op == "!=" || op == "===" || op == "!==" {
        return 7;
    }
    if op == "<" || op == ">" || op == "<=" || op == ">=" || op == "instanceof" || op == "in" {
        return 8;
    }
    if op == "<<" || op == ">>" || op == ">>>" {
        return 9;
    }
    if op == "+" || op == "-" {
        return 10;
    }
    if op == "*" || op == "/" || op == "%" {
        return 11;
    }
    if op == "**" {
        return 12;
    }
    -1
}

fn is_assign_op(op: &str) -> bool {
    op == "="
        || op == "+="
        || op == "-="
        || op == "*="
        || op == "/="
        || op == "%="
        || op == "**="
        || op == "<<="
        || op == ">>="
        || op == ">>>="
        || op == "&="
        || op == "|="
        || op == "^="
        || op == "&&="
        || op == "||="
        || op == "??="
}

pub fn is_reserved(w: &str) -> bool {
    w == "break"
        || w == "case"
        || w == "catch"
        || w == "class"
        || w == "const"
        || w == "continue"
        || w == "debugger"
        || w == "default"
        || w == "delete"
        || w == "do"
        || w == "else"
        || w == "export"
        || w == "extends"
        || w == "finally"
        || w == "for"
        || w == "function"
        || w == "if"
        || w == "import"
        || w == "in"
        || w == "instanceof"
        || w == "new"
        || w == "return"
        || w == "super"
        || w == "switch"
        || w == "this"
        || w == "throw"
        || w == "try"
        || w == "typeof"
        || w == "var"
        || w == "void"
        || w == "while"
        || w == "with"
        || w == "null"
        || w == "true"
        || w == "false"
        || w == "enum"
}

impl Parser {
    pub fn new(src: &str) -> Parser {
        Parser::with_syntax(src, Syntax::tsx())
    }

    pub fn with_syntax(src: &str, syntax: Syntax) -> Parser {
        let mut sx = syntax;
        crate::jsx::read_pragmas(src, &mut sx);
        Parser {
            lx: Lexer::new(src),
            toks: Vec::new(),
            pos: 0,
            ast: Ast::new(),
            error: String::new(),
            syntax: sx,
            no_in: false,
            in_function: false,
            in_class: false,
            splits: Vec::new(),
            no_body: false,
            param_props: Vec::new(),
        }
    }

    /// Copies the nodes of `other` (a template substitution's tree) into
    /// this tree; answers where `root` landed.
    fn graft(&mut self, other: &Ast, root: int) -> int {
        let off = self.ast.nodes.len() as int;
        for nd in other.nodes.iter() {
            let i = self.ast.add(nd.kind, nd.line);
            let m = &mut self.ast.nodes[i as usize];
            m.op = nd.op.clone();
            m.s = nd.s.clone();
            m.num = nd.num;
            m.flags = nd.flags;
            m.a = if nd.a >= 0 { nd.a + off } else { nd.a };
            m.b = if nd.b >= 0 { nd.b + off } else { nd.b };
            m.c = if nd.c >= 0 && nd.kind != N_SUPER_MEMBER { nd.c + off } else { nd.c };
            m.d = if nd.d >= 0 && (nd.kind != N_PROP && nd.kind != N_MEMBER && nd.kind != N_INDEX && nd.kind != N_CALL) { nd.d + off } else { nd.d };
            for x in nd.list.iter() {
                m.list.push(*x + off);
            }
            for x in nd.list2.iter() {
                m.list2.push(*x + off);
            }
        }
        root + off
    }

    // ---- tokens

    /// The token `i`, read on demand.
    pub(crate) fn tk(&mut self, i: int) -> &Tok {
        while (self.toks.len() as int) <= i {
            let t = self.lx.next_token();
            self.toks.push(t);
        }
        &self.toks[i as usize]
    }

    /// Forgets the tokens from `pos` on and puts the lexer back at the
    /// start of the token there (read as script, it may be JSX).
    pub(crate) fn rewind_lexer(&mut self) -> int {
        let start = self.tk(self.pos).start;
        let line = self.tk(self.pos).line;
        self.toks.truncate(self.pos as usize);
        self.splits.clear();
        self.lx.pos = start;
        self.lx.line = line;
        start
    }

    pub(crate) fn kind(&mut self) -> int {
        if !self.error.is_empty() {
            return T_EOF;
        }
        self.tk(self.pos).kind
    }

    pub(crate) fn text(&mut self) -> String {
        self.tk(self.pos).text.clone()
    }

    pub(crate) fn line(&mut self) -> int {
        if (self.pos as usize) < self.toks.len() {
            return self.toks[self.pos as usize].line;
        }
        self.lx.line
    }

    pub(crate) fn escaped(&mut self) -> bool {
        self.tk(self.pos).escaped
    }

    pub(crate) fn peek_kind(&mut self, k: int) -> int {
        if !self.error.is_empty() {
            return T_EOF;
        }
        let i = self.pos + k;
        self.tk(i).kind
    }

    /// A line break before the token `k` ahead.
    pub(crate) fn peek_nl(&mut self, k: int) -> bool {
        let i = self.pos + k;
        self.tk(i).nl
    }

    pub(crate) fn peek_is(&mut self, k: int, s: &str) -> bool {
        if !self.error.is_empty() {
            return false;
        }
        let i = self.pos + k;
        let t = self.tk(i);
        (t.kind == T_PUNCT || (t.kind == T_IDENT && !t.escaped)) && t.text.as_str() == s
    }

    pub(crate) fn is(&mut self, s: &str) -> bool {
        self.peek_is(0, s)
    }

    pub(crate) fn nl_before(&mut self) -> bool {
        self.tk(self.pos).nl
    }

    pub(crate) fn next(&mut self) {
        if !self.error.is_empty() {
            return;
        }
        let k = self.tk(self.pos).kind;
        if k != T_EOF && k != T_ERROR {
            self.pos += 1;
        }
    }

    /// Records the first error; from then on every token reads as the end.
    pub(crate) fn fail(&mut self, msg: &str) {
        if self.error.is_empty() {
            let l = self.line();
            let bad = (self.pos as usize) < self.toks.len() && self.toks[self.pos as usize].kind == T_ERROR;
            if bad {
                self.error = self.toks[self.pos as usize].text.clone();
            } else {
                self.error = format!("SyntaxError: {} (line {})", msg, l);
            }
        }
    }

    pub(crate) fn expect(&mut self, s: &str) {
        if self.is(s) {
            self.next();
        } else {
            let t = self.text();
            self.fail(format!("expected '{}' but found '{}'", s, t).as_str());
        }
    }

    pub(crate) fn eat(&mut self, s: &str) -> bool {
        if self.is(s) {
            self.next();
            return true;
        }
        false
    }

    pub(crate) fn semicolon(&mut self) {
        if self.eat(";") {
            return;
        }
        if self.is("}") || self.kind() == T_EOF || self.nl_before() {
            return;
        }
        let t = self.text();
        self.fail(format!("unexpected token '{}'", t).as_str());
    }

    pub(crate) fn ident_name(&mut self) -> String {
        // any identifier, reserved words included (after `.`, in keys)
        if self.kind() == T_IDENT {
            let s = self.text();
            self.next();
            return s;
        }
        let t = self.text();
        self.fail(format!("expected a name but found '{}'", t).as_str());
        String::new()
    }

    pub(crate) fn binding_ident(&mut self) -> String {
        if self.kind() == T_IDENT {
            let s = self.text();
            if is_reserved(s.as_str()) && !self.escaped() {
                self.fail(format!("unexpected reserved word '{}'", s).as_str());
                return s;
            }
            self.next();
            return s;
        }
        let t = self.text();
        self.fail(format!("expected an identifier but found '{}'", t).as_str());
        String::new()
    }

    pub(crate) fn node(&mut self, kind: int) -> int {
        let l = self.line();
        self.ast.add(kind, l)
    }

    // ---- program

    pub fn parse_program(&mut self) -> int {
        let prog = self.node(N_PROGRAM);
        let mut body: Vec<int> = Vec::new();
        self.directives(prog);
        while self.kind() != T_EOF && self.error.is_empty() {
            let s = self.statement();
            body.push(s);
        }
        self.ast.nodes[prog as usize].list = body;
        prog
    }

    /// `"use strict"` at the top of a body
    fn directives(&mut self, owner: int) {
        let mut k: int = 0;
        while self.peek_kind(k) == T_STR {
            let strict = self.tk(self.pos + k).text.as_str() == "use strict" && !self.tk(self.pos + k).escaped;
            if strict {
                self.ast.nodes[owner as usize].flags |= F_STRICT;
            }
            k += 1;
            if self.peek_is(k, ";") {
                k += 1;
            } else if !self.peek_nl(k) {
                break;
            }
        }
    }

    // ---- statements

    pub(crate) fn statement(&mut self) -> int {
        if self.kind() == T_PUNCT {
            if self.is("{") {
                return self.block();
            }
            if self.is(";") {
                self.next();
                return self.node(N_EMPTY);
            }
        }
        if self.kind() == T_IDENT && !self.escaped() {
            let w = self.text();
            let ws = w.as_str();
            if ws == "const" && self.syntax.typescript && self.peek_is(1, "enum") {
                self.next();
                return self.enum_decl();
            }
            if ws == "var" || ws == "const" {
                let n = self.var_decl();
                self.semicolon();
                return n;
            }
            if ws == "let" && (self.peek_kind(1) == T_IDENT || self.peek_is(1, "[") || self.peek_is(1, "{")) {
                let n = self.var_decl();
                self.semicolon();
                return n;
            }
            if ws == "function" {
                let f = self.function(true, 0);
                return self.unless_signature(f);
            }
            if ws == "async" && self.peek_is(1, "function") && !self.peek_nl(1) {
                self.next();
                let f = self.function(true, F_ASYNC);
                return self.unless_signature(f);
            }
            if ws == "class" {
                return self.class(true);
            }
            if self.syntax.typescript {
                let t = self.ts_statement();
                if t >= 0 {
                    return t;
                }
            }
            if ws == "if" {
                return self.if_stmt();
            }
            if ws == "for" {
                return self.for_stmt();
            }
            if ws == "while" {
                let n = self.node(N_WHILE);
                self.next();
                self.expect("(");
                let t = self.expression();
                self.expect(")");
                let b = self.statement();
                self.ast.nodes[n as usize].a = t;
                self.ast.nodes[n as usize].b = b;
                return n;
            }
            if ws == "do" {
                let n = self.node(N_DOWHILE);
                self.next();
                let b = self.statement();
                if !self.is("while") {
                    self.fail("expected 'while'");
                }
                self.next();
                self.expect("(");
                let t = self.expression();
                self.expect(")");
                self.eat(";");
                self.ast.nodes[n as usize].a = b;
                self.ast.nodes[n as usize].b = t;
                return n;
            }
            if ws == "return" {
                let n = self.node(N_RETURN);
                if !self.in_function {
                    self.fail("return outside a function");
                }
                self.next();
                if !self.is(";") && !self.is("}") && self.kind() != T_EOF && !self.nl_before() {
                    let e = self.expression();
                    self.ast.nodes[n as usize].a = e;
                }
                self.semicolon();
                return n;
            }
            if ws == "break" || ws == "continue" {
                let n = self.node(if ws == "break" { N_BREAK } else { N_CONTINUE });
                self.next();
                if self.kind() == T_IDENT && !self.nl_before() && !is_reserved(self.text().as_str()) {
                    let l = self.text();
                    self.ast.nodes[n as usize].s = l;
                    self.next();
                }
                self.semicolon();
                return n;
            }
            if ws == "throw" {
                let n = self.node(N_THROW);
                self.next();
                if self.nl_before() {
                    self.fail("line break after throw");
                }
                let e = self.expression();
                self.ast.nodes[n as usize].a = e;
                self.semicolon();
                return n;
            }
            if ws == "try" {
                return self.try_stmt();
            }
            if ws == "switch" {
                return self.switch_stmt();
            }
            if ws == "debugger" {
                self.next();
                self.semicolon();
                return self.node(N_EMPTY);
            }
            if ws == "with" {
                self.fail("'with' is not supported");
                return self.node(N_EMPTY);
            }
            if ws == "import" && !self.peek_is(1, "(") && !self.peek_is(1, ".") {
                return self.import_stmt();
            }
            if ws == "export" {
                return self.export_stmt();
            }
            if self.peek_is(1, ":") && !is_reserved(ws) {
                let n = self.node(N_LABELED);
                self.ast.nodes[n as usize].s = w.clone();
                self.next();
                self.next();
                let b = self.statement();
                self.ast.nodes[n as usize].a = b;
                return n;
            }
        }
        let n = self.node(N_EXPR);
        let e = self.expression();
        self.ast.nodes[n as usize].a = e;
        self.semicolon();
        n
    }

    pub(crate) fn block(&mut self) -> int {
        let n = self.node(N_BLOCK);
        self.expect("{");
        let mut body: Vec<int> = Vec::new();
        while !self.is("}") && self.kind() != T_EOF {
            let s = self.statement();
            body.push(s);
        }
        self.expect("}");
        self.ast.nodes[n as usize].list = body;
        n
    }

    /// `var` / `let` / `const` and its declarators, no semicolon.
    pub(crate) fn var_decl(&mut self) -> int {
        let n = self.node(N_VAR);
        let kw = self.text();
        self.next();
        let mut decls: Vec<int> = Vec::new();
        loop {
            let d = self.node(N_DECL);
            let target = self.binding_target();
            self.ast.nodes[d as usize].a = target;
            if self.syntax.typescript {
                self.eat("!");
                if self.eat(":") {
                    self.skip_type();
                }
            }
            if self.eat("=") {
                let init = self.assign();
                self.ast.nodes[d as usize].b = init;
            }
            decls.push(d);
            if !self.eat(",") {
                break;
            }
        }
        self.ast.nodes[n as usize].op = kw;
        self.ast.nodes[n as usize].list = decls;
        n
    }

    /// An identifier or a destructuring pattern.
    pub(crate) fn binding_target(&mut self) -> int {
        if self.is("[") || self.is("{") {
            let e = self.primary();
            return self.to_pattern(e);
        }
        let n = self.node(N_IDENT);
        let name = self.binding_ident();
        self.ast.nodes[n as usize].s = name;
        n
    }

    /// Rewrites an array / object literal (or assignment target) as a
    /// pattern: `a = 1` inside becomes a default, `...a` a rest element.
    fn to_pattern(&mut self, e: int) -> int {
        let k = self.ast.kind(e);
        if k == N_ARRAY {
            let items = self.ast.nodes[e as usize].list.clone();
            let mut out: Vec<int> = Vec::new();
            for it in items {
                if self.ast.kind(it) == N_SPREAD {
                    let inner = self.ast.nodes[it as usize].a;
                    let p = self.to_pattern(inner);
                    let r = self.ast.add(N_REST, 0);
                    self.ast.nodes[r as usize].a = p;
                    out.push(r);
                } else if self.ast.kind(it) == N_HOLE {
                    out.push(it);
                } else {
                    let p = self.to_pattern(it);
                    out.push(p);
                }
            }
            self.ast.nodes[e as usize].list = out;
            return e;
        }
        if k == N_OBJECT {
            let props = self.ast.nodes[e as usize].list.clone();
            for p in props {
                let f = self.ast.nodes[p as usize].flags;
                if (f & F_SPREAD) != 0 {
                    let inner = self.ast.nodes[p as usize].b;
                    let t = self.to_pattern(inner);
                    self.ast.nodes[p as usize].b = t;
                } else {
                    let v = self.ast.nodes[p as usize].b;
                    let t = self.to_pattern(v);
                    self.ast.nodes[p as usize].b = t;
                }
            }
            return e;
        }
        if k == N_ASSIGN && self.ast.nodes[e as usize].op.as_str() == "=" {
            let t = self.ast.nodes[e as usize].a;
            let pt = self.to_pattern(t);
            let d = self.ast.nodes[e as usize].b;
            let n = self.ast.add(N_PAT_DEFAULT, 0);
            self.ast.nodes[n as usize].a = pt;
            self.ast.nodes[n as usize].b = d;
            return n;
        }
        if k == N_PAT_DEFAULT || k == N_IDENT || k == N_MEMBER || k == N_INDEX || k == N_REST {
            return e;
        }
        self.fail("invalid destructuring target");
        e
    }

    fn if_stmt(&mut self) -> int {
        let n = self.node(N_IF);
        self.next();
        self.expect("(");
        let t = self.expression();
        self.expect(")");
        let a = self.statement();
        let mut b: int = -1;
        if self.is("else") {
            self.next();
            b = self.statement();
        }
        self.ast.nodes[n as usize].a = t;
        self.ast.nodes[n as usize].b = a;
        self.ast.nodes[n as usize].c = b;
        n
    }

    fn for_stmt(&mut self) -> int {
        let line = self.line();
        self.next();
        if self.is("await") {
            self.fail("for await is not supported");
        }
        self.expect("(");
        let mut init: int = -1;
        if self.is(";") {
            // no init
        } else {
            let decl = self.is("var") || self.is("const") || (self.is("let") && (self.peek_kind(1) == T_IDENT || self.peek_is(1, "[") || self.peek_is(1, "{")));
            self.no_in = true;
            if decl {
                init = self.var_decl();
            } else {
                init = self.expression();
            }
            self.no_in = false;
            if self.is("of") || self.is("in") {
                let n = self.ast.add(N_FORIN, line);
                let op = self.text();
                self.next();
                let obj = if op.as_str() == "of" { self.assign() } else { self.expression() };
                self.expect(")");
                if !decl {
                    init = self.to_pattern(init);
                }
                let body = self.statement();
                self.ast.nodes[n as usize].op = op;
                self.ast.nodes[n as usize].a = init;
                self.ast.nodes[n as usize].b = obj;
                self.ast.nodes[n as usize].c = body;
                return n;
            }
            if !decl {
                let e = self.ast.add(N_EXPR, line);
                self.ast.nodes[e as usize].a = init;
                init = e;
            }
        }
        let n = self.ast.add(N_FOR, line);
        self.expect(";");
        let mut test: int = -1;
        if !self.is(";") {
            test = self.expression();
        }
        self.expect(";");
        let mut update: int = -1;
        if !self.is(")") {
            update = self.expression();
        }
        self.expect(")");
        let body = self.statement();
        self.ast.nodes[n as usize].a = init;
        self.ast.nodes[n as usize].b = test;
        self.ast.nodes[n as usize].c = update;
        self.ast.nodes[n as usize].d = body;
        n
    }

    fn try_stmt(&mut self) -> int {
        let n = self.node(N_TRY);
        self.next();
        let body = self.block();
        self.ast.nodes[n as usize].a = body;
        if self.is("catch") {
            self.next();
            if self.eat("(") {
                let p = self.binding_target();
                if self.syntax.typescript && self.eat(":") {
                    self.skip_type();
                }
                self.ast.nodes[n as usize].b = p;
                self.expect(")");
            }
            let c = self.block();
            self.ast.nodes[n as usize].c = c;
        }
        if self.is("finally") {
            self.next();
            let f = self.block();
            self.ast.nodes[n as usize].d = f;
        }
        if self.ast.nodes[n as usize].c < 0 && self.ast.nodes[n as usize].d < 0 {
            self.fail("try without catch or finally");
        }
        n
    }

    fn switch_stmt(&mut self) -> int {
        let n = self.node(N_SWITCH);
        self.next();
        self.expect("(");
        let d = self.expression();
        self.expect(")");
        self.expect("{");
        let mut cases: Vec<int> = Vec::new();
        while !self.is("}") && self.kind() != T_EOF {
            let c = self.node(N_CASE);
            if self.is("case") {
                self.next();
                let t = self.expression();
                self.ast.nodes[c as usize].a = t;
            } else if self.is("default") {
                self.next();
            } else {
                self.fail("expected case or default");
                break;
            }
            self.expect(":");
            let mut body: Vec<int> = Vec::new();
            while !self.is("case") && !self.is("default") && !self.is("}") && self.kind() != T_EOF {
                let s = self.statement();
                body.push(s);
            }
            self.ast.nodes[c as usize].list = body;
            cases.push(c);
        }
        self.expect("}");
        self.ast.nodes[n as usize].a = d;
        self.ast.nodes[n as usize].list = cases;
        n
    }

    // ---- functions and classes

    /// `function name(…) { … }`; the current token is `function`.
    pub(crate) fn function(&mut self, decl: bool, extra: int) -> int {
        let n = self.node(N_FUNC);
        self.next();
        let mut flags = extra;
        if self.eat("*") {
            flags |= F_GENERATOR;
        }
        if decl {
            flags |= F_DECL;
        }
        if self.kind() == T_IDENT && !self.is("(") && !self.is("<") {
            let name = self.binding_ident();
            self.ast.nodes[n as usize].s = name;
        } else if decl {
            self.fail("function name expected");
        }
        self.ast.nodes[n as usize].flags = flags;
        if self.syntax.typescript && self.is("<") {
            self.type_params();
        }
        self.function_rest(n);
        if self.no_body && !decl {
            self.fail("function body expected");
        }
        n
    }

    /// A function declaration, or an empty statement for an overload
    /// signature (`function f(a: string): void;`).
    fn unless_signature(&mut self, f: int) -> int {
        if self.no_body {
            self.no_body = false;
            return self.node(N_EMPTY);
        }
        f
    }

    /// Parameters and body of the function node `n`. TypeScript: typed and
    /// optional parameters, a `this` parameter (dropped), a return type,
    /// parameter properties in a constructor (`param_props`), and no body
    /// at all for an overload or abstract signature (`no_body`).
    fn function_rest(&mut self, n: int) {
        self.no_body = false;
        self.param_props = Vec::new();
        let params = self.params(n);
        self.ast.nodes[n as usize].list = params;
        let mut props = self.param_props.clone();
        if self.syntax.typescript && self.is(":") {
            self.next();
            self.return_type();
        }
        if self.syntax.typescript && !self.is("{") {
            self.no_body = true;
            self.semicolon();
            return;
        }
        let body = self.function_body(n);
        self.ast.nodes[n as usize].a = body;
        if !props.is_empty() {
            self.add_param_props(n, &mut props);
        }
    }

    /// `(a, b = 1, ...c)`; the current token is `(`.
    fn params(&mut self, owner: int) -> Vec<int> {
        self.expect("(");
        let mut params: Vec<int> = Vec::new();
        let is_ctor = (self.ast.nodes[owner as usize].flags & F_CTOR) != 0;
        while !self.is(")") && self.kind() != T_EOF {
            if self.syntax.typescript {
                if self.is("this") && (self.peek_is(1, ":") || self.peek_is(1, ",") || self.peek_is(1, ")")) {
                    // `this: T` types `this`; it is no parameter
                    self.next();
                    if self.eat(":") {
                        self.skip_type();
                    }
                    if !self.eat(",") {
                        break;
                    }
                    continue;
                }
                let mut prop = false;
                while self.param_modifier_ahead() {
                    self.next();
                    prop = true;
                }
                if prop {
                    if !is_ctor {
                        self.fail("a parameter property is only allowed in a constructor");
                    }
                    if self.kind() == T_IDENT {
                        let nm = self.text();
                        self.param_props.push(nm);
                    }
                }
            }
            if self.eat("...") {
                let t = self.binding_target();
                self.param_type();
                let r = self.ast.add(N_REST, 0);
                self.ast.nodes[r as usize].a = t;
                params.push(r);
                self.eat(",");
                break;
            }
            let mut t = self.binding_target();
            self.param_type();
            if self.eat("=") {
                let d = self.assign();
                let pd = self.ast.add(N_PAT_DEFAULT, 0);
                self.ast.nodes[pd as usize].a = t;
                self.ast.nodes[pd as usize].b = d;
                t = pd;
            }
            params.push(t);
            if !self.eat(",") {
                break;
            }
        }
        self.expect(")");
        params
    }

    /// `?` and `: T` after a parameter.
    fn param_type(&mut self) {
        if !self.syntax.typescript {
            return;
        }
        self.eat("?");
        if self.eat(":") {
            self.skip_type();
        }
    }

    fn param_modifier_ahead(&mut self) -> bool {
        let is_mod = self.is("public") || self.is("private") || self.is("protected") || self.is("readonly") || self.is("override");
        is_mod && (self.peek_kind(1) == T_IDENT || self.peek_is(1, "{") || self.peek_is(1, "["))
    }

    /// `this.x = x` for each parameter property, first in the constructor
    /// body, or after its `super(...)` call.
    fn add_param_props(&mut self, f: int, names: &mut Vec<String>) {
        let body = self.ast.nodes[f as usize].a;
        let line = self.ast.nodes[f as usize].line;
        let old = self.ast.nodes[body as usize].list.clone();
        let mut at: int = 0;
        let mut i: int = 0;
        while i < old.len() as int {
            let st = old[i as usize];
            if self.ast.kind(st) == N_EXPR && self.ast.kind(self.ast.nodes[st as usize].a) == N_SUPER_CALL {
                at = i + 1;
                break;
            }
            i += 1;
        }
        let mut list: Vec<int> = Vec::new();
        let mut j: int = 0;
        while j < old.len() as int {
            if j == at {
                for nm in names.iter() {
                    let st = self.this_assign(nm.as_str(), line);
                    list.push(st);
                }
            }
            list.push(old[j as usize]);
            j += 1;
        }
        if at >= old.len() as int {
            for nm in names.iter() {
                let st = self.this_assign(nm.as_str(), line);
                list.push(st);
            }
        }
        self.ast.nodes[body as usize].list = list;
    }

    /// `this.name = name;`
    fn this_assign(&mut self, name: &str, line: int) -> int {
        let this_n = self.ast.add(N_THIS, line);
        let m = self.ast.add(N_MEMBER, line);
        self.ast.nodes[m as usize].a = this_n;
        self.ast.nodes[m as usize].s = String::from(name);
        let v = self.mk_ident(name, line);
        let asg = self.ast.add(N_ASSIGN, line);
        self.ast.nodes[asg as usize].op = String::from("=");
        self.ast.nodes[asg as usize].a = m;
        self.ast.nodes[asg as usize].b = v;
        let st = self.ast.add(N_EXPR, line);
        self.ast.nodes[st as usize].a = asg;
        st
    }

    pub(crate) fn function_body(&mut self, owner: int) -> int {
        let saved = self.in_function;
        self.in_function = true;
        let n = self.node(N_BLOCK);
        self.expect("{");
        self.directives(owner);
        let mut body: Vec<int> = Vec::new();
        while !self.is("}") && self.kind() != T_EOF {
            let s = self.statement();
            body.push(s);
        }
        self.expect("}");
        self.ast.nodes[n as usize].list = body;
        self.in_function = saved;
        n
    }

    /// True when the tokens from here are an arrow function's head.
    /// TypeScript: the head may carry type parameters and a return type,
    /// `<T>(a: T): T =>`.
    pub(crate) fn arrow_ahead(&mut self) -> bool {
        if !self.error.is_empty() {
            return false;
        }
        let mut k: int = 0;
        if self.peek_is(0, "async") && !self.peek_nl(1) && (self.peek_kind(1) == T_IDENT || self.peek_is(1, "(") || (self.syntax.typescript && self.peek_is(1, "<"))) {
            if self.peek_kind(1) == T_IDENT {
                return self.peek_is(2, "=>");
            }
            k = 1;
        }
        if self.peek_kind(k) == T_IDENT && !self.peek_is(k, "(") {
            return self.peek_is(k + 1, "=>") && k == 0;
        }
        if self.syntax.typescript && self.peek_is(k, "<") {
            // `<T>(…) =>`: read the type parameters, then look again. In
            // TSX `<T>` opens an element; `<T,>` and `<T extends …>` do not.
            if self.syntax.jsx {
                let mut j = k + 1;
                if self.peek_is(j, "const") && self.peek_kind(j + 1) == T_IDENT {
                    j += 1;
                }
                if !(self.peek_kind(j) == T_IDENT && (self.peek_is(j + 1, ",") || self.peek_is(j + 1, "extends"))) {
                    return false;
                }
            }
            let save = self.save();
            self.pos += k;
            self.type_params();
            let ok = self.error.is_empty() && self.peek_is(0, "(") && self.arrow_ahead();
            self.restore(save);
            return ok;
        }
        if !self.peek_is(k, "(") {
            return false;
        }
        let mut depth = 0;
        let mut i = self.pos + k;
        loop {
            let tkind = self.tk(i).kind;
            if tkind == T_PUNCT {
                let s = self.tk(i).text.clone();
                if s.as_str() == "(" || s.as_str() == "[" || s.as_str() == "{" {
                    depth += 1;
                } else if s.as_str() == ")" || s.as_str() == "]" || s.as_str() == "}" {
                    depth -= 1;
                    if depth == 0 {
                        let u_arrow = self.tk(i + 1).kind == T_PUNCT && self.tk(i + 1).text.as_str() == "=>" && !self.tk(i + 1).nl;
                        if u_arrow {
                            return true;
                        }
                        let u_colon = self.tk(i + 1).kind == T_PUNCT && self.tk(i + 1).text.as_str() == ":";
                        if self.syntax.typescript && u_colon {
                            // a return type, then `=>`
                            let save = self.save();
                            self.pos = i + 2;
                            self.return_type();
                            let ok = self.error.is_empty() && self.is("=>") && !self.nl_before();
                            self.restore(save);
                            return ok;
                        }
                        return false;
                    }
                }
            } else if tkind == T_EOF || tkind == T_ERROR {
                return false;
            }
            i += 1;
        }
    }

    fn arrow(&mut self) -> int {
        let n = self.node(N_FUNC);
        let mut flags = F_ARROW;
        if self.is("async") && !self.peek_is(1, "=>") {
            self.next();
            flags |= F_ASYNC;
        }
        self.ast.nodes[n as usize].flags = flags;
        if self.syntax.typescript && self.is("<") {
            self.type_params();
        }
        if self.kind() == T_IDENT {
            let p = self.node(N_IDENT);
            let name = self.binding_ident();
            self.ast.nodes[p as usize].s = name;
            self.ast.nodes[n as usize].list = vec![p];
        } else {
            let params = self.params(n);
            self.ast.nodes[n as usize].list = params;
            if self.syntax.typescript && self.is(":") {
                self.next();
                self.return_type();
            }
        }
        self.expect("=>");
        if self.is("{") {
            let b = self.function_body(n);
            self.ast.nodes[n as usize].a = b;
        } else {
            let saved = self.in_function;
            self.in_function = true;
            let saved_no_in = self.no_in;
            let e = self.assign();
            self.no_in = saved_no_in;
            self.in_function = saved;
            self.ast.nodes[n as usize].a = e;
            self.ast.nodes[n as usize].flags |= F_EXPR_BODY;
        }
        n
    }

    /// A property key: returns the key node; `computed` when `[expr]`.
    fn prop_key(&mut self, computed: &mut bool, private: &mut bool) -> int {
        if self.eat("[") {
            *computed = true;
            let e = self.assign();
            self.expect("]");
            return e;
        }
        if self.kind() == T_PRIVATE {
            *private = true;
            let n = self.node(N_STR);
            let s = format!("\u{1}#{}", self.text());
            self.ast.nodes[n as usize].s = s;
            self.next();
            return n;
        }
        if self.kind() == T_STR {
            let n = self.node(N_STR);
            let s = self.text();
            self.ast.nodes[n as usize].s = s;
            self.next();
            return n;
        }
        if self.kind() == T_NUM {
            let n = self.node(N_NUM);
            let v = self.tk(self.pos).num;
            self.ast.nodes[n as usize].num = v;
            self.next();
            return n;
        }
        let n = self.node(N_STR);
        let s = self.ident_name();
        self.ast.nodes[n as usize].s = s;
        n
    }

    /// A method's function node after its key: `(params) { body }`.
    fn method(&mut self, flags: int, name: &str) -> int {
        let f = self.node(N_FUNC);
        self.ast.nodes[f as usize].flags = flags | F_METHOD;
        self.ast.nodes[f as usize].s = String::from(name);
        self.function_rest(f);
        f
    }

    pub(crate) fn class(&mut self, decl: bool) -> int {
        let n = self.node(N_CLASS);
        self.next();
        if self.kind() == T_IDENT && !self.is("extends") && !self.is("{") && !self.is("implements") {
            let name = self.binding_ident();
            self.ast.nodes[n as usize].s = name;
        } else if decl {
            self.fail("class name expected");
        }
        if decl {
            self.ast.nodes[n as usize].flags |= F_DECL;
        }
        let ts = self.syntax.typescript;
        if ts && self.is("<") {
            self.type_params();
        }
        if self.eat("extends") {
            let sup = self.lhs();
            self.ast.nodes[n as usize].a = sup;
            self.ast.nodes[n as usize].flags |= F_DERIVED;
            if ts && self.is("<") {
                self.type_args();
            }
        }
        if ts && self.eat("implements") {
            loop {
                self.skip_type();
                if !self.eat(",") {
                    break;
                }
            }
        }
        let saved = self.in_class;
        self.in_class = true;
        self.expect("{");
        let mut members: Vec<int> = Vec::new();
        while !self.is("}") && self.kind() != T_EOF {
            if self.eat(";") {
                continue;
            }
            if self.is("@") {
                self.fail("decorators are not supported");
                break;
            }
            let m = self.node(N_PROP);
            let mut flags = 0;
            // TypeScript's modifiers, in any order with `static`; a member
            // that is only a declaration (`declare`, `abstract`) emits nothing
            let mut erased = false;
            loop {
                if self.is("static") && self.member_name_ahead() {
                    self.next();
                    flags |= F_STATIC;
                    continue;
                }
                if self.is("static") && self.peek_is(1, "{") {
                    break;
                }
                if ts && self.class_modifier_ahead() {
                    if self.is("declare") || self.is("abstract") {
                        erased = true;
                    }
                    self.next();
                    continue;
                }
                break;
            }
            if (flags & F_STATIC) == 0 && self.is("static") && self.peek_is(1, "{") {
                self.next();
                // a static block: statements of the static initializer
                let b = self.function_body(m);
                self.ast.nodes[m as usize].flags = F_STATIC | F_FIELD;
                self.ast.nodes[m as usize].b = b;
                self.ast.nodes[m as usize].d = 1;
                members.push(m);
                continue;
            }
            if ts && self.is("[") && self.peek_kind(1) == T_IDENT && self.peek_is(2, ":") {
                // an index signature, `[key: string]: T;`
                self.skip_balanced();
                if self.eat(":") {
                    self.skip_type();
                }
                self.semicolon();
                continue;
            }
            let mut fflags = 0;
            if self.is("async") && self.member_name_ahead() && !self.peek_is(1, "=") && !self.peek_nl(1) {
                self.next();
                fflags |= F_ASYNC;
            }
            if self.eat("*") {
                fflags |= F_GENERATOR;
            }
            if (self.is("get") || self.is("set")) && self.member_name_ahead() {
                if self.is("get") {
                    flags |= F_GETTER;
                } else {
                    flags |= F_SETTER;
                }
                self.next();
            }
            let mut computed = false;
            let mut private = false;
            let key = self.prop_key(&mut computed, &mut private);
            if computed {
                flags |= F_COMPUTED;
            }
            if private {
                flags |= F_PRIVATE;
            }
            self.ast.nodes[m as usize].a = key;
            if ts {
                // optional `x?`, definite `x!`, a method's type parameters
                if !self.eat("?") {
                    self.eat("!");
                }
                if self.is("<") {
                    self.type_params();
                }
            }
            if self.is("(") {
                let kname = if computed { String::new() } else { self.key_name(key) };
                let is_ctor = !computed && (flags & F_STATIC) == 0 && kname.as_str() == "constructor" && self.ast.kind(key) == N_STR;
                let mut mf = fflags | (flags & (F_GETTER | F_SETTER | F_STATIC));
                if is_ctor {
                    mf |= F_CTOR;
                }
                let f = self.method(mf, kname.as_str());
                if self.no_body || erased {
                    // an overload or abstract signature
                    continue;
                }
                if is_ctor {
                    self.ast.nodes[n as usize].b = f;
                    continue;
                }
                self.ast.nodes[m as usize].b = f;
            } else {
                flags |= F_FIELD;
                if ts && self.eat(":") {
                    self.skip_type();
                }
                if self.eat("=") {
                    // a field initializer runs as a method of the instance
                    let saved_fn = self.in_function;
                    self.in_function = true;
                    let e = self.assign();
                    self.in_function = saved_fn;
                    self.ast.nodes[m as usize].b = e;
                }
                self.semicolon();
                if erased {
                    continue;
                }
            }
            self.ast.nodes[m as usize].flags = flags;
            members.push(m);
        }
        self.expect("}");
        self.in_class = saved;
        // fields and static blocks become two methods: one run on each
        // new instance, one run once on the constructor
        let line = self.line();
        let mut methods: Vec<int> = Vec::new();
        let mut inst: Vec<int> = Vec::new();
        let mut stat: Vec<int> = Vec::new();
        for m in members {
            let f = self.ast.nodes[m as usize].flags;
            if (f & F_FIELD) == 0 {
                methods.push(m);
                continue;
            }
            let st = if self.ast.nodes[m as usize].d == 1 {
                self.ast.nodes[m as usize].b
            } else {
                self.field_statement(m, line)
            };
            if (f & F_STATIC) != 0 {
                stat.push(st);
            } else {
                inst.push(st);
            }
        }
        self.ast.nodes[n as usize].list = methods;
        if !inst.is_empty() {
            let fnode = self.synthetic_method(inst, line, "");
            self.ast.nodes[n as usize].c = fnode;
        }
        if !stat.is_empty() {
            let fnode = self.synthetic_method(stat, line, "");
            self.ast.nodes[n as usize].d = fnode;
        }
        if self.ast.nodes[n as usize].b < 0 {
            // the default constructor
            let name = self.ast.nodes[n as usize].s.clone();
            let mut body: Vec<int> = Vec::new();
            let derived = (self.ast.nodes[n as usize].flags & F_DERIVED) != 0;
            let mut params: Vec<int> = Vec::new();
            if derived {
                let id = self.ast.add(N_IDENT, line);
                self.ast.nodes[id as usize].s = String::from("args");
                let r = self.ast.add(N_REST, line);
                self.ast.nodes[r as usize].a = id;
                params.push(r);
                let id2 = self.ast.add(N_IDENT, line);
                self.ast.nodes[id2 as usize].s = String::from("args");
                let sp = self.ast.add(N_SPREAD, line);
                self.ast.nodes[sp as usize].a = id2;
                let call = self.ast.add(N_SUPER_CALL, line);
                self.ast.nodes[call as usize].list = vec![sp];
                let st = self.ast.add(N_EXPR, line);
                self.ast.nodes[st as usize].a = call;
                body.push(st);
            }
            let f = self.synthetic_method(body, line, name.as_str());
            self.ast.nodes[f as usize].list = params;
            self.ast.nodes[f as usize].flags |= F_CTOR;
            self.ast.nodes[n as usize].b = f;
        }
        let ctor = self.ast.nodes[n as usize].b;
        let cname = self.ast.nodes[n as usize].s.clone();
        self.ast.nodes[ctor as usize].s = cname;
        if (self.ast.nodes[n as usize].flags & F_DERIVED) != 0 {
            self.ast.nodes[ctor as usize].flags |= F_DERIVED;
        }
        n
    }

    /// `this.key = init` defining the field `m`
    fn field_statement(&mut self, m: int, line: int) -> int {
        let f = self.ast.nodes[m as usize].flags;
        let key = self.ast.nodes[m as usize].a;
        let init = self.ast.nodes[m as usize].b;
        let this_n = self.ast.add(N_THIS, line);
        let target: int;
        if (f & F_COMPUTED) != 0 {
            target = self.ast.add(N_INDEX, line);
            self.ast.nodes[target as usize].a = this_n;
            self.ast.nodes[target as usize].b = key;
        } else {
            target = self.ast.add(N_MEMBER, line);
            self.ast.nodes[target as usize].a = this_n;
            let kname = self.key_name(key);
            self.ast.nodes[target as usize].s = kname;
        }
        let value = if init >= 0 { init } else { self.ast.add(N_UNDEF, line) };
        let asg = self.ast.add(N_ASSIGN, line);
        self.ast.nodes[asg as usize].op = String::from("define");
        self.ast.nodes[asg as usize].a = target;
        self.ast.nodes[asg as usize].b = value;
        let st = self.ast.add(N_EXPR, line);
        self.ast.nodes[st as usize].a = asg;
        st
    }

    fn synthetic_method(&mut self, body: Vec<int>, line: int, name: &str) -> int {
        let f = self.ast.add(N_FUNC, line);
        self.ast.nodes[f as usize].flags = F_METHOD | F_STRICT;
        self.ast.nodes[f as usize].s = String::from(name);
        let b = self.ast.add(N_BLOCK, line);
        self.ast.nodes[b as usize].list = body;
        self.ast.nodes[f as usize].a = b;
        f
    }

    /// After a word like `static`, `get` or `async`: does a member name
    /// follow (so the word is a modifier, not the name)?
    fn member_name_ahead(&mut self) -> bool {
        let k = self.peek_kind(1);
        if k == T_IDENT || k == T_STR || k == T_NUM || k == T_PRIVATE {
            return true;
        }
        self.peek_is(1, "[") || self.peek_is(1, "*") || self.peek_is(1, "#")
    }

    fn class_modifier_ahead(&mut self) -> bool {
        let w = self.is("public") || self.is("private") || self.is("protected") || self.is("readonly") || self.is("override") || self.is("abstract") || self.is("declare") || self.is("accessor");
        w && self.member_name_ahead() && !self.peek_nl(1)
    }

    fn key_name(&self, key: int) -> String {
        let k = &self.ast.nodes[key as usize];
        if k.kind == N_NUM {
            return crate::num::number_to_string(k.num);
        }
        k.s.clone()
    }

    // ---- expressions

    pub fn expression(&mut self) -> int {
        let first = self.assign();
        if !self.is(",") {
            return first;
        }
        let n = self.node(N_SEQ);
        let mut items: Vec<int> = vec![first];
        while self.eat(",") {
            let e = self.assign();
            items.push(e);
        }
        self.ast.nodes[n as usize].list = items;
        n
    }

    pub(crate) fn assign(&mut self) -> int {
        if self.arrow_ahead() {
            return self.arrow();
        }
        if self.is("yield") && self.in_function {
            self.fail("generators are not supported");
            return self.node(N_UNDEF);
        }
        let line = self.line();
        let left = self.conditional();
        if self.kind() == T_PUNCT {
            let op = self.text();
            if is_assign_op(op.as_str()) {
                self.next();
                let mut target = left;
                let lk = self.ast.kind(left);
                if op.as_str() == "=" && (lk == N_ARRAY || lk == N_OBJECT) {
                    target = self.to_pattern(left);
                } else if !(lk == N_IDENT || lk == N_MEMBER || lk == N_INDEX || lk == N_SUPER_MEMBER) {
                    self.fail("invalid assignment target");
                }
                let right = self.assign();
                let n = self.ast.add(N_ASSIGN, line);
                self.ast.nodes[n as usize].op = op;
                self.ast.nodes[n as usize].a = target;
                self.ast.nodes[n as usize].b = right;
                return n;
            }
        }
        left
    }

    fn conditional(&mut self) -> int {
        let line = self.line();
        let t = self.binary(0);
        if !self.is("?") {
            return t;
        }
        self.next();
        let saved = self.no_in;
        self.no_in = false;
        let a = self.assign();
        self.no_in = saved;
        self.expect(":");
        let b = self.assign();
        let n = self.ast.add(N_COND, line);
        self.ast.nodes[n as usize].a = t;
        self.ast.nodes[n as usize].b = a;
        self.ast.nodes[n as usize].c = b;
        n
    }

    fn binary(&mut self, min: int) -> int {
        let line = self.line();
        let mut left = self.unary();
        loop {
            if self.syntax.typescript && (self.is("as") || self.is("satisfies")) && !self.nl_before() && 8 >= min {
                // `x as T`, `x satisfies T`, `x as const`: the type goes
                self.next();
                if !self.eat("const") {
                    self.skip_type();
                }
                continue;
            }
            if !(self.kind() == T_PUNCT || (self.kind() == T_IDENT && (self.is("instanceof") || self.is("in")))) {
                break;
            }
            let op = self.text();
            if op.as_str() == "in" && self.no_in {
                break;
            }
            let prec = binary_prec(op.as_str());
            if prec < 0 || prec <= min - 1 || prec < min {
                break;
            }
            self.next();
            // `**` groups to the right
            let right = if op.as_str() == "**" { self.binary(prec) } else { self.binary(prec + 1) };
            let kind = if op.as_str() == "&&" || op.as_str() == "||" || op.as_str() == "??" { N_LOGICAL } else { N_BINARY };
            let n = self.ast.add(kind, line);
            self.ast.nodes[n as usize].op = op;
            self.ast.nodes[n as usize].a = left;
            self.ast.nodes[n as usize].b = right;
            left = n;
        }
        left
    }

    pub(crate) fn unary(&mut self) -> int {
        let line = self.line();
        if self.kind() == T_PUNCT {
            let op = self.text();
            let s = op.as_str();
            if s == "!" || s == "-" || s == "+" || s == "~" {
                self.next();
                let a = self.unary();
                if self.is("**") {
                    self.fail("unary operator before **");
                }
                let n = self.ast.add(N_UNARY, line);
                self.ast.nodes[n as usize].op = op;
                self.ast.nodes[n as usize].a = a;
                return n;
            }
            if s == "++" || s == "--" {
                self.next();
                let a = self.unary();
                let k = self.ast.kind(a);
                if !(k == N_IDENT || k == N_MEMBER || k == N_INDEX || k == N_SUPER_MEMBER) {
                    self.fail("invalid update target");
                }
                let n = self.ast.add(N_UPDATE, line);
                self.ast.nodes[n as usize].op = op;
                self.ast.nodes[n as usize].a = a;
                self.ast.nodes[n as usize].flags = 1; // prefix
                return n;
            }
        }
        if self.kind() == T_IDENT && !self.escaped() {
            let op = self.text();
            let s = op.as_str();
            if s == "typeof" || s == "void" || s == "delete" || (s == "await" && self.in_function) {
                self.next();
                let a = self.unary();
                let n = self.ast.add(N_UNARY, line);
                self.ast.nodes[n as usize].op = op;
                self.ast.nodes[n as usize].a = a;
                return n;
            }
        }
        let e = self.lhs();
        if self.kind() == T_PUNCT && !self.nl_before() && (self.is("++") || self.is("--")) {
            let k = self.ast.kind(e);
            if !(k == N_IDENT || k == N_MEMBER || k == N_INDEX || k == N_SUPER_MEMBER) {
                self.fail("invalid update target");
            }
            let op = self.text();
            self.next();
            let n = self.ast.add(N_UPDATE, line);
            self.ast.nodes[n as usize].op = op;
            self.ast.nodes[n as usize].a = e;
            return n;
        }
        e
    }

    fn arguments(&mut self) -> Vec<int> {
        self.expect("(");
        let mut args: Vec<int> = Vec::new();
        let saved = self.no_in;
        self.no_in = false;
        while !self.is(")") && self.kind() != T_EOF {
            if self.is("...") {
                let sp = self.node(N_SPREAD);
                self.next();
                let e = self.assign();
                self.ast.nodes[sp as usize].a = e;
                args.push(sp);
            } else {
                let e = self.assign();
                args.push(e);
            }
            if !self.eat(",") {
                break;
            }
        }
        self.no_in = saved;
        self.expect(")");
        args
    }

    /// Member access, calls, `new`, optional chains.
    pub(crate) fn lhs(&mut self) -> int {
        let line = self.line();
        let mut e: int;
        if self.is("new") {
            self.next();
            if self.eat(".") {
                let t = self.ident_name();
                if t.as_str() != "target" {
                    self.fail("new.target expected");
                }
                e = self.ast.add(N_NEW_TARGET, line);
            } else {
                let callee = self.member_only();
                let n = self.ast.add(N_NEW, line);
                self.ast.nodes[n as usize].a = callee;
                if self.is("(") {
                    let args = self.arguments();
                    self.ast.nodes[n as usize].list = args;
                }
                e = n;
            }
        } else if self.is("super") {
            self.next();
            if self.is("(") {
                let args = self.arguments();
                e = self.ast.add(N_SUPER_CALL, line);
                self.ast.nodes[e as usize].list = args;
            } else if self.eat(".") {
                let k = self.ast.add(N_STR, line);
                let name = self.ident_name();
                self.ast.nodes[k as usize].s = name;
                e = self.ast.add(N_SUPER_MEMBER, line);
                self.ast.nodes[e as usize].a = k;
            } else if self.eat("[") {
                let k = self.expression();
                self.expect("]");
                e = self.ast.add(N_SUPER_MEMBER, line);
                self.ast.nodes[e as usize].a = k;
                self.ast.nodes[e as usize].c = 1;
            } else {
                self.fail("unexpected super");
                e = self.ast.add(N_UNDEF, line);
            }
        } else {
            e = self.primary();
        }
        let mut chain = false;
        loop {
            let l = self.line();
            if self.is(".") {
                self.next();
                let n = self.ast.add(N_MEMBER, l);
                if self.kind() == T_PRIVATE {
                    let s = format!("\u{1}#{}", self.text());
                    self.ast.nodes[n as usize].s = s;
                    self.next();
                } else {
                    let name = self.ident_name();
                    self.ast.nodes[n as usize].s = name;
                }
                self.ast.nodes[n as usize].a = e;
                e = n;
            } else if self.is("?.") {
                self.next();
                chain = true;
                if self.kind() == T_PRIVATE {
                    // `o?.#x`
                    let n = self.ast.add(N_MEMBER, l);
                    let s = format!("\u{1}#{}", self.text());
                    self.ast.nodes[n as usize].s = s;
                    self.next();
                    self.ast.nodes[n as usize].a = e;
                    self.ast.nodes[n as usize].d = 1;
                    e = n;
                } else if self.is("(") {
                    let args = self.arguments();
                    let n = self.ast.add(N_CALL, l);
                    self.ast.nodes[n as usize].a = e;
                    self.ast.nodes[n as usize].list = args;
                    self.ast.nodes[n as usize].d = 1;
                    e = n;
                } else if self.eat("[") {
                    let k = self.expression();
                    self.expect("]");
                    let n = self.ast.add(N_INDEX, l);
                    self.ast.nodes[n as usize].a = e;
                    self.ast.nodes[n as usize].b = k;
                    self.ast.nodes[n as usize].d = 1;
                    e = n;
                } else {
                    let n = self.ast.add(N_MEMBER, l);
                    let name = self.ident_name();
                    self.ast.nodes[n as usize].s = name;
                    self.ast.nodes[n as usize].a = e;
                    self.ast.nodes[n as usize].d = 1;
                    e = n;
                }
            } else if self.is("[") {
                self.next();
                let saved = self.no_in;
                self.no_in = false;
                let k = self.expression();
                self.no_in = saved;
                self.expect("]");
                let n = self.ast.add(N_INDEX, l);
                self.ast.nodes[n as usize].a = e;
                self.ast.nodes[n as usize].b = k;
                e = n;
            } else if self.is("(") {
                let args = self.arguments();
                let n = self.ast.add(N_CALL, l);
                self.ast.nodes[n as usize].a = e;
                self.ast.nodes[n as usize].list = args;
                e = n;
            } else if self.kind() == T_TEMPLATE {
                let t = self.template(true);
                let n = self.ast.add(N_TAGGED, l);
                self.ast.nodes[n as usize].a = e;
                self.ast.nodes[n as usize].b = t;
                e = n;
            } else if self.syntax.typescript && self.is("!") && !self.nl_before() {
                // `x!`, not null
                self.next();
            } else if self.syntax.typescript && self.is("<") && self.call_type_args() {
                // `f<T>(…)`: the arguments follow
            } else {
                break;
            }
        }
        if chain {
            let n = self.ast.add(N_OPT_CHAIN, line);
            self.ast.nodes[n as usize].a = e;
            return n;
        }
        e
    }

    /// The callee of `new`: member accesses without calls.
    fn member_only(&mut self) -> int {
        let mut e: int;
        if self.is("new") {
            let line = self.line();
            self.next();
            let callee = self.member_only();
            let n = self.ast.add(N_NEW, line);
            self.ast.nodes[n as usize].a = callee;
            if self.is("(") {
                let args = self.arguments();
                self.ast.nodes[n as usize].list = args;
            }
            e = n;
        } else {
            e = self.primary();
        }
        loop {
            let l = self.line();
            if self.eat(".") {
                let n = self.ast.add(N_MEMBER, l);
                let name = self.ident_name();
                self.ast.nodes[n as usize].s = name;
                self.ast.nodes[n as usize].a = e;
                e = n;
            } else if self.eat("[") {
                let k = self.expression();
                self.expect("]");
                let n = self.ast.add(N_INDEX, l);
                self.ast.nodes[n as usize].a = e;
                self.ast.nodes[n as usize].b = k;
                e = n;
            } else if self.syntax.typescript && self.is("<") && self.call_type_args() {
                // `new C<T>(…)`
            } else {
                break;
            }
        }
        e
    }

    /// A template literal: N_STR parts (raw text in `op`, flags 1 when the
    /// cooked string is undefined) and the substitutions.
    fn template(&mut self, tagged: bool) -> int {
        let n = self.node(N_TEMPLATE);
        let parts = self.tk(self.pos).parts.clone();
        let exprs = self.tk(self.pos).exprs.clone();
        let raws = self.tk(self.pos).raws.clone();
        let oks = self.tk(self.pos).cooked_ok.clone();
        let line = self.line();
        self.next();
        let mut strs: Vec<int> = Vec::new();
        let mut i: usize = 0;
        for p in parts {
            let s = self.ast.add(N_STR, line);
            self.ast.nodes[s as usize].s = p;
            if i < raws.len() {
                self.ast.nodes[s as usize].op = raws[i].clone();
            }
            if i < oks.len() && !oks[i] {
                if !tagged {
                    self.fail("Invalid escape sequence in template");
                }
                self.ast.nodes[s as usize].flags = 1;
            }
            strs.push(s);
            i += 1;
        }
        let mut es: Vec<int> = Vec::new();
        for src in exprs {
            let mut sp = Parser::with_syntax(src.as_str(), self.syntax.clone());
            sp.in_function = self.in_function;
            sp.in_class = self.in_class;
            let e = sp.expression();
            if sp.kind() != T_EOF && sp.error.is_empty() {
                sp.fail("bad template substitution");
            }
            if !sp.error.is_empty() && self.error.is_empty() {
                self.error = sp.error.clone();
            }
            let g = self.graft(&sp.ast, e);
            es.push(g);
        }
        self.ast.nodes[n as usize].list = strs;
        self.ast.nodes[n as usize].list2 = es;
        n
    }

    pub(crate) fn primary(&mut self) -> int {
        let line = self.line();
        let k = self.kind();
        if k == T_NUM {
            let n = self.node(N_NUM);
            let v = self.tk(self.pos).num;
            self.ast.nodes[n as usize].num = v;
            self.next();
            return n;
        }
        if k == T_STR {
            let n = self.node(N_STR);
            let s = self.text();
            self.ast.nodes[n as usize].s = s;
            self.next();
            return n;
        }
        if k == T_TEMPLATE {
            return self.template(false);
        }
        if k == T_REGEX {
            let n = self.node(N_REGEX);
            let s = self.text();
            let f = self.tk(self.pos).flags.clone();
            self.ast.nodes[n as usize].s = s;
            self.ast.nodes[n as usize].op = f;
            self.next();
            return n;
        }
        if k == T_PRIVATE {
            // `#x in obj`
            let n = self.node(N_STR);
            let s = format!("\u{1}#{}", self.text());
            self.ast.nodes[n as usize].s = s;
            self.next();
            return n;
        }
        if k == T_IDENT {
            let w = self.text();
            let escaped = self.escaped();
            let ws = w.as_str();
            if !escaped {
                if ws == "function" {
                    return self.function(false, 0);
                }
                if ws == "async" && self.peek_is(1, "function") && !self.peek_nl(1) {
                    self.next();
                    return self.function(false, F_ASYNC);
                }
                if ws == "class" {
                    return self.class(false);
                }
                if ws == "this" {
                    self.next();
                    return self.ast.add(N_THIS, line);
                }
                if ws == "null" {
                    self.next();
                    return self.ast.add(N_NULL, line);
                }
                if ws == "true" {
                    self.next();
                    return self.ast.add(N_TRUE, line);
                }
                if ws == "false" {
                    self.next();
                    return self.ast.add(N_FALSE, line);
                }
                if ws == "import" {
                    self.fail("modules are not supported");
                    return self.ast.add(N_UNDEF, line);
                }
                if is_reserved(ws) {
                    self.fail(format!("unexpected token '{}'", ws).as_str());
                    return self.ast.add(N_UNDEF, line);
                }
            }
            let n = self.node(N_IDENT);
            self.ast.nodes[n as usize].s = w;
            self.next();
            return n;
        }
        if self.is("(") {
            self.next();
            let saved = self.no_in;
            self.no_in = false;
            let e = self.expression();
            self.no_in = saved;
            self.expect(")");
            return e;
        }
        if self.is("[") {
            let n = self.node(N_ARRAY);
            self.next();
            let mut items: Vec<int> = Vec::new();
            let saved = self.no_in;
            self.no_in = false;
            while !self.is("]") && self.kind() != T_EOF {
                if self.is(",") {
                    self.next();
                    let h = self.node(N_HOLE);
                    items.push(h);
                    continue;
                }
                if self.is("...") {
                    let sp = self.node(N_SPREAD);
                    self.next();
                    let e = self.assign();
                    self.ast.nodes[sp as usize].a = e;
                    items.push(sp);
                } else {
                    let e = self.assign();
                    items.push(e);
                }
                if !self.is("]") {
                    self.expect(",");
                }
            }
            self.no_in = saved;
            self.expect("]");
            self.ast.nodes[n as usize].list = items;
            return n;
        }
        if self.is("{") {
            return self.object_literal();
        }
        if self.is("<") && self.syntax.jsx {
            return self.jsx_element();
        }
        if self.is("<") && self.syntax.typescript {
            // `<T>x`, a type assertion (not in TSX, where it is an element)
            self.type_args();
            return self.unary();
        }
        let t = self.text();
        self.fail(format!("unexpected token '{}'", t).as_str());
        self.ast.add(N_UNDEF, line)
    }

    fn object_literal(&mut self) -> int {
        let n = self.node(N_OBJECT);
        self.next();
        let mut props: Vec<int> = Vec::new();
        let saved = self.no_in;
        self.no_in = false;
        while !self.is("}") && self.kind() != T_EOF {
            let p = self.node(N_PROP);
            if self.eat("...") {
                let e = self.assign();
                self.ast.nodes[p as usize].b = e;
                self.ast.nodes[p as usize].flags = F_SPREAD;
                props.push(p);
                if !self.is("}") {
                    self.expect(",");
                }
                continue;
            }
            let mut flags = 0;
            let mut fflags = 0;
            if self.is("async") && !self.peek_is(1, "(") && !self.peek_is(1, ":") && !self.peek_is(1, ",") && !self.peek_is(1, "}") && !self.peek_is(1, "=") {
                self.next();
                fflags |= F_ASYNC;
            }
            if self.eat("*") {
                fflags |= F_GENERATOR;
            }
            if (self.is("get") || self.is("set")) && !self.peek_is(1, "(") && !self.peek_is(1, ":") && !self.peek_is(1, ",") && !self.peek_is(1, "}") && !self.peek_is(1, "=") {
                if self.is("get") {
                    flags |= F_GETTER;
                } else {
                    flags |= F_SETTER;
                }
                self.next();
            }
            let key_tok_ident = self.kind() == T_IDENT;
            let key_text = self.text();
            let mut computed = false;
            let mut private = false;
            let key = self.prop_key(&mut computed, &mut private);
            if computed {
                flags |= F_COMPUTED;
            }
            self.ast.nodes[p as usize].a = key;
            if self.syntax.typescript && self.is("<") {
                self.type_params();
            }
            if self.is("(") {
                let kname = if computed { String::new() } else { self.key_name(key) };
                let f = self.method(fflags | (flags & (F_GETTER | F_SETTER)), kname.as_str());
                if self.no_body {
                    self.fail("method body expected");
                }
                self.ast.nodes[p as usize].b = f;
            } else if (flags & (F_GETTER | F_SETTER)) != 0 {
                self.fail("getter or setter without a body");
            } else if self.eat(":") {
                let v = self.assign();
                self.ast.nodes[p as usize].b = v;
            } else if key_tok_ident && !computed {
                // shorthand `{ a }` or, as a pattern, `{ a = 1 }`
                flags |= F_SHORTHAND;
                let ln = self.line();
                let id = self.ast.add(N_IDENT, ln);
                self.ast.nodes[id as usize].s = key_text.clone();
                if self.is("=") {
                    self.next();
                    let d = self.assign();
                    let ln2 = self.line();
                    let a = self.ast.add(N_ASSIGN, ln2);
                    self.ast.nodes[a as usize].op = String::from("=");
                    self.ast.nodes[a as usize].a = id;
                    self.ast.nodes[a as usize].b = d;
                    self.ast.nodes[p as usize].b = a;
                } else {
                    if is_reserved(key_text.as_str()) {
                        self.fail("unexpected reserved word");
                    }
                    self.ast.nodes[p as usize].b = id;
                }
            } else {
                self.fail("expected ':' in object literal");
            }
            self.ast.nodes[p as usize].flags = flags;
            props.push(p);
            if !self.is("}") {
                self.expect(",");
            }
        }
        self.no_in = saved;
        self.expect("}");
        self.ast.nodes[n as usize].list = props;
        n
    }
}
