// SPDX-License-Identifier: AGPL-3.0-or-later
//! Coverage feedback from LLVM's SanitizerCoverage, on stable Rust.
//!
//! A build with
//!
//!     RUSTFLAGS="-C passes=sancov-module \
//!       -C llvm-args=-sanitizer-coverage-level=3 \
//!       -C llvm-args=-sanitizer-coverage-inline-8bit-counters"
//!
//! (run-local.sh COV=1) gives every edge of the engine a byte counter;
//! LLVM hands the counter section to `__sanitizer_cov_8bit_counters_init`
//! when the process starts. A plain build never calls it, and coverage is
//! then off: `enabled()` is false and the driver mutates the seeds only.
//!
//! A feature is an edge with its hit count rounded to a bucket (1, 2, 3,
//! 4–7, 8–15, 16–31, 32–127, 128+), as AFL counts them: a program that
//! runs a loop more often than any before it is kept too.

use std::collections::HashSet;
use std::sync::Mutex;

static REGIONS: Mutex<Vec<(usize, usize)>> = Mutex::new(Vec::new());

#[no_mangle]
pub extern "C" fn __sanitizer_cov_8bit_counters_init(start: *mut u8, stop: *mut u8) {
    let (a, b) = (start as usize, stop as usize);
    if b <= a {
        return;
    }
    if let Ok(mut r) = REGIONS.lock() {
        // each module's constructor may report the same linked section
        if !r.iter().any(|&(s, _)| s == a) {
            r.push((a, b - a));
        }
    }
}

#[no_mangle]
pub extern "C" fn __sanitizer_cov_pcs_init(_beg: *const usize, _end: *const usize) {}

pub fn enabled() -> bool {
    REGIONS.lock().map(|r| !r.is_empty()).unwrap_or(false)
}

/// Number of counters (edges) in the build; 0 when coverage is off.
pub fn edges() -> usize {
    REGIONS.lock().map(|r| r.iter().map(|&(_, n)| n).sum()).unwrap_or(0)
}

fn each_region(mut f: impl FnMut(&mut [u8])) {
    let r = match REGIONS.lock() {
        Ok(r) => r.clone(),
        Err(_) => return,
    };
    for (s, n) in r {
        // SAFETY: LLVM gives the section's bounds; it lives as long as the
        // process and only the instrumented code writes it (one thread)
        let c = unsafe { std::slice::from_raw_parts_mut(s as *mut u8, n) };
        f(c);
    }
}

/// Zero every counter: called before a program runs.
pub fn reset() {
    each_region(|c| c.fill(0));
}

fn bucket(n: u8) -> u32 {
    match n {
        0 => 0,
        1 => 1,
        2 => 2,
        3 => 3,
        4..=7 => 4,
        8..=15 => 5,
        16..=31 => 6,
        32..=127 => 7,
        _ => 8,
    }
}

/// The features the last program lit that `seen` does not hold yet; they
/// are added to it.
pub fn new_features(seen: &mut HashSet<u32>) -> Vec<u32> {
    let mut out = Vec::new();
    let mut base = 0u32;
    each_region(|c| {
        for (i, &n) in c.iter().enumerate() {
            if n != 0 {
                let f = (base + i as u32) * 9 + bucket(n);
                if seen.insert(f) {
                    out.push(f);
                }
            }
        }
        base += c.len() as u32;
    });
    out
}
