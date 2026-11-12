//! vue/multiline-html-element-content-newline

use oxc::span::Span;

use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::rules::singleline_html_element_content_newline::{default_ignores, is_ignored, strs};
use crate::utils::tpl_style::{js_trim, non_ws_after, non_ws_before};
use crate::utils::tpl_valid::body_elements;

struct R {
    ignores: Vec<String>,
    ignore_when_empty: bool,
    allow_empty_lines: bool,
}

fn phrase(n: usize) -> String {
    if n == 0 { "no".into() } else { n.to_string() }
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        let lines = ctx.file.lines;
        let line = |o: u32| lines.line_of(o);
        let src = ctx.source();
        let invalid = |n: usize| {
            if self.allow_empty_lines {
                n == 0
            } else {
                n != 1
            }
        };
        let mut ignored: Option<Span> = None;
        for e in body_elements(doc) {
            if let Some(s) = ignored {
                if e.span.start >= s.start && e.span.end <= s.end {
                    continue;
                }
                ignored = None;
            }
            if is_ignored(&self.ignores, &e.name, &e.raw_name) {
                ignored = Some(e.span);
                continue;
            }
            let Some(end_tag) = e.end_tag else { continue };
            if e.start_tag.self_closing || line(e.span.start) >= line(end_tag.start) {
                continue;
            }
            let st = e.start_tag.span;
            let first = non_ws_after(doc, st.end);
            if self.ignore_when_empty
                && e.children.is_empty()
                && first.is_none_or(|t| t.span.start >= end_tag.start)
            {
                continue;
            }
            let (Some(first), Some(last)) = (first, non_ws_before(doc, end_tag.start)) else {
                continue;
            };
            let before = line(first.span.start).saturating_sub(line(st.end));
            let after = line(end_tag.start).saturating_sub(line(last.span.end));
            if invalid(before) {
                let r = Span::new(st.end, first.span.start);
                ctx.report_fix(
                    r,
                    Some("unexpectedAfterClosingBracket"),
                    format!(
                        "Expected 1 line break after opening tag (`<{}>`), but {} line breaks found.",
                        e.raw_name,
                        phrase(before)
                    ),
                    Some(Fix::replace(r, "\n")),
                );
            }
            if js_trim(&src[st.end as usize..end_tag.start as usize]).is_empty() {
                continue;
            }
            if invalid(after) {
                let r = Span::new(last.span.end, end_tag.start);
                ctx.report_fix(
                    r,
                    Some("unexpectedBeforeOpeningBracket"),
                    format!(
                        "Expected 1 line break before closing tag (`</{}>`), but {} line breaks found.",
                        e.name,
                        phrase(after)
                    ),
                    Some(Fix::replace(r, "\n")),
                );
            }
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let o = opts.get(0);
    Some(Box::new(R {
        ignores: strs(&o["ignores"]).unwrap_or_else(default_ignores),
        ignore_when_empty: o["ignoreWhenEmpty"].as_bool() != Some(false),
        allow_empty_lines: o["allowEmptyLines"].as_bool() == Some(true),
    }))
}
