//! vue/valid-v-else

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::template::prev_element_has_if;
use crate::utils::tpl_valid::{ValueRule, body_directives, check_simple};

struct ValidVElse;

impl Rule for ValidVElse {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (e, _, a, k) in body_directives(doc, "else") {
            if !prev_element_has_if(doc, e) {
                ctx.report(a.span, "'v-else' directives require being preceded by the element which has a 'v-if' or 'v-else-if' directive.");
            }
            if e.has_directive("if") {
                ctx.report(a.span, "'v-else' and 'v-if' directives can't exist on the same element. You may want 'v-else-if' directives.");
            }
            if e.has_directive("else-if") {
                ctx.report(
                    a.span,
                    "'v-else' and 'v-else-if' directives can't exist on the same element.",
                );
            }
            check_simple(ctx, doc, a, k, ValueRule::Forbidden);
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(ValidVElse))
}
