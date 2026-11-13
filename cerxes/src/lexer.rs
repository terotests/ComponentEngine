// SPDX-License-Identifier: AGPL-3.0-or-later
//! Tokens of a script, CEr's lexer (../cer/src/lexer.rs) read on demand:
//! the parser asks for the next token, and can move the lexer back to a
//! token's start to read JSX there character by character. Whether a `/`
//! starts a regular expression or divides is decided from the token before
//! it. A bad token is not fatal at once: it becomes a `T_ERROR` token, an
//! error only if the parser reaches it (lookahead over JSX read as script
//! may pass through text that is no script).

use ranger::prelude::*;

pub const T_EOF: int = 0;
pub const T_NUM: int = 1;
pub const T_STR: int = 2;
pub const T_IDENT: int = 3;
pub const T_PUNCT: int = 4;
pub const T_TEMPLATE: int = 5;
pub const T_REGEX: int = 6;
pub const T_PRIVATE: int = 7;
/// a token the lexer could not read; `text` is the message
pub const T_ERROR: int = 8;

pub struct Tok {
    pub kind: int,
    pub text: String,
    pub num: double,
    /// a line break before this token (automatic semicolons, `return`)
    pub nl: bool,
    pub line: int,
    /// a template's or a regular expression's extra (Lexer.xs), -1 for
    /// none: most tokens have none, and the token stays small
    pub x: int,
    /// an identifier written with an escape, or a string with an octal
    /// escape: not a keyword
    pub escaped: bool,
    /// where the token starts in the source (a char index)
    pub start: int,
    /// where the token ends (a char index)
    pub end: int,
    /// what a `(` or `)` did to the lexer's parentheses (PAREN_*), so that
    /// a rewind can undo it
    pub paren: int,
}

pub const PAREN_NONE: int = 0;
pub const PAREN_OPEN: int = 1;
pub const PAREN_CLOSE_KEYWORD: int = 2;
pub const PAREN_CLOSE: int = 3;
pub const PAREN_CLOSE_EXTRA: int = 4;

impl Tok {
    fn new(kind: int, line: int, nl: bool) -> Tok {
        Tok {
            kind: kind,
            text: String::new(),
            num: 0.0,
            nl: nl,
            line: line,
            x: -1,
            escaped: false,
            start: 0,
            end: -1,
            paren: PAREN_NONE,
        }
    }
}

/// What a template or a regular expression token has besides its text.
pub struct TokX {
    /// a template: the cooked strings around the substitutions
    pub parts: Vec<String>,
    /// a template: the source text of each `${…}`
    pub exprs: Vec<String>,
    /// a template: the raw strings, and whether each cooked string is
    /// valid (an invalid escape is allowed in a tagged template only)
    pub raws: Vec<String>,
    pub cooked_ok: Vec<bool>,
    /// a regular expression: its flags
    pub flags: String,
}

impl TokX {
    pub fn new() -> TokX {
        TokX { parts: Vec::new(), exprs: Vec::new(), raws: Vec::new(), cooked_ok: Vec::new(), flags: String::new() }
    }
}

pub struct Lexer {
    /// the tokens' extras (Tok.x)
    pub xs: Vec<TokX>,
    pub src: Vec<char>,
    pub pos: int,
    pub line: int,
    pub error: String,
    /// what the previous token allows a `/` to be
    pub regex_ok: bool,
    /// the open parentheses: whether each follows `if`, `while`, `for` or
    /// `with`, after whose `)` a `/` starts a regular expression
    parens: Vec<bool>,
    /// the previous token was one of those keywords
    paren_keyword: bool,
    puncts: Vec<String>,
    /// by a punctuator's first byte (below 128) the first in `puncts` that
    /// starts with it, and for each the next one that does: -1 for none
    pfirst: Vec<int>,
    pnext: Vec<int>,
    /// JSX: `tokenize` writes each element as the tokens of its factory's
    /// call (`jsx_tokens`), for pcomp
    pub jsx: bool,
    pub jsx_factory: String,
    pub jsx_fragment: String,
    /// tokens read ahead (an element's), the next one last
    pend: Vec<Tok>,
    /// the previous token was `.` or `?.`
    after_dot: bool,
    /// where a `type A<…> =` alias is read (0 none, 1 after `type`, 2 after
    /// its name, 3 after its `=`, 4 in its type parameters, `type_depth`
    /// their `<` open): after the `=` a `<` is a type's, not an element
    type_stage: int,
    type_depth: int,
    /// where a word `of` just ended (-1 none): `of<T>(` right after it is
    /// a method's type parameters (`static of<T>()`), not an element
    of_end: int,
    /// the native stack where reading began (stack_deeper_than)
    pub sp0: int,
    /// inside a substitution's scan for its `}`: a template read there is
    /// dropped, so its substitutions' text is not copied
    scanning: int,
}

/// The next token (pcomp reads them one at a time, never all of them as
/// `Tok`s); after an error (`error`), T_EOF.
pub fn next_tok(lx: &mut Lexer) -> Tok {
    let t = lx.next_token();
    if t.kind == T_ERROR {
        lx.error = t.text.clone();
        return Tok::new(T_EOF, lx.line, true);
    }
    t
}

/// A lexer for `src`; with `jsx` its elements are read as their factory's
/// calls (`@jsx` pragmas as `factory` and `fragment`).
pub fn lexer_for(src: &str, jsx: bool, factory: &str, fragment: &str) -> Lexer {
    let mut lx = Lexer::new(src);
    lx.jsx = jsx;
    lx.jsx_factory = String::from(factory);
    lx.jsx_fragment = String::from(fragment);
    lx
}

pub fn is_id_start(c: char) -> bool {
    if (c as int) < 128 {
        return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || c == '_' || c == '$';
    }
    crate::regex::id_wide(c, true)
}

pub fn is_id_part(c: char) -> bool {
    if (c as int) < 128 {
        return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || c == '_' || c == '$';
    }
    c == '\u{200c}' || c == '\u{200d}' || crate::regex::id_wide(c, false)
}

fn hex_val(c: char) -> int {
    if c >= '0' && c <= '9' {
        return (c as int) - 48;
    }
    if c >= 'a' && c <= 'f' {
        return (c as int) - 87;
    }
    if c >= 'A' && c <= 'F' {
        return (c as int) - 55;
    }
    -1
}

