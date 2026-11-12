//! vue/no-negated-v-if-condition

use std::collections::HashSet;

use oxc::ast::ast::{BinaryOperator, Expression, UnaryOperator};

use crate::ast::{Attribute, Child, Document, Element};
use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::js::skip_parens;
use crate::utils::template::{directive_expression, directive_key};
use crate::utils::tpl_valid::{body_directives, expr_span};

struct R;

fn next_sibling<'d>(doc: &'d Document, e: &Element) -> Option<&'d Element> {
    let p = doc.element(e.parent?);
    let i = p.children.iter().position(|c| *c == Child::Element(e.id))?;
    p.children[i + 1..].iter().find_map(|c| match c {
        Child::Element(id) => Some(doc.element(*id)),
        _ => None,
    })
}

fn cond_directive(e: &Element) -> Option<&Attribute> {
    e.attributes()
        .iter()
        .find(|a| directive_key(a).is_some_and(|k| matches!(k.name.name.as_str(), "if" | "else-if" | "else")))
}

fn converted(doc: &Document, x: &Expression) -> String {
    match skip_parens(x) {
        Expression::UnaryExpression(u) if u.operator == UnaryOperator::LogicalNot => {
            doc.source[u.span.start as usize + 1..u.span.end as usize].to_string()
        }
        Expression::BinaryExpression(b) => {
            let (l, r) = (doc.slice(expr_span(&b.left)), doc.slice(expr_span(&b.right)));
            match b.operator {
                BinaryOperator::Inequality => format!("{l} == {r}"),
                BinaryOperator::StrictInequality => format!("{l} === {r}"),
                _ => doc.slice(expr_span(x)).to_string(),
            }
        }
        _ => doc.slice(expr_span(x)).to_string(),
    }
}

fn swap(doc: &Document, ife: &Element, elsee: &Element, x: &Expression) -> Option<Fix> {
    let cond = converted(doc, x);
    let (ifd, elsed) = (cond_directive(ife)?, cond_directive(elsee)?);
    let name = &directive_key(ifd)?.name.name;
    let new_if = format!(
        "{}v-{name}=\"{cond}\"{}",
        &doc.source[elsee.span.start as usize..elsed.span.start as usize],
        &doc.source[elsed.span.end as usize..elsee.span.end as usize]
    );
    let new_else = format!(
        "{}v-else{}",
        &doc.source[ife.span.start as usize..ifd.span.start as usize],
        &doc.source[ifd.span.end as usize..ife.span.end as usize]
    );
    Fix::merge(doc.source, vec![Fix::replace(ife.span, new_if), Fix::replace(elsee.span, new_else)])
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        let mut done = HashSet::new();
        let mut dirs = body_directives(doc, "if");
        dirs.extend(body_directives(doc, "else-if"));
        dirs.sort_by_key(|d| d.2.span.start);
        for (e, _, a, _) in dirs {
            let Some(x) = directive_expression(doc, a) else { continue };
            let negated = match skip_parens(x) {
                Expression::UnaryExpression(u) => u.operator == UnaryOperator::LogicalNot,
                Expression::BinaryExpression(b) => {
                    matches!(b.operator, BinaryOperator::Inequality | BinaryOperator::StrictInequality)
                }
                _ => false,
            };
            if !negated {
                continue;
            }
            let Some(next) = next_sibling(doc, e).filter(|n| n.has_directive("else")) else { continue };
            if !done.insert((e.span.start, next.span.start)) {
                continue;
            }
            let fix = swap(doc, e, next, x);
            ctx.report_fix(expr_span(x), None, "Unexpected negated condition in v-if with v-else.", fix);
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(R))
}
