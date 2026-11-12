// SPDX-License-Identifier: AGPL-3.0-or-later
//
// cerxes-fuzz: see README.md.
//
//   cerxes-fuzz run   [--time S] [--jobs N] [--seed N] [--out DIR] [--node]
//                     [--heavy] [--rc-check] [--mutate P] [--corpus FILE]...
//   cerxes-fuzz repro FILE.js           run the oracles in this process
//   cerxes-fuzz min   FILE.js [--out F] shrink a finding, keeping its class
//   cerxes-fuzz gen   [--seed N]        print one generated program
//   cerxes-fuzz worker                  (internal) programs on stdin

mod gen;

use cerxes::Engine;
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------- oracles

/// One run of a program in one engine configuration.
struct Run {
    /// printed lines, then `=> <completion or error>`
    lines: Vec<String>,
    /// gave up after the time budget (an endless or very long program)
    slow: bool,
    /// longest wall time one slice took before the engine gave control back
    max_slice_ms: f64,
}

#[derive(Clone, Copy)]
struct Config {
    regs: bool,
    pcomp: bool,
    tsx: bool,
    /// slices this long; a run with snapshots takes one at every suspension
    slice_ms: f64,
    snapshots: bool,
    rc_check: bool,
}

/// Wall-time budget of one engine run before it counts as slow.
const BUDGET_MS: f64 = 3000.0;
/// One slice may overrun its length by this much before it is a finding:
/// the host could not have stopped the engine in time.
const SLICE_GAP_MS: f64 = 1000.0;

fn now_ms(t0: Instant) -> f64 {
    t0.elapsed().as_secs_f64() * 1000.0
}

fn run_once(src: &str, c: Config) -> Run {
    let mut e = Engine::new_with(c.pcomp);
    e.vm.regs = c.regs;
    e.vm.rc_check = c.rc_check;
    if c.tsx {
        e.set_syntax(true, true);
    } else {
        e.set_syntax(false, false);
    }
    e.set_slice(c.slice_ms);
    let t0 = Instant::now();
    let mut lines: Vec<String> = Vec::new();
    let mut last = now_ms(t0);
    let mut max_slice = 0.0f64;
    let mut r = e.eval(src);
    let mut slow = false;
    loop {
        let t = now_ms(t0);
        max_slice = max_slice.max(t - last);
        last = t;
        if !e.is_suspended() {
            break;
        }
        if t > BUDGET_MS {
            slow = true;
            break;
        }
        if c.snapshots {
            // carry the printed lines over by hand: the snapshot holds the
            // engine, not what the host already read from it
            for i in 0..e.output_count() {
                lines.push(e.output_at(i));
            }
            e.clear_output();
            let snap = e.snapshot();
            if !snap.is_empty() {
                let mut e2 = Engine::from_snapshot(&snap);
                if !e2.error.is_empty() {
                    lines.push(format!("!! from_snapshot: {}", e2.error));
                    return Run { lines, slow: false, max_slice_ms: max_slice };
                }
                e2.vm.regs = c.regs;
                e2.vm.rc_check = c.rc_check;
                e2.set_slice(c.slice_ms);
                e2.syntax = e.syntax.clone();
                e = e2;
            }
        }
        last = now_ms(t0);
        r = e.resume();
    }
    for i in 0..e.output_count() {
        lines.push(e.output_at(i));
    }
    if !slow {
        lines.push(format!("=> {}", r));
    }
    Run { lines, slow, max_slice_ms: max_slice }
}

