//! Template helpers (ports of the template parts of eslint-plugin-vue `lib/utils/index.ts`)
//! and the shared name lists from `lib/utils/*.json`.

use std::collections::HashSet;
use std::sync::LazyLock;

use oxc::ast::ast::Expression;

use crate::LintFile;
use crate::ast::*;

fn set(json: &str) -> HashSet<String> {
    serde_json::from_str::<Vec<String>>(json)
        .expect("valid name list")
        .into_iter()
        .collect()
}

macro_rules! name_set {
    ($id:ident, $file:literal) => {
        pub static $id: LazyLock<HashSet<String>> =
            LazyLock::new(|| set(include_str!(concat!("data/", $file))));
    };
}

name_set!(HTML_ELEMENT_NAMES, "html-elements.json");
name_set!(SVG_ELEMENT_NAMES, "svg-elements.json");
name_set!(MATH_ELEMENT_NAMES, "math-elements.json");
name_set!(VOID_ELEMENT_NAMES, "void-elements.json");
name_set!(
    DEPRECATED_HTML_ELEMENT_NAMES,
    "deprecated-html-elements.json"
);
name_set!(
    INLINE_NON_VOID_ELEMENT_NAMES,
    "inline-non-void-elements.json"
);
name_set!(SVG_ATTRIBUTES_WEIRD_CASE, "svg-attributes-weird-case.json");
name_set!(DOM_EVENTS, "dom-events.json");
name_set!(KEY_ALIASES, "key-aliases.json");
name_set!(JS_RESERVED, "js-reserved.json");
name_set!(VUE3_EXPORT_NAMES, "vue3-export-names.json");

/// `vue-reserved.json`, `vue-component-options.json` are objects, exposed raw.
pub static VUE_RESERVED: LazyLock<serde_json::Value> =
    LazyLock::new(|| serde_json::from_str(include_str!("data/vue-reserved.json")).unwrap());
pub static VUE_COMPONENT_OPTIONS: LazyLock<serde_json::Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("data/vue-component-options.json")).unwrap()
});

pub const VUE2_BUILTIN_COMPONENT_NAMES: &[&str] = &[
    "template",
    "slot",
    "component",
    "Component",
    "transition",
    "Transition",
    "transition-group",
    "TransitionGroup",
    "keep-alive",
    "KeepAlive",
];
pub const VUE3_BUILTIN_COMPONENT_NAMES: &[&str] = &[
    "template",
    "slot",
    "component",
    "Component",
    "transition",
    "Transition",
    "transition-group",
    "TransitionGroup",
    "keep-alive",
    "KeepAlive",
    "teleport",
    "Teleport",
    "suspense",
    "Suspense",
];
pub const VUE_BUILTIN_ELEMENT_NAMES: &[&str] = &["template", "slot", "component"];
pub const BUILTIN_DIRECTIVE_NAMES: &[&str] = &[
    "bind", "on", "text", "html", "show", "if", "else", "else-if", "for", "model", "slot", "pre",
    "cloak", "once", "memo", "is",
];

pub fn is_html_well_known_element_name(name: &str) -> bool {
    HTML_ELEMENT_NAMES.contains(name)
}
pub fn is_svg_well_known_element_name(name: &str) -> bool {
    SVG_ELEMENT_NAMES.contains(name)
}
pub fn is_math_well_known_element_name(name: &str) -> bool {
    MATH_ELEMENT_NAMES.contains(name)
}
pub fn is_html_void_element_name(name: &str) -> bool {
    VOID_ELEMENT_NAMES.contains(name)
}
pub fn is_builtin_component_name(name: &str) -> bool {
    VUE3_BUILTIN_COMPONENT_NAMES.contains(&name) || VUE2_BUILTIN_COMPONENT_NAMES.contains(&name)
}
pub fn is_vue_builtin_element_name(name: &str) -> bool {
    VUE_BUILTIN_ELEMENT_NAMES.contains(&name.to_ascii_lowercase().as_str())
}
pub fn is_builtin_directive_name(name: &str) -> bool {
    BUILTIN_DIRECTIVE_NAMES.contains(&name)
}

