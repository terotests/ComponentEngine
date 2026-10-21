// SPDX-License-Identifier: AGPL-3.0-or-later
//! Modules the host gives as source: `defineSourceModule(name, source,
//! path)` (prelude.rs) keeps a module's text, and the first `import` of it
//! runs the text once, as a module of its own:
//!
//!   import { lerp, project } from "./scene.ts"
//!
//! A module's text is read as a function body whose top-level names stay
//! its own, `export` notes what it hands out (ts.rs `module_export`), and
//! the body ends by returning those as the module's exports:
//!
//!   (function () {'use strict';<the module's text>
//!   ;return {"lerp": lerp, "project": project};
//!   })
//!
//! The module's lines keep their numbers (the head is on its first line).
//! Its own imports name it as where they stand (`__cerxesImport("m",
//! "its/path")`), so "./x" and "../x" are read from its folder.
//!
//! The registry (`REGISTRY`) runs a module's text through the global `eval`
//! with `HEADER` and the module's path in front: `Parser::new` reads such a
//! text as the module (`module_parser_of`), so CEr's evaluator, which
//! CErXes shares, needs nothing of its own.

use ranger::prelude::*;

use crate::parser::{Parser, Syntax};

use crate::emit::quote;

/// A module's text as the function expression that runs it and returns its
/// exports, or the syntax error naming the module's file.
pub struct ModuleText {
    pub text: String,
    pub error: String,
}

/// The function that runs the module `src` (read as `path`'s extension
/// says) and returns its exports, as source text.
pub fn module_text(src: &str, path: &str) -> ModuleText {
    // the first read finds what the module exports
    let mut p = Parser::with_syntax(src, Syntax::for_path(path));
    p.module_path = String::from(path);
    p.parse_program();
    if !p.error.is_empty() {
        return ModuleText { text: String::new(), error: in_file(p.error.as_str(), path) };
    }
    let mut fields: Vec<String> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let mut i: usize = 0;
    while i < p.exports.len() {
        let name = p.exports[i].0.clone();
        if seen.contains(&name) {
            return ModuleText { text: String::new(), error: in_file(format!("SyntaxError: '{}' is exported twice", name).as_str(), path) };
        }
        fields.push(format!("{}: {}", quote(name.as_str()), p.exports[i].1));
        seen.push(name);
        i += 1;
    }
    let own = format!("{{{}}}", fields.join(", "));
    let exports = if p.export_stars.is_empty() {
        own
    } else {
        // `export * from "m"`: m's exports but its default, under the
        // module's own
        let mut parts: Vec<String> = Vec::new();
        for m in p.export_stars.iter() {
            parts.push(format!("__cerxesStar(__cerxesImport({}, {}))", quote(m), quote(path)));
        }
        format!("Object.assign({{}}, {}, {})", parts.join(", "), own)
    };
    ModuleText { text: format!("(function () {{'use strict';{}\n;return {};\n}})", src, exports), error: String::new() }
}

/// A syntax error as one of the module's file: `… (line 3)` reads
/// `… (lib/x.ts line 3)`.
fn in_file(error: &str, path: &str) -> String {
    if !error.contains("(line ") {
        return format!("{} ({})", error, path);
    }
    let mut out = String::new();
    let mut first = true;
    for part in error.split("(line ") {
        if first {
            out.push_str(part);
            first = false;
        } else {
            out.push_str(format!("({} line {}", path, part).as_str());
        }
    }
    out
}

/// What a text `eval` reads as a module starts with: `/*@cerxesModule
/// <path>*/` and then the module's source, on the same line.
pub const HEADER: &str = "/*@cerxesModule ";

/// The parser of a text that starts with `HEADER`: the module's function
/// (`module_text`), or a parser whose error is the module's syntax error.
pub fn module_parser_of(src: &str) -> Parser {
    // the path runs to the first "*/" (the registry lets no path hold one)
    let mut path = String::new();
    let mut body = String::new();
    let mut at: int = 0;
    let skip = HEADER.chars().count() as int;
    let mut in_path = true;
    let mut star = false;
    for c in src.chars() {
        if at >= skip {
            if in_path {
                if star && c == '/' {
                    in_path = false;
                } else if c == '*' {
                    star = true;
                } else {
                    if star {
                        path.push('*');
                    }
                    path.push(c);
                    star = false;
                }
            } else {
                body.push(c);
            }
        }
        at += 1;
    }
    let m = module_text(body.as_str(), path.as_str());
    if !m.error.is_empty() {
        let mut p = module_parser("", path.as_str());
        p.error = m.error;
        return p;
    }
    module_parser(m.text.as_str(), path.as_str())
}

