//! vue/no-duplicate-class-names

use std::collections::{HashMap, HashSet};

use oxc::ast::ast::*;
use oxc::span::Span;

use crate::ast::{AttributeKey, AttributeValue, Document};
use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::js::skip_parens;
use crate::utils::template::{directive_expression, directive_value};
use crate::utils::tpl_misc::{body_attributes, js_split_ws, quasi_span, static_arg};
use crate::utils::tpl_valid::expr_span;

struct R;

#[derive(Clone, Copy, PartialEq, Eq)]
enum PKind {
    Binary,
    Template,
    Other,
}

type Collected = (Span, bool, Option<(Span, PKind)>);

struct ClassNode {
    span: Span,
    /// `getRawValue(node)`: literal value, or the raw text of a template element.
    value: String,
    uncond: bool,
    parent_expr: Option<(Span, PKind)>,
    /// `node.parent` span.
    parent: Span,
}

fn extract(
    x: &Expression,
    uncond: bool,
    parent_expr: Option<(Span, PKind)>,
    container: Span,
    out: &mut Vec<ClassNode>,
) {
    let here = |k: PKind| Some((expr_span(x), k));
    let parent = parent_expr.map_or(container, |p| p.0);
    match skip_parens(x) {
        Expression::StringLiteral(s) => {
            out.push(ClassNode { span: s.span, value: s.value.display().to_string(), uncond, parent_expr, parent });
        }
        Expression::ObjectExpression(o) => {
            for p in &o.properties {
                if let ObjectPropertyKind::ObjectProperty(p) = p
                    && let PropertyKey::StringLiteral(k) = &p.key
                {
                    out.push(ClassNode {
                        span: k.span,
                        value: k.value.display().to_string(),
                        uncond: false,
                        parent_expr: here(PKind::Other),
                        parent: p.span,
                    });
                }
            }
        }
        Expression::ArrayExpression(a) => {
            for el in &a.elements {
                if let Some(e) = el.as_expression() {
                    extract(e, uncond, here(PKind::Other), container, out);
                }
            }
        }
        Expression::ConditionalExpression(c) => {
            extract(&c.consequent, false, here(PKind::Other), container, out);
            extract(&c.alternate, false, here(PKind::Other), container, out);
        }
        Expression::TemplateLiteral(t) => {
            for (i, q) in t.quasis.iter().enumerate() {
                out.push(ClassNode {
                    span: quasi_span(t, i),
                    value: q.value.raw.to_string(),
                    uncond,
                    parent_expr: here(PKind::Template),
                    parent: expr_span(x),
                });
            }
            for e in &t.expressions {
                extract(e, uncond, here(PKind::Template), container, out);
            }
        }
        Expression::BinaryExpression(b) if b.operator == BinaryOperator::Addition => {
            extract(&b.left, uncond, here(PKind::Binary), container, out);
            extract(&b.right, uncond, here(PKind::Binary), container, out);
        }
        Expression::LogicalExpression(l) => {
            extract(&l.left, uncond, here(PKind::Other), container, out);
            extract(&l.right, false, here(PKind::Other), container, out);
        }
        _ => {}
    }
}

fn class_names(s: &str) -> Vec<&str> {
    s.split_whitespace().collect()
}

fn is_ws(s: &str) -> bool {
    !s.is_empty() && s.chars().all(char::is_whitespace)
}

fn append_to_last(kept: &mut [String], token: &str) {
    for j in (0..kept.len()).rev() {
        if !is_ws(&kept[j]) {
            let first = js_split_ws(&kept[j]).into_iter().next().unwrap_or_default();
            kept[j] = first + token;
            return;
        }
    }
}

fn remove_duplicates(raw: &str) -> String {
    let mut chars = raw.chars();
    let quote = chars.next().map(String::from).unwrap_or_default();
    let inner: String = {
        let v: Vec<char> = raw.chars().collect();
        if v.len() >= 2 { v[1..v.len() - 1].iter().collect() } else { String::new() }
    };
    let tokens = js_split_ws(&inner);
    let mut kept: Vec<String> = Vec::new();
    let mut used: HashSet<&str> = HashSet::new();
    let mut i = 0;
    while i < tokens.len() {
        let t = &tokens[i];
        if t.is_empty() {
            i += 1;
            continue;
        }
        if is_ws(t) {
            match kept.last_mut() {
                Some(l) => l.push_str(t),
                None => kept.push(t.clone()),
            }
        } else if used.contains(t.as_str()) {
            if !kept.is_empty() && i + 1 < tokens.len() && is_ws(&tokens[i + 1]) {
                let next = tokens[i + 1].clone();
                append_to_last(&mut kept, &next);
                i += 1;
            }
        } else {
            kept.push(t.clone());
            used.insert(t);
        }
        i += 1;
    }
    let trailing = inner.ends_with(char::is_whitespace);
    if let Some(last) = kept.last_mut()
        && !last.is_empty()
        && !trailing
        && !is_ws(last)
    {
        *last = js_split_ws(last).into_iter().next().unwrap_or_default();
    }
    format!("{quote}{}{quote}", kept.concat())
}

