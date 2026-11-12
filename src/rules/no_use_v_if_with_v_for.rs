//! vue/no-use-v-if-with-v-for

use oxc::ast::ast::Expression;

use crate::ast::{VExpression, VariableKind};
use crate::rule::{Ctx, Rule, RuleOptions, fmt_msg};
use crate::utils::js::skip_parens;
use crate::utils::template::{directive_value, value_span};
use crate::utils::tpl_valid::{body_directives, expr_span};

struct R {
    allow: bool,
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (e, _, a, _) in body_directives(doc, "if") {
            let Some(vfor) = e.directive("for", None) else { continue };
            let uses = directive_value(doc, a).is_some_and(|c| {
                c.references.iter().any(|r| {
                    e.variables.iter().any(|v| v.name == r.name && v.kind == VariableKind::VFor)
                })
            });
            if !uses {
                ctx.report(a.span, "This 'v-if' should be moved to the wrapper element.");
                continue;
            }
            if self.allow {
                continue;
            }
            let Some(Some(VExpression::For { right, .. })) = directive_value(doc, vfor).map(|c| &c.expression)
            else {
                continue;
            };
            let _ = value_span;
            let (name, kind) = match skip_parens(right) {
                Expression::Identifier(id) => (id.name.to_string(), "variable"),
                _ => (doc.slice(expr_span(right)).to_string(), "expression"),
            };
            ctx.report(
                a.span,
                fmt_msg(
                    "The '{{iteratorName}}' {{kind}} inside 'v-for' directive should be replaced with a computed property that returns filtered array instead. You should not mix 'v-for' with 'v-if'.",
                    &[("iteratorName", &name), ("kind", kind)],
                ),
            );
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(R { allow: opts.get(0)["allowUsingIterationVar"].as_bool() == Some(true) }))
}
