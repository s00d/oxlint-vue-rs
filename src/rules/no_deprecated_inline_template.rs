//! vue/no-deprecated-inline-template

use crate::ast::AttributeKey;
use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::tpl_misc::body_attributes;

struct R;

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (_, a) in body_attributes(doc) {
            if let AttributeKey::Plain(k) = &a.key
                && k.raw_name == "inline-template"
            {
                ctx.report(k.span, "`inline-template` are deprecated.");
            }
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(R))
}
