// CEr's unit cases: tests/cases.txt, each script in a fresh engine, its
// value compared with what Node answers. (../cerxes runs the same cases.)
use cer::Engine;

/// (title, script, expected) of every case in `text`
fn parse(text: &str) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    let mut cur: Option<(String, Vec<String>, Vec<String>, bool)> = None;
    for line in text.lines() {
        if let Some(head) = line.strip_prefix("==== ") {
            if let Some((t, s, e, _)) = cur.take() {
                out.push((t, s.join("\n"), e.join("\n").trim_end().to_string()));
            }
            cur = Some((head.trim().to_string(), Vec::new(), Vec::new(), false));
        } else if let Some(c) = cur.as_mut() {
            if line == "----" && !c.3 {
                c.3 = true;
            } else if c.3 {
                if !line.starts_with('#') {
                    c.2.push(line.to_string());
                }
            } else {
                c.1.push(line.to_string());
            }
        }
    }
    if let Some((t, s, e, _)) = cur.take() {
        out.push((t, s.join("\n"), e.join("\n").trim_end().to_string()));
    }
    out
}

#[test]
fn cases_txt() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/cases.txt")).unwrap();
    let all = parse(&text);
    assert!(all.len() > 40);
    let mut failed = Vec::new();
    for (title, src, want) in all.iter() {
        let mut e = Engine::new();
        let got = e.eval(src);
        if &got != want {
            failed.push(format!("[{}]\n  want: {}\n  got:  {}", title, want, got));
        }
    }
    assert!(failed.is_empty(), "{} of {} cases failed:\n{}", failed.len(), all.len(), failed.join("\n"));
}

/// The same cases with a reclaim at every safe point, each checked against
/// a full mark: reference counting frees nothing a script can still reach
/// (Vm::rc_stress).
#[test]
fn cases_txt_rc_checked() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/cases.txt")).unwrap();
    let all = parse(&text);
    let mut failed = Vec::new();
    for (title, src, want) in all.iter() {
        let mut e = Engine::new();
        e.vm.rc_stress();
        let got = e.eval(src);
        if &got != want || e.vm.rc_errors > 0 {
            failed.push(format!("[{}]\n  want: {}\n  got:  {}\n  rc: {}", title, want, got, e.vm.rc_log.join("; ")));
        }
    }
    assert!(failed.is_empty(), "{} of {} cases failed:\n{}", failed.len(), all.len(), failed.join("\n"));
}

/// The same cases on the register VM (Vm.regs), with and without the
/// reference counting check.
#[test]
fn cases_txt_regs() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/cases.txt")).unwrap();
    let all = parse(&text);
    let mut failed = Vec::new();
    for stress in [false, true] {
        for (title, src, want) in all.iter() {
            let mut e = Engine::new();
            e.vm.regs = true;
            if stress {
                e.vm.rc_stress();
            }
            let got = e.eval(src);
            if &got != want || e.vm.rc_errors > 0 {
                failed.push(format!("[{}{}]\n  want: {}\n  got:  {}", title, if stress { ", rc checked" } else { "" }, want, got));
            }
        }
    }
    assert!(failed.is_empty(), "{} of {} cases failed:\n{}", failed.len(), 2 * all.len(), failed.join("\n"));
}

/// The same cases through the parser and the tree compiler (compiler.rs),
/// on both machines.
#[test]
fn cases_txt_tree() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/cases.txt")).unwrap();
    let all = parse(&text);
    let mut failed = Vec::new();
    for regs in [false, true] {
        for (title, src, want) in all.iter() {
            let mut e = Engine::new_with(false);
            e.vm.regs = regs;
            let got = e.eval(src);
            if &got != want {
                failed.push(format!("[{}{}]\n  want: {}\n  got:  {}", title, if regs { ", regs" } else { "" }, want, got));
            }
        }
    }
    assert!(failed.is_empty(), "{} of {} cases failed:\n{}", failed.len(), 2 * all.len(), failed.join("\n"));
}

