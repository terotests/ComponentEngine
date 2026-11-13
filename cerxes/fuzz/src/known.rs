// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Findings that are known and not fixed yet (known/known.txt), so that a
// run reports what is new. A known finding is still counted (`known
// <class>` in the summary, with hits per entry), so a fix shows as an
// entry whose hits drop to zero. Programs that match a `skip` line run
// only now and then: a known hang otherwise eats a worker for its whole
// timeout every time.
//
// known.txt, tab-separated, `#` starts a comment:
//   <class>  <text>               a finding of <class> whose signature or
//                                 detail contains <text> (* = any)
//   skip     ends-with <text>     a program that ends with <text>
//   skip     contains <text>      a program that contains <text>
//   skip     lone-backslash       a `\` outside strings, comments and
//                                 regexps that does not start `\u`
// A third / fourth column is a note. In <text>, \t \n \\ are escapes.

use std::path::Path;

pub struct Entry {
    pub class: String,
    pub text: String,
    pub note: String,
}

pub struct Skip {
    pub ends: bool,
    pub lone_backslash: bool,
    pub text: String,
    pub note: String,
}

#[derive(Default)]
pub struct Known {
    pub entries: Vec<Entry>,
    pub skips: Vec<Skip>,
}

fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some(x) => out.push(x),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

impl Known {
    pub fn load(path: &Path) -> Known {
        let mut k = Known::default();
        let text = std::fs::read_to_string(path).unwrap_or_default();
        for line in text.lines() {
            let l = line.trim_end();
            if l.trim().is_empty() || l.trim_start().starts_with('#') {
                continue;
            }
            let f: Vec<&str> = l.split('\t').filter(|x| !x.is_empty()).collect();
            if f.len() < 2 {
                continue;
            }
            if f[0] == "skip" && f.len() >= 3 {
                k.skips.push(Skip { ends: f[1] == "ends-with", lone_backslash: false, text: unescape(f[2]), note: f.get(3).unwrap_or(&"").to_string() });
            } else if f[0] == "skip" && f[1] == "lone-backslash" {
                k.skips.push(Skip { ends: false, lone_backslash: true, text: String::from("\\"), note: f.get(2).unwrap_or(&"").to_string() });
            } else {
                k.entries.push(Entry { class: f[0].to_string(), text: unescape(f[1]), note: f.get(2).unwrap_or(&"").to_string() });
            }
        }
        k
    }

    /// The entry a finding is, if it is known.
    pub fn find(&self, class: &str, sig: &str, detail: &str) -> Option<usize> {
        self.entries.iter().position(|e| e.class == class && (e.text == "*" || sig.contains(&e.text) || detail.contains(&e.text)))
    }

    /// The skip line a program matches.
    pub fn skip(&self, src: &str) -> Option<usize> {
        let end = src.trim_end();
        self.skips.iter().position(|s| if s.lone_backslash { lone_backslash(src) } else if s.ends { end.ends_with(&s.text) } else { src.contains(&s.text) })
    }
}

/// A backslash in code (not in a string, template, comment or regexp)
/// that does not start a `\u` escape. A rough scan: a regexp is a `/`
/// after an operator or bracket. A wrong guess only skips a program.
pub fn lone_backslash(src: &str) -> bool {
    let b: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut prev = '\n'; // last char that was not white space
    while i < b.len() {
        let c = b[i];
        match c {
            '\'' | '"' | '`' => {
                i += 1;
                while i < b.len() && b[i] != c {
                    if b[i] == '\\' {
                        i += 1;
                    } else if c != '`' && b[i] == '\n' {
                        break;
                    }
                    i += 1;
                }
            }
            '/' if i + 1 < b.len() && b[i + 1] == '/' => {
                while i < b.len() && b[i] != '\n' {
                    i += 1;
                }
            }
            '/' if i + 1 < b.len() && b[i + 1] == '*' => {
                i += 2;
                while i + 1 < b.len() && !(b[i] == '*' && b[i + 1] == '/') {
                    i += 1;
                }
                i += 1;
            }
            '/' if "(,=:[!&|?{};+-*%<>~^\n".contains(prev) || after_keyword(&b[..i]) => {
                // a regexp literal
                i += 1;
                let mut class = false;
                while i < b.len() && b[i] != '\n' && (class || b[i] != '/') {
                    if b[i] == '\\' {
                        i += 1;
                    } else if b[i] == '[' {
                        class = true;
                    } else if b[i] == ']' {
                        class = false;
                    }
                    i += 1;
                }
            }
            '\\' => {
                if b.get(i + 1) != Some(&'u') {
                    return true;
                }
            }
            _ => {}
        }
        if i < b.len() && !b[i].is_whitespace() {
            prev = b[i];
        }
        i += 1;
    }
    false
}

/// The code before a `/` ends with a keyword after which an expression
/// starts (`return /a/`, `await /a/`).
fn after_keyword(before: &[char]) -> bool {
    let mut j = before.len();
    while j > 0 && before[j - 1].is_whitespace() {
        j -= 1;
    }
    let end = j;
    while j > 0 && (before[j - 1].is_alphanumeric() || before[j - 1] == '_' || before[j - 1] == '$') {
        j -= 1;
    }
    let w: String = before[j..end].iter().collect();
    matches!(w.as_str(), "return" | "await" | "yield" | "typeof" | "in" | "of" | "case" | "void" | "delete" | "instanceof" | "new" | "else" | "do" | "throw")
}

#[cfg(test)]
mod tests {
    use super::lone_backslash;
    #[test]
    fn backslashes() {
        assert!(lone_backslash("l\\et r;"));
        assert!(lone_backslash("x = 1; a\\"));
        assert!(!lone_backslash("var s = 'a\\nb'; var r = /\\d+/g; // \\x\n/* \\q */ var t = `\\``;"));
        assert!(!lone_backslash("async function f() { await /(?<y>\\d+)/.exec('1'); return /\\w/; }"));
        assert!(!lone_backslash("var \\u0061 = 1; f(/[\\]/]+/.test('x'));"));
    }
}