/// A snapshot with a few bytes changed must be refused or restored, never
/// crash the host.
fn corrupt_snapshot(src: &str, seed: u64) -> Option<String> {
    let mut e = Engine::new();
    e.set_syntax(false, false);
    e.set_slice(20.0);
    e.eval(src);
    let t0 = Instant::now();
    while e.is_suspended() && now_ms(t0) < BUDGET_MS {
        e.resume();
    }
    if e.is_suspended() {
        return None;
    }
    let snap = e.snapshot();
    if snap.is_empty() {
        return None;
    }
    let mut rng = gen::Rng::new(seed);
    let mut bytes: Vec<char> = snap.chars().collect();
    for _ in 0..1 + rng.below(4) {
        let i = rng.below(bytes.len());
        bytes[i] = *['0', '9', '-', ' ', '\n', 'x', '"', '\u{0}'].get(rng.below(8)).unwrap();
    }
    let bad: String = bytes.into_iter().collect();
    let r = catch_unwind(AssertUnwindSafe(|| {
        let mut e2 = Engine::from_snapshot(&bad);
        e2.set_slice(20.0);
        e2.eval("typeof v0 === 'undefined' ? 0 : __s(v0, 0)");
        let t0 = Instant::now();
        while e2.is_suspended() && now_ms(t0) < BUDGET_MS {
            e2.resume();
        }
    }));
    match r {
        Ok(_) => None,
        Err(_) => Some(format!("from_snapshot of a corrupted snapshot panicked: {}", last_panic())),
    }
}

static PANIC_MSG: Mutex<String> = Mutex::new(String::new());

fn last_panic() -> String {
    PANIC_MSG.lock().map(|m| m.clone()).unwrap_or_default()
}

fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            String::from("?")
        };
        let loc = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        if let Ok(mut m) = PANIC_MSG.lock() {
            *m = format!("{} at {}", msg, loc);
        }
    }));
}

/// The verdict on one program: `ok`, `slow`, or `<class>\t<signature>\t<detail>`.
/// The signature groups findings of one bug (a panic's location, …).
fn judge(src: &str, seed: u64, generated: bool, opts: &Opts) -> (String, Vec<String>) {
    let mut rng = gen::Rng::new(seed ^ 0xA5A5);
    let base = Config { regs: true, pcomp: true, tsx: false, slice_ms: 50.0, snapshots: false, rc_check: opts.rc_check };
    // the reference: the register VM, the product default
    let a = match catch_unwind(AssertUnwindSafe(|| run_once(src, base))) {
        Ok(r) => r,
        Err(_) => {
            let p = last_panic();
            return (format!("panic\t{}\tregister VM: {}", panic_sig(&p), p), Vec::new());
        }
    };
    if a.max_slice_ms > SLICE_GAP_MS + base.slice_ms {
        return (format!("slice-gap\tslice\tone 50 ms slice ran {:.0} ms before the engine yielded", a.max_slice_ms), a.lines);
    }
    if a.slow {
        return (String::from("slow"), a.lines);
    }
    // the same program on other configurations must print the same lines
    let mut others: Vec<(&str, Config)> = vec![("stack VM", Config { regs: false, ..base })];
    if rng.chance(30) {
        others.push(("tree compiler", Config { pcomp: false, ..base }));
    }
    if generated && rng.chance(20) {
        // a mutated script read as TypeScript may mean something else (`a < b > (c)`)
        others.push(("read as TSX", Config { tsx: true, ..base }));
    }
    if rng.chance(40) {
        others.push(("slices + snapshot/restore", Config { slice_ms: 0.02, snapshots: true, ..base }));
    }
    for (name, c) in others {
        let b = match catch_unwind(AssertUnwindSafe(|| run_once(src, c))) {
            Ok(r) => r,
            Err(_) => {
                let p = last_panic();
                return (format!("panic\t{}\t{}: {}", panic_sig(&p), name, p), a.lines);
            }
        };
        if b.slow {
            continue;
        }
        if let Some((d, sig)) = first_diff(&a.lines, &b.lines) {
            let class = match name {
                "stack VM" => "mismatch-vm",
                "tree compiler" => "mismatch-compiler",
                "read as TSX" => "mismatch-tsx",
                _ => "mismatch-snapshot",
            };
            return (format!("{}\t{}\tregister VM vs {}: {}", class, sig, name, d), a.lines);
        }
    }
    if rng.chance(10) {
        if let Some(d) = corrupt_snapshot(src, seed) {
            return (format!("snapshot-corrupt\t{}\t{}", panic_sig(&last_panic()), d), a.lines);
        }
    }
    (String::from("ok"), a.lines)
}

