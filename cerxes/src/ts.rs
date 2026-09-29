// SPDX-License-Identifier: AGPL-3.0-or-later
//! TypeScript's statements: the declarations erased (`type`, `interface`,
//! `declare`, `import type`), the ones with a runtime meaning written as
//! the JavaScript TypeScript emits for them (`enum`, `namespace`), and
//! `import` / `export`, which CErXes reads without a module system: an
//! `export` keyword is dropped, a type-only import is erased, a value
//! import is an error.

use ranger::prelude::*;

use cer::ast::*;
use crate::lexer::*;
use crate::parser::Parser;

fn is_identifier(s: &str) -> bool {
    let mut first = true;
    for c in s.chars() {
        if first {
            if !is_id_start(c) {
                return false;
            }
            first = false;
        } else if !is_id_part(c) {
            return false;
        }
    }
    !first && !crate::parser::is_reserved(s)
}

impl Parser {
    // ---- small trees

    pub(crate) fn mk_ident(&mut self, name: &str, line: int) -> int {
        let n = self.ast.add(N_IDENT, line);
        self.ast.nodes[n as usize].s = String::from(name);
        n
    }

    pub(crate) fn mk_str(&mut self, s: &str, line: int) -> int {
        let n = self.ast.add(N_STR, line);
        self.ast.nodes[n as usize].s = String::from(s);
        n
    }

    fn mk_assign(&mut self, target: int, value: int, line: int) -> int {
        let n = self.ast.add(N_ASSIGN, line);
        self.ast.nodes[n as usize].op = String::from("=");
        self.ast.nodes[n as usize].a = target;
        self.ast.nodes[n as usize].b = value;
        n
    }

    /// `obj["key"]`
    fn mk_index(&mut self, obj: &str, key: &str, line: int) -> int {
        let o = self.mk_ident(obj, line);
        let k = self.mk_str(key, line);
        let n = self.ast.add(N_INDEX, line);
        self.ast.nodes[n as usize].a = o;
        self.ast.nodes[n as usize].b = k;
        n
    }

    fn mk_stmt(&mut self, e: int, line: int) -> int {
        let n = self.ast.add(N_EXPR, line);
        self.ast.nodes[n as usize].a = e;
        n
    }

    fn mk_var(&mut self, name: &str, init: int, line: int) -> int {
        let v = self.ast.add(N_VAR, line);
        self.ast.nodes[v as usize].op = String::from("var");
        let d = self.ast.add(N_DECL, line);
        let id = self.mk_ident(name, line);
        self.ast.nodes[d as usize].a = id;
        self.ast.nodes[d as usize].b = init;
        self.ast.nodes[v as usize].list = vec![d];
        v
    }

    /// `var name; (function (name) { body })(name || (name = {}));` as a
    /// block named `name` (so an enclosing namespace can export it).
    fn mk_object_iife(&mut self, name: &str, body: Vec<int>, line: int) -> int {
        let decl = self.mk_var(name, -1, line);
        let f = self.ast.add(N_FUNC, line);
        let p = self.mk_ident(name, line);
        self.ast.nodes[f as usize].list = vec![p];
        let b = self.ast.add(N_BLOCK, line);
        self.ast.nodes[b as usize].list = body;
        self.ast.nodes[f as usize].a = b;
        let left = self.mk_ident(name, line);
        let target = self.mk_ident(name, line);
        let obj = self.ast.add(N_OBJECT, line);
        let asg = self.mk_assign(target, obj, line);
        let or = self.ast.add(N_LOGICAL, line);
        self.ast.nodes[or as usize].op = String::from("||");
        self.ast.nodes[or as usize].a = left;
        self.ast.nodes[or as usize].b = asg;
        let call = self.ast.add(N_CALL, line);
        self.ast.nodes[call as usize].a = f;
        self.ast.nodes[call as usize].list = vec![or];
        let st = self.mk_stmt(call, line);
        let blk = self.ast.add(N_BLOCK, line);
        self.ast.nodes[blk as usize].list = vec![decl, st];
        self.ast.nodes[blk as usize].s = String::from(name);
        blk
    }

    // ---- statements

    /// A TypeScript statement starting at the current word, or -1.
    pub(crate) fn ts_statement(&mut self) -> int {
        let w = self.text();
        let ws = w.as_str();
        let same_line_ident = self.peek_kind(1) == T_IDENT && !self.peek_nl(1);
        if ws == "type" && same_line_ident {
            // `type A<T> = …;`
            self.next();
            self.ident_name();
            if self.is("<") {
                self.type_params();
            }
            self.expect("=");
            self.skip_type();
            self.semicolon();
            return self.node(N_EMPTY);
        }
        if ws == "interface" && same_line_ident {
            self.next();
            self.ident_name();
            if self.is("<") {
                self.type_params();
            }
            if self.eat("extends") {
                loop {
                    self.skip_type();
                    if !self.eat(",") {
                        break;
                    }
                }
            }
            if !self.is("{") {
                self.fail("expected '{' in an interface");
                return self.node(N_EMPTY);
            }
            self.skip_balanced();
            return self.node(N_EMPTY);
        }
        if ws == "enum" && same_line_ident {
            return self.enum_decl();
        }
        if ws == "declare" && self.peek_kind(1) == T_IDENT && !self.peek_nl(1) {
            return self.declare_stmt();
        }
        if ws == "abstract" && self.peek_is(1, "class") && !self.peek_nl(1) {
            self.next();
            return self.class(true);
        }
        if (ws == "namespace" || ws == "module") && same_line_ident {
            self.next();
            return self.namespace_rest();
        }
        -1
    }

