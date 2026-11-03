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