fn panic_sig(p: &str) -> String {
    // "msg at file:line": the location is the bug, the message's numbers vary
    match p.rfind(" at ") {
        Some(i) => p[i + 4..].to_string(),
        None => p.chars().filter(|c| !c.is_ascii_digit()).take(60).collect(),
    }
}

/// The first line two runs disagree on: (description, signature).
fn first_diff(a: &[String], b: &[String]) -> Option<(String, String)> {
    let n = a.len().max(b.len());
    for i in 0..n {
        let x = a.get(i).map(|s| s.as_str()).unwrap_or("<none>");
        let y = b.get(i).map(|s| s.as_str()).unwrap_or("<none>");
        if x != y {
            return Some((format!("line {}: {:?} vs {:?}", i + 1, clip(x), clip(y)), diff_sig(x, y)));
        }
    }
    None
}

fn clip(s: &str) -> String {
    if s.chars().count() > 160 {
        let t: String = s.chars().take(160).collect();
        format!("{}…", t)
    } else {
        s.to_string()
    }
}

/// Mismatches group by the words that appear on one side only (`x:` the
/// reference, `y:` the other run), so one bug seen in many programs
/// (a RegExp's source lost after a snapshot: `y:undefined`) is one finding.
fn diff_sig(x: &str, y: &str) -> String {
    fn words(s: &str) -> Vec<String> {
        let mut w: Vec<String> = s
            .split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .filter(|t| t.len() >= 3 && !t.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false))
            .map(|t| t.to_string())
            .collect();
        w.sort();
        w.dedup();
        w
    }
    let a = words(x);
    let b = words(y);
    let only_a: Vec<String> = a.iter().filter(|t| !b.contains(t)).take(3).cloned().collect();
    let only_b: Vec<String> = b.iter().filter(|t| !a.contains(t)).take(3).cloned().collect();
    let kind = |s: &str| if s.starts_with("=> ") { "result" } else if s.contains(" throw ") { "throw" } else if s == "<none>" { "missing" } else { "line" };
    format!("{}/{} x:{} y:{}", kind(x), kind(y), only_a.join(","), only_b.join(","))
}

// ---------------------------------------------------------------- worker

/// Reads programs framed as `<seed> <len>\n<bytes>` on stdin, answers
/// `<len>\n<verdict>\n<lines…>` on stdout.
fn worker(opts: &Opts) {
    install_panic_hook();
    let stdin = std::io::stdin();
    let mut inp = BufReader::new(stdin.lock());
    let stdout = std::io::stdout();
    loop {
        let mut head = String::new();
        if inp.read_line(&mut head).unwrap_or(0) == 0 {
            return;
        }
        let mut it = head.split_whitespace();
        let seed: u64 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let len: usize = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let generated = it.next().map(|s| s == "g").unwrap_or(false);
        let mut buf = vec![0u8; len];
        if inp.read_exact(&mut buf).is_err() {
            return;
        }
        let src = String::from_utf8_lossy(&buf).to_string();
        let (verdict, lines) = judge(&src, seed, generated, opts);
        let mut body = verdict.replace('\n', " ");
        body.push('\n');
        for l in lines {
            body.push_str(&l.replace('\n', "\\n"));
            body.push('\n');
        }
        let mut out = stdout.lock();
        let _ = write!(out, "{}\n{}", body.len(), body);
        let _ = out.flush();
    }
}

struct Worker {
    child: Child,
    stdin: ChildStdin,
    rx: mpsc::Receiver<Option<String>>,
    stderr_path: PathBuf,
}

