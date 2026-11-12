//! vue/prefer-separate-static-class

use oxc::ast::ast::*;
use oxc::span::Span;

use crate::ast::{Attribute, AttributeKey, AttributeValue, Document, Element};
use crate::rule::{Ctx, Fix, Rule, RuleOptions, fmt_msg};
use crate::utils::js::{is_string_literal, skip_parens};
use crate::utils::template::directive_expression;
use crate::utils::tpl_misc::{body_attributes, quasi_span, static_arg, token_before, tokens_after};
use crate::utils::tpl_valid::expr_span;

struct R;

/// Where a static class node sits (for the fix).
#[derive(Clone, Copy)]
struct ListPos {
    /// The array / object literal holding the node (ESTree range).
    list: Span,
    len: usize,
    /// Span of the list element (the property for an object key).
    element: Span,
}

struct Found {
    span: Span,
    name: Option<String>,
    quasi: bool,
    pos: Option<ListPos>,
}

fn static_name(x: &Expression) -> Option<String> {
    match skip_parens(x) {
        Expression::StringLiteral(s) => Some(s.value.display().to_string()),
        Expression::TemplateLiteral(t) => crate::utils::js::template_value(t),
        _ => None,
    }
}

fn find(x: &Expression, pos: Option<ListPos>, out: &mut Vec<Found>) {
    let x = skip_parens(x);
    if let Expression::TemplateLiteral(t) = x
        && !t.expressions.is_empty()
    {
        let n = t.expressions.len();
        for (i, q) in t.quasis.iter().enumerate() {
            let Some(cooked) = q.value.cooked.as_ref() else { continue };
            let v = cooked.display().to_string();
            let v = v.as_str();
            if !v.trim().is_empty()
                && (i == 0 || v.starts_with(char::is_whitespace))
                && (i == n || v.ends_with(char::is_whitespace))
            {
                out.push(Found {
                    span: quasi_span(t, i),
                    name: Some(v.split_whitespace().collect::<Vec<_>>().join(" ")),
                    quasi: true,
                    pos: None,
                });
            }
        }
        return;
    }
    if is_string_literal(x) {
        out.push(Found { span: expr_span(x), name: static_name(x), quasi: false, pos });
        return;
    }
    match x {
        Expression::ArrayExpression(a) => {
            for el in &a.elements {
                if let Some(e) = el.as_expression() {
                    let p = ListPos { list: a.span, len: a.elements.len(), element: expr_span(e) };
                    find(e, Some(p), out);
                }
            }
        }
        Expression::ObjectExpression(o) => {
            for p in &o.properties {
                let ObjectPropertyKind::ObjectProperty(p) = p else { continue };
                if !matches!(skip_parens(&p.value), Expression::BooleanLiteral(b) if b.value) {
                    continue;
                }
                let lp = Some(ListPos { list: o.span, len: o.properties.len(), element: p.span });
                match &p.key {
                    PropertyKey::StaticIdentifier(id) if !p.computed => {
                        out.push(Found { span: id.span, name: Some(id.name.to_string()), quasi: false, pos: lp });
                    }
                    k => {
                        if let Some(ke) = k.as_expression()
                            && is_string_literal(ke)
                        {
                            out.push(Found { span: expr_span(ke), name: static_name(ke), quasi: false, pos: lp });
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

/// Remove a list element with its comma. Comments between the two stay (eslint-plugin-vue removes the
/// whole range and the comments with it).
fn remove_with_comma(doc: &Document, node: Span) -> Fix {
    let comment_in = |s: u32, e: u32| doc.comments.iter().any(|c| s <= c.span.start && c.span.end <= e);
    let pair = |a: Span, b: Span| Fix::merge(doc.source, vec![Fix::remove(a), Fix::remove(b)]).expect("disjoint");
    if let Some(prev) = token_before(doc, node.start)
        && doc.slice(prev.span) == ","
    {
        if comment_in(prev.span.end, node.start) {
            return pair(prev.span, node);
        }
        return Fix::remove(Span::new(prev.span.start, node.end));
    }
    let mut after = tokens_after(doc, node.end);
    if let (Some(n1), Some(n2)) = (after.next(), after.next())
        && doc.slice(n1.span) == ","
        && (n2.kind != crate::ast::TokenKind::Punctuator || !matches!(doc.slice(n2.span), "]" | "}"))
    {
        if comment_in(node.end, n2.span.start) {
            return pair(node, n1.span);
        }
        return Fix::remove(Span::new(node.start, n2.span.start));
    }
    Fix::remove(node)
}

fn fix(doc: &Document, e: &Element, a: &Attribute, top: &Expression, f: &Found, class: &str) -> Option<Fix> {
    if f.quasi {
        return None;
    }
    let mut edits = Vec::new();
    let mut removed = false;
    if is_string_literal(top) {
        edits.push(Fix::remove(a.span));
        removed = true;
    } else if let Some(p) = f.pos {
        if p.list == expr_span(top) && p.len == 1 {
            edits.push(Fix::remove(a.span));
            removed = true;
        } else if p.len == 1 {
            edits.push(remove_with_comma(doc, p.list));
        } else {
            edits.push(remove_with_comma(doc, p.element));
        }
    }
    let existing = e.attributes().iter().find_map(|s| match (&s.key, &s.value) {
        (AttributeKey::Plain(k), Some(AttributeValue::Literal(l))) if k.name == "class" => Some(l),
        _ => None,
    });
    match existing {
        Some(l) => edits.push(Fix::replace(l.span, format!("\"{} {class}\"", l.value))),
        None => edits.push(Fix::insert(a.span.start, format!("class=\"{class}\"{}", if removed { "" } else { " " }))),
    }
    Fix::merge(doc.source, edits)
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (e, a) in body_attributes(doc) {
            let AttributeKey::Directive(k) = &a.key else { continue };
            if k.name.name != "bind"
                || !static_arg(k).is_some_and(|i| i.name == "class")
                || k.modifiers.iter().any(|m| m.name == "attr" || m.name == "prop")
            {
                continue;
            }
            let Some(top) = directive_expression(doc, a) else { continue };
            let mut found = Vec::new();
            find(top, None, &mut found);
            for f in &found {
                let Some(class) = &f.name else { continue };
                let fx = fix(doc, e, a, top, f, class);
                ctx.report_fix(
                    f.span,
                    None,
                    fmt_msg(
                        "Static class \"{{className}}\" should be in a static `class` attribute.",
                        &[("className", class)],
                    ),
                    fx,
                );
            }
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(R))
}