pub fn is_html_element(e: &Element) -> bool {
    e.namespace == Namespace::Html
}
pub fn is_svg_element(e: &Element) -> bool {
    e.namespace == Namespace::Svg
}
pub fn is_math_element(e: &Element) -> bool {
    e.namespace == Namespace::MathMl
}

/// `utils.isCustomComponent(node, shouldIgnoreElementNamespaces)`.
pub fn is_custom_component(e: &Element, ignore_namespaces: bool) -> bool {
    if e.attr("is").is_some() || e.directive("bind", Some("is")).is_some() || e.has_directive("is")
    {
        return true;
    }
    let html = is_html_well_known_element_name(&e.raw_name);
    let svg = is_svg_well_known_element_name(&e.raw_name);
    let math = is_math_well_known_element_name(&e.raw_name);
    if ignore_namespaces {
        !html && !svg && !math
    } else {
        (is_html_element(e) && !html)
            || (is_svg_element(e) && !svg)
            || (is_math_element(e) && !math)
    }
}

/// Element children of the element's parent (or of the fragment).
pub fn sibling_elements<'d>(doc: &'d Document, e: &Element) -> Vec<&'d Element> {
    let children = match e.parent {
        Some(p) => &doc.element(p).children,
        None => &doc.children,
    };
    children
        .iter()
        .filter_map(|c| {
            if let Child::Element(id) = c {
                Some(doc.element(*id))
            } else {
                None
            }
        })
        .collect()
}

/// `utils.prevSibling(node)`: previous sibling *element* (text/mustache ignored).
pub fn prev_sibling<'d>(doc: &'d Document, e: &Element) -> Option<&'d Element> {
    let sib = sibling_elements(doc, e);
    let i = sib.iter().position(|s| s.id == e.id)?;
    i.checked_sub(1).map(|j| sib[j])
}

/// `utils.prevElementHasIf(node)`.
pub fn prev_element_has_if(doc: &Document, e: &Element) -> bool {
    prev_sibling(doc, e).is_some_and(|p| p.has_directive("if") || p.has_directive("else-if"))
}

/// `utils.iterateChildElementsChains(node)`: groups of children connected by v-if / v-else-if / v-else.
pub fn child_element_chains(doc: &Document, children: &[Child]) -> Vec<Vec<ElementId>> {
    let mut out = Vec::new();
    let mut chain: Vec<ElementId> = Vec::new();
    let mut is_vif = false;
    for c in children {
        match c {
            Child::Element(id) => {
                let el = doc.element(*id);
                let connected = if el.has_directive("if") {
                    is_vif = true;
                    false
                } else if el.has_directive("else-if") {
                    let c = is_vif;
                    is_vif = true;
                    c
                } else if el.has_directive("else") {
                    let c = is_vif;
                    is_vif = false;
                    c
                } else {
                    is_vif = false;
                    false
                };
                if connected {
                    chain.push(*id);
                } else {
                    if !chain.is_empty() {
                        out.push(std::mem::take(&mut chain));
                    }
                    chain.push(*id);
                }
            }
            Child::Text(t) if doc.text(*t).value.trim().is_empty() => {}
            _ => is_vif = false,
        }
    }
    if !chain.is_empty() {
        out.push(chain);
    }
    out
}

/// Directive key of an attribute, if it is a directive.
pub fn directive_key(a: &Attribute) -> Option<&DirectiveKey> {
    match &a.key {
        AttributeKey::Directive(k) => Some(k),
        AttributeKey::Plain(_) => None,
    }
}

/// Expression container of a directive value.
pub fn directive_value<'d, 'a>(
    doc: &'d Document<'a>,
    a: &Attribute,
) -> Option<&'d ExprContainer<'a>> {
    match &a.value {
        Some(AttributeValue::Expr(id)) => Some(doc.expr(*id)),
        _ => None,
    }
}

/// The plain JS expression of a directive value (`None` for empty, v-for, v-on statements, …).
pub fn directive_expression<'d, 'a>(
    doc: &'d Document<'a>,
    a: &Attribute,
) -> Option<&'d Expression<'a>> {
    match &directive_value(doc, a)?.expression {
        Some(VExpression::Expression(e)) => Some(e),
        _ => None,
    }
}

