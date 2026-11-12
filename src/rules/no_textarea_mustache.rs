//! vue/no-textarea-mustache

use crate::ast::ExprParent;
use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::template::template_expr_containers;

struct NoTextareaMustache;

impl Rule for NoTextareaMustache {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for c in template_expr_containers(doc) {
            let ExprParent::Element(Some(p)) = c.parent else {
                continue;
            };
            let el = doc.element(p);
            if el.raw_name == "textarea" || doc.ancestors(p).any(|a| a.raw_name == "textarea") {
                ctx.report(c.span, "Unexpected mustache. Use 'v-model' instead.");
            }
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(NoTextareaMustache))
}
