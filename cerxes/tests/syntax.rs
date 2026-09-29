// CErXes: TypeScript and JSX read into CEr's tree. Runs the cases of
// tests/cases.txt, and CEr's own cases (../cer/tests/cases.txt, plain
// JavaScript) both as JavaScript and as TSX, each in a fresh engine;
// bench/cases.mjs runs the same cases on the Ranger builds.
use cerxes::Engine;

/// (syntax, title, script, expected) of every case in a cases file; a
/// case's head is `==== <syntax> [title]`
fn cases(file: &str) -> Vec<(String, String, String, String)> {
    let text = std::fs::read_to_string(format!("{}/{}", env!("CARGO_MANIFEST_DIR"), file)).unwrap();
    let mut out = Vec::new();
    let mut cur: Option<(String, Vec<String>, Vec<String>, bool)> = None;
    let mut push = |c: Option<(String, Vec<String>, Vec<String>, bool)>, out: &mut Vec<(String, String, String, String)>| {
        if let Some((head, s, e, _)) = c {
            let mut parts = head.splitn(2, ' ');
            let mode = parts.next().unwrap_or("").to_string();
            let title = parts.next().unwrap_or("").to_string();
            out.push((mode, title, s.join("\n"), e.join("\n").trim_end().to_string()));
        }
    };
    for line in text.lines() {
        if let Some(head) = line.strip_prefix("==== ") {
            push(cur.take(), &mut out);
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
    push(cur.take(), &mut out);
    out
}

/// Runs `all` with each case's syntax, or with `force` when given.
fn check(all: &[(String, String, String, String)], force: Option<&str>) -> Vec<String> {
    let mut failed = Vec::new();
    for (mode, title, src, want) in all.iter() {
        let m = force.unwrap_or(mode.as_str());
        let mut e = Engine::new();
        e.set_syntax(m == "tsx" || m == "ts", m == "tsx" || m == "jsx");
        let got = e.eval(src);
        if &got != want {
            failed.push(format!("[{} {}] {}\n  want: {}\n  got:  {}", m, title, src, want, got));
        }
    }
    failed
}

#[test]
fn cerxes_cases() {
    let all = cases("tests/cases.txt");
    assert!(all.len() > 50);
    let failed = check(&all, None);
    assert!(failed.is_empty(), "{} of {} cases failed:\n{}", failed.len(), all.len(), failed.join("\n"));
}

#[test]
fn cer_cases_as_javascript() {
    let all = cases("../cer/tests/cases.txt");
    assert!(all.len() > 40);
    let failed = check(&all, None);
    assert!(failed.is_empty(), "{} of {} cases failed:\n{}", failed.len(), all.len(), failed.join("\n"));
}

#[test]
fn cer_cases_as_tsx() {
    let all = cases("../cer/tests/cases.txt");
    let failed = check(&all, Some("tsx"));
    assert!(failed.is_empty(), "{} of {} cases failed:\n{}", failed.len(), all.len(), failed.join("\n"));
}
