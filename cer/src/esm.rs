// SPDX-License-Identifier: AGPL-3.0-or-later
//! ECMAScript modules as the specification writes them, for the module
//! loader in prelude.rs (`__cerxesESM`): `esm_compile` reads a module's
//! source on its tokens and answers its import and export entries and the
//! script that runs it.
//!
//! The script is a function of the module's imports object (an accessor per
//! imported name, which reads the exporting module's binding live) that
//! answers the module's body as a generator:
//!
//!   (function (__cerxesImports, __cerxesMeta, __cerxesRef, __cerxesSetExports) {
//!   with (__cerxesImports) { return function* __cerxesModuleBody() { 'use strict';
//!     __cerxesSetExports({ "x": () => x, "default": () => __cerxesDefault }); yield;
//!     <the module's text, its import declarations and `export` gone>
//!   }; } })
//!
//! Calling the generator function makes the module's environment (its
//! functions hoisted: a cycle may call them before the module runs); the
//! first `next()` hands over the getters of its local exports; the second
//! runs the body. A module with top-level `await` is an async generator.
//! pcomp knows the body by its name: no `return`, `new.target`, further
//! `yield` or `await` as a name there (module code).
//! The module's lines keep their numbers.

use ranger::prelude::*;

use crate::lexer::{Lexer, Tok, T_EOF, T_IDENT, T_NUM, T_PUNCT, T_STR, T_TEMPLATE, T_REGEX};

/// What a module is: its script, entries and requests, as JSON for the
/// loader, or a syntax error.
pub struct EsmModule {
    pub json: String,
    pub error: String,
}

struct Esm {
    toks: Vec<Tok>,
    src: Vec<char>,
    /// [local name, request, import name ("*": the namespace)]
    imports: Vec<Vec<String>>,
    /// export name → local name
    local_names: Vec<String>,
    local_locals: Vec<String>,
    /// [export name, request, import name ("*": the namespace)]
    indirect: Vec<Vec<String>>,
    stars: Vec<String>,
    requests: Vec<String>,
    exported: Vec<String>,
    /// the text, with spans replaced: (start, end, replacement) in order
    cuts_a: Vec<int>,
    cuts_b: Vec<int>,
    cuts_t: Vec<String>,
    /// an anonymous `export default function` (named "default")
    default_fn: bool,
    error: String,
    pend: Vec<Vec<String>>,
}

/// A JSON string literal.
fn jq(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
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
        } else if (c as u32) < 32 || c == '\u{2028}' || c == '\u{2029}' {
            out.push_str(format!("\\u{:04x}", c as u32).as_str());
        } else {
            out.push(c);
        }
    }
    out.push('"');
    out
}

fn jlist(v: &Vec<String>) -> String {
    let mut parts: Vec<String> = Vec::new();
    for s in v.iter() {
        parts.push(jq(s.as_str()));
    }
    format!("[{}]", parts.join(","))
}

fn jlist2(v: &Vec<Vec<String>>) -> String {
    let mut parts: Vec<String> = Vec::new();
    for s in v.iter() {
        parts.push(jlist(s));
    }
    format!("[{}]", parts.join(","))
}

impl Esm {
    fn kind(&self, i: int) -> int {
        if i < 0 || i >= self.toks.len() as int {
            return T_EOF;
        }
        self.toks[i as usize].kind
    }

    fn is(&self, i: int, s: &str) -> bool {
        if i < 0 || i >= self.toks.len() as int {
            return false;
        }
        let t = &self.toks[i as usize];
        (t.kind == T_PUNCT || t.kind == T_IDENT) && !t.escaped && t.text.as_str() == s
    }

    fn isnt(&self, i: int, s: &str) -> bool {
        !self.is(i, s)
    }

    fn text(&self, i: int) -> String {
        if i < 0 || i >= self.toks.len() as int {
            return String::new();
        }
        self.toks[i as usize].text.clone()
    }

    fn nl(&self, i: int) -> bool {
        if i < 0 || i >= self.toks.len() as int {
            return true;
        }
        self.toks[i as usize].nl
    }

    fn fail(&mut self, msg: &str) {
        if self.error.is_empty() {
            self.error = format!("SyntaxError: {}", msg);
        }
    }