/// Text made again and again over a large live heap: the strings that
/// reference counting let go are freed by a reclaim, so they bring no
/// collection of the whole heap; strings held by cycles still do.
#[test]
fn strings_alone_reclaim_before_collecting() {
    for regs in [false, true] {
        let mut e = Engine::new();
        e.vm.regs = regs;
        e.eval("var keep = []; for (var i = 0; i < 200000; i++) keep.push({ i: i }); 0");
        let before = e.gc_runs();
        let got = e.eval("var base = new Array(100001).join('x'), big = ''; for (var r = 0; r < 3000; r++) big = base + r; big.length");
        assert_eq!(got, "100004");
        let runs = e.gc_runs() - before;
        assert!(runs <= 1, "{} collections for text that reference counting frees (regs {})", runs, regs);
        let before = e.gc_runs();
        e.eval("for (var r = 0; r < 800; r++) { var a = {}; a.self = a; a.s = new Array(20001).join('x') + r; } 0");
        assert!(e.gc_runs() > before, "strings held by cycles brought no collection (regs {})", regs);
    }
}

/// Date's local time and Intl's default zone in four zones, with a daylight
/// saving change each way (src/tz.rs, src/tzdata.rs); the answers are Node's.
#[cfg(feature = "tzdata")]
#[test]
fn time_zones() {
    let src = "var d = new Date(2026, 2, 29, 3, 30), s = new Date(2026, 9, 25, 3, 30), o = [];\no.push(d.toString(), s.toString(), d.getTimezoneOffset(), s.getTimezoneOffset(), d.toISOString(), s.toISOString());\no.push(new Date(\"2026-07-01T12:00\").getHours(), new Date(1e12).getDate(), new Date(-3e12).getTimezoneOffset());\nvar u = new Date(2026, 0, 31, 12); u.setMonth(1); o.push(u.getMonth(), u.getDate());\no.push(new Intl.DateTimeFormat(\"en-US\", { timeZone: \"America/New_York\", timeZoneName: \"short\", hour: \"numeric\" }).format(d));\no.push(new Intl.DateTimeFormat(\"en-US\", { timeZoneName: \"long\", month: \"short\", day: \"numeric\", hour: \"numeric\", minute: \"2-digit\" }).format(s));\no.join(\" | \")";
    let want = [
        ("Europe/Helsinki", "Sun Mar 29 2026 04:30:00 GMT+0300 (Eastern European Summer Time) | Sun Oct 25 2026 03:30:00 GMT+0300 (Eastern European Summer Time) | -180 | -180 | 2026-03-29T01:30:00.000Z | 2026-10-25T00:30:00.000Z | 12 | 9 | -99 | 2 | 3 | 9 PM EDT | Oct 25, 3:30 AM Eastern European Summer Time"),
        ("America/New_York", "Sun Mar 29 2026 03:30:00 GMT-0400 (Eastern Daylight Time) | Sun Oct 25 2026 03:30:00 GMT-0400 (Eastern Daylight Time) | 240 | 240 | 2026-03-29T07:30:00.000Z | 2026-10-25T07:30:00.000Z | 12 | 8 | 296 | 2 | 3 | 3 AM EDT | Oct 25, 3:30 AM Eastern Daylight Time"),
        ("Australia/Sydney", "Sun Mar 29 2026 03:30:00 GMT+1100 (Australian Eastern Daylight Time) | Sun Oct 25 2026 03:30:00 GMT+1100 (Australian Eastern Daylight Time) | -660 | -660 | 2026-03-28T16:30:00.000Z | 2026-10-24T16:30:00.000Z | 12 | 9 | -604 | 2 | 3 | 12 PM EDT | Oct 25, 3:30 AM Australian Eastern Daylight Time"),
        ("Asia/Kolkata", "Sun Mar 29 2026 03:30:00 GMT+0530 (India Standard Time) | Sun Oct 25 2026 03:30:00 GMT+0530 (India Standard Time) | -330 | -330 | 2026-03-28T22:00:00.000Z | 2026-10-24T22:00:00.000Z | 12 | 9 | -321 | 2 | 3 | 6 PM EDT | Oct 25, 3:30 AM India Standard Time"),
    ];
    for (zone, answer) in want.iter() {
        let mut e = Engine::new();
        assert!(e.set_time_zone(zone), "{} unknown", zone);
        assert_eq!(&e.eval(src), answer, "in {}", zone);
    }
    let mut e = Engine::new();
    assert!(!e.set_time_zone("Mars/Olympus"));
    assert_eq!(e.time_zone(), "UTC");
}

/// Without the `tzdata` feature the only zone is UTC.
#[cfg(not(feature = "tzdata"))]
#[test]
fn time_zones_left_out() {
    let mut e = Engine::new();
    assert!(!e.set_time_zone("Europe/Helsinki"));
    assert!(e.set_time_zone("UTC"));
    assert_eq!(e.eval("new Date(2026, 6, 1).getTimezoneOffset()"), "0");
}