/// Punctuators, longest first so the scan takes the longest match.
/// punct_firsts and punct_nexts: the punctuators by their first byte.
fn punct_firsts() -> Vec<int> {
    let ps = puncts();
    let mut first: Vec<int> = Vec::new();
    let mut i: usize = 0;
    while i < 128 {
        first.push(-1);
        i += 1;
    }
    let mut k = ps.len();
    while k > 0 {
        k -= 1;
        let b = ps[k].as_bytes()[0] as usize;
        first[b] = k as int;
    }
    first
}

fn punct_nexts() -> Vec<int> {
    let ps = puncts();
    let mut next: Vec<int> = Vec::new();
    let mut k: usize = 0;
    while k < ps.len() {
        let b = ps[k].as_bytes()[0];
        let mut j = k + 1;
        let mut nx: int = -1;
        while j < ps.len() {
            if ps[j].as_bytes()[0] == b {
                nx = j as int;
                break;
            }
            j += 1;
        }
        next.push(nx);
        k += 1;
    }
    next
}

fn puncts() -> Vec<String> {
    let list = vec![
        ">>>=", "...", "===", "!==", "**=", "<<=", ">>=", ">>>", "&&=", "||=", "??=", "=>", "==", "!=", "<=", ">=",
        "&&", "||", "??", "?.", "++", "--", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "<<", ">>", "**", "{", "}",
        "(", ")", "[", "]", ";", ",", "<", ">", "+", "-", "*", "/", "%", "&", "|", "^", "!", "~", "?", ":", "=", ".",
        "@", "#",
    ];
    let mut out: Vec<String> = Vec::new();
    for p in list {
        out.push(String::from(p));
    }
    out
}

/// Keywords after which a `/` starts a regular expression.
fn regex_after_word(w: &str) -> bool {
    w == "return"
        || w == "typeof"
        || w == "instanceof"
        || w == "in"
        || w == "of"
        || w == "new"
        || w == "delete"
        || w == "void"
        || w == "throw"
        || w == "case"
        || w == "do"
        || w == "else"
        || w == "yield"
        || w == "await"
}

/// A lone surrogate escape (\uD83D) keeps its unit (jsstr::push_cp).
pub fn push_code_point(out: &mut String, cp: int) {
    crate::jsstr::push_code(out, cp);
}

impl Lexer {
    pub fn new(src: &str) -> Lexer {
        Lexer {
            xs: Vec::new(),
            src: src.chars().collect::<Vec<char>>(),
            pos: 0,
            line: 1,
            error: String::new(),
            regex_ok: true,
            parens: Vec::new(),
            paren_keyword: false,
            puncts: puncts(),
            pfirst: punct_firsts(),
            pnext: punct_nexts(),
            jsx: false,
            jsx_factory: String::from("__jsx"),
            jsx_fragment: String::from("__jsxFragment"),
            pend: Vec::new(),
            after_dot: false,
            type_stage: 0,
            type_depth: 0,
            of_end: -1,
            sp0: crate::vm::native_sp(),
            scanning: 0,
        }
    }

    pub fn at(&self, i: int) -> char {
        if i < 0 || i >= self.src.len() as int {
            return '\0';
        }
        self.src[i as usize]
    }

    pub fn cur(&self) -> char {
        self.at(self.pos)
    }

    fn fail(&mut self, msg: &str) {
        if self.error.is_empty() {
            self.error = format!("SyntaxError: {} (line {})", msg, self.line);
        }
    }

