//! vue/prefer-v-model

use oxc::ast::ast::{AssignmentOperator, BindingPattern, Expression, Statement};
use oxc::span::{GetSpan, Span};

use crate::ast::{Attribute, AttributeKey, Document, VExpression};
use crate::rule::{Ctx, Fix, Rule, RuleOptions, Suggestion, fmt_msg};
use crate::utils::casing::camel_case;
use crate::utils::js;
use crate::utils::template::{directive_value, is_custom_component};
use crate::utils::tpl_style::is_js_ws;
use crate::utils::tpl_valid::body_elements;

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(PreferVModel))
}

struct PreferVModel;

fn static_arg_raw(a: &Attribute) -> Option<&str> {
    match &a.key {
        AttributeKey::Directive(k) => match &k.argument {
            Some(crate::ast::DirectiveArgument::Static(id)) => Some(&id.raw_name),
            _ => None,
        },
        _ => None,
    }
}

fn update_prop(a: &Attribute) -> Option<&str> {
    static_arg_raw(a)?.strip_prefix("update:")
}

fn assign<'b, 'a>(e: &'b Expression<'a>) -> Option<&'b oxc::ast::ast::AssignmentExpression<'a>> {
    match js::skip_parens(e) {
        Expression::AssignmentExpression(a) if a.operator == AssignmentOperator::Assign => Some(a),
        _ => None,
    }
}

fn is_mirror(doc: &Document, bind: &Expression, on: &VExpression) -> bool {
    let bind_text = doc.slice(js::skip_parens(bind).span());
    match on {
        VExpression::On { body, .. } => {
            let [Statement::ExpressionStatement(s)] = body.as_slice() else {
                return false;
            };
            assign(&s.expression).is_some_and(|a| {
                doc.slice(a.left.span()) == bind_text
                    && matches!(js::skip_parens(&a.right), Expression::Identifier(i) if i.name == "$event")
            })
        }
        VExpression::Expression(e) => {
            let Expression::ArrowFunctionExpression(f) = js::skip_parens(e) else {
                return false;
            };
            if f.params.items.len() != 1 || f.params.rest.is_some() {
                return false;
            }
            let p = &f.params.items[0];
            let BindingPattern::BindingIdentifier(param) = &p.pattern else { return false };
            if p.initializer.is_some() {
                return false;
            }
            let Some(body) = f.body.as_expression() else {
                return false;
            };
            assign(body).is_some_and(|a| {
                doc.slice(a.left.span()) == bind_text
                    && matches!(js::skip_parens(&a.right), Expression::Identifier(i) if i.name == param.name)
            })
        }
        _ => false,
    }
}

impl Rule for PreferVModel {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let Some(doc) = ctx.doc() else { return };
        let src = ctx.source();
        for e in body_elements(doc) {
            if !is_custom_component(e, false) {
                continue;
            }
            let mods_empty = |a: &Attribute| matches!(&a.key, AttributeKey::Directive(k) if k.modifiers.is_empty());
            let dname = |a: &Attribute| match &a.key {
                AttributeKey::Directive(k) => k.name.name.clone(),
                _ => String::new(),
            };
            let binds: Vec<&Attribute> = e
                .attributes()
                .iter()
                .filter(|a| dname(a) == "bind" && static_arg_raw(a).is_some() && mods_empty(a))
                .collect();
            let ons: Vec<&Attribute> = e
                .attributes()
                .iter()
                .filter(|a| dname(a) == "on" && update_prop(a).is_some() && mods_empty(a))
                .collect();
            for b in binds {
                let Some(prop) = static_arg_raw(b) else { continue };
                let norm = camel_case(prop);
                let Some(on) = ons.iter().find(|o| update_prop(o).map(camel_case).as_deref() == Some(&norm))
                else {
                    continue;
                };
                let Some(bc) = directive_value(doc, b) else { continue };
                let Some(VExpression::Expression(bind_expr)) = &bc.expression else { continue };
                let Some(on_expr) = directive_value(doc, on).and_then(|c| c.expression.as_ref()) else {
                    continue;
                };
                if !is_mirror(doc, bind_expr, on_expr) {
                    continue;
                }
                let vmodel = if norm == "modelValue" { "v-model".to_string() } else { format!("v-model:{prop}") };
                let event = update_prop(on).unwrap_or(prop);
                let text = format!("{vmodel}={}", doc.slice(bc.span));
                let before = &src[..on.span.start as usize];
                let trimmed = before.trim_end_matches(is_js_ws);
                let remove_start = trimmed.len() as u32;
                let fix = Fix::merge(
                    src,
                    vec![Fix::replace(b.span, text), Fix::remove(Span::new(remove_start, on.span.end))],
                );
                let sugg = fix
                    .map(|fix| {
                        vec![Suggestion {
                            message_id: Some("replaceWithVModel"),
                            desc: fmt_msg("Replace with `{{ vModelName }}`.", &[("vModelName", &vmodel)]),
                            fix,
                        }]
                    })
                    .unwrap_or_default();
                ctx.push(
                    b.span,
                    Some("preferVModel"),
                    fmt_msg(
                        "Prefer `{{ vModelName }}` over the `:{{ propName }}`/`@update:{{ eventName }}` pair.",
                        &[("vModelName", &vmodel), ("propName", prop), ("eventName", event)],
                    ),
                    None,
                    sugg,
                );
            }
        }
    }
}