    fn cut(&mut self, a: int, b: int, t: &str) {
        self.cuts_a.push(a);
        self.cuts_b.push(b);
        self.cuts_t.push(String::from(t));
    }

    /// The source span of tokens a..b (inclusive) blanked, its line breaks
    /// kept.
    fn blank(&mut self, a: int, b: int, t: &str) {
        let from = self.toks[a as usize].start;
        let to = self.toks[b as usize].end;
        let mut out = String::from(t);
        let mut k = from;
        while k < to {
            if self.src[k as usize] == '\n' {
                out.push('\n');
            }
            k += 1;
        }
        self.cut(from, to, out.as_str());
    }

    /// The token after the one matching the bracket at i.
    fn after_mate(&self, i: int) -> int {
        let mut depth: int = 0;
        let mut k = i;
        let n = self.toks.len() as int;
        while k < n {
            if self.kind(k) == T_PUNCT {
                let t = self.text(k);
                if t == "(" || t == "[" || t == "{" {
                    depth += 1;
                } else if t == ")" || t == "]" || t == "}" {
                    depth -= 1;
                    if depth == 0 {
                        return k + 1;
                    }
                }
            }
            k += 1;
        }
        n
    }

    fn request(&mut self, m: &str) {
        if !self.requests.contains(&String::from(m)) {
            self.requests.push(String::from(m));
        }
    }

    fn export_name(&mut self, name: &str) {
        if self.exported.contains(&String::from(name)) {
            self.fail(format!("Duplicate export of '{}'", name).as_str());
            return;
        }
        self.exported.push(String::from(name));
    }

    fn add_local(&mut self, export: &str, local: &str) {
        self.export_name(export);
        self.local_names.push(String::from(export));
        self.local_locals.push(String::from(local));
    }

    /// A module specifier at i (a string), its `with { … }` after it: the
    /// token after them.
    fn from_clause(&mut self, i: int) -> (String, int) {
        if self.kind(i) != T_STR {
            self.fail("module specifier expected");
            return (String::new(), i);
        }
        let m = self.text(i);
        let mut k = i + 1;
        if self.is(k, "with") && self.is(k + 1, "{") && !self.nl(k) {
            k = self.after_mate(k + 1);
        }
        (m, k)
    }

    /// The `;` that ends a declaration at i (or where automatic semicolon
    /// insertion puts one): the token after it.
    fn end_semi(&mut self, i: int) -> int {
        if self.is(i, ";") {
            return i + 1;
        }
        if self.kind(i) == T_EOF || self.nl(i) || self.is(i, "}") {
            return i;
        }
        self.fail("Missing semicolon");
        i
    }

    /// Whether the token at i starts a statement at the top level (after
    /// `export` / `export default`).
    fn stmt_start(&self, i: int) -> bool {
        let mut p = i - 1;
        if self.is(p, "default") && self.is(p - 1, "export") {
            p -= 2;
        } else if self.is(p, "export") {
            p -= 1;
        }
        p < 0 || self.is(p, ";") || self.is(p, "}")
    }

    /// An optional `;` at i: the token after the statement.
    fn semi(&self, i: int) -> int {
        if self.is(i, ";") {
            return i + 1;
        }
        i
    }

    /// A ModuleExportName at i (an identifier, or a string): its text.
    fn export_name_at(&mut self, i: int) -> String {
        let k = self.kind(i);
        if k == T_IDENT || k == T_STR {
            return self.text(i);
        }
        self.fail("export name expected");
        String::new()
    }

    /// The end of a statement or expression starting at i: the `;` at depth
    /// 0 (included) or the token automatic semicolon insertion stops at.
    fn stmt_end(&self, i: int) -> int {
        let mut depth: int = 0;
        let mut k = i;
        let n = self.toks.len() as int;
        while k < n {
            let kd = self.kind(k);
            if kd == T_EOF {
                return k;
            }
            if kd == T_PUNCT {
                let t = self.text(k);
                if t == "(" || t == "[" || t == "{" {
                    depth += 1;
                } else if t == ")" || t == "]" || t == "}" {
                    if depth == 0 {
                        return k;
                    }
                    depth -= 1;
                } else if t == ";" && depth == 0 {
                    return k + 1;
                }
            }
            if depth == 0 && k > i && self.nl(k) && self.ends_expr(k - 1) && self.starts_stmt(k) {
                return k;
            }
            k += 1;
        }
        n
    }

