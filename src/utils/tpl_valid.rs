//! Helpers for the directive-validity / template-structure rules (`valid-v-*`, `no-*-template-*`, …).

use oxc::ast::ast::Expression;
use oxc::span::{GetSpan, Span};

use crate::ast::*;
use crate::rule::Ctx;
use crate::utils::js::skip_parens;
use crate::utils::template::{directive_key, is_empty_value_directive};

/// Every element of the template body in pre-order, the root `<template>` included
/// (what `defineTemplateBodyVisitor` visits).
pub fn body_elements<'d>(doc: &'d Document) -> Vec<&'d Element> {
    let Some(root) = doc.template_body else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        let e = doc.element(id);
        out.push(e);
        for c in e.children.iter().rev() {
            if let Child::Element(c) = c {
                stack.push(*c);
            }
        }
    }
    out
}

/// `(element, attribute index, attribute, key)` for every `v-<name>` directive in the template body.
pub fn body_directives<'d>(
    doc: &'d Document,
    name: &str,
) -> Vec<(&'d Element, usize, &'d Attribute, &'d DirectiveKey)> {
    let mut out = Vec::new();
    for e in body_elements(doc) {
        for (i, a) in e.attributes().iter().enumerate() {
            if let Some(k) = directive_key(a)
                && k.name.name == name
            {
                out.push((e, i, a, k));
            }
        }
    }
    out
}

/// Span of a directive argument node (identifier or dynamic container).
pub fn arg_span(doc: &Document, k: &DirectiveKey) -> Option<Span> {
    match k.argument.as_ref()? {
        DirectiveArgument::Static(id) => Some(id.span),
        DirectiveArgument::Dynamic(e) => Some(doc.expr(*e).span),
    }
}

/// Raw source text of a directive argument (`sourceCode.getText(key.argument)`).
pub fn arg_text<'a>(doc: &Document<'a>, k: &DirectiveKey) -> Option<&'a str> {
    arg_span(doc, k).map(|s| doc.slice(s))
}

/// First modifier start .. last modifier end.
pub fn modifiers_span(k: &DirectiveKey) -> Option<Span> {
    Some(Span::new(
        k.modifiers.first()?.span.start,
        k.modifiers.last()?.span.end,
    ))
}

/// `!node.value || utils.isEmptyValueDirective(node)`.
pub fn missing_value(doc: &Document, a: &Attribute) -> bool {
    a.value.is_none() || is_empty_value_directive(doc, a)
}

/// What a simple directive expects as value.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ValueRule {
    /// `require no attribute value` (reported on the value).
    Forbidden,
    /// `require that attribute value` (reported on the directive).
    Required,
}

/// The argument / modifier / value checks shared by most `valid-v-*` rules.
pub fn check_simple(
    ctx: &mut Ctx,
    doc: &Document,
    a: &Attribute,
    k: &DirectiveKey,
    value: ValueRule,
) {
    let d = &k.name.name;
    if let Some(s) = arg_span(doc, k) {
        ctx.report(s, format!("'v-{d}' directives require no argument."));
    }
    if let Some(s) = modifiers_span(k) {
        ctx.report(s, format!("'v-{d}' directives require no modifier."));
    }
    match value {
        ValueRule::Forbidden => {
            if let Some(s) = crate::utils::template::value_span(doc, a) {
                ctx.report(s, format!("'v-{d}' directives require no attribute value."));
            }
        }
        ValueRule::Required => {
            if missing_value(doc, a) {
                ctx.report(
                    a.span,
                    format!("'v-{d}' directives require that attribute value."),
                );
            }
        }
    }
}

/// Run [`check_simple`] over every `v-<name>` of the template body.
pub fn run_simple(ctx: &mut Ctx, name: &str, value: ValueRule) {
    let Some(doc) = ctx.doc() else { return };
    for (_, _, a, k) in body_directives(doc, name) {
        check_simple(ctx, doc, a, k, value);
    }
}

