// SPDX-License-Identifier: AGPL-3.0-or-later
// Lexer, parser and both compilers on arbitrary text, as JS and as TSX:
// a SyntaxError is fine, a panic or a sanitizer report is a finding.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let src = String::from_utf8_lossy(data);
    let mut e = cerxes::Engine::new();
    e.set_syntax(false, false);
    let _ = e.compile(&src);
    e.set_syntax(true, true);
    let _ = e.compile(&src);
    let mut t = cerxes::Engine::new_with(false);
    t.set_syntax(false, false);
    let _ = t.compile(&src);
});
