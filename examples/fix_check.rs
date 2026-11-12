//! Fix-verification driver for `scripts/fix-check.mjs`:
//! `cargo run --release --example fix_check -- <units.jsonl> <configs.json> <out.jsonl> [threads]`.
//!
//! units: one JSON object per line `{id, path, code, sfc?, ts?, jsx?, settings?}`.
//! configs: `[{rules: [[name, options]…], settings?, mode: "fix" | "suggest" | "each"}]` —
//! `fix` = `--fix`, `suggest` = `--fix-suggestions`, `each` = every suggestion applied alone.
//! Writes one line per (unit, config, change): `{u, c, desc?, passes, problems, script, p, s, m}` where
//! the output is `code[..p] + m + code[len-s..]` (UTF-8 byte offsets).

#[path = "../tests/fixcheck/mod.rs"]
mod fixcheck;

use std::io::{BufRead, Write};
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use oxlint_vue_rs::{ConfiguredRule, FileOptions};
use serde_json::{Value, json};

fn diff(a: &str, b: &str) -> (usize, usize, String) {
    let p = a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count();
    let mut p = p;
    while !a.is_char_boundary(p) || !b.is_char_boundary(p) {
        p -= 1;
    }
    let max = a.len().min(b.len()) - p;
    let mut s = a
        .bytes()
        .rev()
        .zip(b.bytes().rev())
        .take(max)
        .take_while(|(x, y)| x == y)
        .count();
    while !a.is_char_boundary(a.len() - s) || !b.is_char_boundary(b.len() - s) {
        s -= 1;
    }
    (p, s, b[p..b.len() - s].to_string())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let units: Vec<Value> = std::io::BufReader::new(std::fs::File::open(&args[1]).unwrap())
        .lines()
        .map(|l| serde_json::from_str(&l.unwrap()).unwrap())
        .collect();
    let configs: Vec<Value> =
        serde_json::from_str(&std::fs::read_to_string(&args[2]).unwrap()).unwrap();
    let out = Mutex::new(std::io::BufWriter::new(
        std::fs::File::create(&args[3]).unwrap(),
    ));
    let threads = args
        .get(4)
        .and_then(|t| t.parse().ok())
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(4, |n| n.get()));
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    std::thread::scope(|sc| {
        for _ in 0..threads {
            sc.spawn(|| {
                // Rules are instantiated per thread and reused across units.
                let built: Vec<(Vec<ConfiguredRule>, &str)> = configs
                    .iter()
                    .map(|c| {
                        let settings = c.get("settings").cloned().unwrap_or(Value::Null);
                        let rules = c["rules"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .filter_map(|r| {
                                let opts = r[1].as_array().cloned().unwrap_or_default();
                                fixcheck::rule(r[0].as_str().unwrap(), &opts, &settings)
                            })
                            .collect();
                        (rules, c["mode"].as_str().unwrap_or("fix"))
                    })
                    .collect();
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(u) = units.get(i) else { break };
                    let code = u["code"].as_str().unwrap();
                    let path = Path::new(u["path"].as_str().unwrap());
                    let mut lines = Vec::new();
                    for (ci, (rules, mode)) in built.iter().enumerate() {
                        if rules.is_empty() {
                            continue;
                        }
                        let settings = configs[ci].get("settings").cloned().unwrap_or(Value::Null);
                        let opts = FileOptions {
                            sfc: u["sfc"].as_bool(),
                            ts: u["ts"].as_bool().unwrap_or(false),
                            jsx: u["jsx"].as_bool().unwrap_or(false),
                            settings: if settings.is_null() {
                                u.get("settings").cloned().unwrap_or(Value::Null)
                            } else {
                                settings
                            },
                            template_directives: true,
                            ..FileOptions::default()
                        };
                        let results: Vec<(Option<String>, fixcheck::Outcome)> = if *mode == "each" {
                            fixcheck::check_suggestions(code, path, &opts, rules)
                                .into_iter()
                                .map(|(d, o)| (Some(d), o))
                                .collect()
                        } else {
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                fixcheck::check(code, path, &opts, rules, *mode == "suggest")
                            }))
                            .unwrap_or_else(|_| {
                                Some(fixcheck::Outcome {
                                    problems: vec!["panic".into()],
                                    after: code.to_string(),
                                    ..Default::default()
                                })
                            })
                            .map(|o| (None, o))
                            .into_iter()
                            .collect()
                        };
                        for (desc, o) in results {
                            let (p, s, m) = diff(code, &o.after);
                            let mut rec = json!({
                                "u": i, "c": ci, "passes": o.passes, "problems": o.problems,
                                "script": o.script_changed, "p": p, "s": s, "m": m,
                            });
                            if let Some(d) = desc {
                                rec["desc"] = json!(d);
                            }
                            lines.push(rec.to_string());
                        }
                    }
                    let mut w = out.lock().unwrap();
                    for l in lines {
                        writeln!(w, "{l}").unwrap();
                    }
                    let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                    if n.is_multiple_of(500) {
                        eprintln!("{n}/{}", units.len());
                    }
                }
            });
        }
    });
    out.lock().unwrap().flush().unwrap();
}