/// `node.type === 'ChainExpression' && node.expression.type === 'MemberExpression'`.
pub fn is_optional_chaining_member(e: &Expression) -> bool {
    matches!(e, Expression::ChainExpression(c) if c.expression.is_member_expression())
}

/// `isMaybeNullObjectMemberExpression`.
pub fn is_maybe_null_object_member(e: &Expression) -> bool {
    let Some(m) = e.as_member_expression() else {
        return false;
    };
    match skip_parens(m.object()) {
        Expression::ChainExpression(_) | Expression::NullLiteral(_) => true,
        o if o.is_member_expression() => is_maybe_null_object_member(o),
        _ => false,
    }
}

/// Whether the expression (parens skipped) is exactly the identifier at `span`
/// (`reference.id.parent.type === 'VExpressionContainer'`).
pub fn is_whole_identifier(e: &Expression, span: Span) -> bool {
    matches!(skip_parens(e), Expression::Identifier(id) if id.span == span)
}

/// `utils.equalTokens`: same token kinds and values over both ranges (template tokens).
pub fn equal_tokens(doc: &Document, a: Span, b: Span) -> bool {
    let (ta, tb) = (doc.tokens_in(a), doc.tokens_in(b));
    ta.len() == tb.len()
        && ta
            .iter()
            .zip(tb)
            .all(|(x, y)| x.kind == y.kind && doc.slice(x.span) == doc.slice(y.span))
}

/// ESTree-ish node type equality for two expressions (parens skipped; member / literal kinds unified).
pub fn same_type(a: &Expression, b: &Expression) -> bool {
    fn norm(e: &Expression) -> u8 {
        match e {
            _ if e.is_member_expression() => 1,
            _ if e.is_literal() => 2,
            _ => 0,
        }
    }
    let (a, b) = (skip_parens(a), skip_parens(b));
    let (na, nb) = (norm(a), norm(b));
    if na != 0 || nb != 0 {
        return na == nb;
    }
    std::mem::discriminant(a) == std::mem::discriminant(b)
}

/// Span of an expression with parens skipped (ESTree node range).
pub fn expr_span(e: &Expression) -> Span {
    skip_parens(e).span()
}

/// Span of a child node (element, text or mustache).
pub fn child_span(doc: &Document, c: Child) -> Span {
    match c {
        Child::Element(id) => doc.element(id).span,
        Child::Text(id) => doc.text(id).span,
        Child::Expr(id) => doc.expr(id).span,
    }
}

/// `xml-name-validator` `name()`: XML 1.0 `Name` production.
pub fn is_xml_name(s: &str) -> bool {
    fn start(c: char) -> bool {
        matches!(c, ':' | 'A'..='Z' | '_' | 'a'..='z' | '\u{C0}'..='\u{D6}' | '\u{D8}'..='\u{F6}' | '\u{F8}'..='\u{2FF}'
            | '\u{370}'..='\u{37D}' | '\u{37F}'..='\u{1FFF}' | '\u{200C}'..='\u{200D}' | '\u{2070}'..='\u{218F}'
            | '\u{2C00}'..='\u{2FEF}' | '\u{3001}'..='\u{D7FF}' | '\u{F900}'..='\u{FDCF}' | '\u{FDF0}'..='\u{FFFD}'
            | '\u{10000}'..='\u{EFFFF}')
    }
    fn rest(c: char) -> bool {
        start(c)
            || matches!(c, '-' | '.' | '0'..='9' | '\u{B7}' | '\u{300}'..='\u{36F}' | '\u{203F}'..='\u{2040}')
    }
    let mut it = s.chars();
    it.next().is_some_and(start) && it.all(rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_names() {
        assert!(is_xml_name("foo-bar.baz:qux"));
        assert!(is_xml_name("_a1"));
        assert!(!is_xml_name("1a"));
        assert!(!is_xml_name("a\"b"));
        assert!(!is_xml_name(""));
    }
}