    fn ends_expr(&self, i: int) -> bool {
        let kd = self.kind(i);
        if kd == T_IDENT || kd == T_NUM || kd == T_STR || kd == T_TEMPLATE || kd == T_REGEX {
            return true;
        }
        self.is(i, ")") || self.is(i, "]") || self.is(i, "}") || self.is(i, "++") || self.is(i, "--")
    }

    fn starts_stmt(&self, i: int) -> bool {
        let kd = self.kind(i);
        if kd == T_IDENT || kd == T_NUM || kd == T_STR || kd == T_TEMPLATE {
            let t = self.text(i);
            return !(t == "in" || t == "of" || t == "instanceof");
        }
        self.is(i, "{") || self.is(i, "!") || self.is(i, "~") || self.is(i, "++") || self.is(i, "--") || self.is(i, ";") || self.is(i, "@")
    }

    /// The binding names of the pattern at i, up to (not past) the token
    /// that ends it: answers that token.
    fn pattern_names(&mut self, i: int, out: &mut Vec<String>) -> int {
        if self.kind(i) == T_IDENT && self.isnt(i, "{") && self.isnt(i, "[") {
            out.push(self.text(i));
            return i + 1;
        }
        if self.is(i, "[") {
            let mut k = i + 1;
            while self.isnt(k, "]") && self.kind(k) != T_EOF && self.error.is_empty() {
                if self.is(k, ",") {
                    k += 1;
                    continue;
                }
                if self.is(k, "...") {
                    k += 1;
                }
                k = self.pattern_names(k, out);
                if self.is(k, "=") {
                    k = self.skip_value(k + 1);
                }
                if self.is(k, ",") {
                    k += 1;
                }
            }
            return k + 1;
        }
        if self.is(i, "{") {
            let mut k = i + 1;
            while self.isnt(k, "}") && self.kind(k) != T_EOF && self.error.is_empty() {
                if self.is(k, "...") {
                    k = self.pattern_names(k + 1, out);
                } else {
                    // the key: a name, a string or number, or [computed]
                    let key_ident = self.kind(k) == T_IDENT;
                    let key_at = k;
                    if self.is(k, "[") {
                        k = self.after_mate(k);
                    } else {
                        k += 1;
                    }
                    if self.is(k, ":") {
                        k = self.pattern_names(k + 1, out);
                    } else if key_ident {
                        out.push(self.text(key_at));
                    }
                    if self.is(k, "=") {
                        k = self.skip_value(k + 1);
                    }
                }
                if self.is(k, ",") {
                    k += 1;
                }
            }
            return k + 1;
        }
        self.fail("binding pattern expected");
        i + 1
    }

    /// An initializer's value from i: the `,` / `;` / closing bracket at
    /// depth 0 that ends it (not included).
    fn skip_value(&self, i: int) -> int {
        let mut depth: int = 0;
        let mut k = i;
        let n = self.toks.len() as int;
        while k < n {
            let kd = self.kind(k);
            if kd == T_EOF {
                return k;
            }
            if kd == T_PUNCT {
                let t = self.text(k);
                if t == "(" || t == "[" || t == "{" {
                    depth += 1;
                } else if t == ")" || t == "]" || t == "}" {
                    if depth == 0 {
                        return k;
                    }
                    depth -= 1;
                } else if (t == "," || t == ";") && depth == 0 {
                    return k;
                }
            }
            if depth == 0 && k > i && self.nl(k) && self.ends_expr(k - 1) && self.starts_stmt(k) {
                return k;
            }
            k += 1;
        }
        n
    }

    /// `var` / `let` / `const` declarators from i (after the keyword): their
    /// names; answers the statement's end.
    fn declarators(&mut self, i: int, out: &mut Vec<String>) -> int {
        let mut k = i;
        loop {
            k = self.pattern_names(k, out);
            if !self.error.is_empty() {
                return k;
            }
            if self.is(k, "=") {
                k = self.skip_value(k + 1);
            }
            if self.is(k, ",") {
                k += 1;
                continue;
            }
            break;
        }
        self.semi(k)
    }

