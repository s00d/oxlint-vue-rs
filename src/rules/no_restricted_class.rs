//! vue/no-restricted-class

use oxc::ast::ast::{ArrayExpressionElement, BinaryOperator, Expression, ObjectPropertyKind};
use oxc::span::{GetSpan, Span};

use crate::ast::{AttributeKey, AttributeValue, VExpression};
use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::js::{self, skip_parens};
use crate::utils::regexp::GroupMatcher;
use crate::utils::template::directive_key;
use crate::utils::tpl_misc::quasi_span;
use crate::utils::tpl_style::is_js_ws;
use crate::utils::tpl_valid::body_elements;

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let pats = opts.options.iter().filter_map(|v| v.as_str());
    Some(Box::new(NoRestrictedClass {
        matcher: GroupMatcher::new(pats),
    }))
}

struct NoRestrictedClass {
    matcher: GroupMatcher,
}

/// JS `s.split(/\s+/)`.
fn split_ws(s: &str) -> impl Iterator<Item = &str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut in_ws = false;
    for (i, c) in s.char_indices() {
        let w = is_js_ws(c);
        if w && !in_ws {
            parts.push(&s[start..i]);
        } else if !w && in_ws {
            start = i;
        }
        in_ws = w;
    }
    parts.push(if in_ws { "" } else { &s[start..] });
    parts.into_iter()
}

/// `extractClassNames`: (class name, report span).
fn extract(e: &Expression, text_only: bool, out: &mut Vec<(String, Span)>) {
    let e = skip_parens(e);
    let literal = match e {
        Expression::StringLiteral(s) => Some(s.value.display().to_string()),
        Expression::NumericLiteral(n) => Some(js::js_number(n.value)),
        Expression::BooleanLiteral(b) => Some(b.value.to_string()),
        Expression::NullLiteral(_) => Some("null".into()),
        Expression::BigIntLiteral(b) => Some(b.value.to_string()),
        Expression::RegExpLiteral(r) => Some(r.regex.to_string()),
        _ => None,
    };
    if let Some(v) = literal {
        out.extend(split_ws(&v).map(|c| (c.to_string(), e.span())));
        return;
    }
    match e {
        Expression::TemplateLiteral(t) => {
            for (i, q) in t.quasis.iter().enumerate() {
                if let Some(c) = &q.value.cooked {
                    let c = c.display().to_string();
                    out.extend(split_ws(&c).map(|n| (n.to_string(), quasi_span(t, i))));
                }
            }
            for x in &t.expressions {
                extract(x, true, out);
            }
        }
        Expression::BinaryExpression(b) => {
            if b.operator == BinaryOperator::Addition {
                extract(&b.left, true, out);
                extract(&b.right, true, out);
            }
        }
        _ if text_only => {}
        Expression::ObjectExpression(o) => {
            for p in &o.properties {
                let ObjectPropertyKind::ObjectProperty(p) = p else {
                    continue;
                };
                let Some(name) = js::prop_name(p).filter(|n| !n.is_empty()) else {
                    continue;
                };
                out.extend(split_ws(&name).map(|c| (c.to_string(), p.key.span())));
            }
        }
        Expression::ArrayExpression(a) => {
            for el in &a.elements {
                match el {
                    ArrayExpressionElement::SpreadElement(_) | ArrayExpressionElement::Elision(_) => {}
                    el => {
                        if let Some(x) = el.as_expression() {
                            extract(x, false, out);
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

impl Rule for NoRestrictedClass {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let Some(doc) = ctx.doc() else { return };
        let mut found: Vec<(String, Span)> = Vec::new();
        for e in body_elements(doc) {
            for a in e.attributes() {
                match (&a.key, &a.value) {
                    (AttributeKey::Plain(k), Some(AttributeValue::Literal(l))) if k.name == "class" => {
                        found.extend(split_ws(&l.value).map(|c| (c.to_string(), l.span)));
                    }
                    (AttributeKey::Directive(_), Some(AttributeValue::Expr(id))) => {
                        let k = directive_key(a).unwrap();
                        if k.name.name != "bind" || k.static_argument() != Some("class") {
                            continue;
                        }
                        let expr = match &doc.expr(*id).expression {
                            Some(VExpression::Expression(x)) => x,
                            Some(VExpression::FilterSequence { expression, .. }) => expression,
                            _ => continue,
                        };
                        extract(expr, false, &mut found);
                    }
                    _ => {}
                }
            }
        }
        for (name, span) in found {
            if self.matcher.is_match(&name) {
                ctx.report_id(
                    span,
                    "forbiddenClass",
                    format!("'{name}' class is not allowed."),
                );
            }
        }
    }
}
