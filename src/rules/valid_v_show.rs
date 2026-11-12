//! vue/valid-v-show

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::tpl_valid::{ValueRule, body_directives, check_simple};

struct ValidVShow;

impl Rule for ValidVShow {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (e, _, a, k) in body_directives(doc, "show") {
            check_simple(ctx, doc, a, k, ValueRule::Required);
            if e.name == "template" {
                ctx.report(
                    a.span,
                    "'v-show' directives cannot be put on <template> tags.",
                );
            }
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(ValidVShow))
}