    /// Skips spaces and comments; true when a line break was passed.
    pub fn skip_space(&mut self) -> bool {
        let mut nl = false;
        let n = self.src.len() as int;
        while self.pos < n {
            let c = self.cur();
            if c == '\n' || c == '\r' || c == '\u{2028}' || c == '\u{2029}' {
                nl = true;
                if c == '\n' {
                    self.line += 1;
                }
                self.pos += 1;
            } else if c == ' ' || c == '\t' || c == '\u{b}' || c == '\u{c}' || c == '\u{a0}' || c == '\u{feff}' || (c as int > 127 && c.is_whitespace()) {
                self.pos += 1;
            } else if c == '/' && self.at(self.pos + 1) == '/' {
                while self.pos < n && self.cur() != '\n' && self.cur() != '\r' {
                    self.pos += 1;
                }
            } else if c == '/' && self.at(self.pos + 1) == '*' {
                self.pos += 2;
                let mut closed = false;
                while self.pos < n {
                    if self.cur() == '*' && self.at(self.pos + 1) == '/' {
                        self.pos += 2;
                        closed = true;
                        break;
                    }
                    if self.cur() == '\n' {
                        nl = true;
                        self.line += 1;
                    }
                    self.pos += 1;
                }
                if !closed {
                    self.fail("unterminated comment");
                }
            } else if c == '<' && self.at(self.pos + 1) == '!' && self.at(self.pos + 2) == '-' && self.at(self.pos + 3) == '-' {
                // an HTML-like comment, as a script allows
                while self.pos < n && self.cur() != '\n' {
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
        nl
    }

    /// The next token; after the end, `T_EOF` again and again.
    pub fn next_token(&mut self) -> Tok {
        if !self.pend.is_empty() {
            return self.pend.pop().unwrap();
        }
        let nl = self.skip_space();
        if self.jsx && self.error.is_empty() && self.type_stage != 3 && self.jsx_starts() {
            self.type_stage = 0;
            let mut out: Vec<Tok> = Vec::new();
            self.jsx_tokens(&mut out, nl);
            if !self.error.is_empty() {
                return self.error_token();
            }
            out.reverse();
            self.pend = out;
            return self.pend.pop().unwrap();
        }
        self.token_after_space(nl)
    }

    /// The token at `pos`, the space before it skipped (`nl`: a line break
    /// was in it).
    fn token_after_space(&mut self, nl: bool) -> Tok {
        let n = self.src.len() as int;
        if !self.error.is_empty() {
            return self.error_token();
        }
        if self.pos >= n {
            return Tok::new(T_EOF, self.line, true);
        }
        let start = self.pos;
        let keyword = self.paren_keyword;
        self.paren_keyword = false;
        let mut t = self.read_token(nl, keyword);
        t.start = start;
        t.end = self.pos;
        if !self.error.is_empty() {
            return self.error_token();
        }
        if self.jsx {
            self.type_alias_step(&t);
            self.of_end = if t.kind == T_IDENT && !t.escaped && t.text.as_str() == "of" { self.pos } else { -1 };
        }
        t
    }

    /// Follows a `type A<…> =` alias (`type_stage`) past the token `t`.
    fn type_alias_step(&mut self, t: &Tok) {
        let st = self.type_stage;
        let ident = t.kind == T_IDENT && !t.escaped;
        let punct = t.kind == T_PUNCT;
        let w = t.text.as_str();
        if st == 4 {
            if punct && w == "<" {
                self.type_depth += 1;
            } else if punct && (w == ">" || w == ">>" || w == ">>>") {
                self.type_depth -= w.as_bytes().len() as int;
                if self.type_depth <= 0 {
                    self.type_stage = 2;
                    self.type_depth = 0;
                }
            } else if t.kind == T_EOF {
                self.type_stage = 0;
            }
            return;
        }
        if st == 1 && ident {
            self.type_stage = 2;
        } else if st == 2 && punct && w == "=" {
            self.type_stage = 3;
        } else if st == 2 && punct && w == "<" {
            self.type_stage = 4;
            self.type_depth = 1;
        } else if ident && w == "type" {
            self.type_stage = 1;
        } else {
            self.type_stage = 0;
        }
    }

    /// All of plain JavaScript's tokens, ending in T_EOF (the single-pass
    /// compiler's input); on an error, `error` holds it and the tokens stop.
    pub fn tokenize(&mut self) -> Vec<Tok> {
        let mut toks: Vec<Tok> = Vec::new();
        loop {
            let t = self.next_token();
            let k = t.kind;
            if k == T_ERROR {
                self.error = t.text.clone();
                break;
            }
            grow_toks(&mut toks);
            toks.push(t);
            if k == T_EOF {
                break;
            }
        }
        toks
    }

    /// The pending error as a token; the lexer stops at the end.
    fn error_token(&mut self) -> Tok {
        let mut t = Tok::new(T_ERROR, self.line, false);
        t.text = self.error.clone();
        t.start = self.pos;
        self.error = String::new();
        self.pos = self.src.len() as int;
        t
    }

    /// Undoes what a token did to the parentheses (its `paren`; the parser
    /// forgets the token).
    pub fn unread_paren(&mut self, paren: int) {
        if paren == PAREN_OPEN {
            self.parens.pop();
        } else if paren == PAREN_CLOSE_KEYWORD {
            self.parens.push(true);
        } else if paren == PAREN_CLOSE {
            self.parens.push(false);
        }
    }

    fn read_token(&mut self, nl: bool, keyword: bool) -> Tok {
        let c = self.cur();
        let line = self.line;
        if is_id_start(c) || c == '\\' {
            let mut t = Tok::new(T_IDENT, line, nl);
            t.text = self.read_ident(&mut t.escaped);
            // a name after `.` is a property's, whatever its word
            self.regex_ok = regex_after_word(t.text.as_str()) && !t.escaped && !self.after_dot;
            self.after_dot = false;
            let w = t.text.as_str();
            self.paren_keyword = !t.escaped && (w == "if" || w == "while" || w == "for" || w == "with");
            return t;
        }
        if c == '#' && (is_id_start(self.at(self.pos + 1)) || self.at(self.pos + 1) == '\\') {
            self.pos += 1;
            let mut t = Tok::new(T_PRIVATE, line, nl);
            t.text = self.read_ident(&mut t.escaped);
            self.regex_ok = false;
            self.after_dot = false;
            return t;
        }
        if (c >= '0' && c <= '9') || (c == '.' && self.at(self.pos + 1) >= '0' && self.at(self.pos + 1) <= '9') {
            let mut t = Tok::new(T_NUM, line, nl);
            t.num = self.read_number(&mut t.text);
            self.regex_ok = false;
            return t;
        }
        if c == '"' || c == '\'' {
            let mut t = Tok::new(T_STR, line, nl);
            t.text = self.read_string(c, &mut t.escaped);
            self.regex_ok = false;
            return t;
        }
        if c == '`' {
            let mut t = Tok::new(T_TEMPLATE, line, nl);
            self.read_template(&mut t);
            self.regex_ok = false;
            return t;
        }
        if c == '/' && self.regex_ok {
            let mut t = Tok::new(T_REGEX, line, nl);
            self.read_regex(&mut t);
            self.regex_ok = false;
            return t;
        }
        let c0 = c as int;
        // the punctuators that start with c, longest first
        let mut k: int = if c0 < 128 { self.pfirst[c0 as usize] } else { -1 };
        while k >= 0 {
            let ki = k as usize;
            k = self.pnext[ki];
            let plen = self.puncts[ki].as_bytes().len() as int;
            let mut ok = true;
            let mut j: int = 1;
            while j < plen {
                if (self.at(self.pos + j) as int) != (self.puncts[ki].as_bytes()[j as usize] as int) {
                    ok = false;
                    break;
                }
                j += 1;
            }
            // `?.` followed by a digit is `?` and a number
            if ok && plen == 2 && c0 == 63 && (self.at(self.pos + 1) as int) == 46 {
                let d = self.at(self.pos + 2);
                if d >= '0' && d <= '9' {
                    ok = false;
                }
            }
            if ok {
                let p = self.puncts[ki].clone();
                let mut t = Tok::new(T_PUNCT, line, nl);
                self.pos += plen;
                let s = p.as_str();
                self.after_dot = s == "." || s == "?.";
                self.regex_ok = !(s == ")" || s == "]" || s == "}" || s == "++" || s == "--");
                if s == "}" {
                    self.regex_ok = true;
                }
                if s == "(" {
                    self.parens.push(keyword);
                    t.paren = PAREN_OPEN;
                } else if s == ")" {
                    let n = self.parens.len();
                    if n == 0 {
                        t.paren = PAREN_CLOSE_EXTRA;
                    } else if self.parens[n - 1] {
                        // `if (…) /re/.test(s)`: the condition's `)`
                        self.regex_ok = true;
                        t.paren = PAREN_CLOSE_KEYWORD;
                        self.parens.pop();
                    } else {
                        t.paren = PAREN_CLOSE;
                        self.parens.pop();
                    }
                }
                t.text = p;
                return t;
            }
        }
        self.fail(format!("unexpected character '{}'", c).as_str());
        Tok::new(T_ERROR, line, nl)
    }

    fn read_unicode_escape(&mut self) -> int {
        // after `\u`
        if self.cur() == '{' {
            self.pos += 1;
            let mut v: int = 0;
            let mut digits = 0;
            while self.cur() != '}' {
                let h = hex_val(self.cur());
                if h < 0 {
                    self.fail("bad unicode escape");
                    return 0;
                }
                v = v * 16 + h;
                digits += 1;
                self.pos += 1;
            }
            self.pos += 1;
            if digits == 0 || v > 0x10ffff {
                self.fail("bad unicode escape");
            }
            return v;
        }
        let mut v: int = 0;
        let mut k = 0;
        while k < 4 {
            let h = hex_val(self.cur());
            if h < 0 {
                self.fail("bad unicode escape");
                return 0;
            }
            v = v * 16 + h;
            self.pos += 1;
            k += 1;
        }
        v
    }

    fn read_ident(&mut self, escaped: &mut bool) -> String {
        let mut s = String::new();
        loop {
            let c = self.cur();
            if c == '\\' && self.at(self.pos + 1) == 'u' {
                self.pos += 2;
                let cp = self.read_unicode_escape();
                let ok = match char::from_u32(cp as u32) {
                    Some(ch) => {
                        if s.is_empty() {
                            is_id_start(ch)
                        } else {
                            is_id_part(ch)
                        }
                    }
                    None => false,
                };
                if !ok {
                    self.fail("Invalid Unicode escape sequence");
                }
                push_code_point(&mut s, cp);
                *escaped = true;
                continue;
            }
            if is_id_part(c) {
                s.push(c);
                self.pos += 1;
                continue;
            }
            break;
        }
        if s.is_empty() {
            // a `\` that starts no escape: step over it, or the lexer
            // reads the same empty name forever
            self.fail("Invalid or unexpected token");
            self.pos += 1;
        }
        s
    }

    fn read_number(&mut self, raw: &mut String) -> double {
        let start = self.pos;
        let c = self.cur();
        let nx = self.at(self.pos + 1);
        if c == '0' && (nx == 'x' || nx == 'X' || nx == 'o' || nx == 'O' || nx == 'b' || nx == 'B') {
            let base: double = if nx == 'x' || nx == 'X' {
                16.0
            } else if nx == 'o' || nx == 'O' {
                8.0
            } else {
                2.0
            };
            self.pos += 2;
            let mut v: double = 0.0;
            let mut digits = 0;
            let mut text = String::new();
            loop {
                let d = self.cur();
                if d == '_' {
                    let nh = hex_val(self.at(self.pos + 1));
                    if digits == 0 || nh < 0 || (nh as double) >= base {
                        self.fail("invalid numeric separator");
                    }
                    self.pos += 1;
                    continue;
                }
                let h = hex_val(d);
                if h < 0 || (h as double) >= base {
                    break;
                }
                v = v * base + (h as double);
                text.push(d);
                digits += 1;
                self.pos += 1;
            }
            if self.cur() == 'n' {
                // a BigInt literal: its decimal text, marked (as CEr's lexer)
                self.pos += 1;
                let dec = crate::bigint::from_radix(text.as_str(), base as int);
                *raw = format!("n:{}", dec);
            }
            if digits == 0 {
                self.fail("missing digits");
            }
            self.check_after_number();
            return v;
        }
        // legacy octal: 017
        if c == '0' && nx >= '0' && nx <= '9' {
            let mut k = self.pos + 1;
            let mut octal = true;
            while self.at(k) >= '0' && self.at(k) <= '9' {
                if self.at(k) >= '8' {
                    octal = false;
                }
                k += 1;
            }
            if octal {
                let mut v: double = 0.0;
                self.pos += 1;
                while self.cur() >= '0' && self.cur() <= '7' {
                    v = v * 8.0 + (((self.cur() as int) - 48) as double);
                    self.pos += 1;
                }
                raw.push_str("octal");
                return v;
            }
        }
        let mut text = String::new();
        let lead0 = c == '0';
        self.dec_digits(&mut text, lead0);
        // NonOctalDecimalIntegerLiteral (08, 09): legacy, not in strict code
        let nonoct = lead0 && nx >= '0' && nx <= '9';
        if nonoct {
            raw.push_str("octal");
        }
        if self.cur() == 'n' {
            if nonoct {
                self.fail("Invalid BigInt literal");
            }
            // a BigInt literal: its decimal text, marked (as CEr's lexer)
            self.pos += 1;
            self.check_after_number();
            let dec = crate::bigint::from_radix(text.as_str(), 10);
            *raw = format!("n:{}", dec);
            return text.parse::<f64>().unwrap_or(0.0);
        }
        if self.cur() == '.' {
            text.push('.');
            self.pos += 1;
            if self.cur() == '_' {
                self.fail("invalid numeric separator");
            }
            self.dec_digits(&mut text, false);
        }
        if self.cur() == 'e' || self.cur() == 'E' {
            let save = self.pos;
            let mut exp = String::from("e");
            self.pos += 1;
            if self.cur() == '+' || self.cur() == '-' {
                exp.push(self.cur());
                self.pos += 1;
            }
            if self.cur() == '_' {
                self.fail("invalid numeric separator");
            }
            let before = exp.as_bytes().len();
            self.dec_digits(&mut exp, false);
            let digits = exp.as_bytes().len() - before;
            if digits == 0 {
                self.pos = save;
            } else {
                text.push_str(exp.as_str());
            }
        }
        let _ = start;
        self.check_after_number();
        if text.starts_with(".") {
            text = format!("0{}", text);
        }
        if text.ends_with(".") {
            text.push('0');
        }
        text.parse::<f64>().unwrap_or(0.0)
    }

    /// Decimal digits with numeric separators (each `_` between two
    /// digits); none after a leading 0 (0_1, 08_1)
    fn dec_digits(&mut self, text: &mut String, lead0: bool) {
        loop {
            let d = self.cur();
            if d >= '0' && d <= '9' {
                text.push(d);
                self.pos += 1;
                continue;
            }
            if d == '_' {
                let prev = self.at(self.pos - 1);
                let nx = self.at(self.pos + 1);
                if lead0 || !(prev >= '0' && prev <= '9') || !(nx >= '0' && nx <= '9') {
                    self.fail("invalid numeric separator");
                    return;
                }
                self.pos += 1;
                continue;
            }
            break;
        }
    }

    fn check_after_number(&mut self) {
        let c = self.cur();
        if is_id_start(c) || (c >= '0' && c <= '9') {
            self.fail("identifier starts immediately after numeric literal");
        }
    }

    /// One escape sequence after the backslash; appends its value.
    fn read_escape(&mut self, out: &mut String, in_template: bool, octal: &mut bool) {
        let c = self.cur();
        self.pos += 1;
        if c == 'n' {
            out.push('\n');
        } else if c == 't' {
            out.push('\t');
        } else if c == 'r' {
            out.push('\r');
        } else if c == 'b' {
            out.push('\u{8}');
        } else if c == 'f' {
            out.push('\u{c}');
        } else if c == 'v' {
            out.push('\u{b}');
        } else if c == '0' && !(self.cur() >= '0' && self.cur() <= '9') {
            out.push('\0');
        } else if c >= '0' && c <= '7' {
            if in_template {
                self.fail("octal escape in template");
            }
            *octal = true;
            let mut v: int = (c as int) - 48;
            if self.cur() >= '0' && self.cur() <= '7' {
                v = v * 8 + ((self.cur() as int) - 48);
                self.pos += 1;
                if c <= '3' && self.cur() >= '0' && self.cur() <= '7' {
                    v = v * 8 + ((self.cur() as int) - 48);
                    self.pos += 1;
                }
            }
            push_code_point(out, v);
        } else if c == '8' || c == '9' {
            // NonOctalDecimalEscapeSequence: an error where octal ones are
            if in_template {
                self.fail("octal escape in template");
            }
            *octal = true;
            out.push(c);
        } else if c == 'x' {
            let a = hex_val(self.cur());
            let b = hex_val(self.at(self.pos + 1));
            if a < 0 || b < 0 {
                self.fail("bad hex escape");
                return;
            }
            self.pos += 2;
            push_code_point(out, a * 16 + b);
        } else if c == 'u' {
            let mut cp = self.read_unicode_escape();
            // a surrogate pair written as two escapes
            if cp >= 0xd800 && cp <= 0xdbff && self.cur() == '\\' && self.at(self.pos + 1) == 'u' {
                let save = self.pos;
                self.pos += 2;
                let lo = self.read_unicode_escape();
                if lo >= 0xdc00 && lo <= 0xdfff {
                    cp = 0x10000 + ((cp - 0xd800) * 1024) + (lo - 0xdc00);
                } else {
                    self.pos = save;
                }
            }
            push_code_point(out, cp);
        } else if c == '\r' {
            if self.cur() == '\n' {
                self.pos += 1;
            }
            self.line += 1;
        } else if c == '\n' || c == '\u{2028}' || c == '\u{2029}' {
            self.line += 1;
        } else {
            out.push(c);
        }
    }

    fn read_string(&mut self, q: char, octal: &mut bool) -> String {
        self.pos += 1;
        let mut s = String::new();
        let n = self.src.len() as int;
        loop {
            if self.pos >= n {
                self.fail("unterminated string");
                break;
            }
            let c = self.cur();
            if c == q {
                self.pos += 1;
                break;
            }
            if c == '\n' || c == '\r' {
                self.fail("unterminated string");
                break;
            }
            if c == '\\' {
                self.pos += 1;
                self.read_escape(&mut s, false, octal);
                continue;
            }
            // a character among the lone-surrogate stand-ins as its two units
            if (c as int) >= crate::jsstr::LONE {
                push_code_point(&mut s, c as int);
            } else {
                s.push(c);
            }
            self.pos += 1;
        }
        s
    }

    /// The raw text of a template part: the source with CR and CRLF as LF.
    fn raw_between(&self, from: int, to: int) -> String {
        let mut out = String::new();
        let mut i = from;
        while i < to {
            let c = self.src[i as usize];
            if c == '\r' {
                out.push('\n');
                if i + 1 < to && self.src[(i + 1) as usize] == '\n' {
                    i += 1;
                }
            } else {
                out.push(c);
            }
            i += 1;
        }
        out
    }

    /// An escape in a template: false when it is not a valid one (the
    /// cooked string is then undefined).
    fn template_escape(&mut self, out: &mut String) -> bool {
        let c = self.cur();
        if c >= '0' && c <= '9' {
            if c == '0' && !(self.at(self.pos + 1) >= '0' && self.at(self.pos + 1) <= '9') {
                self.pos += 1;
                out.push('\0');
                return true;
            }
            self.pos += 1;
            return false;
        }
        if c == 'x' {
            if hex_val(self.at(self.pos + 1)) < 0 || hex_val(self.at(self.pos + 2)) < 0 {
                self.pos += 1;
                return false;
            }
        }
        if c == 'u' {
            if self.at(self.pos + 1) == '{' {
                let mut i = self.pos + 2;
                let mut v: int = 0;
                let mut digits = 0;
                while hex_val(self.at(i)) >= 0 {
                    v = v * 16 + hex_val(self.at(i));
                    if v > 0x10ffff {
                        break;
                    }
                    digits += 1;
                    i += 1;
                }
                if digits == 0 || v > 0x10ffff || self.at(i) != '}' {
                    self.pos += 1;
                    return false;
                }
            } else {
                let mut k = 1;
                while k <= 4 {
                    if hex_val(self.at(self.pos + k)) < 0 {
                        self.pos += 1;
                        return false;
                    }
                    k += 1;
                }
            }
        }
        let mut octal = false;
        self.read_escape(out, true, &mut octal);
        true
    }

    /// A substitution's source up to its `}`, braces and string literals
    /// balanced; the current position is after `${`.
    fn substitution_chars(&mut self) -> String {
        let n = self.src.len() as int;
        let mut depth = 1;
        let mut e = String::new();
        while self.pos < n {
            let d = self.cur();
            if d == '{' {
                depth += 1;
            } else if d == '}' {
                depth -= 1;
                if depth == 0 {
                    self.pos += 1;
                    break;
                }
            } else if d == '"' || d == '\'' || d == '`' {
                // copy a nested literal whole
                e.push(d);
                self.pos += 1;
                let mut inner = 0;
                while self.pos < n {
                    let x = self.cur();
                    e.push(x);
                    self.pos += 1;
                    if x == '\\' {
                        e.push(self.cur());
                        self.pos += 1;
                        continue;
                    }
                    if d == '`' && x == '$' && self.cur() == '{' {
                        inner += 1;
                    } else if d == '`' && x == '}' && inner > 0 {
                        inner -= 1;
                    } else if x == d && inner == 0 {
                        break;
                    }
                }
                continue;
            } else if d == '\n' {
                self.line += 1;
            }
            e.push(d);
            self.pos += 1;
        }
        e
    }

    fn read_template(&mut self, t: &mut Tok) {
        let mut x = TokX::new();
        self.pos += 1;
        let n = self.src.len() as int;
        if crate::vm::stack_deeper_than(self.sp0, crate::vm::default_stack_budget()) {
            // templates nested in substitutions, read recursively
            self.fail("Maximum call stack size exceeded");
            self.pos = n;
            return;
        }
        let mut cur = String::new();
        let mut ok = true;
        let mut start = self.pos;
        loop {
            if self.pos >= n {
                self.fail("unterminated template");
                break;
            }
            let c = self.cur();
            if c == '`' {
                let raw = self.raw_between(start, self.pos);
                x.raws.push(raw);
                self.pos += 1;
                break;
            }
            if c == '\\' {
                self.pos += 1;
                if !self.template_escape(&mut cur) {
                    ok = false;
                }
                continue;
            }
            if c == '$' && self.at(self.pos + 1) == '{' {
                let raw = self.raw_between(start, self.pos);
                x.raws.push(raw);
                self.pos += 2;
                x.parts.push(cur.clone());
                x.cooked_ok.push(ok);
                ok = true;
                cur = String::new();
                // the substitution's source: its tokens up to the `}` that
                // closes it, so that a string, regular expression, comment
                // or template inside it may hold any brace
                let from = self.pos;
                let from_line = self.line;
                let saved_parens = self.parens.clone();
                let saved_regex = self.regex_ok;
                let saved_kw = self.paren_keyword;
                self.regex_ok = true;
                self.paren_keyword = false;
                let mut depth = 0;
                let mut to = n;
                // the templates read on the way are read again with the
                // substitution: their extras go (kept, a template nested n
                // deep would hold n copies of the text below it)
                let xs_mark = self.xs.len();
                self.scanning += 1;
                loop {
                    let tok = self.next_token();
                    if tok.kind == T_ERROR {
                        self.error = tok.text.clone();
                        break;
                    }
                    if tok.kind == T_EOF {
                        self.fail("unterminated template");
                        break;
                    }
                    if tok.kind == T_PUNCT {
                        if tok.text.as_str() == "{" || tok.text.as_str() == "${" {
                            depth += 1;
                        } else if tok.text.as_str() == "}" {
                            if depth == 0 {
                                to = tok.start;
                                break;
                            }
                            depth -= 1;
                        }
                    }
                }
                self.scanning -= 1;
                self.xs.truncate(xs_mark);
                self.regex_ok = saved_regex;
                self.paren_keyword = saved_kw;
                self.parens = saved_parens;
                let mut e = String::new();
                if !self.error.is_empty() {
                    // not JavaScript tokens (JSX in the substitution): the
                    // braces and quotes balanced instead
                    self.error = String::new();
                    self.pos = from;
                    self.line = from_line;
                    e = self.substitution_chars();
                } else if self.scanning == 0 {
                    let mut k = from;
                    while k < to {
                        e.push(self.src[k as usize]);
                        k += 1;
                    }
                }
                x.exprs.push(e);
                start = self.pos;
                continue;
            }
            if c == '\r' {
                // line terminators in a template are normalised to \n
                cur.push('\n');
                self.pos += 1;
                if self.cur() == '\n' {
                    self.pos += 1;
                }
                self.line += 1;
                continue;
            }
            if c == '\n' {
                self.line += 1;
            }
            cur.push(c);
            self.pos += 1;
        }
        x.parts.push(cur);
        x.cooked_ok.push(ok);
        if x.raws.len() < x.parts.len() {
            x.raws.push(String::new());
        }
        t.x = self.xs.len() as int;
        self.xs.push(x);
    }

    fn read_regex(&mut self, t: &mut Tok) {
        self.pos += 1;
        let n = self.src.len() as int;
        let mut body = String::new();
        let mut in_class = false;
        loop {
            if self.pos >= n || self.cur() == '\n' {
                self.fail("unterminated regular expression");
                return;
            }
            let c = self.cur();
            if c == '\\' {
                body.push(c);
                body.push(self.at(self.pos + 1));
                self.pos += 2;
                continue;
            }
            if c == '[' {
                in_class = true;
            } else if c == ']' {
                in_class = false;
            } else if c == '/' && !in_class {
                self.pos += 1;
                break;
            }
            body.push(c);
            self.pos += 1;
        }
        let mut flags = String::new();
        while is_id_part(self.cur()) {
            flags.push(self.cur());
            self.pos += 1;
        }
        t.text = body;
        let mut x = TokX::new();
        x.flags = flags;
        t.x = self.xs.len() as int;
        self.xs.push(x);
    }
}

// ---- JSX as tokens (lexer_for): an element is written as the tokens of
// its factory's call, as crate::jsx reads it into the tree:
//   <div id="a" {...p}>Hi {name}</div>  →  __jsx("div", {"id": "a", ...p}, "Hi ", name)

impl Lexer {
    /// A `<` here opens an element: where an expression starts, before a
    /// name or `>`, and not TypeScript's `<T,>` / `<T extends …>`.
    fn jsx_starts(&self) -> bool {
        if !self.regex_ok || self.cur() != '<' || self.pos == self.of_end {
            return false;
        }
        let c = self.at(self.pos + 1);
        if c == '>' {
            return true;
        }
        if !is_id_start(c) {
            return false;
        }
        let mut j = self.pos + 1;
        while is_id_part(self.at(j)) || self.at(j) == '-' {
            j += 1;
        }
        while self.at(j) == ' ' || self.at(j) == '\t' || self.at(j) == '\n' || self.at(j) == '\r' {
            j += 1;
        }
        if self.at(j) == ',' {
            return false;
        }
        if self.at(j) == 'e' && self.at(j + 1) == 'x' && self.at(j + 2) == 't' && self.at(j + 3) == 'e' && self.at(j + 4) == 'n' && self.at(j + 5) == 'd' && self.at(j + 6) == 's' && !is_id_part(self.at(j + 7)) {
            return false;
        }
        true
    }

    fn jsx_tok(&self, kind: int, text: &str, line: int, at: int) -> Tok {
        let mut t = Tok::new(kind, line, false);
        t.text = String::from(text);
        t.start = at;
        t.end = at;
        t
    }

    fn jsx_punct(&self, out: &mut Vec<Tok>, p: &str, line: int, at: int) {
        let t = self.jsx_tok(T_PUNCT, p, line, at);
        out.push(t);
    }

    fn jsx_str(&self, out: &mut Vec<Tok>, s: &str, line: int, at: int) {
        let t = self.jsx_tok(T_STR, s, line, at);
        out.push(t);
    }

    /// A dotted name's tokens: `React.createElement`, `this.Comp`.
    fn jsx_dotted(&self, out: &mut Vec<Tok>, dotted: &str, line: int, at: int) {
        let mut first = true;
        for part in dotted.split('.') {
            if !first {
                self.jsx_punct(out, ".", line, at);
            }
            let t = self.jsx_tok(T_IDENT, part, line, at);
            out.push(t);
            first = false;
        }
    }

    fn jsx_skip(&mut self) {
        self.skip_space();
    }

    fn jsx_expect(&mut self, c: char) {
        self.jsx_skip();
        if self.cur() == c {
            self.pos += 1;
            return;
        }
        let f = self.cur();
        self.fail(format!("expected '{}' in JSX but found '{}'", c, f).as_str());
    }

    /// The script inside `{…}` (the lexer is past the `{`) as tokens, up
    /// to its `}`, which is read too.
    fn jsx_script(&mut self, out: &mut Vec<Tok>) {
        self.regex_ok = true;
        let mut depth: int = 0;
        loop {
            if !self.error.is_empty() {
                return;
            }
            let nl = self.skip_space();
            if self.pos >= self.src.len() as int {
                self.fail("unterminated JSX expression");
                return;
            }
            if self.cur() == '}' && depth == 0 {
                self.pos += 1;
                return;
            }
            if self.jsx_starts() {
                self.jsx_tokens(out, nl);
                continue;
            }
            let start = self.pos;
            let keyword = self.paren_keyword;
            self.paren_keyword = false;
            let mut t = self.read_token(nl, keyword);
            t.start = start;
            t.end = self.pos;
            if !self.error.is_empty() {
                return;
            }
            if t.kind == T_PUNCT {
                if t.text.as_str() == "{" {
                    depth += 1;
                } else if t.text.as_str() == "}" {
                    depth -= 1;
                }
            }
            out.push(t);
        }
    }

    /// A tag name: its text (to match the closing tag), and its tokens.
    fn jsx_tag_tokens(&mut self, out: &mut Vec<Tok>, line: int, at: int) -> String {
        let first = self.jsx_ident();
        if first.is_empty() {
            let c = self.cur();
            self.fail(format!("expected a JSX tag name but found '{}'", c).as_str());
            return first;
        }
        if self.cur() == ':' {
            self.pos += 1;
            let second = self.jsx_ident();
            let text = format!("{}:{}", first, second);
            self.jsx_str(out, text.as_str(), line, at);
            return text;
        }
        if self.cur() == '.' {
            let mut text = first.clone();
            while self.cur() == '.' {
                self.pos += 1;
                let p = self.jsx_ident();
                if p.is_empty() {
                    self.fail("expected a name after '.' in a JSX tag");
                    break;
                }
                text.push('.');
                text.push_str(p.as_str());
            }
            self.jsx_dotted(out, text.as_str(), line, at);
            return text;
        }
        let c0 = first.chars().next().unwrap_or(' ');
        if (c0 >= 'a' && c0 <= 'z') || first.contains('-') {
            self.jsx_str(out, first.as_str(), line, at);
        } else {
            self.jsx_dotted(out, first.as_str(), line, at);
        }
        first
    }

    /// An element or a fragment at `<`, as its factory's call.
    pub fn jsx_tokens(&mut self, out: &mut Vec<Tok>, nl: bool) {
        let line = self.line;
        let at = self.pos;
        self.pos += 1;
        self.jsx_skip();
        let f = self.jsx_factory.clone();
        let fstart = out.len();
        self.jsx_dotted(out, f.as_str(), line, at);
        out[fstart].nl = nl;
        self.jsx_punct(out, "(", line, at);
        let fragment = self.cur() == '>';
        let mut tag_text = String::new();
        let mut closed = false;
        if fragment {
            self.pos += 1;
            let fr = self.jsx_fragment.clone();
            self.jsx_dotted(out, fr.as_str(), line, at);
            self.jsx_punct(out, ",", line, at);
            let t = self.jsx_tok(T_IDENT, "null", line, at);
            out.push(t);
        } else {
            tag_text = self.jsx_tag_tokens(out, line, at);
            self.jsx_punct(out, ",", line, at);
            if self.cur() == '<' {
                self.fail("type arguments on a JSX element are not supported");
                return;
            }
            let mut props = 0;
            loop {
                self.jsx_skip();
                if !self.error.is_empty() || self.pos >= self.src.len() as int {
                    self.fail("unterminated JSX tag");
                    return;
                }
                let c = self.cur();
                if c == '/' {
                    self.pos += 1;
                    self.jsx_expect('>');
                    closed = true;
                    break;
                }
                if c == '>' {
                    self.pos += 1;
                    break;
                }
                let al = self.line;
                let ap = self.pos;
                self.jsx_punct(out, if props == 0 { "{" } else { "," }, al, ap);
                props += 1;
                if c == '{' {
                    // `{...props}`
                    self.pos += 1;
                    self.jsx_script(out);
                    continue;
                }
                let mut name = self.jsx_ident();
                if name.is_empty() {
                    self.fail(format!("unexpected '{}' in a JSX tag", c).as_str());
                    return;
                }
                if self.cur() == ':' {
                    self.pos += 1;
                    let second = self.jsx_ident();
                    name = format!("{}:{}", name, second);
                }
                self.jsx_str(out, name.as_str(), al, ap);
                self.jsx_punct(out, ":", al, ap);
                self.jsx_skip();
                if self.cur() == '=' {
                    self.pos += 1;
                    self.jsx_skip();
                    let v = self.cur();
                    let vl = self.line;
                    let vp = self.pos;
                    if v == '"' || v == '\'' {
                        let raw = self.jsx_string();
                        if !self.error.is_empty() {
                            return;
                        }
                        let s = decode_entities(raw.as_str());
                        self.jsx_str(out, s.as_str(), vl, vp);
                    } else if v == '{' {
                        self.pos += 1;
                        let before = out.len();
                        self.jsx_script(out);
                        if out.len() == before {
                            self.fail("a JSX attribute value must not be empty");
                            return;
                        }
                    } else if v == '<' {
                        self.jsx_tokens(out, false);
                    } else {
                        self.fail("expected a JSX attribute value");
                        return;
                    }
                } else {
                    let t = self.jsx_tok(T_IDENT, "true", al, ap);
                    out.push(t);
                }
            }
            if props == 0 {
                let t = self.jsx_tok(T_IDENT, "null", line, at);
                out.push(t);
            } else {
                self.jsx_punct(out, "}", line, at);
            }
        }
        if !closed && self.error.is_empty() {
            self.jsx_children_tokens(out);
            if !self.error.is_empty() {
                return;
            }
            // `</name>`
            self.pos += 1;
            self.jsx_expect('/');
            self.jsx_skip();
            let mut end = String::new();
            if !fragment {
                let mut scratch: Vec<Tok> = Vec::new();
                end = self.jsx_tag_tokens(&mut scratch, line, at);
            }
            if self.error.is_empty() && end != tag_text {
                self.fail(format!("expected '</{}>' but found '</{}>'", tag_text, end).as_str());
                return;
            }
            self.jsx_expect('>');
        }
        self.jsx_punct(out, ")", line, at);
        // after an element a `/` divides
        self.regex_ok = false;
    }

    /// Children up to the `<` of the closing tag, each after a `,`.
    fn jsx_children_tokens(&mut self, out: &mut Vec<Tok>) {
        loop {
            if !self.error.is_empty() {
                return;
            }
            if self.pos >= self.src.len() as int {
                self.fail("unterminated JSX element");
                return;
            }
            let c = self.cur();
            let line = self.line;
            let at = self.pos;
            if c == '<' {
                let mut j = self.pos + 1;
                while self.at(j) == ' ' || self.at(j) == '\t' || self.at(j) == '\n' || self.at(j) == '\r' {
                    j += 1;
                }
                if self.at(j) == '/' {
                    return;
                }
                self.jsx_punct(out, ",", line, at);
                self.jsx_tokens(out, false);
                continue;
            }
            if c == '{' {
                self.pos += 1;
                let comma = out.len();
                self.jsx_punct(out, ",", line, at);
                self.jsx_script(out);
                if out.len() == comma + 1 {
                    // `{/* a comment */}`
                    out.pop();
                }
                continue;
            }
            let raw = self.jsx_text();
            let t = crate::jsx::clean_text(raw.as_str());
            if !t.is_empty() {
                let s = decode_entities(t.as_str());
                self.jsx_punct(out, ",", line, at);
                self.jsx_str(out, s.as_str(), line, at);
            }
        }
    }
}

// ---- JSX, read character by character from where the parser put `pos`

impl Lexer {
    /// A JSX identifier: a name that may also hold `-` (`data-id`).
    pub fn jsx_ident(&mut self) -> String {
        let mut s = String::new();
        if !is_id_start(self.cur()) {
            return s;
        }
        while is_id_part(self.cur()) || self.cur() == '-' {
            s.push(self.cur());
            self.pos += 1;
        }
        s
    }

    /// Text between tags up to the next `{` or `<`, as written.
    pub fn jsx_text(&mut self) -> String {
        let n = self.src.len() as int;
        let mut s = String::new();
        while self.pos < n {
            let c = self.cur();
            if c == '{' || c == '<' {
                break;
            }
            if c == '\n' {
                self.line += 1;
            }
            s.push(c);
            self.pos += 1;
        }
        s
    }

    /// An attribute's quoted value: no escapes, line breaks allowed.
    pub fn jsx_string(&mut self) -> String {
        let q = self.cur();
        self.pos += 1;
        let n = self.src.len() as int;
        let mut s = String::new();
        while self.pos < n && self.cur() != q {
            if self.cur() == '\n' {
                self.line += 1;
            }
            s.push(self.cur());
            self.pos += 1;
        }
        if self.pos >= n {
            self.fail("unterminated JSX string");
            return s;
        }
        self.pos += 1;
        s
    }
}

fn entity(name: &str) -> int {
    if name == "amp" {
        return 38;
    }
    if name == "lt" {
        return 60;
    }
    if name == "gt" {
        return 62;
    }
    if name == "quot" {
        return 34;
    }
    if name == "apos" {
        return 39;
    }
    if name == "nbsp" {
        return 160;
    }
    if name == "copy" {
        return 169;
    }
    if name == "reg" {
        return 174;
    }
    if name == "trade" {
        return 8482;
    }
    if name == "hellip" {
        return 8230;
    }
    if name == "mdash" {
        return 8212;
    }
    if name == "ndash" {
        return 8211;
    }
    if name == "laquo" {
        return 171;
    }
    if name == "raquo" {
        return 187;
    }
    if name == "middot" {
        return 183;
    }
    if name == "times" {
        return 215;
    }
    if name == "bull" {
        return 8226;
    }
    if name == "euro" {
        return 8364;
    }
    -1
}

/// JSX text and attribute strings take HTML character references:
/// `&amp;`, `&#123;`, `&#x7B;` and the common named ones.
pub fn decode_entities(s: &str) -> String {
    let cs = s.chars().collect::<Vec<char>>();
    let n = cs.len() as int;
    let mut out = String::new();
    let mut i: int = 0;
    while i < n {
        let c = cs[i as usize];
        if c == '&' {
            let mut j = i + 1;
            let mut name = String::new();
            while j < n && j - i <= 10 && cs[j as usize] != ';' {
                name.push(cs[j as usize]);
                j += 1;
            }
            if j < n && cs[j as usize] == ';' && !name.is_empty() {
                let cp: int;
                if name.starts_with("#") {
                    // `&#123;`, `&#x7B;`
                    let ds = name.chars().collect::<Vec<char>>();
                    let hex = ds.len() > 1 && (ds[1] == 'x' || ds[1] == 'X');
                    let base: int = if hex { 16 } else { 10 };
                    let mut k: int = if hex { 2 } else { 1 };
                    let mut v: int = 0;
                    let mut ok = (k as usize) < ds.len();
                    while (k as usize) < ds.len() {
                        let h = hex_val(ds[k as usize]);
                        if h < 0 || h >= base || v > 0x10ffff {
                            ok = false;
                            break;
                        }
                        v = v * base + h;
                        k += 1;
                    }
                    cp = if ok { v } else { -1 };
                } else {
                    cp = entity(name.as_str());
                }
                if cp >= 0 {
                    push_code_point(&mut out, cp);
                    i = j + 1;
                    continue;
                }
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Rust: the token buffer grows by a quarter (at least 256 tokens), not by
/// doubling.
#[ranger::target(rust)]
pub fn grow_toks(v: &mut Vec<Tok>) {
    if v.len() == v.capacity() {
        let n = v.len() / 4;
        v.reserve_exact(if n < 256 { 256 } else { n });
    }
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn grow_toks(_v: &mut Vec<Tok>) {}
