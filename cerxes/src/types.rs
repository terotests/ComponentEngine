// SPDX-License-Identifier: AGPL-3.0-or-later
//! TypeScript's type grammar, read only to be skipped: a type leaves no
//! nodes. Object and tuple types are skipped as balanced brackets; what
//! follows a type decides where it ends, as in TypeScript's own parser.
//!
//! Where an expression may or may not go on with type arguments (`f<T>(x)`
//! against `a < b`), the parser reads them speculatively: `save`, try,
//! and `restore` when they are not followed by a call.

use ranger::prelude::*;

use crate::lexer::*;
use crate::parser::Parser;

impl Parser {
    // ---- speculation

    /// Where the parser is: position, `>` splits, nodes.
    pub(crate) fn save(&self) -> (int, int, int) {
        (self.pos, self.splits.len() as int, self.ast.nodes.len() as int)
    }

    /// Back to `save`, forgetting any error since.
    pub(crate) fn restore(&mut self, s: (int, int, int)) {
        while self.splits.len() as int > s.1 {
            let (i, t) = self.splits.pop().unwrap();
            if (i as usize) < self.toks.len() {
                self.toks[i as usize].text = t;
                self.toks[i as usize].start -= 1;
            }
        }
        self.pos = s.0;
        self.error = String::new();
        self.ast.nodes.truncate(s.2 as usize);
    }

    /// Type arguments before a call, `f<T>(…)` / ``f<T>`…` ``; false (and
    /// nothing read) when the `<` is a comparison.
    pub(crate) fn call_type_args(&mut self) -> bool {
        if !self.error.is_empty() {
            return false;
        }
        let s = self.save();
        self.type_args();
        if self.error.is_empty() && (self.is("(") || self.kind() == T_TEMPLATE) {
            return true;
        }
        self.restore(s);
        false
    }

    // ---- brackets

    /// Skips from an opening `(`, `[` or `{` past its match.
    pub(crate) fn skip_balanced(&mut self) {
        let mut depth = 0;
        loop {
            let k = self.kind();
            if k == T_EOF || k == T_ERROR {
                self.fail("unbalanced brackets");
                return;
            }
            if k == T_PUNCT {
                let t = self.text();
                let s = t.as_str();
                if s == "(" || s == "[" || s == "{" {
                    depth += 1;
                } else if s == ")" || s == "]" || s == "}" {
                    depth -= 1;
                    if depth == 0 {
                        self.next();
                        return;
                    }
                }
            }
            self.next();
        }
    }

    /// A closing `>`, split off `>>`, `>=`, `>>=` … when the lexer read
    /// more (`Array<Array<number>>`).
    pub(crate) fn eat_gt(&mut self) {
        if self.is(">") {
            self.next();
            return;
        }
        let t = self.text();
        if self.kind() == T_PUNCT && t.len() > 1 && t.starts_with(">") {
            let i = self.pos;
            self.splits.push((i, t.clone()));
            let rest = t.chars().skip(1).collect::<String>();
            let tok = &mut self.toks[i as usize];
            tok.text = rest;
            tok.start += 1;
            return;
        }
        self.fail(format!("expected '>' but found '{}'", t).as_str());
    }

    // ---- type parameters and arguments

    /// `<T, U extends X = Y>`
    pub(crate) fn type_params(&mut self) {
        self.expect("<");
        while !self.is(">") && self.kind() != T_EOF {
            while (self.is("const") || self.is("in") || self.is("out")) && self.peek_kind(1) == T_IDENT {
                self.next();
            }
            self.ident_name();
            if self.eat("extends") {
                self.skip_type();
            }
            if self.eat("=") {
                self.skip_type();
            }
            if !self.eat(",") {
                break;
            }
        }
        self.eat_gt();
    }

    /// `<A, B>`
    pub(crate) fn type_args(&mut self) {
        self.expect("<");
        while !self.is(">") && self.kind() != T_EOF {
            self.skip_type();
            if !self.eat(",") {
                break;
            }
        }
        self.eat_gt();
    }

    // ---- types

    pub(crate) fn skip_type(&mut self) {
        self.ty(true);
    }