fn spawn_worker(opts: &Opts, id: usize) -> Worker {
    let exe = std::env::current_exe().expect("own path");
    let stderr_path = opts.out.join(format!(".worker{}.stderr", id));
    let err = std::fs::File::create(&stderr_path).expect("stderr file");
    let mut cmd = if opts.mem_mb > 0 && cfg!(unix) {
        // the address-space limit turns a runaway allocation into an abort we
        // can see, instead of the machine swapping (Linux; macOS ignores -v)
        let mut c = Command::new("sh");
        c.arg("-c").arg(format!("ulimit -v {} 2>/dev/null; exec \"$0\" \"$@\"", opts.mem_mb * 1024)).arg(&exe);
        c
    } else {
        Command::new(&exe)
    };
    cmd.arg("worker").env("RUST_BACKTRACE", "0");
    if opts.rc_check {
        cmd.arg("--rc-check");
    }
    let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::from(err)).spawn().expect("worker");
    let stdin = child.stdin.take().unwrap();
    let stdout: ChildStdout = child.stdout.take().unwrap();
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
    Worker { child, stdin, rx, stderr_path }
}

enum Outcome {
    Answer(String),
    /// the worker died: abort, stack overflow, out of memory, a signal
    Died(String),
    /// no answer within the timeout: the engine did not yield
    Hang,
}

fn exec(w: &mut Worker, src: &str, seed: u64, generated: bool, timeout: Duration) -> Outcome {
    let head = format!("{} {} {}\n", seed, src.len(), if generated { "g" } else { "m" });
    if w.stdin.write_all(head.as_bytes()).and_then(|_| w.stdin.write_all(src.as_bytes())).and_then(|_| w.stdin.flush()).is_err() {
        return Outcome::Died(death_note(w));
    }
    match w.rx.recv_timeout(timeout) {
        Ok(Some(s)) => Outcome::Answer(s),
        Ok(None) => Outcome::Died(death_note(w)),
        Err(_) => Outcome::Hang,
    }
}

fn death_note(w: &mut Worker) -> String {
    let status = w.child.wait().map(|s| format!("{}", s)).unwrap_or_default();
    let err = std::fs::read_to_string(&w.stderr_path).unwrap_or_default();
    // the lines that say what happened, not the backtrace
    let keys = ["panicked", "fatal", "memory allocation", "overflow", "error", "abort", "SIG"];
    let notable: Vec<&str> = err.lines().filter(|l| keys.iter().any(|k| l.contains(k))).take(4).collect();
    let text = if notable.is_empty() { err.lines().rev().take(4).collect::<Vec<_>>().join(" / ") } else { notable.join(" / ") };
    format!("{} | {}", status, text)
}

fn death_class(note: &str) -> (&'static str, String) {
    let n = note.to_lowercase();
    if n.contains("stack overflow") || n.contains("has overflowed its stack") {
        ("stack-overflow", String::from("stack overflow"))
    } else if n.contains("memory allocation") || n.contains("capacity overflow") || n.contains("out of memory") {
        ("oom", String::from("allocation failed"))
    } else {
        let sig: String = note.split(" | ").next().unwrap_or("").to_string();
        ("crash", sig)
    }
}

// ---------------------------------------------------------------- driver

#[derive(Clone)]
struct Opts {
    time_s: u64,
    jobs: usize,
    seed: u64,
    out: PathBuf,
    node: bool,
    heavy: bool,
    rc_check: bool,
    mutate_pct: usize,
    corpus: Vec<PathBuf>,
    timeout_s: u64,
    mem_mb: u64,
    max_execs: u64,
    /// min / repro: the program was generated, not mutated (`--generated`)
    generated: bool,
}

