// SPDX-License-Identifier: AGPL-3.0-or-later
//! JSX, the classic transform: an element is a call of the factory
//! (`__jsx` unless a `@jsx` comment names another) with its type, its
//! props (`null` when it has none) and its children:
//!
//! ```text
//! <div id="a" {...p}>Hi {name}</div>  →  __jsx("div", {id: "a", ...p}, "Hi ", name)
//! <Card.Title />                      →  __jsx(Card.Title, null)
//! <>a</>                              →  __jsx(__jsxFragment, null, "a")
//! ```
//!
//! A tag is a string when it starts lower-case or holds `-` or `:`,
//! otherwise the value of that name. Text follows React's whitespace rule
//! (lines trimmed, blank lines dropped, the rest joined by one space) and
//! takes HTML character references.
//!
//! The lexer reads script ahead of the parser, so at a `<` that opens an
//! element the parser throws those tokens away, reads the element character
//! by character, and hands back to tokens for each `{…}` inside it.

use ranger::prelude::*;

use cer::ast::*;
use crate::lexer::*;
use crate::parser::{Parser, Syntax};

/// `@jsx h` and `@jsxFrag Fragment` anywhere in the source (they belong in
/// a comment) replace the factory and the fragment.
pub fn read_pragmas(src: &str, sx: &mut Syntax) {
    if !src.contains("@jsx") {
        return;
    }
    let cs = src.chars().collect::<Vec<char>>();
    let n = cs.len() as int;
    let mut i: int = 0;
    while i + 4 < n {
        if cs[i as usize] == '@' && cs[(i + 1) as usize] == 'j' && cs[(i + 2) as usize] == 's' && cs[(i + 3) as usize] == 'x' {
            let mut j = i + 4;
            let mut word = String::new();
            while j < n && cs[j as usize].is_alphabetic() {
                word.push(cs[j as usize]);
                j += 1;
            }
            if j < n && (cs[j as usize] == ' ' || cs[j as usize] == '\t') && (word.is_empty() || word.as_str() == "Frag") {
                while j < n && (cs[j as usize] == ' ' || cs[j as usize] == '\t') {
                    j += 1;
                }
                let mut name = String::new();
                while j < n && (is_id_part(cs[j as usize]) || cs[j as usize] == '.') {
                    name.push(cs[j as usize]);
                    j += 1;
                }
                if !name.is_empty() {
                    if word.is_empty() {
                        sx.jsx_factory = name;
                    } else {
                        sx.jsx_fragment = name;
                    }
                }
            }
            i = j;
            continue;
        }
        i += 1;
    }
}

/// React's rule for text between tags (Babel's
/// `cleanJSXElementLiteralChild`).
fn clean_text(raw: &str) -> String {
    let lines = raw.split('\n').map(|l| l.trim_end_matches('\r').replace('\t', " ")).collect::<Vec<String>>();
    let mut last_non_empty: int = -1;
    let mut i: int = 0;
    while i < lines.len() as int {
        if lines[i as usize].chars().any(|c| c != ' ') {
            last_non_empty = i;
        }
        i += 1;
    }
    let mut out = String::new();
    let last = (lines.len() as int) - 1;
    let mut k: int = 0;
    while k <= last {
        let mut t = lines[k as usize].clone();
        if k != 0 {
            t = String::from(t.trim_start_matches(' '));
        }
        if k != last {
            t = String::from(t.trim_end_matches(' '));
        }
        if !t.is_empty() {
            out.push_str(t.as_str());
            if k != last_non_empty {
                out.push(' ');
            }
        }
        k += 1;
    }
    out
}

impl Parser {
    /// An element at the current `<` token.
    pub(crate) fn jsx_element(&mut self) -> int {
        self.rewind_lexer();
        let e = self.jsx_at();
        // after an element a `/` divides
        self.lx.regex_ok = false;
        e
    }

    /// A dotted name as an expression: `React.createElement`.
    fn jsx_name_expr(&mut self, dotted: &str, line: int) -> int {
        let mut e: int = -1;
        for part in dotted.split('.') {
            if e < 0 {
                if part == "this" {
                    e = self.ast.add(N_THIS, line);
                } else {
                    e = self.mk_ident(part, line);
                }
            } else {
                let m = self.ast.add(N_MEMBER, line);
                self.ast.nodes[m as usize].a = e;
                self.ast.nodes[m as usize].s = String::from(part);
                e = m;
            }
        }
        e
    }

