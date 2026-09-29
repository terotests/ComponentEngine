// CErXes: TypeScript and JSX read into CEr's tree. Runs the cases of
// tests/cases.txt, each in a fresh engine; bench/cases.mjs runs the same
// cases on the Ranger build.
use cerxes::Engine;

/// (syntax, script, expected) of every case
fn cases() -> Vec<(String, String, String)> {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/cases.txt")).unwrap();
    let mut out = Vec::new();
    let mut cur: Option<(String, Vec<String>, Vec<String>, bool)> = None;
    for line in text.lines() {
        if let Some(mode) = line.strip_prefix("==== ") {
            if let Some((m, s, e, _)) = cur.take() {
                out.push((m, s.join("\n"), e.join("\n").trim_end().to_string()));
            }
            cur = Some((mode.trim().to_string(), Vec::new(), Vec::new(), false));
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
    if let Some((m, s, e, _)) = cur.take() {
        out.push((m, s.join("\n"), e.join("\n").trim_end().to_string()));
    }
    out
}

#[test]
fn cases_txt() {
    let all = cases();
    assert!(all.len() > 50);
    let mut failed = Vec::new();
    for (mode, src, want) in all.iter() {
        let mut e = Engine::new();
        e.set_syntax(mode.contains("ts"), mode.contains('x'));
        let got = e.eval(src);
        if &got != want {
            failed.push(format!("[{}] {}\n  want: {}\n  got:  {}", mode, src, want, got));
        }
    }
    assert!(failed.is_empty(), "{} of {} cases failed:\n{}", failed.len(), all.len(), failed.join("\n"));
}
