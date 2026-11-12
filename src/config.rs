//! Reads the same `.oxlintrc.json` oxlint uses.
//!
//! oxlint rejects unknown `vue/<rule>` names (and reserves the `vue` JS plugin name), so our rules
//! live under the alias of the shim JS plugin, e.g.
//! `"jsPlugins": [{ "name": "vue-rs", "specifier": "oxlint-vue-rs" }]` → `"vue-rs/html-indent": "warn"`.
//! The shim registers every rule name, so oxlint validates the config; this crate does the work.
//! A bare `"jsPlugins": ["oxlint-vue-rs"]` uses the default name `vue-rs`.
//!
//! Supported keys: `rules`, `overrides[{files, rules}]`, `ignorePatterns`, `settings`, `extends` (paths).

use std::path::{Component, Path, PathBuf};

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
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
    /// `files` globs, matched against the path relative to the config directory.
    pub files: GlobSet,
    pub rules: Vec<(String, RuleSetting)>,
}

#[derive(Debug, Clone, Default)]
pub struct Config {
    /// Absolute directory globs are relative to.
    pub base: PathBuf,
    /// Accepted rule prefixes (alias names of the shim plugin).
    pub prefixes: Vec<String>,
    pub rules: Vec<(String, RuleSetting)>,
    pub overrides: Vec<Override>,
    pub ignore_patterns: Vec<String>,
    /// `ignorePatterns` compiled with gitignore semantics, rooted at `base`.
    ignore: Option<Gitignore>,
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
            // Copy whole UTF-8 sequences (an escaped char too).
            let len = if c == b'\\' && i + 1 < b.len() {
                1 + utf8_len(b[i + 1])
            } else {
                utf8_len(c)
            };
            let len = len.min(b.len() - i);
            out.push_str(&src[i..i + len]);
            if c == b'"' {
                in_str = false;
            }
            i += len;
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
                // Look past whitespace and comments for a closing bracket.
                let mut j = i + 1;
                loop {
                    if j < b.len() && b[j].is_ascii_whitespace() {
                        j += 1;
                    } else if b.get(j) == Some(&b'/') && b.get(j + 1) == Some(&b'/') {
                        while j < b.len() && b[j] != b'\n' {
                            j += 1;
                        }
                    } else if b.get(j) == Some(&b'/') && b.get(j + 1) == Some(&b'*') {
                        j += 2;
                        while j + 1 < b.len() && !(b[j] == b'*' && b[j + 1] == b'/') {
                            j += 1;
                        }
                        j += 2;
                    } else {
                        break;
                    }
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
        let path = absolute(path);
        let mut cfg = Config {
            base: path.parent().map(Path::to_path_buf).unwrap_or_default(),
            settings: Value::Object(Map::new()),
            ..Default::default()
        };
        cfg.merge_file(&path, 0)?;
        if cfg.prefixes.is_empty() {
            cfg.prefixes.push(DEFAULT_PREFIX.to_string());
        }
        if !cfg.ignore_patterns.is_empty() {
            cfg.ignore = Some(
                gitignore(&cfg.base, &cfg.ignore_patterns)
                    .map_err(|e| format!("{}: ignorePatterns: {e}", path.display()))?,
            );
        }
        Ok(cfg)
    }

    /// The config used when no config file is found.
    pub fn empty(base: &Path) -> Config {
        Config {
            base: absolute(base),
            prefixes: vec![DEFAULT_PREFIX.into()],
            settings: Value::Object(Map::new()),
            ..Default::default()
        }
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
                let p = resolve_extends(dir, e).ok_or_else(|| {
                    format!(
                        "{}: invalid config file: extends {e:?} not found",
                        path.display()
                    )
                })?;
                self.merge_file(&p, depth + 1)?;
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
                let pats: Vec<&str> = o
                    .get("files")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(Value::as_str).collect())
                    .unwrap_or_default();
                let files = override_globs(&pats)
                    .map_err(|e| format!("{}: overrides.files: {e}", path.display()))?;
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
    /// `file` is absolute (see [`absolute`]).
    pub fn rules_for(&self, file: &Path) -> Vec<(String, RuleSetting)> {
        let mut out = self.rules.clone();
        let Ok(rel) = file.strip_prefix(&self.base) else {
            return out;
        };
        for o in &self.overrides {
            if o.files.is_match(rel) {
                for (n, s) in &o.rules {
                    out.retain(|(m, _)| m != n);
                    out.push((n.clone(), s.clone()));
                }
            }
        }
        out
    }

