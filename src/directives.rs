//! Inline disable directives: `eslint-disable`, `eslint-enable`, `eslint-disable-line`,
//! `eslint-disable-next-line` and the same with `oxlint-` — in script comments and (for the CLI)
//! in template `<!-- … -->` comments, like eslint-plugin-vue's `comment-directive`.
//!
//! A rule list matches `vue/<rule>` or `<prefix>/<rule>`; no list = every rule.

use std::collections::HashMap;

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

/// `html` = a template comment (eslint-plugin-vue `comment-directive`): rules split on `[\s,]+`;
/// otherwise ESLint core: rules split on `,`. Both drop a `\s-{2,}\s` description.
fn parse(text: &str, html: bool) -> Option<(Kind, Vec<String>)> {
    let t = crate::rules::comment_directive::strip(text).trim_start();
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
    let sep = |c: char| c == ',' || (html && c.is_whitespace());
    let rules = rest
        .split(sep)
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
    let mut add = |text: &str, start: u32, end: u32, html: bool| {
        let Some((kind, rules)) = parse(text, html) else {
            return;
        };
        let (line_start, line_end) = (file.lines.line_of(start), file.lines.line_of(end));
        // ESLint ignores a multi-line `-line` directive; eslint-plugin-vue also a multi-line `-next-line`.
        if line_start != line_end && (kind == Kind::Line || (html && kind == Kind::NextLine)) {
            return;
        }
        dirs.push(Directive {
            kind,
            at: start,
            end,
            line_start,
            line_end,
            rules,
        });
    };
    if let Some(p) = file.program {
        for c in &p.comments {
            let s = c.content_span();
            add(file.slice(s), c.span.start, c.span.end, false);
        }
    }
    if template && let Some(doc) = &file.doc {
        // HTML comments and the JS comments inside template expressions (`templateBody.comments`).
        for c in doc.comments.iter().filter(|c| {
            matches!(
                c.kind,
                TokenKind::HTMLComment | TokenKind::LineComment | TokenKind::BlockComment
            )
        }) {
            add(&c.value, c.span.start, c.span.end, true);
        }
    }
    if dirs.is_empty() {
        return diags;
    }
    dirs.sort_by_key(|d| d.at);
    // Line directives by the line they silence; block ones in source order.
    let mut by_line: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut blocks = Vec::new();
    for (i, d) in dirs.iter().enumerate() {
        match d.kind {
            Kind::Line => by_line.entry(d.line_start).or_default().push(i),
            Kind::NextLine => by_line.entry(d.line_end + 1).or_default().push(i),
            Kind::Disable | Kind::Enable => blocks.push(i),
        }
    }
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
    // Per rule: block directives applied so far (index into `blocks`) and the active disables.
    // Diagnostics come sorted by start, so each rule sweeps `blocks` once.
    type Sweep = (usize, Vec<(usize, Option<usize>)>);
    let mut sweeps: HashMap<&str, Sweep> = HashMap::new();
    let kept: Vec<Diagnostic> = diags
        .into_iter()
        .filter(|d| {
            // `comment-directive` reports are never silenced (the upstream processor handles them first).
            if d.rule == UNUSED_RULE {
                return true;
            }
            let line = file.lines.line_of(d.span.start);
            for &i in by_line.get(&line).into_iter().flatten() {
                if let Some(e) = entry(&dirs[i], d.rule) {
                    used.push((i, e));
                    return false;
                }
            }
            let (next, active) = sweeps.entry(d.rule).or_default();
            if *next > 0 && dirs[blocks[*next - 1]].at > d.span.start {
                // Out of order (not from `lint_source`): restart this rule's sweep.
                *next = 0;
                active.clear();
            }
            while let Some(&i) = blocks.get(*next) {
                let dir = &dirs[i];
                if dir.at > d.span.start {
                    break;
                }
                match dir.kind {
                    Kind::Disable => {
                        if let Some(e) = entry(dir, d.rule) {
                            active.push((i, e));
                        }
                    }
                    _ if matches(&dir.rules, d.rule, prefixes) => active.clear(),
                    _ => {}
                }
                *next += 1;
            }
            if active.is_empty() {
                return true;
            }
            used.extend(active.iter().copied());
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
            parse(" eslint-disable-next-line vue/a, vue-rs/b -- why", false).unwrap(),
            (Kind::NextLine, vec!["vue/a".into(), "vue-rs/b".into()])
        );
        assert_eq!(
            parse("oxlint-disable", false).unwrap(),
            (Kind::Disable, vec![])
        );
        assert!(parse("eslint-disabled", false).is_none());
        // `\s-{2,}\s` description separator (tab, newline); `--x` is part of the rule name.
        assert_eq!(
            parse("eslint-disable-line vue/a\t-- tab", false).unwrap().1,
            vec!["vue/a".to_string()]
        );
        assert_eq!(
            parse("eslint-disable vue/a\n--- why", false).unwrap().1,
            vec!["vue/a".to_string()]
        );
        assert_eq!(
            parse("eslint-disable-line vue/x --x", false).unwrap().1,
            vec!["vue/x --x".to_string()]
        );
        // Template lists split on whitespace too; script lists only on commas.
        assert_eq!(
            parse(" eslint-disable-next-line vue/a vue/b,vue/c ", true)
                .unwrap()
                .1,
            vec!["vue/a".to_string(), "vue/b".into(), "vue/c".into()]
        );
        assert!(matches(&["vue-rs/x".into()], "x", &["vue-rs".into()]));
        assert!(!matches(&["other/x".into()], "x", &["vue-rs".into()]));
    }
}
