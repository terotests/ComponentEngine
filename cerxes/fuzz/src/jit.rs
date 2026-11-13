// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The WebAssembly + JIT configuration (audit F8): the engine as the
// browser and the WASM host run it, compiled to wasm32, with the baseline
// JIT (cerxes::wjit) off, on at once (min 0) and at the product threshold
// (64). jit/worker.mjs runs the three; this compares them with each other
// and with the native reference run:
//   mismatch-wasm  native register VM vs the wasm interpreter (JIT off):
//                  a wasm32-only bug (usize is 32 bits there, …)
//   mismatch-jit   JIT off vs on (min 0 or 64)
//   jit-error      a trap or an invalid JIT module in one of them

use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

pub struct JitWorker {
    child: Child,
    stdin: ChildStdin,
    rx: mpsc::Receiver<Option<String>>,
}

pub fn spawn(wasm: &Path) -> Option<JitWorker> {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("jit/worker.mjs");
    let mut child = Command::new("node").arg(script).arg(wasm).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let stdin = child.stdin.take()?;
    let stdout = child.stdout.take()?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut r = BufReader::new(stdout);
        loop {
            let mut head = String::new();
            if r.read_line(&mut head).unwrap_or(0) == 0 {
                let _ = tx.send(None);
                return;
            }
            let len: usize = head.trim().parse().unwrap_or(0);
            let mut buf = vec![0u8; len];
            if r.read_exact(&mut buf).is_err() {
                let _ = tx.send(None);
                return;
            }
            if tx.send(Some(String::from_utf8_lossy(&buf).to_string())).is_err() {
                return;
            }
        }
    });
    Some(JitWorker { child, stdin, rx })
}

impl JitWorker {
    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub enum JitOutcome {
    /// (off, on0, on64): printed lines, None when the run was slow
    Lines(Option<Vec<String>>, Option<Vec<String>>, Option<Vec<String>>),
    Died,
    Hang,
}

pub fn exec(w: &mut JitWorker, src: &str, timeout: Duration) -> JitOutcome {
    if w.stdin.write_all(format!("{}\n", src.len()).as_bytes()).and_then(|_| w.stdin.write_all(src.as_bytes())).and_then(|_| w.stdin.flush()).is_err() {
        return JitOutcome::Died;
    }
    match w.rx.recv_timeout(timeout) {
        Ok(Some(json)) => JitOutcome::Lines(field(&json, "off"), field(&json, "on0"), field(&json, "on64")),
        Ok(None) => JitOutcome::Died,
        Err(_) => JitOutcome::Hang,
    }
}

/// `"name":{"lines":[…],"slow":b,…}` out of the worker's JSON, without a
/// JSON library: the lines array is the only one with strings.
fn field(json: &str, name: &str) -> Option<Vec<String>> {
    let key = format!("\"{}\":{{\"lines\":[", name);
    let start = json.find(&key)? + key.len();
    let mut out = Vec::new();
    let b: Vec<char> = json[start..].chars().collect();
    let mut i = 0;
    while i < b.len() && b[i] != ']' {
        if b[i] == '"' {
            i += 1;
            let mut s = String::new();
            while i < b.len() && b[i] != '"' {
                if b[i] == '\\' && i + 1 < b.len() {
                    i += 1;
                    match b[i] {
                        'n' => s.push('\n'),
                        't' => s.push('\t'),
                        'r' => s.push('\r'),
                        'u' if i + 4 < b.len() => {
                            let h: String = b[i + 1..i + 5].iter().collect();
                            if let Some(c) = u32::from_str_radix(&h, 16).ok().and_then(char::from_u32) {
                                s.push(c);
                            } else {
                                s.push('\u{FFFD}');
                            }
                            i += 4;
                        }
                        c => s.push(c),
                    }
                } else {
                    s.push(b[i]);
                }
                i += 1;
            }
            out.push(s);
        }
        i += 1;
    }
    let rest: String = b[i..].iter().take(30).collect();
    if rest.contains("\"slow\":true") {
        return None;
    }
    Some(out)
}
