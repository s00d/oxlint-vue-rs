//! vue/no-spaces-around-equal-signs-in-attribute

use oxc::span::Span;

use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::template::value_span;
use crate::utils::tpl_style::key_span;
use crate::utils::tpl_valid::body_elements;

struct R;

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for e in body_elements(doc) {
            for a in e.attributes() {
                let Some(v) = value_span(doc, a) else {
                    continue;
                };
                // `:foo` same-name shorthand: the value lies inside the key (JS slice → "").
                if v.start < key_span(a).end {
                    continue;
                }
                let range = Span::new(key_span(a).end, v.start);
                let eq = doc.slice(range);
                let expect = eq.trim();
                if eq != expect {
                    ctx.report_fix(
                        range,
                        Some("unexpectedSpaces"),
                        "Unexpected spaces found around equal signs.",
                        Some(Fix::replace(range, expect)),
                    );
                }
            }
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(R))
}
