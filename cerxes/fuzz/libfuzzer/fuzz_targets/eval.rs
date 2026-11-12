// SPDX-License-Identifier: AGPL-3.0-or-later
// Runs arbitrary text on the register VM in 5 ms slices for at most 1 s:
// an endless loop is fine (the slices stop it), a panic, an abort or a
// slice the engine does not give back (libFuzzer's -timeout) is a finding.
#![no_main]
use libfuzzer_sys::fuzz_target;
use std::time::Instant;

fuzz_target!(|data: &[u8]| {
    let src = String::from_utf8_lossy(data);
    let mut e = cerxes::Engine::new();
    e.vm.regs = true;
    e.set_syntax(false, false);
    e.set_slice(5.0);
    let t0 = Instant::now();
    e.eval(&src);
    while e.is_suspended() && t0.elapsed().as_millis() < 1000 {
        e.resume();
    }
});
