//! Fix-verification core, shared by `tests/fixes.rs` (fast checks on the upstream inputs) and
//! `examples/fix_check.rs` (the corpus driver behind `scripts/fix-check.mjs`).
//!
//! For one input and one rule configuration it runs the `--fix` loop (single ESLint-style passes until
//! stable, max 10) and checks what can be checked without Vue's compiler: the output still parses
//! (our parser, oxc for every script block), the loop converges without cycling, and nothing is lost
//! (line endings, trailing newline, comments, disable directives, non-ASCII text, other SFC blocks).
//! Whether a script block's AST changed (modulo spans, comments and parentheses) is reported, not
//! judged: that depends on the rule. Template render code is compared in `scripts/fix-check.mjs`.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::Path;

use oxc::allocator::Allocator;
use oxc::codegen::{Codegen, CodegenOptions, CommentOptions};
use oxc::parser::{ParseOptions, Parser};
use oxc::span::SourceType;
use oxlint_vue_rs::parser::{ParserOptions, parse_document};
use oxlint_vue_rs::{
    ConfiguredRule, Diagnostic, FileOptions, PARSING_ERROR, Severity, apply_fixes, configure,
    lint_source, script_blocks,
};
use serde_json::Value;

pub const MAX_PASSES: usize = 10;

/// What the fix loop did to one input.
#[derive(Debug, Default)]
pub struct Outcome {
    /// Output of the first pass (ESLint `--fix` single pass).
    pub first: String,
    /// Output after the loop.
    pub after: String,
    pub passes: usize,
    /// Problems found (empty = clean). Short tags with details: `parse:`, `script-parse:`, `cycle`,
    /// `no-converge`, `eol:`, `trailing-newline`, `comments:`, `directives`, `non-ascii`, `blocks`.
    pub problems: Vec<String>,
    /// Script blocks whose AST changed (block index).
    pub script_changed: Vec<usize>,
}

pub fn rule(name: &str, options: &[Value], settings: &Value) -> Option<ConfiguredRule> {
    configure(name, Severity::Error, options, settings)
        .ok()
        .flatten()
}

/// Promote the first suggestion of each fix-less diagnostic to a fix (CLI `--fix-suggestions`).
pub fn promote(ds: &mut [Diagnostic]) {
    for d in ds {
        if d.fix.is_none()
            && let Some(s) = d.suggestions.first()
        {
            d.fix = Some(s.fix.clone());
        }
    }
}

/// Run the `--fix` loop. `None` when nothing is fixable.
pub fn fix_loop(
    code: &str,
    path: &Path,
    opts: &FileOptions,
    rules: &[ConfiguredRule],
    suggest: bool,
) -> Option<(String, String, usize, Vec<String>)> {
    let lint = |s: &str| {
        let mut ds = lint_source(s, path, opts, rules);
        if suggest {
            promote(&mut ds);
        }
        ds
    };
    let mut src = code.to_string();
    let mut seen = vec![src.clone()];
    let mut first = None;
    let mut problems = Vec::new();
    let mut passes = 0;
    loop {
        let ds = lint(&src);
        let Some(next) = apply_fixes(&src, &ds) else {
            break;
        };
        if next == src {
            // A fix that rewrites text to itself: harmless but reported as fixable forever.
            problems.push("noop-fix".to_string());
            break;
        }
        if passes == MAX_PASSES {
            problems.push("no-converge".to_string());
            break;
        }
        passes += 1;
        first.get_or_insert_with(|| next.clone());
        if seen.contains(&next) {
            problems.push("cycle".to_string());
            src = next;
            break;
        }
        seen.push(next.clone());
        src = next;
    }
    first.map(|f| (f, src, passes, problems))
}

/// Parse errors as our parser and oxc report them (`no-parsing-error` + fatal script errors).
fn parse_errors(code: &str, path: &Path, opts: &FileOptions) -> Vec<String> {
    let r = [rule("no-parsing-error", &[], &Value::Null).unwrap()];
    let o = FileOptions {
        no_inline_config: true,
        ..opts.clone()
    };
    lint_source(code, path, &o, &r)
        .into_iter()
        .map(|d| {
            if d.rule == PARSING_ERROR {
                "fatal".to_string()
            } else {
                d.message
            }
        })
        .collect()
}

