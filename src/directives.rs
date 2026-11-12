//! Inline disable directives: `eslint-disable`, `eslint-enable`, `eslint-disable-line`,
//! `eslint-disable-next-line` and the same with `oxlint-` — in script comments and (for the CLI)
//! in template `<!-- … -->` comments, like eslint-plugin-vue's `comment-directive`.
//!
//! A rule list matches `vue/<rule>` or `<prefix>/<rule>`; no list = every rule.

use crate::Diagnostic;
use crate::LintFile;
use crate::ast::TokenKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Disable,
    Enable,
    Line,
    NextLine,
}

#[derive(Debug)]
struct Directive {
    kind: Kind,
    /// Byte offset where the directive takes effect (comment start for line kinds).
    at: u32,
    /// Comment end offset.
    end: u32,
    /// 0-based line of the comment start / end.
    line_start: usize,
    line_end: usize,
    rules: Vec<String>,
}

fn parse(text: &str) -> Option<(Kind, Vec<String>)> {
    let t = text.trim_start();
    let body = t
        .strip_prefix("eslint-")
        .or_else(|| t.strip_prefix("oxlint-"))?;
    let (kind, rest) = if let Some(r) = body.strip_prefix("disable-next-line") {
        (Kind::NextLine, r)
    } else if let Some(r) = body.strip_prefix("disable-line") {
        (Kind::Line, r)
    } else if let Some(r) = body.strip_prefix("disable") {
        (Kind::Disable, r)
    } else {
        (Kind::Enable, body.strip_prefix("enable")?)
    };
    if !(rest.is_empty() || rest.starts_with(char::is_whitespace)) {
        return None;
    }
    // `-- description` ends the rule list.
    let rest = rest.split(" --").next().unwrap_or("");
    let rules = rest
        .split(',')
        .map(|r| r.trim().to_string())
        .filter(|r| !r.is_empty())
        .collect();
    Some((kind, rules))
}

fn matches(rules: &[String], rule: &str, prefixes: &[String]) -> bool {
    rules.is_empty()
        || rules.iter().any(|r| {
            r.split_once('/')
                .is_some_and(|(p, n)| n == rule && (p == "vue" || prefixes.iter().any(|x| x == p)))
        })
}

/// Drop diagnostics silenced by directives. `template` = also honor `<!-- -->` template comments.
pub fn filter(
    file: &LintFile,
    diags: Vec<Diagnostic>,
    prefixes: &[String],
    template: bool,
) -> Vec<Diagnostic> {
    let mut dirs = Vec::new();
    let mut add = |text: &str, start: u32, end: u32| {
        if let Some((kind, rules)) = parse(text) {
            dirs.push(Directive {
                kind,
                at: start,
                end,
                line_start: file.lines.line_of(start),
                line_end: file.lines.line_of(end),
                rules,
            });
        }
    };
    if let Some(p) = file.program {
        for c in &p.comments {
            let s = c.content_span();
            add(file.slice(s), c.span.start, c.span.end);
        }
    }
    if template && let Some(doc) = &file.doc {
        for c in doc
            .comments
            .iter()
            .filter(|c| c.kind == TokenKind::HTMLComment)
        {
            add(&c.value, c.span.start, c.span.end);
        }
    }
    if dirs.is_empty() {
        return diags;
    }
    dirs.sort_by_key(|d| d.at);
    // Directives (by index) that silenced something: `(directive, Some(rule entry))` for a listed rule,
    // `(directive, None)` for a directive without a rule list. Feeds `comment-directive`'s unused reports.
    let mut used: Vec<(usize, Option<usize>)> = Vec::new();
    let entry = |dir: &Directive, rule: &str| -> Option<Option<usize>> {
        if dir.rules.is_empty() {
            return Some(None);
        }
        dir.rules
            .iter()
            .position(|r| matches(std::slice::from_ref(r), rule, prefixes))
            .map(Some)
    };
    let kept: Vec<Diagnostic> = diags
        .into_iter()
        .filter(|d| {
            // `comment-directive` reports are never silenced (the upstream processor handles them first).
            if d.rule == UNUSED_RULE {
                return true;
            }
            let line = file.lines.line_of(d.span.start);
            let mut active: Vec<(usize, Option<usize>)> = Vec::new();
            for (i, dir) in dirs.iter().enumerate() {
                match dir.kind {
                    Kind::Line if dir.line_start == line || dir.line_end == line => {
                        if let Some(e) = entry(dir, d.rule) {
                            used.push((i, e));
                            return false;
                        }
                    }
                    Kind::NextLine if dir.line_end + 1 == line => {
                        if let Some(e) = entry(dir, d.rule) {
                            used.push((i, e));
                            return false;
                        }
                    }
                    Kind::Disable if dir.at <= d.span.start => {
                        if let Some(e) = entry(dir, d.rule) {
                            active.push((i, e));
                        }
                    }
                    Kind::Enable
                        if dir.at <= d.span.start && matches(&dir.rules, d.rule, prefixes) =>
                    {
                        active.clear()
                    }
                    _ => {}
                }
            }
            if active.is_empty() {
                return true;
            }
            used.extend(active);
            false
        })
        .collect();
    // Drop `comment-directive` unused reports whose directive (or rule entry) did silence something.
    kept.into_iter()
        .filter(|d| {
            if d.rule != UNUSED_RULE {
                return true;
            }
            let Some(i) = dirs
                .iter()
                .position(|x| x.at <= d.span.start && d.span.end <= x.end)
            else {
                return true;
            };
            match d.message_id {
                Some("unused") => !used.iter().any(|u| u.0 == i),
                Some("unusedRule") => {
                    let name = file.slice(d.span);
                    !used
                        .iter()
                        .any(|u| u.0 == i && u.1.is_some_and(|e| dirs[i].rules[e] == name))
                }
                _ => true,
            }
        })
        .collect()
}

/// Rule whose "unused directive" reports are resolved here.
const UNUSED_RULE: &str = "comment-directive";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses() {
        assert_eq!(
            parse(" eslint-disable-next-line vue/a, vue-rs/b -- why").unwrap(),
            (Kind::NextLine, vec!["vue/a".into(), "vue-rs/b".into()])
        );
        assert_eq!(parse("oxlint-disable").unwrap(), (Kind::Disable, vec![]));
        assert!(parse("eslint-disabled").is_none());
        assert!(matches(&["vue-rs/x".into()], "x", &["vue-rs".into()]));
        assert!(!matches(&["other/x".into()], "x", &["vue-rs".into()]));
    }
}
