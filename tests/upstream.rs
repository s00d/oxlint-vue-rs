//! Runs every captured eslint-plugin-vue RuleTester case (tests/upstream/*.json) against our rules.
//!
//! Ground truth = the real eslint-plugin-vue rule run through ESLint (see tests/upstream/README.md).
//! Compared per case: diagnostics (line, column, endLine, endColumn, message), fix output,
//! suggestions (desc + output).
//!
//! Env: `UPSTREAM_RULE=a,b` filter rules; `UPSTREAM_VERBOSE=1` print every mismatch;
//! `UPSTREAM_STATUS=path` write a JSON status summary.

use std::collections::BTreeMap;
use std::path::Path;

use oxlint_vue_rs::{FileOptions, Severity, apply_fixes, configure, lint_source};
use serde_json::{Value, json};

#[derive(Default, Debug)]
struct Tally {
    total: usize,
    pass: usize,
    skipped: usize,
    implemented: bool,
    failures: Vec<String>,
}

fn file_options(case: &Value) -> FileOptions {
    let lo = &case["languageOptions"];
    let filename = case["filename"].as_str().unwrap_or("test.js");
    let parser = lo["parser"].as_str().unwrap_or("espree");
    let sp = &lo["scriptParser"];
    let ts = parser == "ts"
        || sp.as_str() == Some("ts")
        || sp.get("js").and_then(Value::as_str) == Some("ts")
        || (parser != "vue" && sp.get("ts").is_some() && filename.ends_with(".ts"));
    FileOptions {
        // vue-eslint-parser `isVueFile`: `.vue` path or code starting with `<` (`/^\s*</u`).
        sfc: Some(
            parser == "vue"
                && (filename.ends_with(".vue")
                    || case["code"]
                        .as_str()
                        .unwrap_or("")
                        .trim_start()
                        .starts_with('<')),
        ),
        ts,
        jsx: lo["ecmaFeatures"]["jsx"].as_bool() == Some(true),
        settings: case["settings"].clone(),
        globals: case["languageOptions"]["globals"].clone(),
        ..FileOptions::default()
    }
}

fn msg_key(m: &Value) -> (u64, u64, u64, u64, String) {
    (
        m["line"].as_u64().unwrap_or(0),
        m["column"].as_u64().unwrap_or(0),
        m["endLine"].as_u64().unwrap_or(0),
        m["endColumn"].as_u64().unwrap_or(0),
        m["message"].as_str().unwrap_or("").to_string(),
    )
}

