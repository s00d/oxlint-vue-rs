//! vue/max-attributes-per-line

use oxc::span::Span;

use crate::ast::Attribute;
use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::tpl_misc::token_before;
use crate::utils::tpl_style::key_span;
use crate::utils::tpl_valid::body_elements;

struct R {
    singleline: usize,
    multiline: usize,
}

fn max(v: &serde_json::Value) -> Option<usize> {
    v.as_f64()
        .or_else(|| v["max"].as_f64())
        .map(|n| n.max(0.0) as usize)
}

impl R {
    fn show(ctx: &mut Ctx, attrs: &[Attribute]) {
        let Some(doc) = ctx.doc() else { return };
        for (i, a) in attrs.iter().enumerate() {
            let fix = if i == 0 {
                token_before(doc, a.span.start)
                    .map(|p| Fix::replace(Span::new(p.span.end, a.span.start), "\n"))
            } else {
                None
            };
            let name = doc.slice(key_span(a));
            ctx.report_fix(
                a.span,
                Some("shouldBeOnNewLine"),
                format!("'{name}' should be on a new line."),
                fix,
            );
        }
    }
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        let lines = ctx.file.lines;
        for e in body_elements(doc) {
            let attrs = e.attributes();
            if attrs.is_empty() {
                continue;
            }
            let st = e.start_tag.span;
            if lines.line_of(st.start) == lines.line_of(st.end) {
                if attrs.len() > self.singleline {
                    Self::show(ctx, &attrs[self.singleline..]);
                }
                continue;
            }
            let mut start = 0;
            for i in 1..=attrs.len() {
                if i == attrs.len()
                    || lines.line_of(attrs[i - 1].span.end) != lines.line_of(attrs[i].span.start)
                {
                    let group = &attrs[start..i];
                    if group.len() > self.multiline {
                        Self::show(ctx, &group[self.multiline..]);
                    }
                    start = i;
                }
            }
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let o = opts.get(0);
    Some(Box::new(R {
        singleline: max(&o["singleline"]).unwrap_or(1),
        multiline: max(&o["multiline"]).unwrap_or(1),
    }))
}
