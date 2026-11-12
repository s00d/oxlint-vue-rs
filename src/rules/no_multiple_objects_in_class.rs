//! vue/no-multiple-objects-in-class

use oxc::ast::ast::Expression;

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::js::skip_parens;
use crate::utils::template::directive_expression;
use crate::utils::tpl_misc::static_arg;
use crate::utils::tpl_valid::body_directives;

struct R;

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (_, _, a, k) in body_directives(doc, "bind") {
            if !static_arg(k).is_some_and(|i| i.name == "class") {
                continue;
            }
            let Some(Expression::ArrayExpression(arr)) = directive_expression(doc, a).map(skip_parens) else {
                continue;
            };
            let n = arr
                .elements
                .iter()
                .filter(|el| el.as_expression().is_some_and(|e| matches!(skip_parens(e), Expression::ObjectExpression(_))))
                .count();
            if n > 1 {
                ctx.report(a.span, "Unexpected multiple objects. Merge objects.");
            }
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(R))
}