    /// `declare …`: read and dropped.
    fn declare_stmt(&mut self) -> int {
        self.next();
        if self.is("module") || self.is("namespace") || self.is("global") {
            while !self.is("{") && !self.is(";") && self.kind() != T_EOF {
                self.next();
            }
            if self.is("{") {
                self.skip_balanced();
            } else {
                self.eat(";");
            }
            return self.node(N_EMPTY);
        }
        if self.is("const") && self.peek_is(1, "enum") {
            self.next();
        }
        if self.is("enum") {
            self.enum_decl();
        } else {
            self.statement();
        }
        self.no_body = false;
        self.node(N_EMPTY)
    }

    /// `enum E { A, B = 5, C = "c" }`, the current token `enum`, as
    /// TypeScript emits it: numbers map back to names, members read each
    /// other by bare name in initializers.
    pub(crate) fn enum_decl(&mut self) -> int {
        let line = self.line();
        self.next();
        let name = self.binding_ident();
        self.expect("{");
        let mut body: Vec<int> = Vec::new();
        let mut prev = String::new();
        let mut first = true;
        while !self.is("}") && self.kind() != T_EOF {
            let l = self.line();
            let member: String;
            if self.kind() == T_STR {
                member = self.text();
                self.next();
            } else {
                member = self.ident_name();
            }
            let value: int;
            let mut is_string = false;
            if self.eat("=") {
                is_string = self.kind() == T_STR || (self.kind() == T_TEMPLATE && self.tk(self.pos).exprs.is_empty());
                value = self.assign();
            } else if first {
                value = self.ast.add(N_NUM, l);
            } else {
                // the previous member plus one
                let p = self.mk_index(name.as_str(), prev.as_str(), l);
                let one = self.ast.add(N_NUM, l);
                self.ast.nodes[one as usize].num = 1.0;
                let add = self.ast.add(N_BINARY, l);
                self.ast.nodes[add as usize].op = String::from("+");
                self.ast.nodes[add as usize].a = p;
                self.ast.nodes[add as usize].b = one;
                value = add;
            }
            let set = self.mk_index(name.as_str(), member.as_str(), l);
            let asg = self.mk_assign(set, value, l);
            if is_string {
                // E["A"] = "a";
                let st = self.mk_stmt(asg, l);
                body.push(st);
            } else {
                // E[E["A"] = v] = "A";
                let o = self.mk_ident(name.as_str(), l);
                let back = self.ast.add(N_INDEX, l);
                self.ast.nodes[back as usize].a = o;
                self.ast.nodes[back as usize].b = asg;
                let nm = self.mk_str(member.as_str(), l);
                let asg2 = self.mk_assign(back, nm, l);
                let st = self.mk_stmt(asg2, l);
                body.push(st);
            }
            if is_identifier(member.as_str()) && member != name {
                // later initializers name this member bare
                let rd = self.mk_index(name.as_str(), member.as_str(), l);
                let v = self.mk_var(member.as_str(), rd, l);
                body.push(v);
            }
            prev = member;
            first = false;
            if !self.eat(",") {
                break;
            }
        }
        self.expect("}");
        self.mk_object_iife(name.as_str(), body, line)
    }

    /// A namespace after `namespace`: `A.B.C { … }` nests, each inner one
    /// exported from the outer.
    fn namespace_rest(&mut self) -> int {
        let line = self.line();
        let name = self.binding_ident();
        let mut body: Vec<int> = Vec::new();
        if self.eat(".") {
            let inner = self.namespace_rest();
            let inner_name = self.ast.nodes[inner as usize].s.clone();
            body.push(inner);
            let e = self.export_assign(name.as_str(), inner_name.as_str(), line);
            body.push(e);
        } else {
            self.expect("{");
            while !self.is("}") && self.kind() != T_EOF {
                if self.is("export") {
                    self.next();
                    let st = self.statement();
                    let names = self.declared_names(st);
                    body.push(st);
                    for nm in names {
                        let e = self.export_assign(name.as_str(), nm.as_str(), line);
                        body.push(e);
                    }
                } else {
                    let st = self.statement();
                    body.push(st);
                }
            }
            self.expect("}");
        }
        self.mk_object_iife(name.as_str(), body, line)
    }