    /// `ignorePatterns` (gitignore semantics, relative to the config directory). `file` is absolute.
    pub fn is_ignored(&self, file: &Path) -> bool {
        match (&self.ignore, file.strip_prefix(&self.base)) {
            (Some(gi), Ok(rel)) => gi.matched_path_or_any_parents(rel, false).is_ignore(),
            _ => false,
        }
    }
}

/// gitignore-style matcher rooted at `root` (oxlint `ignorePatterns`, `--ignore-pattern`).
pub fn gitignore(root: &Path, patterns: &[String]) -> Result<Gitignore, String> {
    let mut b = GitignoreBuilder::new(root);
    for p in patterns {
        b.add_line(None, p).map_err(|e| e.to_string())?;
    }
    b.build().map_err(|e| e.to_string())
}

/// oxlint override globs: a pattern without `/` matches at any depth (`**/` prefix), `*` stops at `/`.
fn override_globs(pats: &[&str]) -> Result<GlobSet, String> {
    let mut b = GlobSetBuilder::new();
    for p in pats {
        let p = p.strip_prefix("./").unwrap_or(p);
        let p = if p.contains('/') {
            p.to_string()
        } else {
            format!("**/{p}")
        };
        b.add(
            GlobBuilder::new(&p)
                .literal_separator(true)
                .build()
                .map_err(|e| e.to_string())?,
        );
    }
    b.build().map_err(|e| e.to_string())
}

