//! vue/valid-v-memo

use oxc::ast::ast::Expression;

use crate::ast::VExpression;
use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::js::skip_parens;
use crate::utils::template::directive_value;
use crate::utils::tpl_valid::{ValueRule, body_directives, check_simple, expr_span, missing_value};

struct ValidVMemo;

impl Rule for ValidVMemo {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (e, _, a, k) in body_directives(doc, "memo") {
            if doc.ancestors(e.id).any(|p| p.has_directive("for")) {
                ctx.report(k.span, "'v-memo' directive does not work inside 'v-for'.");
            }
            check_simple(ctx, doc, a, k, ValueRule::Required);
            if missing_value(doc, a) {
                continue;
            }
            let Some(VExpression::Expression(expr)) =
                directive_value(doc, a).and_then(|c| c.expression.as_ref())
            else {
                continue;
            };
            let mut stack: Vec<&Expression> = vec![expr];
            while let Some(x) = stack.pop() {
                let x = skip_parens(x);
                match x {
                    Expression::ObjectExpression(_)
                    | Expression::ClassExpression(_)
                    | Expression::ArrowFunctionExpression(_)
                    | Expression::FunctionExpression(_)
                    | Expression::TemplateLiteral(_)
                    | Expression::UnaryExpression(_)
                    | Expression::BinaryExpression(_)
                    | Expression::PrivateInExpression(_)
                    | Expression::UpdateExpression(_) => ctx.report(
                        expr_span(x),
                        "'v-memo' directives require the attribute value to be an array.",
                    ),
                    _ if x.is_literal() => ctx.report(
                        expr_span(x),
                        "'v-memo' directives require the attribute value to be an array.",
                    ),
                    Expression::AssignmentExpression(a) => stack.push(&a.right),
                    Expression::TSAsExpression(a) => stack.push(&a.expression),
                    Expression::SequenceExpression(s) => stack.extend(s.expressions.last()),
                    Expression::ConditionalExpression(c) => {
                        stack.extend([&c.consequent, &c.alternate])
                    }
                    _ => {}
                }
            }
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(ValidVMemo))
}
