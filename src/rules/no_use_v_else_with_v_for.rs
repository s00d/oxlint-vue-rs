//! vue/no-use-v-else-with-v-for

use crate::rule::{Ctx, Rule, RuleOptions, fmt_msg};
use crate::utils::tpl_valid::body_directives;

struct R;

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (e, _, _, _) in body_directives(doc, "for") {
            let d = if e.has_directive("else-if") {
                "v-else-if"
            } else if e.has_directive("else") {
                "v-else"
            } else {
                continue;
            };
            ctx.report(
                e.span,
                fmt_msg(
                    "Unexpected `{{ directiveName }}` and `v-for` on the same element. Move `{{ directiveName }}` to a wrapper element instead.",
                    &[("directiveName", d)],
                ),
            );
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(R))
}
