//! Reads the same `.oxlintrc.json` oxlint uses.
//!
//! oxlint rejects unknown `vue/<rule>` names (and reserves the `vue` JS plugin name), so our rules
//! live under the alias of the shim JS plugin, e.g.
//! `"jsPlugins": [{ "name": "vue-rs", "specifier": "oxlint-vue-rs" }]` → `"vue-rs/html-indent": "warn"`.
//! The shim registers every rule name, so oxlint validates the config; this crate does the work.
//! A bare `"jsPlugins": ["oxlint-vue-rs"]` uses the default name `vue-rs`.
//!
//! Supported keys: `rules`, `overrides[{files, rules}]`, `ignorePatterns`, `settings`, `extends` (paths).

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::Severity;

pub const DEFAULT_PREFIX: &str = "vue-rs";
pub const PACKAGE: &str = "oxlint-vue-rs";

#[derive(Debug, Clone)]
pub struct RuleSetting {
    pub severity: Option<Severity>, // None = off
    pub options: Vec<Value>,
}

#[derive(Debug, Clone, Default)]
pub struct Override {
    pub files: Vec<String>,
    pub rules: Vec<(String, RuleSetting)>,
}

#[derive(Debug, Clone, Default)]
pub struct Config {
    /// Directory globs are relative to.
    pub base: PathBuf,
    /// Accepted rule prefixes (alias names of the shim plugin).
    pub prefixes: Vec<String>,
    pub rules: Vec<(String, RuleSetting)>,
    pub overrides: Vec<Override>,
    pub ignore_patterns: Vec<String>,
    pub settings: Value,
    /// oxlint `globals` (name → "readonly" | "writable" | "off").
    pub globals: Value,
}

/// Strip `//` and `/* */` comments and trailing commas (JSONC → JSON).
pub fn strip_jsonc(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    let mut in_str = false;
    while i < b.len() {
        let c = b[i];
        if in_str {
            out.push(c as char);
            if c == b'\\' && i + 1 < b.len() {
                out.push(b[i + 1] as char);
                i += 2;
                continue;
            }
            if c == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        match c {
            b'"' => {
                in_str = true;
                out.push('"');
                i += 1;
            }
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                    i += 1;
                }
                i += 2;
            }
            b',' => {
                let mut j = i + 1;
                while j < b.len() && b[j].is_ascii_whitespace() {
                    j += 1;
                }
                if !matches!(b.get(j), Some(b'}' | b']')) {
                    out.push(',');
                }
                i += 1;
            }
            _ => {
                // Copy whole UTF-8 sequences.
                let len = utf8_len(c);
                out.push_str(&src[i..i + len]);
                i += len;
            }
        }
    }
    out
}

fn utf8_len(b: u8) -> usize {
    match b {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    }
}

pub fn parse_severity(v: &Value) -> Result<Option<Severity>, String> {
    match v {
        Value::String(s) => match s.as_str() {
            "off" | "allow" => Ok(None),
            "warn" => Ok(Some(Severity::Warn)),
            "error" | "deny" => Ok(Some(Severity::Error)),
            _ => Err(format!("invalid severity {s:?}")),
        },
        Value::Number(n) => match n.as_u64() {
            Some(0) => Ok(None),
            Some(1) => Ok(Some(Severity::Warn)),
            Some(2) => Ok(Some(Severity::Error)),
            _ => Err(format!("invalid severity {n}")),
        },
        _ => Err(format!("invalid severity {v}")),
    }
}

pub fn parse_rule_setting(v: &Value) -> Result<RuleSetting, String> {
    match v {
        Value::Array(a) if !a.is_empty() => Ok(RuleSetting {
            severity: parse_severity(&a[0])?,
            options: a[1..].to_vec(),
        }),
        _ => Ok(RuleSetting {
            severity: parse_severity(v)?,
            options: Vec::new(),
        }),
    }
}

fn plugin_prefixes(obj: &Map<String, Value>) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(Value::Array(ps)) = obj.get("jsPlugins") {
        for p in ps {
            match p {
                Value::String(s) if is_ours(s) => out.push(DEFAULT_PREFIX.to_string()),
                Value::Object(o) => {
                    if o.get("specifier")
                        .and_then(Value::as_str)
                        .is_some_and(is_ours)
                        && let Some(n) = o.get("name").and_then(Value::as_str)
                    {
                        out.push(n.to_string());
                    }
                }
                _ => {}
            }
        }
    }
    out
}