    fn jsx_space(&mut self) {
        self.lx.skip_space();
    }

    fn jsx_expect(&mut self, c: char) {
        self.jsx_space();
        if self.lx.cur() == c {
            self.lx.pos += 1;
            return;
        }
        let found = self.lx.cur();
        self.fail(format!("expected '{}' in JSX but found '{}'", c, found).as_str());
    }

    /// From tokens to the script inside `{`; the lexer is past the `{`.
    fn jsx_enter(&mut self) {
        self.toks.truncate(self.pos as usize);
        self.lx.regex_ok = true;
    }

    /// Back from tokens at the closing `}` to characters after it.
    fn jsx_leave(&mut self) {
        if !self.is("}") {
            let t = self.text();
            self.fail(format!("expected '}}' in JSX but found '{}'", t).as_str());
            return;
        }
        let start = self.tk(self.pos).start;
        let line = self.tk(self.pos).line;
        self.toks.truncate(self.pos as usize);
        self.splits.clear();
        self.lx.pos = start + 1;
        self.lx.line = line;
    }

    /// A tag name: its text (to match the closing tag) and its value.
    fn jsx_tag(&mut self) -> (String, int) {
        let line = self.lx.line;
        let first = self.lx.jsx_ident();
        if first.is_empty() {
            let c = self.lx.cur();
            self.fail(format!("expected a JSX tag name but found '{}'", c).as_str());
            return (first, -1);
        }
        if self.lx.cur() == ':' {
            self.lx.pos += 1;
            let second = self.lx.jsx_ident();
            let text = format!("{}:{}", first, second);
            let n = self.mk_str(text.as_str(), line);
            return (text, n);
        }
        if self.lx.cur() == '.' {
            let mut text = first.clone();
            while self.lx.cur() == '.' {
                self.lx.pos += 1;
                let p = self.lx.jsx_ident();
                if p.is_empty() {
                    self.fail("expected a name after '.' in a JSX tag");
                    break;
                }
                text.push('.');
                text.push_str(p.as_str());
            }
            let n = self.jsx_name_expr(text.as_str(), line);
            return (text, n);
        }
        let c0 = first.chars().next().unwrap_or(' ');
        if (c0 >= 'a' && c0 <= 'z') || first.contains('-') {
            let n = self.mk_str(first.as_str(), line);
            return (first, n);
        }
        let n = self.jsx_name_expr(first.as_str(), line);
        (first, n)
    }