    /// `ns.name = name;`
    fn export_assign(&mut self, ns: &str, name: &str, line: int) -> int {
        let o = self.mk_ident(ns, line);
        let m = self.ast.add(N_MEMBER, line);
        self.ast.nodes[m as usize].a = o;
        self.ast.nodes[m as usize].s = String::from(name);
        let v = self.mk_ident(name, line);
        let asg = self.mk_assign(m, v, line);
        self.mk_stmt(asg, line)
    }

    /// The names a declaration statement binds.
    fn declared_names(&self, st: int) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let k = self.ast.kind(st);
        let nd = &self.ast.nodes[st as usize];
        if k == N_VAR {
            for d in nd.list.iter() {
                let t = self.ast.nodes[*d as usize].a;
                if self.ast.kind(t) == N_IDENT {
                    out.push(self.ast.nodes[t as usize].s.clone());
                }
            }
        } else if (k == N_FUNC || k == N_CLASS) && (nd.flags & F_DECL) != 0 {
            out.push(nd.s.clone());
        } else if k == N_BLOCK && !nd.s.is_empty() {
            out.push(nd.s.clone());
        }
        out
    }

    // ---- modules, without a module system

    /// Skips the rest of an `import` / `export` clause through its module
    /// name and semicolon.
    fn skip_module_clause(&mut self) {
        loop {
            if self.kind() == T_EOF {
                return;
            }
            if self.is(";") {
                self.next();
                return;
            }
            if self.is("{") {
                self.skip_balanced();
                if !self.is("from") {
                    self.semicolon();
                    return;
                }
                continue;
            }
            if self.is("from") || self.kind() == T_STR {
                if self.is("from") {
                    self.next();
                }
                if self.kind() == T_STR {
                    self.next();
                }
                if self.is("with") || self.is("assert") {
                    self.next();
                    if self.is("{") {
                        self.skip_balanced();
                    }
                }
                self.semicolon();
                return;
            }
            self.next();
        }
    }

    /// `import`: erased when it imports only types; `import A = B.C` an
    /// alias; anything else needs modules, which CEr does not have.
    pub(crate) fn import_stmt(&mut self) -> int {
        let line = self.line();
        let ts = self.syntax.typescript;
        if ts && self.peek_is(1, "type") && (self.peek_kind(2) == T_IDENT || self.peek_is(2, "{") || self.peek_is(2, "*")) && !self.peek_is(2, "from") {
            self.next();
            self.skip_module_clause();
            return self.node(N_EMPTY);
        }
        if ts && self.peek_is(1, "{") {
            // `import { type A, type B } from "m"`: only types
            let save = self.save();
            self.next();
            self.next();
            let mut only_types = true;
            while !self.is("}") && self.kind() != T_EOF {
                if !(self.is("type") && self.peek_kind(1) == T_IDENT && !self.peek_is(1, "as")) {
                    only_types = false;
                }
                while !self.is(",") && !self.is("}") && self.kind() != T_EOF {
                    self.next();
                }
                self.eat(",");
            }
            self.restore(save);
            if only_types {
                self.next();
                self.skip_module_clause();
                return self.node(N_EMPTY);
            }
        }
        if ts && self.peek_kind(1) == T_IDENT && self.peek_is(2, "=") && !self.peek_is(3, "require") {
            // `import A = B.C;`
            self.next();
            let name = self.binding_ident();
            self.expect("=");
            let e = self.lhs();
            self.semicolon();
            let v = self.ast.add(N_VAR, line);
            self.ast.nodes[v as usize].op = String::from("var");
            let d = self.ast.add(N_DECL, line);
            let id = self.mk_ident(name.as_str(), line);
            self.ast.nodes[d as usize].a = id;
            self.ast.nodes[d as usize].b = e;
            self.ast.nodes[v as usize].list = vec![d];
            return v;
        }
        self.fail("modules are not supported (import)");
        self.node(N_EMPTY)
    }

    /// `export`: the declaration stays, the keyword goes; re-exports and
    /// export lists are dropped.
    pub(crate) fn export_stmt(&mut self) -> int {
        self.next();
        if self.eat("default") {
            if self.is("function") || self.is("class") || (self.is("async") && self.peek_is(1, "function")) || self.is("abstract") || self.is("interface") {
                return self.statement();
            }
            let n = self.node(N_EXPR);
            let e = self.assign();
            self.ast.nodes[n as usize].a = e;
            self.semicolon();
            return n;
        }
        if self.is("=") {
            self.next();
            self.assign();
            self.semicolon();
            return self.node(N_EMPTY);
        }
        if self.is("{") || self.is("*") || (self.is("type") && self.peek_is(1, "{")) || (self.is("type") && self.peek_is(1, "*")) {
            self.skip_module_clause();
            return self.node(N_EMPTY);
        }
        if self.is("as") && self.peek_is(1, "namespace") {
            self.skip_module_clause();
            return self.node(N_EMPTY);
        }
        if self.is("import") {
            return self.import_stmt();
        }
        self.statement()
    }
}
