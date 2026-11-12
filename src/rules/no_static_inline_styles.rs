//! vue/no-static-inline-styles

use oxc::ast::ast::*;
use oxc::span::Span;

use crate::ast::{Attribute, AttributeKey, Document};
use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::js::skip_parens;
use crate::utils::template::directive_expression;
use crate::utils::tpl_misc::{body_attributes, static_arg};

struct R {
    allow_binding: bool,
}

fn is_static_value(p: &ObjectProperty) -> bool {
    !p.computed
        && skip_parens(&p.value).is_literal()
        && matches!(&p.key, PropertyKey::StaticIdentifier(_))
            | p.key.as_expression().is_some_and(|k| k.is_literal())
}

fn report_spans(doc: &Document, a: &Attribute) -> Vec<Span> {
    let Some(x) = directive_expression(doc, a) else {
        return Vec::new();
    };
    // Outer `None` = array hole (skipped); inner `None` = not an object literal.
    let elements: Vec<Option<Option<&ObjectExpression>>> = match skip_parens(x) {
        Expression::ObjectExpression(o) => vec![Some(Some(&**o))],
        Expression::ArrayExpression(arr) => arr
            .elements
            .iter()
            .map(|el| match el {
                ArrayExpressionElement::Elision(_) => None,
                _ => Some(el.as_expression().and_then(|e| match skip_parens(e) {
                    Expression::ObjectExpression(o) => Some(&**o),
                    _ => None,
                })),
            })
            .collect(),
        _ => return Vec::new(),
    };
    let mut out = Vec::new();
    for el in elements.into_iter().flatten() {
        let Some(obj) = el else { return out };
        let mut all_static = true;
        for p in &obj.properties {
            let ObjectPropertyKind::ObjectProperty(p) = p else {
                return out;
            };
            if p.computed {
                return out;
            }
            if is_static_value(p) {
                out.push(p.span);
            } else {
                all_static = false;
            }
        }
        if !all_static {
            return out;
        }
    }
    vec![a.span]
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (_, a) in body_attributes(doc) {
            match &a.key {
                AttributeKey::Plain(k) if k.name == "style" => {
                    ctx.report(a.span, "`style` attributes are forbidden.");
                }
                AttributeKey::Directive(k)
                    if !self.allow_binding
                        && k.name.name == "bind"
                        && static_arg(k).is_some_and(|i| i.name == "style") =>
                {
                    for s in report_spans(doc, a) {
                        ctx.report(s, "Static inline `style` are forbidden.");
                    }
                }
                _ => {}
            }
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(R { allow_binding: opts.get(0)["allowBinding"].as_bool() == Some(true) }))
}
