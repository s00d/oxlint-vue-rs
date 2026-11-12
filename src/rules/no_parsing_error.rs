//! vue/no-parsing-error
//!
//! Reports `templateBody.errors` (HTML parse errors by code + expression syntax errors) as a point
//! location (ESLint gives no end position for these).

use std::collections::HashMap;

use oxc::span::Span;

use crate::rule::{Ctx, Rule, RuleOptions};

/// Codes reported by default; `non-void-html-element-start-tag-with-trailing-solidus` is off.
const DEFAULT_OFF: &[&str] = &["non-void-html-element-start-tag-with-trailing-solidus"];

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
                let on = self
                    .options
                    .get(code)
                    .copied()
                    .unwrap_or(!DEFAULT_OFF.contains(&code.as_str()));
                if !on {
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