    /// `import …` at i (a declaration): its entries; answers the token after.
    fn import_decl(&mut self, i: int) -> int {
        let mut k = i + 1;
        if self.kind(k) == T_STR {
            let (m, e) = self.from_clause(k);
            self.request(m.as_str());
            return self.end_semi(e);
        }
        if (self.is(k, "defer") || self.is(k, "source")) && self.isnt(k + 1, "from") && self.isnt(k + 1, ",") {
            self.fail("import phases are not supported");
            return k;
        }
        let mut names: Vec<String> = Vec::new();
        let mut keys: Vec<String> = Vec::new();
        if self.kind(k) == T_IDENT && self.isnt(k, "{") && self.isnt(k, "*") {
            names.push(self.text(k));
            keys.push(String::from("default"));
            k += 1;
            if self.is(k, ",") {
                k += 1;
            }
        }
        if self.is(k, "*") {
            if self.isnt(k + 1, "as") || self.kind(k + 2) != T_IDENT {
                self.fail("'as' expected");
                return k;
            }
            names.push(self.text(k + 2));
            keys.push(String::from("*"));
            k += 3;
        } else if self.is(k, "{") {
            k += 1;
            while self.isnt(k, "}") && self.kind(k) != T_EOF && self.error.is_empty() {
                let key = self.export_name_at(k);
                let key_is_str = self.kind(k) == T_STR;
                k += 1;
                let mut local = key.clone();
                if self.is(k, "as") {
                    if self.kind(k + 1) != T_IDENT {
                        self.fail("identifier expected after 'as'");
                        return k;
                    }
                    local = self.text(k + 1);
                    k += 2;
                } else if key_is_str {
                    self.fail("a string import name needs 'as'");
                    return k;
                }
                names.push(local);
                keys.push(key);
                if self.is(k, ",") {
                    k += 1;
                } else if self.isnt(k, "}") {
                    self.fail("',' or '}' expected");
                    return k;
                }
            }
            k += 1;
        }
        if self.isnt(k, "from") {
            self.fail("'from' expected");
            return k;
        }
        let (m, e) = self.from_clause(k + 1);
        self.request(m.as_str());
        let mut j: usize = 0;
        while j < names.len() {
            let n = names[j].clone();
            if is_reserved_word(n.as_str()) {
                self.fail(format!("Unexpected reserved word '{}'", n).as_str());
            }
            for im in self.imports.iter() {
                if im[0].as_str() == n.as_str() {
                    self.error = format!("SyntaxError: Identifier '{}' has already been declared", n);
                }
            }
            let mut entry: Vec<String> = Vec::new();
            entry.push(n);
            entry.push(m.clone());
            entry.push(keys[j].clone());
            self.imports.push(entry);
            j += 1;
        }
        self.end_semi(e)
    }

