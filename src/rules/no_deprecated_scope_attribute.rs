//! vue/no-deprecated-scope-attribute

use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::tpl_valid::body_directives;

struct R;

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (_, _, _, k) in body_directives(doc, "scope") {
            ctx.report_fix(
                k.span,
                None,
                "`scope` attributes are deprecated.",
                Some(Fix::replace(k.span, "slot-scope")),
            );
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(R))
}