    /// An element or fragment; the lexer is at its `<`.
    pub(crate) fn jsx_at(&mut self) -> int {
        let line = self.lx.line;
        self.lx.pos += 1;
        self.jsx_space();
        let fragment = self.lx.cur() == '>';
        let mut tag_text = String::new();
        let mut tag: int;
        let mut props: Vec<int> = Vec::new();
        let mut closed = false;
        if fragment {
            self.lx.pos += 1;
            let frag = self.syntax.jsx_fragment.clone();
            tag = self.jsx_name_expr(frag.as_str(), line);
        } else {
            let (t, n) = self.jsx_tag();
            tag_text = t;
            tag = n;
            if self.syntax.typescript && self.lx.cur() == '<' {
                self.fail("type arguments on a JSX element are not supported");
            }
            loop {
                self.jsx_space();
                if !self.error.is_empty() || self.lx.pos >= self.lx.src.len() as int {
                    self.fail("unterminated JSX tag");
                    break;
                }
                let c = self.lx.cur();
                if c == '/' {
                    self.lx.pos += 1;
                    self.jsx_expect('>');
                    closed = true;
                    break;
                }
                if c == '>' {
                    self.lx.pos += 1;
                    break;
                }
                let al = self.lx.line;
                if c == '{' {
                    // `{...props}`
                    self.lx.pos += 1;
                    self.jsx_enter();
                    self.expect("...");
                    let e = self.assign();
                    self.jsx_leave();
                    let p = self.ast.add(N_PROP, al);
                    self.ast.nodes[p as usize].b = e;
                    self.ast.nodes[p as usize].flags = F_SPREAD;
                    props.push(p);
                    continue;
                }
                let mut name = self.lx.jsx_ident();
                if name.is_empty() {
                    self.fail(format!("unexpected '{}' in a JSX tag", c).as_str());
                    break;
                }
                if self.lx.cur() == ':' {
                    self.lx.pos += 1;
                    let second = self.lx.jsx_ident();
                    name = format!("{}:{}", name, second);
                }
                self.jsx_space();
                let value: int;
                if self.lx.cur() == '=' {
                    self.lx.pos += 1;
                    self.jsx_space();
                    let v = self.lx.cur();
                    let vl = self.lx.line;
                    if v == '"' || v == '\'' {
                        let raw = self.lx.jsx_string();
                        if !self.lx.error.is_empty() {
                            let m = self.lx.error.clone();
                            self.lx.error = String::new();
                            self.error = m;
                            break;
                        }
                        let s = decode_entities(raw.as_str());
                        value = self.mk_str(s.as_str(), vl);
                    } else if v == '{' {
                        self.lx.pos += 1;
                        self.jsx_enter();
                        if self.is("}") {
                            self.fail("a JSX attribute value must not be empty");
                            break;
                        }
                        value = self.assign();
                        self.jsx_leave();
                    } else if v == '<' {
                        value = self.jsx_at();
                    } else {
                        self.fail("expected a JSX attribute value");
                        break;
                    }
                } else {
                    value = self.ast.add(N_TRUE, al);
                }
                let p = self.ast.add(N_PROP, al);
                let k = self.mk_str(name.as_str(), al);
                self.ast.nodes[p as usize].a = k;
                self.ast.nodes[p as usize].b = value;
                props.push(p);
            }
        }
        let mut args: Vec<int> = Vec::new();
        if tag < 0 {
            tag = self.ast.add(N_UNDEF, line);
        }
        args.push(tag);
        if props.is_empty() {
            let nl = self.ast.add(N_NULL, line);
            args.push(nl);
        } else {
            let o = self.ast.add(N_OBJECT, line);
            self.ast.nodes[o as usize].list = props;
            args.push(o);
        }
        if !closed && self.error.is_empty() {
            self.jsx_children(&mut args);
            // `</name>`
            self.lx.pos += 1;
            self.jsx_expect('/');
            self.jsx_space();
            let mut end = String::new();
            if !fragment {
                let (t, _) = self.jsx_tag();
                end = t;
            }
            if self.error.is_empty() && end != tag_text {
                self.fail(format!("expected '</{}>' but found '</{}>'", tag_text, end).as_str());
            }
            self.jsx_expect('>');
        }
        let f = self.syntax.jsx_factory.clone();
        let callee = self.jsx_name_expr(f.as_str(), line);
        let call = self.ast.add(N_CALL, line);
        self.ast.nodes[call as usize].a = callee;
        self.ast.nodes[call as usize].list = args;
        call
    }

    /// Children up to the `<` of the closing tag.
    fn jsx_children(&mut self, out: &mut Vec<int>) {
        loop {
            if !self.error.is_empty() {
                return;
            }
            if self.lx.pos >= self.lx.src.len() as int {
                self.fail("unterminated JSX element");
                return;
            }
            let c = self.lx.cur();
            let line = self.lx.line;
            if c == '<' {
                let mut j = self.lx.pos + 1;
                while self.lx.at(j) == ' ' || self.lx.at(j) == '\t' || self.lx.at(j) == '\n' || self.lx.at(j) == '\r' {
                    j += 1;
                }
                if self.lx.at(j) == '/' {
                    return;
                }
                let e = self.jsx_at();
                out.push(e);
                continue;
            }
            if c == '{' {
                self.lx.pos += 1;
                self.jsx_enter();
                if self.is("}") {
                    // `{/* a comment */}`
                    self.jsx_leave();
                    continue;
                }
                if self.is("...") {
                    let sp = self.node(N_SPREAD);
                    self.next();
                    let e = self.assign();
                    self.ast.nodes[sp as usize].a = e;
                    out.push(sp);
                } else {
                    let e = self.expression();
                    out.push(e);
                }
                self.jsx_leave();
                continue;
            }
            let raw = self.lx.jsx_text();
            let t = clean_text(raw.as_str());
            if !t.is_empty() {
                let s = decode_entities(t.as_str());
                let n = self.mk_str(s.as_str(), line);
                out.push(n);
            }
        }
    }
}
