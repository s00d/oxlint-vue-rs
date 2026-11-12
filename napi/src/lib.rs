//! Node addon for the oxlint JS plugin (`npm/index.js`): lints one file with the Rust core and returns
//! the problems with ranges in UTF-16 offsets of the given text — the unit of oxlint's
//! `sourceCode.actualText` / `context.report({ actualRange })` (oxc PR #28720).

use std::path::Path;

use napi_derive::napi;
use oxlint_vue_rs::{FileOptions, Fix, PARSING_ERROR, Severity, configure, lint_source};
use serde_json::{Value, json};

/// Byte offset → UTF-16 offset.
struct Utf16 {
    /// `None` for ASCII text (offsets are equal).
    map: Option<Vec<u32>>,
}

impl Utf16 {
    fn new(s: &str) -> Self {
        if s.is_ascii() {
            return Self { map: None };
        }
        let mut map = vec![0; s.len() + 1];
        let mut u = 0u32;
        for (i, c) in s.char_indices() {
            for b in 0..c.len_utf8() {
                map[i + b] = u;
            }
            u += c.len_utf16() as u32;
        }
        map[s.len()] = u;
        Self { map: Some(map) }
    }
    fn at(&self, byte: u32) -> u32 {
        self.map
            .as_ref()
            .map_or(byte, |m| m[(byte as usize).min(m.len() - 1)])
    }
    fn fix(&self, f: &Fix) -> Value {
        json!({ "range": [self.at(f.span.start), self.at(f.span.end)], "text": f.text })
    }
}

/// Lint `source` (the whole file) with `rules_json` = `[[name, options[]], …]` (names without the plugin
/// prefix). `settings_json` / `globals_json` are oxlint's per-file `settings` / `globals`; `prefix` is the
/// plugin alias accepted in disable comments besides `vue/`.
///
/// Returns a JSON array of `{ rule, range, message, fix, suggestions: [{ desc, fix }] }`, ranges in UTF-16
/// offsets of `source`, `fix` = `{ range, text }` or `null`. Disable comments (template and script) are
/// already applied. A fatal script parse error yields nothing (oxlint reports it itself).
#[napi]
pub fn lint(
    source: String,
    filename: String,
    rules_json: String,
    settings_json: String,
    globals_json: String,
    prefix: String,
) -> napi::Result<String> {
    let err = |e: String| napi::Error::from_reason(format!("oxlint-vue-rs: {e}"));
    let parse = |s: &str| serde_json::from_str::<Value>(s).map_err(|e| err(e.to_string()));
    let settings = parse(&settings_json)?;
    let globals = parse(&globals_json)?;
    let wanted: Vec<(String, Vec<Value>)> =
        serde_json::from_str(&rules_json).map_err(|e| err(e.to_string()))?;
    let mut rules = Vec::with_capacity(wanted.len());
    for (name, options) in &wanted {
        // Severity is oxlint's business; ours only feeds diagnostics we do not return.
        if let Some(r) = configure(name, Severity::Error, options, &settings).map_err(err)? {
            rules.push(r);
        }
    }
    if rules.is_empty() {
        return Ok("[]".into());
    }
    let opts = FileOptions {
        settings,
        globals,
        template_directives: true,
        prefixes: vec![prefix],
        ..Default::default()
    };
    let diags = lint_source(&source, Path::new(&filename), &opts, &rules);
    let u = Utf16::new(&source);
    let out: Vec<Value> = diags
        .iter()
        .filter(|d| d.rule != PARSING_ERROR)
        .map(|d| {
            json!({
                "rule": d.rule,
                "range": [u.at(d.span.start), u.at(d.span.end)],
                "message": d.message,
                "fix": d.fix.as_ref().map(|f| u.fix(f)),
                "suggestions": d.suggestions.iter()
                    .map(|s| json!({ "desc": s.desc, "fix": u.fix(&s.fix) }))
                    .collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(Value::Array(out).to_string())
}
