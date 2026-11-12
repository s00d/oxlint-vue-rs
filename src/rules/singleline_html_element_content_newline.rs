//! vue/singleline-html-element-content-newline

use oxc::span::Span;

use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::casing::{kebab_case, pascal_case};
use crate::utils::template::INLINE_NON_VOID_ELEMENT_NAMES;
use crate::utils::tpl_style::{js_trim, non_ws_after, non_ws_before};
use crate::utils::tpl_valid::body_elements;

pub struct R {
    pub ignores: Vec<String>,
    ignore_when_no_attributes: bool,
    ignore_when_empty: bool,
}

/// Default `ignores` of both content-newline rules.
pub fn default_ignores() -> Vec<String> {
    let mut v: Vec<String> = ["pre", "textarea", "router-link", "nuxt-link"]
        .map(String::from)
        .to_vec();
    v.extend(INLINE_NON_VOID_ELEMENT_NAMES.iter().cloned());
    v
}

pub fn strs(v: &serde_json::Value) -> Option<Vec<String>> {
    v.as_array().map(|a| {
        a.iter()
            .filter_map(|v| v.as_str())
            .map(String::from)
            .collect()
    })
}

pub fn is_ignored(ignores: &[String], name: &str, raw: &str) -> bool {
    let (p, k) = (pascal_case(raw), kebab_case(raw));
    ignores.iter().any(|i| i == name || *i == p || *i == k)
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        let lines = ctx.file.lines;
        let line = |o: u32| lines.line_of(o);
        let src = ctx.source();
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
            if e.start_tag.self_closing {
                continue;
            }
            if line(e.span.start) != line(end_tag.start)
                || (self.ignore_when_no_attributes && e.attributes().is_empty())
            {
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
            let r = Span::new(st.end, first.span.start);
            ctx.report_fix(
                r,
                Some("unexpectedAfterClosingBracket"),
                format!(
                    "Expected 1 line break after opening tag (`<{}>`), but no line breaks found.",
                    e.raw_name
                ),
                Some(Fix::replace(r, "\n")),
            );
            if js_trim(&src[st.end as usize..end_tag.start as usize]).is_empty() {
                continue;
            }
            let r = Span::new(last.span.end, end_tag.start);
            ctx.report_fix(
                r,
                Some("unexpectedBeforeOpeningBracket"),
                format!(
                    "Expected 1 line break before closing tag (`</{}>`), but no line breaks found.",
                    e.raw_name
                ),
                Some(Fix::replace(r, "\n")),
            );
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let o = opts.get(0);
    let mut ignores = strs(&o["ignores"]).unwrap_or_else(default_ignores);
    ignores.extend(strs(&o["externalIgnores"]).unwrap_or_default());
    Some(Box::new(R {
        ignores,
        ignore_when_no_attributes: o["ignoreWhenNoAttributes"].as_bool() != Some(false),
        ignore_when_empty: o["ignoreWhenEmpty"].as_bool() != Some(false),
    }))
}
