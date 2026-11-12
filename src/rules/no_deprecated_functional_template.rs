//! vue/no-deprecated-functional-template

use crate::rule::{Ctx, Rule, RuleOptions};

struct R;

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        let Some(root) = doc.template_body else { return };
        if let Some(a) = doc.element(root).attr("functional") {
            ctx.report(a.span, "The `functional` template are deprecated.");
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(R))
}
