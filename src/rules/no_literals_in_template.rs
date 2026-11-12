//! vue/no-literals-in-template

use oxc::ast::ast::Expression;
use oxc::span::GetSpan;

use crate::ast::{AttributeKey, DirectiveArgument};
use crate::rule::{Ctx, Rule, RuleOptions, fmt_msg};
use crate::utils::js;
use crate::utils::regexp::GroupMatcher;
use crate::utils::template::directive_expression;
use crate::utils::tpl_misc::body_attributes;

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let ignores = opts.get(0)["ignores"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
        .unwrap_or_default();
    Some(Box::new(NoLiteralsInTemplate {
        ignores: GroupMatcher::new(ignores),
    }))
}

struct NoLiteralsInTemplate {
    ignores: GroupMatcher,
}

impl Rule for NoLiteralsInTemplate {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let Some(doc) = ctx.doc() else { return };
        for (_, a) in body_attributes(doc) {
            let AttributeKey::Directive(k) = &a.key else {
                continue;
            };
            if k.name.name != "bind" {
                continue;
            }
            let arg = match &k.argument {
                Some(DirectiveArgument::Static(id)) => Some(id.name.as_str()),
                _ => None,
            };
            if matches!(arg, Some("class" | "style")) || arg.is_some_and(|n| self.ignores.is_match(n)) {
                continue;
            }
            let Some(e) = directive_expression(doc, a) else {
                continue;
            };
            let e = js::skip_parens(e);
            let ty = match e {
                Expression::ObjectExpression(_) => "object",
                Expression::ArrayExpression(_) => "array",
                Expression::FunctionExpression(_) => "function",
                Expression::ArrowFunctionExpression(_) => "arrow function",
                _ => continue,
            };
            let span = e.span();
            let Some(crate::ast::AttributeValue::Expr(id)) = &a.value else {
                continue;
            };
            // `hasReferenceUpperElementVariable`: a reference to a v-for / slot variable inside.
            if doc.expr(*id).references.iter().any(|r| {
                r.variable.is_some() && span.start <= r.span.start && r.span.end <= span.end
            }) {
                continue;
            }
            ctx.report_id(
                span,
                "unexpected",
                fmt_msg("Unexpected {{type}} literal in template.", &[("type", ty)]),
            );
        }
    }
}
