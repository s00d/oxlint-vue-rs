//! vue/require-component-is

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::tpl_valid::body_elements;

struct RequireComponentIs;

impl Rule for RequireComponentIs {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for e in body_elements(doc) {
            if e.name == "component" && e.directive("bind", Some("is")).is_none() {
                ctx.report(
                    e.span,
                    "Expected '<component>' elements to have 'v-bind:is' attribute.",
                );
            }
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(RequireComponentIs))
}