    /// A return type: a type, or a predicate `x is T` / `asserts x is T`.
    pub(crate) fn return_type(&mut self) {
        if self.is("asserts") && self.peek_kind(1) == T_IDENT && !self.peek_nl(1) {
            self.next();
            self.ident_name();
            if self.eat("is") {
                self.skip_type();
            }
            return;
        }
        if self.kind() == T_IDENT && self.peek_is(1, "is") && !self.peek_nl(1) {
            self.next();
            self.next();
            self.skip_type();
            return;
        }
        self.skip_type();
    }

    fn ty(&mut self, cond: bool) {
        if self.is("<") {
            // a generic function type
            self.type_params();
            self.fn_type_rest();
            return;
        }
        if self.is("abstract") && self.peek_is(1, "new") {
            self.next();
        }
        if self.is("new") {
            self.next();
            if self.is("<") {
                self.type_params();
            }
            self.fn_type_rest();
            return;
        }
        self.union_type();
        if cond && self.is("extends") && !self.nl_before() {
            self.next();
            self.ty(false);
            self.expect("?");
            self.ty(true);
            self.expect(":");
            self.ty(true);
        }
    }

    /// `(params) => R`
    fn fn_type_rest(&mut self) {
        if !self.is("(") {
            self.fail("expected '(' in a function type");
            return;
        }
        self.skip_balanced();
        self.expect("=>");
        self.return_type();
    }

    fn union_type(&mut self) {
        self.eat("|");
        self.intersection_type();
        while self.is("|") {
            self.next();
            self.intersection_type();
        }
    }

    fn intersection_type(&mut self) {
        self.eat("&");
        self.operator_type();
        while self.is("&") {
            self.next();
            self.operator_type();
        }
    }

    /// Does a type start at the token `k` ahead?
    fn type_starts(&mut self, k: int) -> bool {
        let kd = self.peek_kind(k);
        if kd == T_IDENT || kd == T_STR || kd == T_NUM || kd == T_TEMPLATE {
            return true;
        }
        self.peek_is(k, "(") || self.peek_is(k, "[") || self.peek_is(k, "{") || self.peek_is(k, "-") || self.peek_is(k, "<")
    }

    fn operator_type(&mut self) {
        if (self.is("keyof") || self.is("unique") || self.is("readonly")) && self.type_starts(1) {
            self.next();
            self.operator_type();
            return;
        }
        if self.is("infer") && self.peek_kind(1) == T_IDENT {
            self.next();
            self.next();
            return;
        }
        self.primary_type();
        // `T[]`, `T[K]`
        while self.is("[") && !self.nl_before() {
            self.next();
            if !self.is("]") {
                self.skip_type();
            }
            self.expect("]");
        }
    }

    fn primary_type(&mut self) {
        let k = self.kind();
        if self.is("(") {
            // parenthesized, or a function type's parameters
            self.skip_balanced();
            if self.is("=>") {
                self.next();
                self.return_type();
            }
            return;
        }
        if self.is("{") || self.is("[") {
            self.skip_balanced();
            return;
        }
        if self.is("-") {
            self.next();
            if self.kind() != T_NUM {
                self.fail("type expected");
                return;
            }
            self.next();
            return;
        }
        if k == T_STR || k == T_NUM || k == T_TEMPLATE {
            self.next();
            return;
        }
        if self.is("typeof") {
            self.next();
            if self.is("import") {
                self.import_type();
            } else {
                self.entity_name();
            }
            if self.is("<") && !self.nl_before() {
                self.type_args();
            }
            return;
        }
        if self.is("import") {
            self.import_type();
            return;
        }
        if k == T_IDENT {
            self.entity_name();
            if self.is("<") && !self.nl_before() {
                self.type_args();
            }
            return;
        }
        let t = self.text();
        self.fail(format!("type expected but found '{}'", t).as_str());
    }

    /// `A.B.C`
    fn entity_name(&mut self) {
        self.ident_name();
        while self.is(".") {
            self.next();
            self.ident_name();
        }
    }

    /// `import("m").A.B<T>`
    fn import_type(&mut self) {
        self.next();
        if !self.is("(") {
            self.fail("expected '(' after import");
            return;
        }
        self.skip_balanced();
        while self.eat(".") {
            self.ident_name();
        }
        if self.is("<") && !self.nl_before() {
            self.type_args();
        }
    }
}