/// Literal value of a plain attribute (`None` when valueless).
pub fn literal_value(a: &Attribute) -> Option<&str> {
    match &a.value {
        Some(AttributeValue::Literal(l)) => Some(&l.value),
        _ => None,
    }
}

/// Span of the attribute's value node (literal or expression container).
pub fn value_span(doc: &Document, a: &Attribute) -> Option<oxc::span::Span> {
    match &a.value {
        Some(AttributeValue::Literal(l)) => Some(l.span),
        Some(AttributeValue::Expr(id)) => Some(doc.expr(*id).span),
        None => None,
    }
}

/// `utils.isEmptyValueDirective`: directive with `=""` (value present, no expression, empty text).
pub fn is_empty_value_directive(doc: &Document, a: &Attribute) -> bool {
    let Some(c) = directive_value(doc, a) else {
        return false;
    };
    if c.expression.is_some() {
        return false;
    }
    let mut t = doc.slice(c.span);
    let b = t.as_bytes();
    if b.len() >= 2 && (b[0] == b'"' || b[0] == b'\'') && b[0] == b[b.len() - 1] {
        t = &t[1..t.len() - 1];
    }
    t.is_empty()
}

/// `utils.isEmptyExpressionValueDirective`: directive whose value holds only whitespace/comments.
pub fn is_empty_expression_value_directive(doc: &Document, a: &Attribute) -> bool {
    let Some(c) = directive_value(doc, a) else {
        return false;
    };
    if c.expression.is_some() {
        return false;
    }
    let mut quote: Option<&str> = None;
    let mut closed = false;
    for t in doc.tokens_in(a.span) {
        if t.span.end <= c.span.start {
            continue;
        }
        if c.span.end <= t.span.start {
            return true;
        }
        let txt = doc.slice(t.span);
        if t.kind == TokenKind::Punctuator && (txt == "\"" || txt == "'") {
            if quote.is_none() {
                quote = Some(txt);
                continue;
            }
            if !closed && quote == Some(txt) {
                closed = true;
                continue;
            }
        }
        if matches!(
            t.kind,
            TokenKind::LineComment | TokenKind::BlockComment | TokenKind::HTMLWhitespace
        ) {
            continue;
        }
        return false;
    }
    true
}

/// `utils.isVBindSameNameShorthand`: `:foo` (Vue 3.4 same-name shorthand, value inside the key).
pub fn is_vbind_same_name_shorthand(doc: &Document, a: &Attribute) -> bool {
    let Some(k) = directive_key(a) else {
        return false;
    };
    if k.name.name != "bind" {
        return false;
    }
    let Some(c) = directive_value(doc, a) else {
        return false;
    };
    matches!(
        &c.expression,
        Some(VExpression::Expression(Expression::Identifier(_)))
    ) && k.span.start <= c.span.start
        && c.span.end <= k.span.end
}

/// All elements of the template body in pre-order (root `<template>` excluded); empty without a template.
pub fn template_elements<'d>(file: &'d LintFile) -> Vec<&'d Element> {
    match &file.doc {
        Some(doc) => doc
            .template_elements()
            .into_iter()
            .map(|id| doc.element(id))
            .collect(),
        None => Vec::new(),
    }
}

/// Every expression container that belongs to the template body (mustaches, directive values, dynamic args).
pub fn template_expr_containers<'d, 'a>(
    doc: &'d Document<'a>,
) -> impl Iterator<Item = &'d ExprContainer<'a>> {
    doc.exprs.iter().filter(move |c| match c.parent {
        ExprParent::Element(Some(e))
        | ExprParent::DirectiveValue(e, _)
        | ExprParent::DirectiveArgument(e, _) => doc.in_template(e),
        ExprParent::Element(None) => false,
    })
}

/// Literal value of the plain attribute `name` of an element.
pub fn literal_value_of<'e>(e: &'e Element, name: &str) -> Option<&'e str> {
    e.attr(name).and_then(literal_value)
}