impl Opts {
    fn parse(args: &[String]) -> (Opts, Vec<String>) {
        let mut o = Opts {
            time_s: 60,
            jobs: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2),
            seed: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(1),
            out: PathBuf::from("fuzz-out"),
            node: false,
            heavy: false,
            rc_check: false,
            mutate_pct: 30,
            corpus: Vec::new(),
            timeout_s: 15,
            mem_mb: 4096,
            max_execs: 0,
            generated: false,
        };
        let mut rest = Vec::new();
        let mut i = 0;
        while i < args.len() {
            let a = args[i].as_str();
            let v = args.get(i + 1).cloned().unwrap_or_default();
            match a {
                "--time" => { o.time_s = v.parse().unwrap_or(60); i += 1; }
                "--jobs" => { o.jobs = v.parse().unwrap_or(1); i += 1; }
                "--seed" => { o.seed = v.parse().unwrap_or(1); i += 1; }
                "--out" => { o.out = PathBuf::from(v); i += 1; }
                "--mutate" => { o.mutate_pct = v.parse().unwrap_or(30); i += 1; }
                "--corpus" => { o.corpus.push(PathBuf::from(v)); i += 1; }
                "--timeout" => { o.timeout_s = v.parse().unwrap_or(15); i += 1; }
                "--mem-mb" => { o.mem_mb = v.parse().unwrap_or(4096); i += 1; }
                "--execs" => { o.max_execs = v.parse().unwrap_or(0); i += 1; }
                "--node" => o.node = true,
                "--heavy" => o.heavy = true,
                "--rc-check" => o.rc_check = true,
                "--generated" => o.generated = true,
                _ => rest.push(args[i].clone()),
            }
            i += 1;
        }
        (o, rest)
    }
}

/// The seeds mutation starts from: the scripts of the engines' case files
/// (cases.txt: `==== syntax`, script, `----`, value) and any .js files.
fn load_corpus(paths: &[PathBuf]) -> Vec<String> {
    let mut out = Vec::new();
    for p in paths {
        let files: Vec<PathBuf> = if p.is_dir() {
            std::fs::read_dir(p).map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).collect()).unwrap_or_default()
        } else {
            vec![p.clone()]
        };
        for f in files {
            let text = match std::fs::read_to_string(&f) {
                Ok(t) => t,
                Err(_) => continue,
            };
            if f.extension().map(|e| e == "txt").unwrap_or(false) {
                let mut cur: Option<String> = None;
                for line in text.lines() {
                    if line.starts_with("==== ") {
                        cur = Some(String::new());
                    } else if line == "----" {
                        if let Some(s) = cur.take() {
                            if !s.trim().is_empty() {
                                out.push(s);
                            }
                        }
                    } else if let Some(s) = cur.as_mut() {
                        s.push_str(line);
                        s.push('\n');
                    }
                }
            } else {
                out.push(text);
            }
        }
    }
    out
}

struct Shared {
    execs: AtomicU64,
    stop: AtomicBool,
    counts: Mutex<HashMap<String, u64>>,
    seen: Mutex<HashSet<String>>,
}

fn save_finding(opts: &Opts, sh: &Shared, class: &str, sig: &str, detail: &str, src: &str, seed: u64, generated: bool) {
    {
        let mut c = sh.counts.lock().unwrap();
        *c.entry(class.to_string()).or_insert(0) += 1;
    }
    let key = format!("{}\t{}", class, sig);
    let fresh = sh.seen.lock().unwrap().insert(key);
    if !fresh {
        return;
    }
    let dir = opts.out.join(class);
    let _ = std::fs::create_dir_all(&dir);
    let name = format!("{:016x}", fnv(sig.as_bytes()));
    let _ = std::fs::write(dir.join(format!("{}.js", name)), src);
    let _ = std::fs::write(dir.join(format!("{}.txt", name)), format!("class: {}\nsignature: {}\nseed: {}\ngenerated: {}\n{}\n", class, sig, seed, if generated { "yes" } else { "no" }, detail));
    eprintln!("\n[new] {} {} — {}", class, name, clip(detail));
}