fn names_msg(names: &[&str]) -> String {
    let list: Vec<String> = names.iter().map(|n| format!("'{n}'")).collect();
    format!("Duplicate class name{} {}.", if names.len() > 1 { "s" } else { "" }, list.join(", "))
}

/// `reportDuplicateClasses`: returns the duplicates it reported.
fn report_duplicates(ctx: &mut Ctx, doc: &Document, span: Span, value: &str) -> Vec<String> {
    let names = class_names(value);
    if value.is_empty() || names.len() <= 1 {
        return Vec::new();
    }
    let mut seen = HashSet::new();
    let mut dups: Vec<&str> = Vec::new();
    for n in names {
        if !seen.insert(n) && !dups.contains(&n) {
            dups.push(n);
        }
    }
    if dups.is_empty() {
        return Vec::new();
    }
    let fix = Fix::replace(span, remove_duplicates(doc.slice(span)));
    ctx.report_fix(span, None, names_msg(&dups), Some(fix));
    dups.into_iter().map(str::to_string).collect()
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (e, a) in body_attributes(doc) {
            match &a.key {
                AttributeKey::Plain(k) if k.name == "class" => {
                    if let Some(AttributeValue::Literal(l)) = &a.value {
                        report_duplicates(ctx, doc, l.span, &l.value);
                    }
                }
                AttributeKey::Directive(k) if static_arg(k).is_some_and(|i| i.name == "class") => {
                    let Some(container) = directive_value(doc, a) else { continue };
                    let static_classes: Option<HashSet<&str>> = e.attributes().iter().find_map(|s| match (&s.key, &s.value) {
                        (AttributeKey::Plain(k), Some(AttributeValue::Literal(l))) if k.name == "class" => {
                            Some(class_names(&l.value).into_iter().collect())
                        }
                        _ => None,
                    });
                    let mut nodes = Vec::new();
                    if let Some(x) = directive_expression(doc, a) {
                        extract(x, true, None, container.span, &mut nodes);
                    }
                    let mut reported: HashSet<String> = HashSet::new();
                    let mut dup_expr: Vec<String> = Vec::new();
                    let mut seen: HashMap<String, Span> = HashMap::new();
                    let mut collected: HashMap<String, Collected> = HashMap::new();
                    for n in &nodes {
                        for r in report_duplicates(ctx, doc, n.span, &n.value) {
                            reported.insert(r);
                        }
                        if n.value.is_empty() {
                            continue;
                        }
                        let names = class_names(&n.value);
                        for c in &names {
                            if reported.contains(*c) {
                                continue;
                            }
                            let add = |c: &str, dup_expr: &mut Vec<String>| {
                                if !dup_expr.iter().any(|d| d == c) {
                                    dup_expr.push(c.to_string());
                                }
                            };
                            if let Some((_, ex_uncond, ex_parent)) = collected.get(*c) {
                                let same_parent = n.parent_expr.is_some()
                                    && *ex_parent == n.parent_expr
                                    && matches!(n.parent_expr, Some((_, PKind::Binary | PKind::Template)));
                                if n.uncond || same_parent || *ex_uncond {
                                    add(c, &mut dup_expr);
                                }
                            } else {
                                collected.insert(c.to_string(), (n.parent, n.uncond, n.parent_expr));
                            }
                            if n.uncond {
                                if seen.contains_key(*c) {
                                    add(c, &mut dup_expr);
                                } else {
                                    seen.insert(c.to_string(), n.parent);
                                }
                            }
                        }
                        if let Some(st) = &static_classes {
                            let inter: Vec<&str> = names.iter().copied().filter(|n| st.contains(n)).collect();
                            if !inter.is_empty() {
                                ctx.report(e.start_tag.span, names_msg(&inter));
                            }
                        }
                    }
                    for c in dup_expr {
                        if let Some(s) = seen.get(&c).copied().or_else(|| collected.get(&c).map(|x| x.0)) {
                            ctx.report(s, names_msg(&[&c]));
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(R))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dedupe() {
        assert_eq!(remove_duplicates("\"foo foo\""), "\"foo\"");
        assert_eq!(remove_duplicates("\"a b a c\""), "\"a b c\"");
        assert_eq!(remove_duplicates("\"a  a \""), "\"a \"");
    }
}
