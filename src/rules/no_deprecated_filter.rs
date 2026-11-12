//! vue/no-deprecated-filter

use crate::ast::VExpression;
use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::tpl_misc::body_exprs;

struct R;

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for c in body_exprs(doc) {
            if let Some(VExpression::FilterSequence { span, .. }) = &c.expression {
                ctx.report(*span, "Filters are deprecated.");
            }
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(R))
}
