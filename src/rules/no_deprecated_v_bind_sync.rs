//! vue/no-deprecated-v-bind-sync

use oxc::ast::ast::Expression;

use crate::ast::{Attribute, Document, Element, VExpression};
use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::js::skip_parens;
use crate::utils::template::{
    directive_value, is_html_well_known_element_name, is_math_well_known_element_name,
    is_svg_well_known_element_name,
};
use crate::utils::tpl_valid::{
    arg_text, body_directives, is_optional_chaining_member, is_whole_identifier,
};

struct R;

/// `v-model:x` compiles only on a component and with an assignable value (`valid-v-model`); `.sync`
/// on a plain element or with another value would become a compile error. eslint-plugin-vue fixes
/// those too.
fn model_compiles(doc: &Document, e: &Element, a: &Attribute) -> bool {
    // Vue 3 compiles `<tr :is="…">` as a plain element (only `is="vue:…"` makes it a component).
    if is_html_well_known_element_name(&e.raw_name)
        || is_svg_well_known_element_name(&e.raw_name)
        || is_math_well_known_element_name(&e.raw_name)
    {
        return false;
    }
    let Some(c) = directive_value(doc, a) else {
        return false;
    };
    let Some(VExpression::Expression(expr)) = &c.expression else {
        return false;
    };
    let x = skip_parens(expr);
    (matches!(x, Expression::Identifier(_)) || x.is_member_expression())
        && !is_optional_chaining_member(x)
        && !c
            .references
            .iter()
            .any(|r| r.variable.is_some() && is_whole_identifier(expr, r.span))
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (e, _, a, k) in body_directives(doc, "bind") {
            if !k.has_modifier("sync") {
                continue;
            }
            let fix = if k.modifiers.len() > 1 || !model_compiles(doc, e, a) {
                None
            } else {
                arg_text(doc, k).map(|t| Fix::replace(k.span, format!("v-model:{t}")))
            };
            ctx.report_fix(
                a.span,
                None,
                "'.sync' modifier on 'v-bind' directive is deprecated. Use 'v-model:propName' instead.",
                fix,
            );
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(R))
}