fn is_ours(spec: &str) -> bool {
    spec == PACKAGE || spec.contains("oxlint-vue-rs")
}

impl Config {
    pub fn load(path: &Path) -> Result<Config, String> {
        let mut cfg = Config {
            base: path.parent().map(Path::to_path_buf).unwrap_or_default(),
            settings: Value::Object(Map::new()),
            ..Default::default()
        };
        cfg.merge_file(path, 0)?;
        if cfg.prefixes.is_empty() {
            cfg.prefixes.push(DEFAULT_PREFIX.to_string());
        }
        // Resolve rules now that every prefix is known.
        Ok(cfg)
    }

    fn merge_file(&mut self, path: &Path, depth: usize) -> Result<(), String> {
        if depth > 16 {
            return Err("extends nesting too deep".into());
        }
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let v: Value = serde_json::from_str(&strip_jsonc(&text))
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let obj = v
            .as_object()
            .ok_or_else(|| format!("{}: config must be an object", path.display()))?;
        let dir = path.parent().unwrap_or(Path::new("."));
        if let Some(Value::Array(ext)) = obj.get("extends") {
            for e in ext.iter().filter_map(Value::as_str) {
                let p = resolve_extends(dir, e);
                if let Some(p) = p {
                    self.merge_file(&p, depth + 1)?;
                }
            }
        }
        for p in plugin_prefixes(obj) {
            if !self.prefixes.contains(&p) {
                self.prefixes.push(p);
            }
        }
        let mut prefixes = self.prefixes.clone();
        if !prefixes.iter().any(|p| p == DEFAULT_PREFIX) {
            prefixes.push(DEFAULT_PREFIX.to_string());
        }
        if let Some(Value::Object(r)) = obj.get("rules") {
            for (k, v) in r {
                if let Some(name) = strip_prefix(&prefixes, k) {
                    let s = parse_rule_setting(v)
                        .map_err(|e| format!("{}: rule {k}: {e}", path.display()))?;
                    self.rules.retain(|(n, _)| n != name);
                    self.rules.push((name.to_string(), s));
                }
            }
        }
        if let Some(Value::Array(ovs)) = obj.get("overrides") {
            for o in ovs {
                let files = o
                    .get("files")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(Value::as_str)
                            .map(String::from)
                            .collect()
                    })
                    .unwrap_or_default();
                let mut ov = Override {
                    files,
                    rules: Vec::new(),
                };
                if let Some(Value::Object(r)) = o.get("rules") {
                    for (k, v) in r {
                        if let Some(name) = strip_prefix(&prefixes, k) {
                            ov.rules.push((name.to_string(), parse_rule_setting(v)?));
                        }
                    }
                }
                self.overrides.push(ov);
            }
        }
        if let Some(Value::Array(ig)) = obj.get("ignorePatterns") {
            self.ignore_patterns
                .extend(ig.iter().filter_map(Value::as_str).map(String::from));
        }
        if let Some(Value::Object(g)) = obj.get("globals") {
            if !self.globals.is_object() {
                self.globals = Value::Object(Map::new());
            }
            if let Value::Object(cur) = &mut self.globals {
                for (k, v) in g {
                    cur.insert(k.clone(), v.clone());
                }
            }
        }
        if let Some(Value::Object(s)) = obj.get("settings")
            && let Value::Object(cur) = &mut self.settings
        {
            for (k, v) in s {
                cur.insert(k.clone(), v.clone());
            }
        }
        Ok(())
    }

    /// Effective rules for a file (base rules, then matching overrides in order).
    pub fn rules_for(&self, file: &Path) -> Vec<(String, RuleSetting)> {
        let mut out = self.rules.clone();
        let rel = file.strip_prefix(&self.base).unwrap_or(file);
        let rel = rel.to_string_lossy().replace('\\', "/");
        for o in &self.overrides {
            if o.files.iter().any(|g| glob_match(g, &rel)) {
                for (n, s) in &o.rules {
                    out.retain(|(m, _)| m != n);
                    out.push((n.clone(), s.clone()));
                }
            }
        }
        out
    }

    pub fn is_ignored(&self, file: &Path) -> bool {
        let rel = file
            .strip_prefix(&self.base)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        self.ignore_patterns.iter().any(|g| {
            glob_match(g, &rel) || glob_match(&format!("{}/**", g.trim_end_matches('/')), &rel)
        })
    }
}

