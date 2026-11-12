//! No rule may panic on any input. Runs every implemented rule (default options) over every captured
//! upstream case and every parser fixture, whole and cut at several points (broken templates,
//! unterminated tags/expressions), as `.vue` and as plain script.

use std::path::Path;

use oxlint_vue_rs::{ConfiguredRule, FileOptions, Severity, configure, lint_source, rules};
use serde_json::Value;

fn all_rules() -> Vec<ConfiguredRule> {
    rules::ALL
        .iter()
        .filter_map(|d| {
            configure(d.name, Severity::Error, &[], &Value::Null)
                .ok()
                .flatten()
        })
        .collect()
}

fn cuts(src: &str) -> Vec<&str> {
    let mut out = vec![src];
    let n = src.len();
    for k in 1..8 {
        let mut i = n * k / 8;
        while i < n && !src.is_char_boundary(i) {
            i += 1;
        }
        out.push(&src[..i]);
    }
    out
}

#[test]
fn rules_never_panic() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let rules = all_rules();
    let mut inputs: Vec<String> = Vec::new();
    for e in std::fs::read_dir(root.join("tests/upstream"))
        .unwrap()
        .flatten()
    {
        if e.path().extension().is_some_and(|x| x == "json") {
            let cases: Vec<Value> =
                serde_json::from_str(&std::fs::read_to_string(e.path()).unwrap()).unwrap();
            inputs.extend(
                cases
                    .iter()
                    .filter_map(|c| c["code"].as_str().map(String::from)),
            );
        }
    }
    for e in std::fs::read_dir(root.join("tests/parser-fixtures"))
        .unwrap()
        .flatten()
    {
        if let Ok(s) = std::fs::read_to_string(e.path().join("source.vue")) {
            inputs.push(s);
        }
    }
    inputs.sort();
    inputs.dedup();
    assert!(
        rules.len() > 50 && inputs.len() > 5000,
        "rules {} inputs {}",
        rules.len(),
        inputs.len()
    );
    let mut failures = Vec::new();
    let mut diags = 0usize;
    for src in &inputs {
        for cut in cuts(src) {
            for (path, ts) in [("t.vue", false), ("t.vue", true), ("t.js", false)] {
                let opts = FileOptions {
                    ts,
                    template_directives: true,
                    ..FileOptions::default()
                };
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    lint_source(cut, Path::new(path), &opts, &rules)
                }));
                diags += r.as_ref().map_or(0, Vec::len);
                if r.is_err() && failures.len() < 20 {
                    failures.push(format!("{path} ts={ts}: {cut:?}"));
                }
            }
        }
    }
    eprintln!(
        "{} inputs, {} rules, {diags} diagnostics",
        inputs.len(),
        rules.len()
    );
    assert!(
        failures.is_empty(),
        "rules panicked on {} inputs:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
