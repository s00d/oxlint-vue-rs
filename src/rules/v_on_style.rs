//! vue/v-on-style

use oxc::span::Span;

use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::tpl_valid::body_directives;

struct R {
    shorthand: bool,
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (_, _, a, k) in body_directives(doc, "on") {
            if k.argument.is_none() || (k.name.raw_name == "@") == self.shorthand {
                continue;
            }
            let pos = a.span.start;
            let (id, msg, fix) = if self.shorthand {
                (
                    "expectedShorthand",
                    "Expected '@' instead of 'v-on:'.",
                    Fix::replace(Span::new(pos, pos + 5), "@"),
                )
            } else {
                (
                    "expectedLonghand",
                    "Expected 'v-on:' instead of '@'.",
                    Fix::replace(Span::new(pos, pos + 1), "v-on:"),
                )
            };
            ctx.report_fix(a.span, Some(id), msg, Some(fix));
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(R {
        shorthand: opts.get(0).as_str() != Some("longform"),
    }))
}
