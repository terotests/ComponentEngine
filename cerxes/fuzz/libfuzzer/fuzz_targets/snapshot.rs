// SPDX-License-Identifier: AGPL-3.0-or-later
// A snapshot is input a host reads from disk or from another machine:
// from_snapshot on arbitrary bytes, and on a real snapshot with the input
// spliced in, must refuse or restore, never crash.
#![no_main]
use libfuzzer_sys::fuzz_target;
use std::sync::OnceLock;

static BASE: OnceLock<String> = OnceLock::new();

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let _ = cerxes::Engine::from_snapshot(&text);
    let base = BASE.get_or_init(|| {
        let mut e = cerxes::Engine::new();
        e.eval("var o = { a: [1, 'x', 2n], m: new Map([[1, 2]]) }; function f(n) { return n + 1; }");
        e.snapshot()
    });
    if data.len() >= 4 && !base.is_empty() {
        let at = (u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize) % base.len();
        let mut s = String::with_capacity(base.len() + data.len());
        let cut = (0..=at).rev().find(|i| base.is_char_boundary(*i)).unwrap_or(0);
        s.push_str(&base[..cut]);
        s.push_str(&String::from_utf8_lossy(&data[4..]));
        let end = (cut + data.len()).min(base.len());
        let end = (end..=base.len()).find(|i| base.is_char_boundary(*i)).unwrap_or(base.len());
        s.push_str(&base[end..]);
        let mut e = cerxes::Engine::from_snapshot(&s);
        e.set_slice(5.0);
        e.eval("typeof f === 'function' ? f(1) : 0");
    }
});