fn fnv(b: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for x in b {
        h ^= *x as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

/// Node's printed lines for the program, or None when Node is missing or
/// took too long.
fn node_lines(src: &str, dir: &Path, id: usize) -> Option<Vec<String>> {
    let f = dir.join(format!(".node{}.js", id));
    std::fs::write(&f, src).ok()?;
    let mut child = Command::new("node").arg("--stack-size=4000").arg(&f).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let t0 = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if t0.elapsed() > Duration::from_secs(10) {
                    let _ = child.kill();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(_) => return None,
        }
    }
    let mut s = String::new();
    child.stdout.take()?.read_to_string(&mut s).ok()?;
    Some(s.lines().map(|l| l.to_string()).collect())
}

fn run(opts: Opts) {
    std::fs::create_dir_all(&opts.out).expect("out dir");
    let corpus = Arc::new(load_corpus(&opts.corpus));
    let sh = Arc::new(Shared { execs: AtomicU64::new(0), stop: AtomicBool::new(false), counts: Mutex::new(HashMap::new()), seen: Mutex::new(HashSet::new()) });
    eprintln!(
        "cerxes-fuzz: {} jobs, {} s, seed {}, corpus {} scripts, out {}{}{}",
        opts.jobs,
        opts.time_s,
        opts.seed,
        corpus.len(),
        opts.out.display(),
        if opts.node { ", vs node" } else { "" },
        if opts.heavy { ", heavy" } else { "" }
    );
    let t0 = Instant::now();
    let mut handles = Vec::new();
    for id in 0..opts.jobs {
        let opts = opts.clone();
        let sh = sh.clone();
        let corpus = corpus.clone();
        handles.push(std::thread::spawn(move || {
            let mut w = spawn_worker(&opts, id);
            let mut rng = gen::Rng::new(opts.seed.wrapping_add(id as u64 * 7919));
            let timeout = Duration::from_secs(opts.timeout_s);
            // a worker that dies again and again on its first program is
            // broken (the binary was rebuilt under it, …), not the engine
            let mut fresh_deaths = 0;
            let mut answered = false;
            while !sh.stop.load(Ordering::Relaxed) {
                let seed = rng.next();
                let mut mutated = !corpus.is_empty() && rng.chance(opts.mutate_pct);
                let src = if mutated {
                    let base = &corpus[rng.below(corpus.len())];
                    let mut r2 = gen::Rng::new(seed);
                    gen::mutate(&mut r2, base)
                } else {
                    let mut g = gen::Gen::new(seed);
                    g.heavy = opts.heavy;
                    let p = g.program();
                    if rng.chance(opts.mutate_pct / 3) {
                        mutated = true;
                        let mut r2 = gen::Rng::new(seed);
                        gen::mutate(&mut r2, &p)
                    } else {
                        p
                    }
                };
                match exec(&mut w, &src, seed, !mutated, timeout) {
                    Outcome::Answer(a) => {
                        answered = true;
                        fresh_deaths = 0;
                        let mut parts = a.splitn(2, '\n');
                        let verdict = parts.next().unwrap_or("").to_string();
                        if verdict == "ok" && opts.node && !mutated && !opts.heavy {
                            let ours: Vec<String> = parts.next().unwrap_or("").lines().filter(|l| !l.starts_with("=> ")).map(|l| l.to_string()).collect();
                            if let Some(theirs) = node_lines(&src, &opts.out, id) {
                                if let Some((d, sig)) = first_diff(&theirs, &ours) {
                                    save_finding(&opts, &sh, "mismatch-node", &sig, &format!("node vs CErXes: {}", d), &src, seed, !mutated);
                                }
                            }
                        } else if verdict != "ok" && verdict != "slow" {
                            let f: Vec<&str> = verdict.splitn(3, '\t').collect();
                            let class = f.first().copied().unwrap_or("?");
                            let sig = f.get(1).copied().unwrap_or("");
                            let detail = f.get(2).copied().unwrap_or("");
                            save_finding(&opts, &sh, class, sig, detail, &src, seed, !mutated);
                        }
                    }
                    Outcome::Died(note) => {
                        if !answered {
                            fresh_deaths += 1;
                            if fresh_deaths >= 20 {
                                eprintln!("worker {} keeps dying before its first answer: {}", id, note);
                                sh.stop.store(true, Ordering::Relaxed);
                                break;
                            }
                        }
                        answered = false;
                        let (class, sig) = death_class(&note);
                        save_finding(&opts, &sh, class, &sig, &note, &src, seed, !mutated);
                        w = spawn_worker(&opts, id);
                    }
                    Outcome::Hang => {
                        let _ = w.child.kill();
                        let _ = w.child.wait();
                        answered = false;
                        save_finding(&opts, &sh, "hang", "no answer", &format!("no answer in {} s: the engine did not yield or compile finished too late", opts.timeout_s), &src, seed, !mutated);
                        w = spawn_worker(&opts, id);
                    }
                }
                let n = sh.execs.fetch_add(1, Ordering::Relaxed) + 1;
                if opts.max_execs > 0 && n >= opts.max_execs {
                    sh.stop.store(true, Ordering::Relaxed);
                }
            }
            let _ = w.child.kill();
        }));
    }
    let mut last_print = Instant::now();
    while t0.elapsed() < Duration::from_secs(opts.time_s) && !sh.stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(200));
        if last_print.elapsed() > Duration::from_secs(10) {
            last_print = Instant::now();
            status(&sh, t0);
        }
    }
    sh.stop.store(true, Ordering::Relaxed);
    for h in handles {
        let _ = h.join();
    }
    status(&sh, t0);
    let summary = summary_text(&sh, t0, &opts);
    let _ = std::fs::write(opts.out.join("summary.txt"), &summary);
    eprintln!("{}", summary);
}

