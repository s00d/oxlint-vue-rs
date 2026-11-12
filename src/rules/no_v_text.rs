//! vue/no-v-text

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::tpl_valid::body_directives;

struct R;

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (_, _, a, _) in body_directives(doc, "text") {
            ctx.report(a.span, "Don't use 'v-text'.");
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(R))
}