/// Per script block: (codegen without comments/parens, error count, comment count).
fn scripts(code: &str, path: &Path, opts: &FileOptions) -> Vec<(String, usize, usize)> {
    let alloc = Allocator::default();
    let sfc = opts
        .sfc
        .unwrap_or_else(|| path.extension().is_some_and(|e| e == "vue"));
    let blocks: Vec<(String, bool, bool)> = if sfc {
        let doc = parse_document(&alloc, code, &ParserOptions::default());
        script_blocks(&doc)
            .into_iter()
            .map(|b| {
                let lang = b.lang.as_deref().unwrap_or("");
                (
                    code[b.content.start as usize..b.content.end as usize].to_string(),
                    opts.ts || matches!(lang, "ts" | "tsx"),
                    opts.jsx || matches!(lang, "tsx" | "jsx"),
                )
            })
            .collect()
    } else {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        vec![(
            code.to_string(),
            opts.ts || matches!(ext, "ts" | "tsx"),
            opts.jsx || matches!(ext, "jsx" | "tsx"),
        )]
    };
    blocks
        .into_iter()
        .map(|(text, ts, jsx)| {
            let alloc = Allocator::default();
            let st = SourceType::default()
                .with_module(true)
                .with_typescript(ts)
                .with_jsx(jsx);
            let ret = Parser::new(&alloc, &text, st)
                .with_options(ParseOptions {
                    allow_return_outside_function: true,
                    preserve_parens: false,
                    ..ParseOptions::default()
                })
                .parse();
            let errors = ret.diagnostics.errors().count() + usize::from(ret.fatal_error);
            let comments = ret.program.comments.len();
            let printed = Codegen::new()
                .with_options(CodegenOptions {
                    comments: CommentOptions::disabled(),
                    ..CodegenOptions::default()
                })
                .build(&ret.program)
                .code;
            (printed, errors, comments)
        })
        .collect()
}

/// Top-level blocks other than `<template>` / `<script>`: (name, trimmed content), sorted.
fn other_blocks(code: &str) -> Vec<(String, String)> {
    let alloc = Allocator::default();
    let doc = parse_document(&alloc, code, &ParserOptions::default());
    let mut v: Vec<(String, String)> = doc
        .top_level()
        .filter(|e| e.name != "template" && e.name != "script")
        .map(|e| {
            let s = e.start_tag.span.end as usize;
            let end = e.end_tag.map_or(e.span.end, |t| t.start) as usize;
            (e.name.clone(), mask_v_bind(code[s..end].trim()))
        })
        .collect();
    v.sort();
    v
}

