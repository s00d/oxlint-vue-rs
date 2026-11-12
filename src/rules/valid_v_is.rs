//! vue/valid-v-is

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::template::{is_html_element, is_html_well_known_element_name};
use crate::utils::tpl_valid::{ValueRule, body_directives, check_simple};

struct ValidVIs;

impl Rule for ValidVIs {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (e, _, a, k) in body_directives(doc, "is") {
            check_simple(ctx, doc, a, k, ValueRule::Required);
            if is_html_element(e) && !is_html_well_known_element_name(&e.raw_name) {
                ctx.report(
                    a.span,
                    format!(
                        "'v-is' directive must be owned by a native HTML element, but '{}' is not.",
                        e.name
                    ),
                );
            }
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(ValidVIs))
}
