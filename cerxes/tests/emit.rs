// CErXes's JavaScript output (src/emit.rs): every case of tests/cases.txt and
// of CEr's cases (as JavaScript and as TSX) written out as JavaScript, then
// run as plain JavaScript in a fresh engine, must answer what the source
// answers. A case whose source is a syntax error must give the same error.
use cerxes::emit::emit_js;
use cerxes::parser::Syntax;
use cerxes::Engine;

fn cases(file: &str) -> Vec<(String, String, String, String)> {
    let text = std::fs::read_to_string(format!("{}/{}", env!("CARGO_MANIFEST_DIR"), file)).unwrap();
    let mut out = Vec::new();
    let mut cur: Option<(String, Vec<String>, Vec<String>, bool)> = None;
    let push = |c: Option<(String, Vec<String>, Vec<String>, bool)>, out: &mut Vec<(String, String, String, String)>| {
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

fn check(all: &[(String, String, String, String)], force: Option<&str>) -> Vec<String> {
    let mut failed = Vec::new();
    for (mode, title, src, want) in all.iter() {
        let m = force.unwrap_or(mode.as_str());
        let sx = Syntax::new(m == "tsx" || m == "ts", m == "tsx" || m == "jsx");
        let r = emit_js(src, sx);
        let got = if !r.error.is_empty() {
            r.error.clone()
        } else {
            let mut e = Engine::new();
            e.set_syntax(false, false);
            e.eval(r.js.as_str())
        };
        // a syntax error's line may differ only when the source's own does
        if &got != want {
            failed.push(format!("[{} {}] {}\n  js:   {}\n  want: {}\n  got:  {}", m, title, src, r.js.trim_end(), want, got));
        }
    }
    failed
}

#[test]
fn emitted_cerxes_cases() {
    let all = cases("tests/cases.txt");
    let failed = check(&all, None);
    assert!(failed.is_empty(), "{} of {} cases failed:\n{}", failed.len(), all.len(), failed.join("\n"));
}

#[test]
fn emitted_cer_cases() {
    let all = cases("../cer/tests/cases.txt");
    let failed = check(&all, None);
    assert!(failed.is_empty(), "{} of {} cases failed:\n{}", failed.len(), all.len(), failed.join("\n"));
}

#[test]
fn emitted_cer_cases_as_tsx() {
    let all = cases("../cer/tests/cases.txt");
    let failed = check(&all, Some("tsx"));
    assert!(failed.is_empty(), "{} of {} cases failed:\n{}", failed.len(), all.len(), failed.join("\n"));
}

/// A statement starts on its source line, so errors point at the .tsx.
#[test]
fn emitted_lines() {
    let src = "type T = {\n  a: number;\n};\nconst x: T = {\n  a: 1,\n};\nfunction f(): number {\n  return x.a;\n}\nthrow new Error('line ' + 10);\n";
    let r = emit_js(src, Syntax::tsx());
    assert!(r.error.is_empty(), "{}", r.error);
    let lines: Vec<&str> = r.js.lines().collect();
    assert!(lines[3].starts_with("const x = {"), "{}", r.js);
    assert!(lines[7].contains("return x.a;"), "{}", r.js);
    assert!(lines[9].starts_with("throw "), "{}", r.js);
}
