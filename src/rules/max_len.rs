//! vue/max-len

use std::sync::LazyLock;

use oxc::parser::Kind;
use oxc::span::Span;
use crate::utils::regexp::JsRegex as Regex;

use crate::ast::{AttributeKey, AttributeValue, TokenKind};
use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::tpl_style::is_js_ws;
use crate::utils::tpl_valid::body_elements;

static URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^:/?#]://[^?#]").unwrap());

struct R {
    code: u64,
    template: u64,
    comments: Option<u64>,
    tab_width: u64,
    ignore_pattern: Option<Regex>,
    ignore_comments: bool,
    ignore_trailing: bool,
    ignore_urls: bool,
    ignore_strings: bool,
    ignore_template_literals: bool,
    ignore_regexp: bool,
    ignore_attr_values: bool,
    ignore_text: bool,
}

fn line_length(line: &str, tab: u64) -> u64 {
    let mut extra = 0u64;
    for (i, u) in line.encode_utf16().enumerate() {
        if u == u16::from(b'\t') {
            let total = i as u64 + extra;
            let prev = if tab > 0 { total % tab } else { 0 };
            extra = (extra + tab - prev).wrapping_sub(1);
        }
    }
    (line.chars().count() as u64).wrapping_add(extra)
}

/// Prefix of `s` up to UTF-16 column `col`.
fn utf16_prefix(s: &str, col: u32) -> &str {
    let mut n = 0u32;
    for (i, c) in s.char_indices() {
        if n >= col {
            return &s[..i];
        }
        n += c.len_utf16() as u32;
    }
    s
}

