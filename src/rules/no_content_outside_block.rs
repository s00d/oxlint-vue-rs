//! vue/no-content-outside-block

use oxc::span::Span;

use crate::ast::Child;
use crate::rule::{Ctx, Fix, Rule, RuleOptions, Suggestion};
use crate::utils::tpl_style::has_invalid_eof;

struct R;

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        if has_invalid_eof(doc) {
            return;
        }
        for c in &doc.children {
            let Child::Text(t) = c else { continue };
            let span = doc.text(*t).span;
            let raw = doc.slice(span);
            let lead = raw.len() - raw.trim_start().len();
            if lead == raw.len() {
                continue;
            }
            let trail = raw.len() - raw.trim_end().len();
            let range = Span::new(span.start + lead as u32, span.end - trail as u32);
            ctx.push(
                range,
                Some("unexpectedContent"),
                "Unexpected content outside of the top-level blocks. It is dropped by the compiler."
                    .into(),
                None,
                vec![Suggestion {
                    message_id: Some("removeContent"),
                    desc: "Remove the content outside of the top-level blocks.".into(),
                    fix: Fix::remove(range),
                }],
            );
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(R))
}