/// `cwd`-joined path with `.` / `..` resolved lexically (symlinks kept).
pub fn absolute(p: &Path) -> PathBuf {
    let joined = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(p)
    };
    let mut out = PathBuf::new();
    for c in joined.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            c => out.push(c),
        }
    }
    out
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

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ovr-cfg-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (f, c) in files {
            let p = dir.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, c).unwrap();
        }
        dir
    }

    #[test]
    fn severities() {
        use serde_json::json;
        assert_eq!(parse_severity(&json!("off")).unwrap(), None);
        assert_eq!(parse_severity(&json!("allow")).unwrap(), None);
        assert_eq!(
            parse_severity(&json!("warn")).unwrap(),
            Some(Severity::Warn)
        );
        assert_eq!(
            parse_severity(&json!("deny")).unwrap(),
            Some(Severity::Error)
        );
        assert_eq!(parse_severity(&json!(0)).unwrap(), None);
        assert_eq!(parse_severity(&json!(1)).unwrap(), Some(Severity::Warn));
        assert_eq!(parse_severity(&json!(2)).unwrap(), Some(Severity::Error));
        assert!(parse_severity(&json!(3)).is_err());
        assert!(parse_severity(&json!("loud")).is_err());
        let s = parse_rule_setting(&json!(["warn", 2, {"a": 1}])).unwrap();
        assert_eq!((s.severity, s.options.len()), (Some(Severity::Warn), 2));
        let s = parse_rule_setting(&json!("error")).unwrap();
        assert_eq!((s.severity, s.options.len()), (Some(Severity::Error), 0));
        assert!(parse_rule_setting(&json!([])).is_err());
    }

    #[test]
    fn plugin_alias_detection() {
        let d = tmp(
            "alias",
            &[(
                ".oxlintrc.json",
                r#"{ "jsPlugins": [{ "name": "vx", "specifier": "oxlint-vue-rs" }, { "name": "other", "specifier": "eslint-plugin-foo" }, "some-plugin"],
            "rules": { "vx/html-indent": "warn", "other/html-quotes": "error", "vue-rs/v-on-style": 1, "vue/no-dupe-keys": 2 } }"#,
            )],
        );
        let c = Config::load(&d.join(".oxlintrc.json")).unwrap();
        assert_eq!(c.prefixes, vec!["vx".to_string()]);
        let names: Vec<_> = c.rules.iter().map(|(n, _)| n.as_str()).collect();
        assert!(
            names.contains(&"html-indent") && names.contains(&"v-on-style"),
            "{names:?}"
        );
        assert!(
            !names.contains(&"html-quotes") && !names.contains(&"no-dupe-keys"),
            "{names:?}"
        );
        let d = tmp(
            "alias2",
            &[(
                ".oxlintrc.json",
                r#"{ "jsPlugins": ["oxlint-vue-rs"], "rules": {} }"#,
            )],
        );
        assert_eq!(
            Config::load(&d.join(".oxlintrc.json")).unwrap().prefixes,
            vec![DEFAULT_PREFIX.to_string()]
        );
        let d = tmp(
            "alias3",
            &[(
                ".oxlintrc.json",
                r#"{ "jsPlugins": [{ "name": "x", "specifier": "./node_modules/oxlint-vue-rs/index.js" }] }"#,
            )],
        );
        assert_eq!(
            Config::load(&d.join(".oxlintrc.json")).unwrap().prefixes,
            vec!["x".to_string()]
        );
        assert!(!is_ours("eslint-plugin-vue"));
    }

    #[test]
    fn extends_chain_settings_and_depth() {
        let d = tmp(
            "ext",
            &[
                (
                    "base.json",
                    r#"{ "rules": { "vue-rs/html-indent": ["warn", 4] }, "settings": { "a": 1 } }"#,
                ),
                (
                    ".oxlintrc.json",
                    r#"{ "extends": ["./base.json"], "rules": { "vue-rs/html-indent": "error" }, "settings": { "b": 2 } }"#,
                ),
            ],
        );
        let c = Config::load(&d.join(".oxlintrc.json")).unwrap();
        let (_, s) = c.rules.iter().find(|(n, _)| n == "html-indent").unwrap();
        assert_eq!(s.severity, Some(Severity::Error));
        assert_eq!(
            (c.settings["a"].clone(), c.settings["b"].clone()),
            (serde_json::json!(1), serde_json::json!(2))
        );
        let d = tmp(
            "loop",
            &[(".oxlintrc.json", r#"{ "extends": ["./.oxlintrc.json"] }"#)],
        );
        assert!(
            Config::load(&d.join(".oxlintrc.json"))
                .unwrap_err()
                .contains("too deep")
        );
    }

    #[test]
    fn jsonc_block_and_line_comments() {
        let v: Value = serde_json::from_str(&strip_jsonc(
            "{/*x*/\"a\":/* y */1, // z\n\"b\": \"/* keep */\", \"c\": [1,/*q*/]}",
        ))
        .unwrap();
        assert_eq!(v["a"], 1);
        assert_eq!(v["b"], "/* keep */");
        assert_eq!(v["c"].as_array().unwrap().len(), 1);
        assert_eq!(strip_jsonc("{\"é\": \"ü\\\"\"}"), "{\"é\": \"ü\\\"\"}");
    }

    // Cases checked against oxlint 1.93 overrides.
    #[test]
    fn globs() {
        let m = |p: &str, f: &str| override_globs(&[p]).unwrap().is_match(f);
        assert!(m("**/*.vue", "a/b/C.vue"));
        assert!(m("*.vue", "a/b/C.vue"));
        assert!(m("x.vue", "src/deep/x.vue"));
        assert!(m("src/**", "src/a/b.vue"));
        assert!(m("./src/**", "src/a/b.vue"));
        assert!(m("src/*.{vue,ts}", "src/a.ts"));
        assert!(m("src/[d]eep/*", "src/deep/x.vue"));
        assert!(!m("src/*.vue", "src/a/b.vue"));
        assert!(!m("deep/*.vue", "src/deep/x.vue"));
        assert!(!m("/src/**", "src/a.vue"));
        assert!(!m("src", "src/a.vue"));
        assert!(m("**/test/**", "x/test/y/z.vue"));
    }

    #[test]
    fn ignore_patterns_are_gitignore() {
        let cfg = Config {
            base: PathBuf::from("/r"),
            ignore: Some(
                gitignore(
                    Path::new("/r"),
                    &["/build".into(), "gen/**".into(), "!gen/keep/**".into()],
                )
                .unwrap(),
            ),
            ..Default::default()
        };
        assert!(cfg.is_ignored(Path::new("/r/build/a.vue")));
        assert!(!cfg.is_ignored(Path::new("/r/x/build/a.vue")));
        assert!(cfg.is_ignored(Path::new("/r/gen/b.vue")));
        assert!(!cfg.is_ignored(Path::new("/r/gen/keep/c.vue")));
        assert!(!cfg.is_ignored(Path::new("/elsewhere/gen/b.vue")));
    }

    #[test]
    fn jsonc_non_ascii_and_comment_after_trailing_comma() {
        let v: Value = serde_json::from_str(&strip_jsonc(
            "{ \"a\": [\"©\", \"\\\"→\"], // c\n \"b\": 1, // c\n /* d */ }",
        ))
        .unwrap();
        assert_eq!(v["a"][0], "©");
        assert_eq!(v["a"][1], "\"→");
        assert_eq!(v["b"], 1);
    }

    #[test]
    fn missing_extends_is_an_error() {
        let dir = std::env::temp_dir().join(format!("ovr-ext-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("c.json");
        std::fs::write(&p, r#"{ "extends": ["./missing.json"] }"#).unwrap();
        let e = Config::load(&p).unwrap_err();
        std::fs::remove_dir_all(&dir).ok();
        assert!(e.contains("missing.json"), "{e}");
    }

    #[test]
    fn absolute_is_lexical() {
        assert_eq!(absolute(Path::new("/a/./b/../c")), PathBuf::from("/a/c"));
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