fn utf16_len(s: &str) -> u32 {
    s.encode_utf16().count() as u32
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let file = ctx.file;
        let src = ctx.source();
        // Upstream iterates ESLint `sourceCode.lines` (all line terminators) but takes the lines of
        // tokens, comments and blocks from parser locations (`\n` only); both index the same arrays.
        let lines = file.lines;
        let text_lines = file.eslint_lines;
        let n = lines.line_count().max(text_lines.line_count());
        let pos = |o: u32| -> (usize, u32) {
            let (l, c) = lines.line_col(src, o);
            (l as usize - 1, c - 1)
        };
        let mark = |v: &mut Vec<bool>, s: Span| {
            let (a, b) = (lines.line_of(s.start), lines.line_of(s.end));
            for x in v.iter_mut().take(b + 1).skip(a) {
                *x = true;
            }
        };
        let mut strings = vec![false; n];
        let mut templates = vec![false; n];
        let mut regexps = vec![false; n];
        let mut attr_values = vec![false; n];
        let mut texts = vec![false; n];
        let mut comment_spans: Vec<Span> = Vec::new();

        // script
        let toks: Vec<_> = file
            .script_tokens
            .iter()
            .filter(|t| t.start() < t.end())
            .collect();
        let script_comments: Vec<Span> = file
            .program
            .map(|p| p.comments.iter().map(|c| c.span).collect())
            .unwrap_or_default();
        for t in &toks {
            let s = Span::new(t.start(), t.end());
            match t.kind() {
                Kind::Str => mark(&mut strings, s),
                Kind::NoSubstitutionTemplate
                | Kind::TemplateHead
                | Kind::TemplateMiddle
                | Kind::TemplateTail => mark(&mut templates, s),
                Kind::RegExp => mark(&mut regexps, s),
                _ => {}
            }
        }
        // vue-eslint-parser adds the `<script>` / `</script>` tags of the parsed block(s) to the
        // script tokens, so the script line range spans the whole element(s).
        let blocks: Vec<Span> = match file.scripts.as_slice() {
            [a, b] if a.setup != b.setup => vec![a.span, b.span],
            [a, ..] => vec![a.span],
            [] => Vec::new(),
        };
        let firsts = [
            toks.first().map(|t| t.start()),
            script_comments.first().map(|c| c.start),
            blocks.iter().map(|b| b.start).min(),
        ];
        let lasts = [
            toks.last().map(|t| t.end()),
            script_comments.last().map(|c| c.end),
            blocks.iter().map(|b| b.end).max(),
        ];
        let script_range = firsts
            .iter()
            .flatten()
            .map(|o| lines.line_of(*o))
            .min()
            .zip(lasts.iter().flatten().map(|o| lines.line_of(*o)).max());
        comment_spans.extend(&script_comments);

        // template
        let mut template_range = None;
        if let Some(doc) = ctx.doc()
            && let Some(root) = doc.template_body
        {
            let rs = doc.element(root).span;
            template_range = Some((lines.line_of(rs.start), lines.line_of(rs.end)));
            for t in doc.tokens_in(rs) {
                match t.kind {
                    TokenKind::String => mark(&mut strings, t.span),
                    TokenKind::Template => mark(&mut templates, t.span),
                    TokenKind::RegularExpression => mark(&mut regexps, t.span),
                    TokenKind::HTMLText => mark(&mut texts, t.span),
                    _ => {}
                }
            }
            // `templateBody.comments` is every document comment (vue-eslint-parser).
            comment_spans.extend(doc.comments.iter().map(|c| c.span));
            for e in body_elements(doc) {
                for a in e.attributes() {
                    if let (AttributeKey::Plain(_), Some(AttributeValue::Literal(l))) =
                        (&a.key, &a.value)
                    {
                        mark(&mut attr_values, l.span);
                    }
                }
            }
        }
        let use_comments = self.ignore_comments || self.comments.is_some() || self.ignore_trailing;
        comment_spans.sort_by_key(|s| s.start);
        let mut by_line: Vec<Vec<Span>> = vec![Vec::new(); n];
        if use_comments {
            for c in &comment_spans {
                for l in by_line
                    .iter_mut()
                    .take(lines.line_of(c.end) + 1)
                    .skip(lines.line_of(c.start))
                {
                    l.push(*c);
                }
            }
        }

        for i in 0..text_lines.line_count() {
            let in_script = script_range.is_some_and(|(a, b)| a <= i && i <= b);
            let in_template = template_range.is_some_and(|(a, b)| a <= i && i <= b);
            if (!in_script && !in_template)
                || (self.ignore_strings && strings[i])
                || (self.ignore_template_literals && templates[i])
                || (self.ignore_regexp && regexps[i])
                || (self.ignore_attr_values && attr_values[i])
                || (self.ignore_text && texts[i])
            {
                continue;
            }
            let start = text_lines.line_start(i).unwrap_or(0);
            let end = text_lines.line_start(i + 1).unwrap_or(src.len() as u32);
            let line = src[start as usize..end as usize]
                .trim_end_matches(['\n', '\r', '\u{2028}', '\u{2029}']);
            let line_len16 = utf16_len(line);
            let mut comment_line = false;
            let mut text: String = line.to_string();
            let mut list = by_line[i].clone();
            if let Some(mut c) = list.pop() {
                let (sl, sc) = pos(c.start);
                let (el, ec) = pos(c.end);
                let first_on_line = utf16_prefix(line, sc).trim_matches(is_js_ws).is_empty();
                let full = (sl < i || (sl == i && first_on_line))
                    && (el > i || (el == i && ec == line_len16));
                let trailing = |t: &str, c: Span| {
                    let (sl, _) = pos(c.start);
                    let (el, ec) = pos(c.end);
                    sl == i && i <= el && (el > i || ec == utf16_len(t))
                };
                let strip = |t: &str, c: Span| {
                    utf16_prefix(t, pos(c.start).1)
                        .trim_end_matches(is_js_ws)
                        .to_string()
                };
                if full {
                    comment_line = true;
                } else if self.ignore_trailing && trailing(line, c) {
                    text = strip(line, c);
                    if let Some(next) = list.pop() {
                        c = next;
                        while trailing(&text, c) {
                            let t = strip(&text, c);
                            if t == text {
                                break;
                            }
                            text = t;
                        }
                    }
                }
            }
            if self
                .ignore_pattern
                .as_ref()
                .is_some_and(|r| r.is_match(&text))
                || (self.ignore_urls && URL.is_match(&text))
                || (comment_line && self.ignore_comments)
            {
                continue;
            }
            let max = (if in_script { self.code } else { 0 }).max(if in_template {
                self.template
            } else {
                0
            });
            let len = line_length(&text, self.tab_width);
            let ln = i as u32 + 1;
            let span = Span::new(start, start + line.len() as u32);
            match self.comments.filter(|_| comment_line) {
                Some(mc) => {
                    if len > mc {
                        ctx.report_at(
                            span,
                            "maxComment",
                            format!(
                                "This line has a comment length of {len}. Maximum allowed is {mc}."
                            ),
                            [ln, mc as u32 + 1, ln, len as u32 + 1],
                        );
                    }
                }
                None => {
                    if len > max {
                        ctx.report_at(
                            span,
                            "max",
                            format!("This line has a length of {len}. Maximum allowed is {max}."),
                            [ln, max as u32 + 1, ln, len as u32 + 1],
                        );
                    }
                }
            }
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let last = opts
        .options
        .last()
        .filter(|v| v.is_object())
        .cloned()
        .unwrap_or_default();
    let num = |v: &serde_json::Value| v.as_f64().filter(|n| *n >= 0.0).map(|n| n as u64);
    let code = num(opts.get(0))
        .or_else(|| num(&last["code"]))
        .unwrap_or(80);
    let tab_width = num(opts.get(1))
        .or_else(|| num(&last["tabWidth"]))
        .unwrap_or(2);
    let b = |k: &str| last[k].as_bool() == Some(true);
    Some(Box::new(R {
        code,
        template: num(&last["template"]).unwrap_or(code),
        comments: num(&last["comments"]).filter(|c| *c > 0),
        tab_width,
        ignore_pattern: last["ignorePattern"]
            .as_str()
            .and_then(|p| Regex::with_flags(p, "u").ok()),
        ignore_comments: b("ignoreComments"),
        ignore_trailing: b("ignoreTrailingComments") || b("ignoreComments"),
        ignore_urls: b("ignoreUrls"),
        ignore_strings: b("ignoreStrings"),
        ignore_template_literals: b("ignoreTemplateLiterals"),
        ignore_regexp: b("ignoreRegExpLiterals"),
        ignore_attr_values: b("ignoreHTMLAttributeValues"),
        ignore_text: b("ignoreHTMLTextContents"),
    }))
}
