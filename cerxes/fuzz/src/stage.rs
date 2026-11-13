// SPDX-License-Identifier: AGPL-3.0-or-later
//
// What a worker is doing right now, kept in a small file the driver reads
// when the worker dies: a SIGSEGV or an abort then has a place ("register
// VM", "hostile-unit run", …) and known crashes of one oracle do not hide
// the crashes of another.

use std::fs::File;
use std::sync::Mutex;

static FILE: Mutex<Option<File>> = Mutex::new(None);

pub fn open(path: &str) {
    if let Ok(f) = std::fs::OpenOptions::new().create(true).write(true).truncate(true).open(path) {
        if let Ok(mut g) = FILE.lock() {
            *g = Some(f);
        }
    }
}

/// Overwrites the stage (64 bytes at offset 0: one pwrite, no growth).
pub fn set(s: &str) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        if let Ok(g) = FILE.lock() {
            if let Some(f) = g.as_ref() {
                let mut b = [b' '; 64];
                let n = s.len().min(63);
                b[..n].copy_from_slice(&s.as_bytes()[..n]);
                b[63] = b'\n';
                let _ = f.write_at(&b, 0);
            }
        }
    }
    #[cfg(not(unix))]
    let _ = s;
}

pub fn read(path: &std::path::Path) -> String {
    std::fs::read_to_string(path).map(|s| s.trim().to_string()).unwrap_or_default()
}
