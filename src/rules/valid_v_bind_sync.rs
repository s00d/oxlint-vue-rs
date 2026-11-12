//! vue/valid-v-bind-sync

use oxc::ast::ast::Expression;

use crate::ast::VExpression;
use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::js::skip_parens;
use crate::utils::template::{directive_value, is_custom_component};
use crate::utils::tpl_valid::{
    body_directives, expr_span, is_maybe_null_object_member, is_optional_chaining_member,
    is_whole_identifier,
};

struct ValidVBindSync;

impl Rule for ValidVBindSync {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (e, _, a, k) in body_directives(doc, "bind") {
            if !k.has_modifier("sync") {
                continue;
            }
            if !is_custom_component(e, false) {
                ctx.report(
                    a.span,
                    format!(
                        "'.sync' modifiers aren't supported on <{}> non Vue-components.",
                        e.name
                    ),
                );
            }
            let Some(c) = directive_value(doc, a) else {
                continue;
            };
            let Some(VExpression::Expression(expr)) = &c.expression else {
                continue;
            };
            let x = skip_parens(expr);
            let span = expr_span(expr);
            if is_optional_chaining_member(x) {
                ctx.report(
                    span,
                    "Optional chaining cannot appear in 'v-bind' with '.sync' modifiers.",
                );
            } else if !(matches!(x, Expression::Identifier(_)) || x.is_member_expression()) {
                ctx.report(
                    span,
                    "'.sync' modifiers require the attribute value which is valid as LHS.",
                );
            } else if is_maybe_null_object_member(x) {
                ctx.report(
                    span,
                    "'.sync' modifier has potential null object property access.",
                );
            }
            for r in &c.references {
                if is_whole_identifier(expr, r.span) && r.variable.is_some() {
                    ctx.report(
                        span,
                        format!(
                            "'.sync' modifiers cannot update the iteration variable '{}' itself.",
                            r.name
                        ),
                    );
                }
            }
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(ValidVBindSync))
}