/// Returns `Err(diff)` on mismatch.
fn run_case(rule: &str, case: &Value) -> Result<(), String> {
    let code = case["code"].as_str().unwrap();
    let filename = case["filename"].as_str().unwrap_or("test.js");
    let opts = file_options(case);
    let options: Vec<Value> = case["options"].as_array().cloned().unwrap_or_default();
    let cfg = configure(rule, Severity::Error, &options, &opts.settings)
        .map_err(|e| e.to_string())?
        .unwrap();
    let path = Path::new(filename);
    let src = code.to_string();
    let rules = [cfg];
    let diags = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        lint_source(&src, path, &opts, &rules)
    }))
    .map_err(|e| {
        format!(
            "PANIC: {:?}",
            e.downcast_ref::<String>()
                .cloned()
                .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
        )
    })?;
    let lines = oxlint_vue_rs::LineIndex::new(&src);

    let mut got: Vec<Value> = diags
        .iter()
        .map(|d| {
            let [l, c, el, ec] = d.location(&lines, &src);
            let sugg: Vec<Value> = d
                .suggestions
                .iter()
                .map(|s| {
                    let mut out = src.clone();
                    out.replace_range(s.fix.span.start as usize..s.fix.span.end as usize, &s.fix.text);
                    json!({"desc": s.desc, "output": out})
                })
                .collect();
            json!({"line": l, "column": c, "endLine": el, "endColumn": ec, "message": d.message, "suggestions": sugg})
        })
        .collect();
    let mut want: Vec<Value> = case["actual"]["messages"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    // A report with a point location (e.g. vue/no-parsing-error) has no end in ESLint output:
    // drop our end position for messages expected without one.
    for g in got.iter_mut() {
        let k = msg_key(g);
        if want.iter().any(|w| {
            w.get("endLine").is_none_or(Value::is_null)
                && (msg_key(w).0, msg_key(w).1, &msg_key(w).4) == (k.0, k.1, &k.4)
        }) {
            g["endLine"] = Value::Null;
            g["endColumn"] = Value::Null;
        }
    }
    got.sort_by_key(msg_key);
    want.sort_by_key(msg_key);

    let mut diff = String::new();
    let gk: Vec<_> = got.iter().map(msg_key).collect();
    let wk: Vec<_> = want.iter().map(msg_key).collect();
    if gk != wk {
        diff.push_str(&format!("  messages:\n    want {wk:?}\n    got  {gk:?}\n"));
    } else {
        for (g, w) in got.iter().zip(&want) {
            let gs: Vec<(String, String)> = g["suggestions"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| {
                    (
                        s["desc"].as_str().unwrap().to_string(),
                        s["output"].as_str().unwrap_or("").to_string(),
                    )
                })
                .collect();
            let ws: Vec<(String, String)> = w["suggestions"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|s| {
                            (
                                s["desc"].as_str().unwrap_or("").to_string(),
                                s["output"].as_str().unwrap_or("").to_string(),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
            if gs != ws {
                diff.push_str(&format!(
                    "  suggestions at {:?}:\n    want {ws:?}\n    got  {gs:?}\n",
                    msg_key(w)
                ));
            }
        }
    }
    let got_out = apply_fixes(&src, &diags);
    let want_out = case["actual"]["output"].as_str().map(str::to_string);
    if got_out != want_out {
        diff.push_str(&format!(
            "  output:\n    want {want_out:?}\n    got  {got_out:?}\n"
        ));
    }
    if diff.is_empty() { Ok(()) } else { Err(diff) }
}

#[test]
fn upstream_cases() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let filter: Option<Vec<String>> = std::env::var("UPSTREAM_RULE")
        .ok()
        .map(|s| s.split(',').map(str::to_string).collect());
    let verbose = std::env::var("UPSTREAM_VERBOSE").is_ok();
    let skips: Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("tests/upstream-skips.json"))
            .unwrap_or_else(|_| "{}".into()),
    )
    .unwrap();

    let mut tallies: BTreeMap<String, Tally> = BTreeMap::new();
    let mut files: Vec<_> = std::fs::read_dir(root.join("tests/upstream"))
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    files.sort();
    for f in files {
        let cases: Vec<Value> =
            serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
        for case in cases {
            let full = case["rule"].as_str().unwrap().to_string();
            let rule = full.split('/').next().unwrap().to_string();
            if filter.as_ref().is_some_and(|fl| !fl.contains(&rule)) {
                continue;
            }
            let t = tallies.entry(rule.clone()).or_default();
            t.total += 1;
            let implemented = configure(&rule, Severity::Error, &[], &Value::Null)
                .ok()
                .flatten()
                .is_some()
                || configure(
                    &rule,
                    Severity::Error,
                    case["options"].as_array().map_or(&[][..], |v| v),
                    &Value::Null,
                )
                .ok()
                .flatten()
                .is_some();
            t.implemented |= implemented;
            if !implemented {
                continue;
            }
            let id = case["id"].as_str().map(str::to_string).unwrap_or_else(|| {
                format!("{}#{}{}", full, case["kind"].as_str().unwrap(), case["idx"])
            });
            if case["languageOptions"]["project"].as_bool() == Some(true) {
                t.skipped += 1; // type-aware case: needs TypeScript type information
                continue;
            }
            if skips.get(&id).is_some() {
                t.skipped += 1;
                continue;
            }
            match run_case(&rule, &case) {
                Ok(()) => t.pass += 1,
                Err(d) => t.failures.push(format!(
                    "{id}\n  code: {:?}\n  options: {}\n{d}",
                    case["code"].as_str().unwrap(),
                    case["options"]
                )),
            }
        }
    }

    let mut failed_rules = 0;
    let (mut impl_n, mut cases_n, mut pass_n) = (0, 0, 0);
    let mut status = serde_json::Map::new();
    for (rule, t) in &tallies {
        status.insert(rule.clone(), json!({"implemented": t.implemented, "total": t.total, "pass": t.pass, "skipped": t.skipped, "fail": t.failures.len()}));
        if !t.implemented {
            continue;
        }
        impl_n += 1;
        cases_n += t.total;
        pass_n += t.pass;
        if !t.failures.is_empty() {
            failed_rules += 1;
            eprintln!(
                "FAIL vue/{rule}: {}/{} pass, {} skipped",
                t.pass, t.total, t.skipped
            );
            let show = if verbose {
                t.failures.len()
            } else {
                3.min(t.failures.len())
            };
            for fl in &t.failures[..show] {
                eprintln!("{fl}");
            }
        }
    }
    let unimpl: Vec<_> = tallies
        .iter()
        .filter(|(_, t)| !t.implemented)
        .map(|(r, _)| r.as_str())
        .collect();
    eprintln!(
        "implemented rules: {impl_n}, cases {pass_n}/{cases_n} pass; not implemented: {} ({})",
        unimpl.len(),
        unimpl.join(" ")
    );
    if let Ok(p) = std::env::var("UPSTREAM_STATUS") {
        std::fs::write(
            p,
            serde_json::to_string_pretty(&Value::Object(status)).unwrap(),
        )
        .unwrap();
    }
    assert_eq!(
        failed_rules, 0,
        "{failed_rules} implemented rules have failing upstream cases"
    );
}
