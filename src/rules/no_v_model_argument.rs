//! vue/no-v-model-argument

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::template::is_custom_component;
use crate::utils::tpl_valid::body_directives;

struct NoVModelArgument;

impl Rule for NoVModelArgument {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (e, _, a, k) in body_directives(doc, "model") {
            if k.argument.is_some() && is_custom_component(e, false) {
                ctx.report(a.span, "'v-model' directives require no argument.");
            }
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(NoVModelArgument))
}
