//! vue/valid-v-model

use oxc::ast::ast::Expression;

use crate::ast::{Document, Element, VExpression};
use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::js::skip_parens;
use crate::utils::template::{directive_value, is_custom_component, literal_value};
use crate::utils::tpl_valid::{
    arg_span, body_directives, expr_span, is_maybe_null_object_member, is_optional_chaining_member,
    is_whole_identifier, missing_value,
};

const VALID_MODIFIERS: [&str; 3] = ["lazy", "number", "trim"];

fn is_valid_element(e: &Element) -> bool {
    let n = e.name.as_str();
    matches!(n, "input" | "select" | "textarea")
        || (!matches!(n, "keep-alive" | "slot" | "transition" | "transition-group")
            && is_custom_component(e, false))
}

fn is_lhs(e: &Expression) -> bool {
    match skip_parens(e) {
        Expression::TSAsExpression(x) => is_lhs(&x.expression),
        Expression::TSNonNullExpression(x) => is_lhs(&x.expression),
        Expression::Identifier(_) => true,
        x => x.is_member_expression(),
    }
}

/// `getVariable(name, element)`: a template variable of that name on the element or an ancestor.
fn has_variable(doc: &Document, e: &Element, name: &str) -> bool {
    std::iter::once(e)
        .chain(doc.ancestors(e.id))
        .any(|el| el.variables.iter().any(|v| v.name == name))
}

struct ValidVModel;

impl Rule for ValidVModel {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (e, _, a, k) in body_directives(doc, "model") {
            if !is_valid_element(e) {
                ctx.report(
                    a.span,
                    format!(
                        "'v-model' directives aren't supported on <{}> elements.",
                        e.name
                    ),
                );
            }
            if e.name == "input"
                && e.attr("type")
                    .is_some_and(|t| literal_value(t) == Some("file"))
            {
                ctx.report(
                    a.span,
                    "'v-model' directives don't support 'file' input type.",
                );
            }
            if !is_custom_component(e, false) {
                if let Some(s) = arg_span(doc, k) {
                    ctx.report(s, "'v-model' directives require no argument.");
                }
                for m in &k.modifiers {
                    if !VALID_MODIFIERS.contains(&m.name.as_str()) {
                        ctx.report(
                            m.span,
                            format!(
                                "'v-model' directives don't support the modifier '{}'.",
                                m.name
                            ),
                        );
                    }
                }
            }
            if missing_value(doc, a) {
                ctx.report(a.span, "'v-model' directives require that attribute value.");
                continue;
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
                    "Optional chaining cannot appear in 'v-model' directives.",
                );
            } else if !is_lhs(x) {
                ctx.report(
                    span,
                    "'v-model' directives require the attribute value which is valid as LHS.",
                );
            } else if is_maybe_null_object_member(x) {
                ctx.report(
                    span,
                    "'v-model' directive has potential null object property access.",
                );
            }
            for r in &c.references {
                if is_whole_identifier(expr, r.span) && has_variable(doc, e, &r.name) {
                    ctx.report(span, format!("'v-model' directives cannot update the iteration variable '{}' itself.", r.name));
                }
            }
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(ValidVModel))
}
