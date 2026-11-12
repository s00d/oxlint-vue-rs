//! vue/no-deprecated-v-is

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::tpl_valid::body_directives;

struct R;

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (_, _, _, k) in body_directives(doc, "is") {
            ctx.report(k.span, "`v-is` directive is deprecated.");
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(R))
}