fn status(sh: &Shared, t0: Instant) {
    let n = sh.execs.load(Ordering::Relaxed);
    let s = t0.elapsed().as_secs_f64();
    let c = sh.counts.lock().unwrap();
    let mut parts: Vec<String> = c.iter().map(|(k, v)| format!("{} {}", k, v)).collect();
    parts.sort();
    eprintln!("[{:>5.0} s] {} execs ({:.1}/s) unique {} | {}", s, n, n as f64 / s.max(0.001), sh.seen.lock().unwrap().len(), parts.join(", "));
}

fn summary_text(sh: &Shared, t0: Instant, opts: &Opts) -> String {
    let n = sh.execs.load(Ordering::Relaxed);
    let c = sh.counts.lock().unwrap();
    let mut s = format!("execs {} in {:.0} s, seed {}, jobs {}\n", n, t0.elapsed().as_secs_f64(), opts.seed, opts.jobs);
    let mut k: Vec<_> = c.iter().collect();
    k.sort();
    for (class, v) in k {
        let unique = sh.seen.lock().unwrap().iter().filter(|x| x.starts_with(&format!("{}\t", class))).count();
        s.push_str(&format!("{:<20} {:>6} hits {:>4} unique\n", class, v, unique));
    }
    s
}

// ---------------------------------------------------------------- min / repro

fn verdict_of(w: &mut Worker, src: &str, opts: &Opts, id: usize) -> (String, Worker) {
    match exec(w, src, opts.seed, opts.generated, Duration::from_secs(opts.timeout_s)) {
        Outcome::Answer(a) => {
            let v = a.lines().next().unwrap_or("").to_string();
            let f: Vec<&str> = v.splitn(3, '\t').collect();
            let key = format!("{}\t{}", f.first().copied().unwrap_or(""), f.get(1).copied().unwrap_or(""));
            (key, spawn_or_keep(None, opts, id))
        }
        Outcome::Died(note) => {
            let (c, s) = death_class(&note);
            (format!("{}\t{}", c, s), spawn_worker(opts, id))
        }
        Outcome::Hang => {
            let _ = w.child.kill();
            let _ = w.child.wait();
            (String::from("hang\tno answer"), spawn_worker(opts, id))
        }
    }
}

fn spawn_or_keep(w: Option<Worker>, opts: &Opts, id: usize) -> Worker {
    match w {
        Some(w) => w,
        None => spawn_worker(opts, id),
    }
}

