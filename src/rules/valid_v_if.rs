//! vue/valid-v-if

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::tpl_valid::{ValueRule, body_directives, check_simple};

struct ValidVIf;

impl Rule for ValidVIf {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (e, _, a, k) in body_directives(doc, "if") {
            if e.has_directive("else") {
                ctx.report(a.span, "'v-if' and 'v-else' directives can't exist on the same element. You may want 'v-else-if' directives.");
            }
            if e.has_directive("else-if") {
                ctx.report(
                    a.span,
                    "'v-if' and 'v-else-if' directives can't exist on the same element.",
                );
            }
            check_simple(ctx, doc, a, k, ValueRule::Required);
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(ValidVIf))
}