/// `<style>` text with the expressions of `v-bind(…)` dropped: template rules format those.
fn mask_v_bind(css: &str) -> String {
    let mut out = String::new();
    let mut rest = css;
    while let Some(i) = rest.find("v-bind(") {
        out.push_str(&rest[..i + 7]);
        let mut depth = 1;
        let mut end = rest.len();
        for (j, c) in rest[i + 7..].char_indices() {
            depth += i32::from(c == '(') - i32::from(c == ')');
            if depth == 0 {
                end = i + 7 + j;
                break;
            }
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

fn html_comments(code: &str, sfc: bool) -> usize {
    if !sfc {
        return 0;
    }
    let alloc = Allocator::default();
    let doc = parse_document(&alloc, code, &ParserOptions::default());
    doc.comments.len()
}

fn non_ascii(s: &str) -> String {
    let mut v: Vec<char> = s
        .chars()
        .filter(|c| !c.is_ascii() && !c.is_whitespace() && *c != '\u{feff}')
        .collect();
    v.sort_unstable();
    v.into_iter().collect()
}

fn count(s: &str, pat: &str) -> usize {
    s.matches(pat).count()
}

/// Line-ending style: (CRLF count, lone LF count, lone CR count).
fn eols(s: &str) -> (usize, usize, usize) {
    let crlf = count(s, "\r\n");
    (crlf, count(s, "\n") - crlf, count(s, "\r") - crlf)
}

/// Multiset difference `a - b` of strings.
fn minus(a: &[String], b: &[String]) -> Vec<String> {
    let mut m: BTreeMap<&str, i64> = BTreeMap::new();
    for x in b {
        *m.entry(x).or_default() += 1;
    }
    let mut out = Vec::new();
    for x in a {
        let e = m.entry(x).or_default();
        if *e > 0 {
            *e -= 1;
        } else {
            out.push(x.clone());
        }
    }
    out
}

/// Compare `before` and `after` (the loop's result). `lossy` relaxes the checks of rules whose fix
/// removes text by design (comments, blocks, non-ASCII content).
pub fn compare(before: &str, after: &str, path: &Path, opts: &FileOptions) -> Outcome {
    let mut o = Outcome::default();
    let sfc = opts
        .sfc
        .unwrap_or_else(|| path.extension().is_some_and(|e| e == "vue"));
    let new_errors = minus(
        &parse_errors(after, path, opts),
        &parse_errors(before, path, opts),
    );
    for e in new_errors {
        o.problems.push(format!("parse:{e}"));
    }
    let (sb, sa) = (scripts(before, path, opts), scripts(after, path, opts));
    if sb.len() != sa.len() {
        o.problems.push("blocks:script-count".into());
    } else {
        let mut comments = (0, 0);
        for (i, (b, a)) in sb.iter().zip(&sa).enumerate() {
            if a.1 > b.1 {
                o.problems.push(format!("script-parse:block{i}"));
            }
            if a.0 != b.0 {
                o.script_changed.push(i);
            }
            comments.0 += b.2;
            comments.1 += a.2;
        }
        if comments.1 < comments.0 {
            o.problems
                .push(format!("comments:script {}->{}", comments.0, comments.1));
        }
    }
    if sfc {
        let (hb, ha) = (html_comments(before, true), html_comments(after, true));
        if ha < hb {
            o.problems.push(format!("comments:html {hb}->{ha}"));
        }
        if other_blocks(before) != other_blocks(after) {
            o.problems.push("blocks".into());
        }
    }
    for pat in ["eslint-disable", "eslint-enable", "oxlint-disable"] {
        if count(after, pat) < count(before, pat) {
            o.problems.push("directives".into());
            break;
        }
    }
    if non_ascii(before) != non_ascii(after) {
        o.problems.push("non-ascii".into());
    }
    let (b, a) = (eols(before), eols(after));
    let mixed_before = [b.0, b.1, b.2].iter().filter(|&&n| n > 0).count() > 1;
    let mixed_after = [a.0, a.1, a.2].iter().filter(|&&n| n > 0).count() > 1;
    if mixed_after && !mixed_before {
        o.problems.push(format!("eol:{b:?}->{a:?}"));
    }
    if before.ends_with('\n') && !after.ends_with('\n') {
        o.problems.push("trailing-newline".into());
    }
    o
}

/// Fix loop + comparison. `None` when the configuration fixes nothing in `code`.
pub fn check(
    code: &str,
    path: &Path,
    opts: &FileOptions,
    rules: &[ConfiguredRule],
    suggest: bool,
) -> Option<Outcome> {
    let (first, after, passes, loop_problems) = fix_loop(code, path, opts, rules, suggest)?;
    let mut o = compare(code, &after, path, opts);
    o.problems.splice(0..0, loop_problems);
    o.first = first;
    o.after = after;
    o.passes = passes;
    Some(o)
}

/// Every suggestion of every diagnostic applied alone (as an editor does): (desc, output, outcome).
pub fn check_suggestions(
    code: &str,
    path: &Path,
    opts: &FileOptions,
    rules: &[ConfiguredRule],
) -> Vec<(String, Outcome)> {
    let mut out = Vec::new();
    for d in lint_source(code, path, opts, rules) {
        for s in &d.suggestions {
            let mut after = code.to_string();
            after.replace_range(
                s.fix.span.start as usize..s.fix.span.end as usize,
                &s.fix.text,
            );
            let mut o = compare(code, &after, path, opts);
            o.after = after;
            o.passes = 1;
            out.push((s.desc.clone(), o));
        }
    }
    out
}