    /// `export …` at i: its entries, its text rewritten; answers the token
    /// after the statement (or after the keyword, for a declaration that
    /// stays to be read on).
    fn export_decl(&mut self, i: int) -> int {
        let k = i + 1;
        if self.is(k, "*") {
            let mut j = k + 1;
            let mut as_name = String::new();
            if self.is(j, "as") {
                as_name = self.export_name_at(j + 1);
                j += 2;
            }
            if self.isnt(j, "from") {
                self.fail("'from' expected");
                return j;
            }
            let (m, e) = self.from_clause(j + 1);
            self.request(m.as_str());
            if as_name.is_empty() {
                self.stars.push(m);
            } else {
                self.export_name(as_name.as_str());
                let mut entry: Vec<String> = Vec::new();
                entry.push(as_name);
                entry.push(m);
                entry.push(String::from("*"));
                self.indirect.push(entry);
            }
            let end = self.end_semi(e);
            self.blank(i, end - 1, "");
            return end;
        }
        if self.is(k, "{") {
            let mut j = k + 1;
            let mut locals: Vec<String> = Vec::new();
            let mut exps: Vec<String> = Vec::new();
            let mut local_strs: Vec<bool> = Vec::new();
            while self.isnt(j, "}") && self.kind(j) != T_EOF && self.error.is_empty() {
                let l = self.export_name_at(j);
                local_strs.push(self.kind(j) == T_STR);
                j += 1;
                let mut x = l.clone();
                if self.is(j, "as") {
                    x = self.export_name_at(j + 1);
                    j += 2;
                }
                locals.push(l);
                exps.push(x);
                if self.is(j, ",") {
                    j += 1;
                } else if self.isnt(j, "}") {
                    self.fail("',' or '}' expected");
                    return j;
                }
            }
            j += 1;
            let mut end = j;
            if self.is(j, "from") {
                let (m, e) = self.from_clause(j + 1);
                self.request(m.as_str());
                let mut q: usize = 0;
                while q < locals.len() {
                    self.export_name(exps[q].as_str());
                    let mut entry: Vec<String> = Vec::new();
                    entry.push(exps[q].clone());
                    entry.push(m.clone());
                    entry.push(locals[q].clone());
                    self.indirect.push(entry);
                    q += 1;
                }
                end = self.end_semi(e);
            } else {
                let mut q: usize = 0;
                while q < locals.len() {
                    if local_strs[q] {
                        self.fail("a string export name needs 'from'");
                        return j;
                    }
                    if is_reserved_word(locals[q].as_str()) {
                        self.fail(format!("Unexpected reserved word '{}'", locals[q]).as_str());
                        return j;
                    }
                    // checked at the end: an import re-exported, or local
                    let mut entry: Vec<String> = Vec::new();
                    entry.push(exps[q].clone());
                    entry.push(locals[q].clone());
                    self.pending_local(entry);
                    q += 1;
                }
                end = self.end_semi(j);
            }
            self.blank(i, end - 1, "");
            return end;
        }
        if self.is(k, "default") {
            let d = k + 1;
            let is_async_fn = self.is(d, "async") && self.is(d + 1, "function") && !self.nl(d + 1);
            if self.is(d, "function") || is_async_fn {
                let f = if is_async_fn { d + 1 } else { d };
                let mut nm = f + 1;
                if self.is(nm, "*") {
                    nm += 1;
                }
                if self.kind(nm) == T_IDENT && self.isnt(nm, "(") {
                    let name = self.text(nm);
                    self.add_local("default", name.as_str());
                    self.blank(i, k, "");
                } else {
                    // an anonymous declaration: hoisted, named "default"
                    self.add_local("default", "__cerxesDefault");
                    self.default_fn = true;
                    self.blank(i, k, "");
                    let at = self.toks[(nm - 1) as usize].end;
                    self.cut(at, at, " __cerxesDefault");
                }
                return d;
            }
            if self.is(d, "class") {
                if self.kind(d + 1) == T_IDENT && self.isnt(d + 1, "extends") && self.isnt(d + 1, "{") {
                    let name = self.text(d + 1);
                    self.add_local("default", name.as_str());
                    self.blank(i, k, "");
                    return d;
                }
                // an anonymous class: a value named "default"
                self.add_local("default", "__cerxesDefault");
                let mut j = d + 1;
                let mut depth: int = 0;
                while self.kind(j) != T_EOF {
                    if self.is(j, "(") || self.is(j, "[") {
                        depth += 1;
                    } else if self.is(j, ")") || self.is(j, "]") {
                        depth -= 1;
                    } else if self.is(j, "{") && depth == 0 {
                        break;
                    }
                    j += 1;
                }
                let close = self.after_mate(j) - 1;
                self.blank(i, k, "const __cerxesDefault = ({ default: ");
                let at = self.toks[close as usize].end;
                self.cut(at, at, " }).default;");
                return d;
            }
            // an expression
            self.add_local("default", "__cerxesDefault");
            let mut end = self.stmt_end(d);
            if end > d && self.is(end - 1, ";") {
                end -= 1;
            }
            self.blank(i, k, "const __cerxesDefault = __cerxesESM.named(");
            let at = if end > d { self.toks[(end - 1) as usize].end } else { self.toks[k as usize].end };
            self.cut(at, at, ");");
            return end;
        }
        // a declaration: `export` goes, the declaration stays
        let mut names: Vec<String> = Vec::new();
        let next: int;
        if self.is(k, "var") || self.is(k, "let") || self.is(k, "const") {
            next = self.declarators(k + 1, &mut names);
        } else if self.is(k, "function") || (self.is(k, "async") && self.is(k + 1, "function")) {
            let mut nm = if self.is(k, "async") { k + 2 } else { k + 1 };
            if self.is(nm, "*") {
                nm += 1;
            }
            if self.kind(nm) != T_IDENT || self.is(nm, "(") {
                self.fail("function name expected");
                return k;
            }
            names.push(self.text(nm));
            next = k;
        } else if self.is(k, "class") {
            if self.kind(k + 1) != T_IDENT || self.is(k + 1, "{") || self.is(k + 1, "extends") {
                self.fail("class name expected");
                return k;
            }
            names.push(self.text(k + 1));
            next = k;
        } else {
            self.fail("Unexpected token after 'export'");
            return k;
        }
        for n in names.iter() {
            let nn = n.clone();
            self.add_local(nn.as_str(), nn.as_str());
        }
        self.blank(i, i, "");
        if next > k {
            return k;
        }
        next
    }