fn strip_prefix<'k>(prefixes: &[String], key: &'k str) -> Option<&'k str> {
    let (p, name) = key.split_once('/')?;
    prefixes.iter().any(|x| x == p).then_some(name)
}

fn resolve_extends(dir: &Path, e: &str) -> Option<PathBuf> {
    let p = dir.join(e);
    if p.is_file() {
        return Some(p);
    }
    // `oxlint-vue-rs/configs/recommended` style: look in node_modules upwards.
    let mut d = Some(dir);
    while let Some(cur) = d {
        for cand in [
            cur.join("node_modules").join(e),
            cur.join("node_modules").join(format!("{e}.json")),
        ] {
            if cand.is_file() {
                return Some(cand);
            }
        }
        d = cur.parent();
    }
    None
}

/// Find `.oxlintrc.json` (or `oxlintrc.json`) from `start` upwards.
pub fn find_config(start: &Path) -> Option<PathBuf> {
    let mut d = Some(start);
    while let Some(cur) = d {
        for n in [".oxlintrc.json", "oxlintrc.json", ".oxlintrc.jsonc"] {
            let p = cur.join(n);
            if p.is_file() {
                return Some(p);
            }
        }
        d = cur.parent();
    }
    None
}

/// Minimal glob: `**` (any dirs), `*` (no `/`), `?`, `{a,b}`; a pattern without `/` matches the basename anywhere.
pub fn glob_match(pat: &str, path: &str) -> bool {
    if let Some(open) = pat.find('{')
        && let Some(close) = pat[open..].find('}').map(|c| c + open)
    {
        return pat[open + 1..close].split(',').any(|alt| {
            glob_match(
                &format!("{}{}{}", &pat[..open], alt, &pat[close + 1..]),
                path,
            )
        });
    }
    let pat = pat.strip_prefix("./").unwrap_or(pat);
    if !pat.contains('/') {
        let base = path.rsplit('/').next().unwrap_or(path);
        return wild(pat.as_bytes(), base.as_bytes()) || wild(pat.as_bytes(), path.as_bytes());
    }
    wild(pat.as_bytes(), path.as_bytes())
}

fn wild(p: &[u8], s: &[u8]) -> bool {
    if p.is_empty() {
        return s.is_empty();
    }
    if p.starts_with(b"**") {
        let rest = p[2..].strip_prefix(b"/").unwrap_or(&p[2..]);
        if wild(rest, s) {
            return true;
        }
        return (0..s.len()).any(|i| (s[i] == b'/' || p.len() == 2) && wild(rest, &s[i + 1..]))
            || (p.len() == 2);
    }
    match p[0] {
        b'*' => (0..=s.len())
            .take_while(|&i| i == 0 || s[i - 1] != b'/')
            .any(|i| wild(&p[1..], &s[i..])),
        b'?' => !s.is_empty() && s[0] != b'/' && wild(&p[1..], &s[1..]),
        c => !s.is_empty() && s[0] == c && wild(&p[1..], &s[1..]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs() {
        assert!(glob_match("**/*.vue", "a/b/C.vue"));
        assert!(glob_match("*.vue", "a/b/C.vue"));
        assert!(glob_match("src/**", "src/a/b.vue"));
        assert!(glob_match("src/*.{vue,ts}", "src/a.ts"));
        assert!(!glob_match("src/*.vue", "src/a/b.vue"));
        assert!(glob_match("**/test/**", "x/test/y/z.vue"));
    }

    #[test]
    fn jsonc() {
        let v: Value = serde_json::from_str(&strip_jsonc(
            "{\n// c\n\"a\": [1,2,], /* x */ \"b\": \"//no\",\n}",
        ))
        .unwrap();
        assert_eq!(v["b"], "//no");
        assert_eq!(v["a"][1], 2);
    }
}
