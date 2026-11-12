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
    /// The text continues an interpolation / concatenation on the left (right) without whitespace, so its
    /// first (last) class name is only a fragment of the real one.
    glued: (bool, bool),
}

/// Whitespace-edge text of a string operand of `+` (`None`: not a string, unknown at lint time).
fn edge_text<'a>(x: &'a Expression, right: bool) -> Option<&'a str> {
    match skip_parens(x) {
        Expression::StringLiteral(s) => s.value.as_str(),
        Expression::TemplateLiteral(t) => {
            let q = if right { t.quasis.last() } else { t.quasis.first() }?;
            Some(q.value.raw.as_str())
        }
        Expression::BinaryExpression(b) if b.operator == BinaryOperator::Addition => {
            edge_text(if right { &b.right } else { &b.left }, right)
        }
        _ => None,
    }
}

/// Class names of a string that are complete: an edge glued to unknown text is a fragment
/// (`text-${color}-600` holds no class `-600`).
fn complete_names(value: &str, (gl, gr): (bool, bool)) -> Vec<&str> {
    let mut names = class_names(value);
    if gr && !value.ends_with(char::is_whitespace) {
        names.pop();
    }
    if gl && !value.starts_with(char::is_whitespace) && !names.is_empty() {
        names.remove(0);
    }
    names
}

fn extract(
    x: &Expression,
    uncond: bool,
    parent_expr: Option<(Span, PKind)>,
    container: Span,
    glued: (bool, bool),
    out: &mut Vec<ClassNode>,
) {
    let here = |k: PKind| Some((expr_span(x), k));
    let parent = parent_expr.map_or(container, |p| p.0);
    match skip_parens(x) {
        Expression::StringLiteral(s) => {
            out.push(ClassNode {
                span: s.span,
                value: s.value.display().to_string(),
                uncond,
                parent_expr,
                parent,
                glued,
            });
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
                        glued: (false, false),
                    });
                }
            }
        }
        Expression::ArrayExpression(a) => {
            for el in &a.elements {
                if let Some(e) = el.as_expression() {
                    extract(e, uncond, here(PKind::Other), container, (false, false), out);
                }
            }
        }
        Expression::ConditionalExpression(c) => {
            extract(&c.consequent, false, here(PKind::Other), container, glued, out);
            extract(&c.alternate, false, here(PKind::Other), container, glued, out);
        }
        Expression::TemplateLiteral(t) => {
            let last = t.quasis.len().saturating_sub(1);
            for (i, q) in t.quasis.iter().enumerate() {
                out.push(ClassNode {
                    span: quasi_span(t, i),
                    value: q.value.raw.to_string(),
                    uncond,
                    parent_expr: here(PKind::Template),
                    parent: expr_span(x),
                    glued: (i > 0 || glued.0, i < last || glued.1),
                });
            }
            for e in &t.expressions {
                extract(e, uncond, here(PKind::Template), container, (true, true), out);
            }
        }
        Expression::BinaryExpression(b) if b.operator == BinaryOperator::Addition => {
            // A known neighbour that ends (starts) with whitespace separates the class names.
            let sep_l = edge_text(&b.left, true).is_some_and(|t| t.ends_with(char::is_whitespace));
            let sep_r = edge_text(&b.right, false).is_some_and(|t| t.starts_with(char::is_whitespace));
            extract(&b.left, uncond, here(PKind::Binary), container, (glued.0, !sep_r), out);
            extract(&b.right, uncond, here(PKind::Binary), container, (!sep_l, glued.1), out);
        }
        Expression::LogicalExpression(l) => {
            extract(&l.left, uncond, here(PKind::Other), container, glued, out);
            extract(&l.right, false, here(PKind::Other), container, glued, out);
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

/// `raw` keeps its delimiters: quotes, or a template element's `` ` `` / `}` and `` ` `` / `${`
/// (upstream treats `}` and `{` as quotes and corrupts the template literal).
fn remove_duplicates(raw: &str) -> String {
    let open = raw.chars().next().map_or(0, char::len_utf8);
    let close = if raw.ends_with("${") { 2 } else { raw.chars().last().map_or(0, char::len_utf8) };
    if raw.len() < open + close {
        return raw.to_string();
    }
    let (head, tail) = (&raw[..open], &raw[raw.len() - close..]);
    let inner = raw[open..raw.len() - close].to_string();
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
    format!("{head}{}{tail}", kept.concat())
}

fn names_msg(names: &[&str]) -> String {
    let list: Vec<String> = names.iter().map(|n| format!("'{n}'")).collect();
    format!("Duplicate class name{} {}.", if names.len() > 1 { "s" } else { "" }, list.join(", "))
}

/// `reportDuplicateClasses`: returns the duplicates it reported.
fn report_duplicates(ctx: &mut Ctx, doc: &Document, span: Span, value: &str, glued: (bool, bool)) -> Vec<String> {
    let all = class_names(value);
    let names = complete_names(value, glued);
    if value.is_empty() || names.len() <= 1 {
        return Vec::new();
    }
    let mut seen = HashSet::new();
    let mut dups: Vec<&str> = Vec::new();
    for &n in &names {
        if !seen.insert(n) && !dups.contains(&n) {
            dups.push(n);
        }
    }
    if dups.is_empty() {
        return Vec::new();
    }
    // The fixer works on the raw text: it would also drop a fragment spelled like a duplicate.
    let count = |v: &[&str], d: &str| v.iter().filter(|n| **n == d).count();
    let fragment_dup = dups.iter().any(|d| count(&all, d) != count(&names, d));
    let fix = (!fragment_dup).then(|| Fix::replace(span, remove_duplicates(doc.slice(span))));
    ctx.report_fix(span, None, names_msg(&dups), fix);
    dups.into_iter().map(str::to_string).collect()
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (e, a) in body_attributes(doc) {
            match &a.key {
                AttributeKey::Plain(k) if k.name == "class" => {
                    if let Some(AttributeValue::Literal(l)) = &a.value {
                        report_duplicates(ctx, doc, l.span, &l.value, (false, false));
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
                        extract(x, true, None, container.span, (false, false), &mut nodes);
                    }
                    let mut reported: HashSet<String> = HashSet::new();
                    let mut dup_expr: Vec<String> = Vec::new();
                    let mut seen: HashMap<String, Span> = HashMap::new();
                    let mut collected: HashMap<String, Collected> = HashMap::new();
                    for n in &nodes {
                        for r in report_duplicates(ctx, doc, n.span, &n.value, n.glued) {
                            reported.insert(r);
                        }
                        if n.value.is_empty() {
                            continue;
                        }
                        let names = complete_names(&n.value, n.glued);
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
        assert_eq!(remove_duplicates("} b b`"), "} b`");
        assert_eq!(remove_duplicates("`b b ${"), "`b ${");
    }
}
