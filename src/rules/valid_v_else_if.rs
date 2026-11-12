//! vue/valid-v-else-if

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::template::prev_element_has_if;
use crate::utils::tpl_valid::{ValueRule, body_directives, check_simple};

struct ValidVElseIf;

impl Rule for ValidVElseIf {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (e, _, a, k) in body_directives(doc, "else-if") {
            if !prev_element_has_if(doc, e) {
                ctx.report(a.span, "'v-else-if' directives require being preceded by the element which has a 'v-if' or 'v-else-if' directive.");
            }
            if e.has_directive("if") {
                ctx.report(
                    a.span,
                    "'v-else-if' and 'v-if' directives can't exist on the same element.",
                );
            }
            if e.has_directive("else") {
                ctx.report(
                    a.span,
                    "'v-else-if' and 'v-else' directives can't exist on the same element.",
                );
            }
            check_simple(ctx, doc, a, k, ValueRule::Required);
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(ValidVElseIf))
}