/// Delta debugging on lines, then on characters: drops chunks while the
/// finding keeps its class and signature.
fn minimize(path: &str, opts: &Opts, out: Option<String>) {
    std::fs::create_dir_all(&opts.out).ok();
    let mut o = opts.clone();
    o.generated = o.generated || finding_was_generated(path);
    // the seed picks which oracles run: the finding's own
    o.seed = finding_seed(path).unwrap_or(1);
    let opts = &o;
    let src = std::fs::read_to_string(path).expect("cannot read");
    let mut w = spawn_worker(opts, 99);
    let (target, w2) = verdict_of(&mut w, &src, opts, 99);
    let _ = w.child.kill();
    w = w2;
    if target.starts_with("ok") || target.starts_with("slow") {
        eprintln!("the program does not reproduce a finding ({})", target.trim());
        return;
    }
    eprintln!("target: {}", target);
    let mut cur: Vec<String> = src.lines().map(|l| format!("{}\n", l)).collect();
    for unit_chars in [false, true] {
        if unit_chars {
            let joined: String = cur.concat();
            cur = joined.chars().map(|c| c.to_string()).collect();
        }
        let mut chunk = (cur.len() / 2).max(1);
        while chunk >= 1 {
            let mut i = 0;
            let mut changed = false;
            while i < cur.len() {
                let end = (i + chunk).min(cur.len());
                let cand: String = cur[..i].concat() + &cur[end..].concat();
                let (v, w2) = verdict_of(&mut w, &cand, opts, 99);
                let _ = w.child.kill();
                w = w2;
                if v == target {
                    cur.drain(i..end);
                    changed = true;
                } else {
                    i += chunk;
                }
            }
            if !changed {
                if chunk == 1 {
                    break;
                }
                chunk /= 2;
            }
            if unit_chars && cur.len() > 4000 {
                // character passes on big inputs take too long; stop at lines
                break;
            }
        }
    }
    let _ = w.child.kill();
    let result = cur.concat();
    let dest = out.unwrap_or_else(|| format!("{}.min.js", path.trim_end_matches(".js")));
    std::fs::write(&dest, &result).expect("write");
    eprintln!("{} bytes -> {} bytes: {}", src.len(), result.len(), dest);
}

/// The `generated: yes` line of a finding's .txt next to its .js.
fn finding_was_generated(path: &str) -> bool {
    std::fs::read_to_string(format!("{}.txt", path.trim_end_matches(".min.js").trim_end_matches(".js")))
        .map(|t| t.lines().any(|l| l == "generated: yes"))
        .unwrap_or(false)
}

/// The `seed:` line of a finding's .txt.
fn finding_seed(path: &str) -> Option<u64> {
    std::fs::read_to_string(format!("{}.txt", path.trim_end_matches(".min.js").trim_end_matches(".js")))
        .ok()
        .and_then(|t| t.lines().find(|l| l.starts_with("seed: ")).and_then(|l| l[6..].trim().parse().ok()))
}

fn repro(path: &str, opts: &Opts) {
    install_panic_hook();
    let mut o = opts.clone();
    o.generated = o.generated || finding_was_generated(path);
    let opts = &o;
    let src = std::fs::read_to_string(path).expect("cannot read");
    let seed = finding_seed(path).unwrap_or(1);
    let (v, lines) = judge(&src, seed, opts.generated, opts);
    println!("verdict: {}", v.replace('\t', " | "));
    for l in lines.iter().take(200) {
        println!("  {}", l);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().cloned().unwrap_or_default();
    let (opts, rest) = Opts::parse(&args[1.min(args.len())..]);
    match cmd.as_str() {
        "worker" => worker(&opts),
        "run" => run(opts),
        "repro" => repro(rest.first().expect("repro FILE.js"), &opts),
        "min" => {
            let out = rest.iter().position(|a| a == "-o").and_then(|i| rest.get(i + 1).cloned());
            minimize(rest.first().expect("min FILE.js"), &opts, out)
        }
        "gen" => {
            let mut g = gen::Gen::new(opts.seed);
            g.heavy = opts.heavy;
            print!("{}", g.program());
        }
        _ => {
            eprintln!("usage: cerxes-fuzz run|repro|min|gen … (see cerxes/fuzz/README.md)");
            std::process::exit(2);
        }
    }
}