/// The syntax the module's text is read with, and the path its imports
/// name it by: what the second read (of `module_text`'s text) needs.
pub fn module_parser(text: &str, path: &str) -> Parser {
    let mut p = Parser::with_syntax(text, Syntax::for_path(path));
    p.module_path = String::from(path);
    p
}

/// The host's modules, as JavaScript run once before any script (by
/// `Engine::new`, and by an emitted script's `emit::runtime`):
///
///   defineModule(name, exports)          a module the host made: its exports
///   defineSourceModule(name, source, path?)
///                                        a module the host gives as text (or,
///                                        emitted, as the function
///                                        `emit::emit_module_js` wrote): run on
///                                        its first import, once; `path` (the
///                                        name when left out) is its file, what
///                                        its own "./x" imports are read from.
///                                        Names that give the same path are one
///                                        module.
///   __cerxesImport(name, from?)          what `import` reads; `from` is the
///                                        importing module's file
///
/// An import cycle is an error naming the modules on it.
pub const REGISTRY: &str = r#"(function () {
var evaluate = eval;
function compile(source, path) {
  if (path.indexOf('*/') >= 0) throw new TypeError("defineSourceModule: a module's path cannot hold '*/'");
  return evaluate('/*@cerxesModule ' + path + '*/' + source);
}
function hide(name, v) { Object.defineProperty(globalThis, name, { value: v, writable: true, enumerable: false, configurable: true }); }
function isObject(v) { return (typeof v === 'object' && v !== null) || typeof v === 'function'; }
var modules = new Map();
var sources = new Map();
var loading = [];
function normal(p) {
  var out = [];
  p.split('/').forEach(function (s) { if (s === '' || s === '.') return; if (s === '..') out.pop(); else out.push(s); });
  return out.join('/');
}
function resolve(name, from) {
  if (typeof from !== 'string' || from === '') return name;
  if (name.slice(0, 2) === './' || name.slice(0, 3) === '../') {
    var at = from.lastIndexOf('/');
    return normal((at >= 0 ? from.slice(0, at + 1) : '') + name);
  }
  if (name.charAt(0) === '/') return normal(name);
  return name;
}
hide('defineModule', function defineModule(name, exports) {
  if (typeof name !== 'string' || name === '') throw new TypeError('defineModule: the module needs a name');
  if (!isObject(exports)) throw new TypeError("defineModule: module '" + name + "' needs an object of exports");
  modules.set(name, exports);
  return exports;
});
hide('defineSourceModule', function defineSourceModule(name, source, path) {
  if (typeof name !== 'string' || name === '') throw new TypeError('defineSourceModule: the module needs a name');
  if (typeof source !== 'string' && typeof source !== 'function') throw new TypeError("defineSourceModule: module '" + name + "' needs its source");
  var file = path === undefined || path === null || path === '' ? name : String(path);
  var s = sources.get(file);
  if (s === undefined || s.path !== file || s.source !== source) s = { source: source, path: file, exports: undefined };
  modules.delete(name);
  modules.delete(file);
  sources.set(name, s);
  sources.set(file, s);
});
hide('__cerxesStar', function __cerxesStar(m) {
  var o = {};
  Object.keys(m).forEach(function (k) { if (k !== 'default') o[k] = m[k]; });
  return o;
});
hide('__cerxesImport', function __cerxesImport(name, from) {
  var key = resolve(name, from);
  if (modules.has(key)) return modules.get(key);
  var s = sources.get(key);
  if (s === undefined) {
    var known = Array.from(modules.keys());
    sources.forEach(function (v, k) { if (known.indexOf(k) < 0) known.push(k); });
    throw new Error("Cannot find module '" + key + "'" + (typeof from === 'string' && from !== '' ? " (imported by " + from + ")" : '') + (known.length ? ' (modules here: ' + known.join(', ') + ')' : ' (no modules are defined here)'));
  }
  if (s.exports !== undefined) return s.exports;
  if (loading.indexOf(s) >= 0) {
    throw new Error('Import cycle: ' + loading.slice(loading.indexOf(s)).map(function (m) { return m.path; }).concat([s.path]).join(' → '));
  }
  loading.push(s);
  try {
    var run = typeof s.source === 'function' ? s.source : compile(s.source, s.path);
    s.exports = run();
  } finally {
    loading.pop();
  }
  return s.exports;
});
})();"#;
