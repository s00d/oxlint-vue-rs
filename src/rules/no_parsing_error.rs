//! vue/no-parsing-error
//!
//! Reports `templateBody.errors` (HTML parse errors by code + expression syntax errors) as a point
//! location (ESLint gives no end position for these).

use std::collections::HashMap;

use oxc::span::Span;

use crate::rule::{Ctx, Rule, RuleOptions};

/// Upstream `DEFAULT_OPTIONS`: the only codes the rule can report (any other code, e.g.
/// `x-missing-interpolation-end`, is skipped; the schema forbids enabling it).
const CODES: &[(&str, bool)] = &[
    ("abrupt-closing-of-empty-comment", true),
    ("absence-of-digits-in-numeric-character-reference", true),
    ("cdata-in-html-content", true),
    ("character-reference-outside-unicode-range", true),
    ("control-character-in-input-stream", true),
    ("control-character-reference", true),
    ("eof-before-tag-name", true),
    ("eof-in-cdata", true),
    ("eof-in-comment", true),
    ("eof-in-tag", true),
    ("incorrectly-closed-comment", true),
    ("incorrectly-opened-comment", true),
    ("invalid-first-character-of-tag-name", true),
    ("missing-attribute-value", true),
    ("missing-end-tag-name", true),
    ("missing-semicolon-after-character-reference", true),
    ("missing-whitespace-between-attributes", true),
    ("nested-comment", true),
    ("noncharacter-character-reference", true),
    ("noncharacter-in-input-stream", true),
    ("null-character-reference", true),
    ("surrogate-character-reference", true),
    ("surrogate-in-input-stream", true),
    ("unexpected-character-in-attribute-name", true),
    ("unexpected-character-in-unquoted-attribute-value", true),
    ("unexpected-equals-sign-before-attribute-name", true),
    ("unexpected-null-character", true),
    ("unexpected-question-mark-instead-of-tag-name", true),
    ("unexpected-solidus-in-tag", true),
    ("unknown-named-character-reference", true),
    ("end-tag-with-attributes", true),
    ("duplicate-attribute", true),
    ("end-tag-with-trailing-solidus", true),
    ("non-void-html-element-start-tag-with-trailing-solidus", false),
    ("x-invalid-end-tag", true),
    ("x-invalid-namespace", true),
];

struct NoParsingError {
    options: HashMap<String, bool>,
}

impl Rule for NoParsingError {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        if doc.template_body.is_none() {
            return;
        }
        for e in &doc.errors {
            if let Some(code) = &e.code {
                let Some(&(_, default)) = CODES.iter().find(|(c, _)| c == code) else {
                    continue;
                };
                if !self.options.get(code).copied().unwrap_or(default) {
                    continue;
                }
            }
            let msg = e.message.strip_suffix('.').unwrap_or(&e.message);
            let mut at = e.offset;
            // vue-eslint-parser raises input-stream errors before advancing its line/column, so the
            // reported location is the previous code point.
            if matches!(
                e.code.as_deref(),
                Some(
                    "control-character-in-input-stream"
                        | "noncharacter-in-input-stream"
                        | "surrogate-in-input-stream"
                )
            ) {
                let before = &doc.source[..(at as usize).min(doc.source.len())];
                at -= before
                    .chars()
                    .next_back()
                    .map_or(0, |c| c.len_utf8() as u32);
            }
            ctx.report(Span::empty(at), format!("Parsing error: {msg}."));
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let options = opts
        .get(0)
        .as_object()
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| Some((k.clone(), v.as_bool()?)))
                .collect()
        })
        .unwrap_or_default();
    Some(Box::new(NoParsingError { options }))
}