    fn pending_local(&mut self, entry: Vec<String>) {
        self.pend.push(entry);
    }
}

/// Words that cannot be a binding.
fn is_reserved_word(s: &str) -> bool {
    let words = ["break", "case", "catch", "class", "const", "continue", "debugger", "default", "delete", "do", "else", "enum", "export", "extends", "false", "finally", "for", "function", "if", "import", "in", "instanceof", "new", "null", "return", "super", "switch", "this", "throw", "true", "try", "typeof", "var", "void", "while", "with", "await", "yield", "let", "static", "implements", "interface", "package", "private", "protected", "public", "eval", "arguments"];
    for w in words.iter() {
        if *w == s {
            return true;
        }
    }
    false
}

/// The module `src` (its file `path`): the JSON the loader reads, or the
/// syntax error.
pub fn esm_compile(src: &str, path: &str, is_async: bool) -> EsmModule {
    let mut lx = Lexer::new(src);
    let toks = lx.tokenize();
    if !lx.error.is_empty() {
        return EsmModule { json: String::new(), error: format!("SyntaxError: {}", lx.error) };
    }
    let mut m = Esm {
        toks: toks,
        src: src.chars().collect::<Vec<char>>(),
        imports: Vec::new(),
        local_names: Vec::new(),
        local_locals: Vec::new(),
        indirect: Vec::new(),
        stars: Vec::new(),
        requests: Vec::new(),
        exported: Vec::new(),
        cuts_a: Vec::new(),
        cuts_b: Vec::new(),
        cuts_t: Vec::new(),
        default_fn: false,
        error: String::new(),
        pend: Vec::new(),
    };
    // HTML-like comments (Annex B) are not in module code: the text between
    // tokens is white space and comments
    let mut g: usize = 0;
    let mut from: int = 0;
    while g <= m.toks.len() && m.error.is_empty() {
        let to = if g < m.toks.len() && m.toks[g].kind != T_EOF && m.toks[g].start >= 0 { m.toks[g].start } else { m.src.len() as int };
        let mut k = from;
        while k < to {
            let c = m.src[k as usize];
            if c == '/' && k + 1 < to && m.src[(k + 1) as usize] == '*' {
                k += 2;
                while k + 1 < to && !(m.src[k as usize] == '*' && m.src[(k + 1) as usize] == '/') {
                    k += 1;
                }
                k += 2;
                continue;
            }
            if c == '/' && k + 1 < to && m.src[(k + 1) as usize] == '/' {
                while k < to && m.src[k as usize] != '\n' {
                    k += 1;
                }
                continue;
            }
            if (c == '<' && k + 3 < to && m.src[(k + 1) as usize] == '!' && m.src[(k + 2) as usize] == '-' && m.src[(k + 3) as usize] == '-') || (c == '-' && k + 2 < to && m.src[(k + 1) as usize] == '-' && m.src[(k + 2) as usize] == '>') {
                m.fail("HTML-like comments are not allowed in module code");
                break;
            }
            k += 1;
        }
        if g >= m.toks.len() || m.toks[g].kind == T_EOF {
            break;
        }
        from = m.toks[g].end;
        g += 1;
    }
    // `import.meta`: the module's meta object
    let n = m.toks.len() as int;
    let mut i: int = 0;
    while i + 2 < n {
        let prev_dot = i > 0 && (m.is(i - 1, ".") || m.is(i - 1, "?."));
        if !prev_dot && m.is(i, "import") && m.is(i + 1, ".") {
            if m.isnt(i + 2, "meta") {
                m.fail("The only valid meta property for import is 'import.meta'");
                break;
            }
            let a = m.toks[i as usize].start;
            let b = m.toks[(i + 2) as usize].end;
            m.cut(a, b, "(0, __cerxesMeta)");
            i += 3;
            continue;
        }
        i += 1;
    }
    // the module's top-level statements: import and export declarations
    let mut depth: int = 0;
    i = 0;
    while i < n && m.error.is_empty() {
        let kd = m.kind(i);
        if kd == T_EOF {
            break;
        }
        if kd == T_PUNCT {
            let t = m.text(i);
            if t == "(" || t == "[" || t == "{" {
                depth += 1;
            } else if t == ")" || t == "]" || t == "}" {
                depth -= 1;
            }
            i += 1;
            continue;
        }
        let prev_dot = i > 0 && (m.is(i - 1, ".") || m.is(i - 1, "?."));
        if depth == 0 && !prev_dot && m.is(i, "import") && m.isnt(i + 1, "(") && m.isnt(i + 1, ".") {
            let end = m.import_decl(i);
            if m.error.is_empty() {
                m.blank(i, end - 1, "");
            }
            i = end;
            continue;
        }
        if depth == 0 && !prev_dot && m.is(i, "export") {
            i = m.export_decl(i);
            continue;
        }
        i += 1;
    }
    if !m.error.is_empty() {
        return EsmModule { json: String::new(), error: m.error.clone() };
    }
    // the top level's declarations: a lexical name declared once, and not
    // also by `var`; import / export only there
    let mut lex: Vec<String> = Vec::new();
    let mut vars: Vec<String> = Vec::new();
    let mut allvars: Vec<String> = Vec::new();
    let mut q: usize = 0;
    while q < m.imports.len() {
        lex.push(m.imports[q][0].clone());
        q += 1;
    }
    depth = 0;
    i = 0;
    while i < n && m.error.is_empty() {
        let kd = m.kind(i);
        if kd == T_EOF {
            break;
        }
        if kd == T_PUNCT {
            let t = m.text(i);
            if t == "(" || t == "[" || t == "{" {
                depth += 1;
            } else if t == ")" || t == "]" || t == "}" {
                depth -= 1;
            }
            i += 1;
            continue;
        }
        let prev_dot = i > 0 && (m.is(i - 1, ".") || m.is(i - 1, "?."));
        if prev_dot || kd != T_IDENT {
            i += 1;
            continue;
        }
        if depth > 0 && (m.is(i, "export") || (m.is(i, "import") && m.isnt(i + 1, "(") && m.isnt(i + 1, "."))) && m.isnt(i + 1, ":") && m.isnt(i + 1, "(") && m.isnt(i + 1, "as") && m.isnt(i - 1, "as") {
            m.fail("import and export declarations may only appear at the top level of a module");
            break;
        }
        if m.is(i, "var") {
            let mut names: Vec<String> = Vec::new();
            m.declarators(i + 1, &mut names);
            let top = depth == 0 && m.stmt_start(i);
            for nm in names.iter() {
                allvars.push(nm.clone());
                if top {
                    vars.push(nm.clone());
                }
            }
        } else if depth == 0 && m.stmt_start(i) {
            let mut names: Vec<String> = Vec::new();
            if m.is(i, "let") || m.is(i, "const") {
                m.declarators(i + 1, &mut names);
            } else if m.is(i, "class") && m.kind(i + 1) == T_IDENT && m.isnt(i + 1, "extends") && m.isnt(i + 1, "{") {
                names.push(m.text(i + 1));
            } else if m.is(i, "function") || (m.is(i, "async") && m.is(i + 1, "function") && !m.nl(i + 1)) {
                let mut f = if m.is(i, "async") { i + 2 } else { i + 1 };
                if m.is(f, "*") {
                    f += 1;
                }
                if m.kind(f) == T_IDENT && m.isnt(f, "(") {
                    names.push(m.text(f));
                }
            }
            for nm in names.iter() {
                lex.push(nm.clone());
            }
        }
        i += 1;
    }
    if m.error.is_empty() {
        let mut a: usize = 0;
        while a < lex.len() {
            let mut b = a + 1;
            while b < lex.len() {
                if lex[a] == lex[b] {
                    m.fail(format!("Identifier '{}' has already been declared", lex[a]).as_str());
                }
                b += 1;
            }
            if vars.contains(&lex[a]) {
                m.fail(format!("Identifier '{}' has already been declared", lex[a]).as_str());
            }
            a += 1;
        }
    }
    if !m.error.is_empty() {
        return EsmModule { json: String::new(), error: m.error.clone() };
    }
    // `export { x }`: x imported is re-exported; else a local binding
    let pend = m.pend.clone();
    for e in pend.iter() {
        let mut found = false;
        let mut q: usize = 0;
        while q < m.imports.len() {
            if m.imports[q][0].as_str() == e[1].as_str() {
                let mut entry: Vec<String> = Vec::new();
                entry.push(e[0].clone());
                entry.push(m.imports[q][1].clone());
                entry.push(m.imports[q][2].clone());
                m.export_name(e[0].as_str());
                m.indirect.push(entry);
                found = true;
                break;
            }
            q += 1;
        }
        if !found {
            if !lex.contains(&e[1]) && !allvars.contains(&e[1]) {
                m.fail(format!("Export '{}' is not defined in module", e[1]).as_str());
            }
            m.add_local(e[0].as_str(), e[1].as_str());
        }
    }
    if !m.error.is_empty() {
        return EsmModule { json: String::new(), error: m.error.clone() };
    }
    // the body: the source with its cuts
    // in source order (stable: an insertion sort)
    let mut ord: Vec<usize> = Vec::new();
    let mut c: usize = 0;
    while c < m.cuts_a.len() {
        let mut p = ord.len();
        while p > 0 && m.cuts_a[ord[p - 1]] > m.cuts_a[c] {
            p -= 1;
        }
        ord.insert(p, c);
        c += 1;
    }
    let mut body = String::new();
    let mut at: int = 0;
    let total = m.src.len() as int;
    let mut oi: usize = 0;
    while oi < ord.len() {
        let c = ord[oi];
        oi += 1;
        let a = m.cuts_a[c];
        let b = m.cuts_b[c];
        if a >= at {
            while at < a {
                body.push(m.src[at as usize]);
                at += 1;
            }
            body.push_str(m.cuts_t[c].as_str());
            if b > at {
                at = b;
            }
        }
    }
    while at < total {
        body.push(m.src[at as usize]);
        at += 1;
    }
    let mut getters: Vec<String> = Vec::new();
    let mut q: usize = 0;
    while q < m.local_names.len() {
        getters.push(format!("{}: () => {}", jq(m.local_names[q].as_str()), m.local_locals[q]));
        q += 1;
    }
    let fix = if m.default_fn { "__cerxesESM.named(__cerxesDefault, true); " } else { "" };
    let text = format!(
        "(function (__cerxesImports, __cerxesMeta, __cerxesRef, __cerxesSetExports) {{ with (__cerxesImports) {{ return {}function* __cerxesModuleBody() {{ 'use strict'; {}__cerxesSetExports({{{}}}); yield; {}\n}}; }} }})",
        if is_async { "async " } else { "" },
        fix,
        getters.join(", "),
        body
    );
    let json = format!(
        "{{\"text\":{},\"path\":{},\"requests\":{},\"imports\":{},\"indirect\":{},\"stars\":{},\"locals\":{}}}",
        jq(text.as_str()),
        jq(path),
        jlist(&m.requests),
        jlist2(&m.imports),
        jlist2(&m.indirect),
        jlist(&m.stars),
        jlist(&m.local_names)
    );
    EsmModule { json: json, error: String::new() }
}
