//! Generic script AST helpers over oxc (ports of the AST helpers in eslint-plugin-vue `lib/utils/index.ts`).
//!
//! ESTree has no `ParenthesizedExpression`; oxc keeps them, so every "skip" helper here also
//! looks through parentheses to keep ESLint's view of the tree.

use oxc::ast::AstKind;
use oxc::ast::ast::*;
use oxc::semantic::{NodeId, Semantic, SymbolId};
use oxc::span::{GetSpan, Span};

use crate::rule::LintFile;

/// Strip parentheses.
pub fn skip_parens<'b, 'a>(mut e: &'b Expression<'a>) -> &'b Expression<'a> {
    while let Expression::ParenthesizedExpression(p) = e {
        e = &p.expression;
    }
    e
}

/// `skipTSAsExpression` (+ parentheses).
pub fn skip_ts_as<'b, 'a>(mut e: &'b Expression<'a>) -> &'b Expression<'a> {
    loop {
        match e {
            Expression::ParenthesizedExpression(p) => e = &p.expression,
            Expression::TSAsExpression(a) => e = &a.expression,
            _ => return e,
        }
    }
}

/// An argument as an expression (`None` for a spread).
pub fn arg_expr<'b, 'a>(a: &'b Argument<'a>) -> Option<&'b Expression<'a>> {
    a.as_expression()
}

/// Number formatting of JS `String(number)` for the common cases.
pub fn js_number(n: f64) -> String {
    if n.is_nan() {
        "NaN".into()
    } else if n.is_infinite() {
        if n > 0.0 {
            "Infinity".into()
        } else {
            "-Infinity".into()
        }
    } else if n == n.trunc() && n.abs() < 1e21 {
        format!("{}", n as i128)
    } else {
        format!("{n}")
    }
}

/// `getStringLiteralValue(node)`: string literal, expression-less template literal (cooked),
/// or `String(value)` of another literal (numbers, booleans, bigint, regexp). `null` → `None`.
pub fn string_literal_value(e: &Expression) -> Option<String> {
    match skip_parens(e) {
        Expression::StringLiteral(s) => Some(s.value.display().to_string()),
        Expression::TemplateLiteral(t) => template_value(t),
        Expression::NumericLiteral(n) => Some(js_number(n.value)),
        Expression::BooleanLiteral(b) => Some(b.value.to_string()),
        Expression::BigIntLiteral(b) => Some(b.value.to_string()),
        Expression::RegExpLiteral(r) => Some(r.regex.to_string()),
        _ => None,
    }
}

/// Cooked value of a template literal without expressions.
pub fn template_value(t: &TemplateLiteral) -> Option<String> {
    if t.expressions.is_empty() && t.quasis.len() == 1 {
        t.quasis[0].value.cooked.map(|c| c.display().to_string())
    } else {
        None
    }
}

/// `isStringLiteral(node)`: a string `Literal` or a `TemplateLiteral` without expressions.
pub fn is_string_literal(e: &Expression) -> bool {
    match skip_parens(e) {
        Expression::StringLiteral(_) => true,
        Expression::TemplateLiteral(t) => t.expressions.is_empty(),
        _ => false,
    }
}

/// Static name of a property key (`getStaticPropertyName` for a key): a non-computed identifier,
/// or a literal / expression-less template literal (computed or not).
pub fn key_name(key: &PropertyKey) -> Option<String> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
        PropertyKey::PrivateIdentifier(_) => None,
        _ => key.as_expression().and_then(string_literal_value),
    }
}

/// `getStaticPropertyName(property)`.
pub fn prop_name(p: &ObjectProperty) -> Option<String> {
    key_name(&p.key)
}

/// `getStaticPropertyName(property) || (key is Identifier && key.name)`: also names computed `[foo]`.
pub fn prop_name_or_ident(p: &ObjectProperty) -> Option<String> {
    prop_name(p).or_else(|| match &p.key {
        PropertyKey::Identifier(id) => Some(id.name.to_string()),
        _ => None,
    })
}

/// `getStaticPropertyName(memberExpression)`.
pub fn member_name(m: &MemberExpression) -> Option<String> {
    match m {
        MemberExpression::StaticMemberExpression(s) => Some(s.property.name.to_string()),
        MemberExpression::ComputedMemberExpression(c) => string_literal_value(&c.expression),
        MemberExpression::PrivateFieldExpression(_) => None,
    }
}

/// `getStaticPropertyName(bindingProperty)` for object patterns.
pub fn binding_prop_name(p: &BindingProperty) -> Option<String> {
    key_name(&p.key)
}

/// Properties (not spreads) of an object expression.
pub fn properties<'b, 'a>(
    obj: &'b ObjectExpression<'a>,
) -> impl Iterator<Item = &'b ObjectProperty<'a>> {
    obj.properties.iter().filter_map(|p| match p {
        ObjectPropertyKind::ObjectProperty(p) => Some(&**p),
        ObjectPropertyKind::SpreadProperty(_) => None,
    })
}

/// `findProperty(obj, name)`.
pub fn find_property<'b, 'a>(
    obj: &'b ObjectExpression<'a>,
    name: &str,
) -> Option<&'b ObjectProperty<'a>> {
    properties(obj).find(|p| prop_name(p).as_deref() == Some(name))
}

/// `findAssignmentProperty(pattern, name)`.
pub fn find_binding_property<'b, 'a>(
    pat: &'b ObjectPattern<'a>,
    name: &str,
) -> Option<&'b BindingProperty<'a>> {
    pat.properties
        .iter()
        .find(|p| binding_prop_name(p).as_deref() == Some(name))
}

/// Span of an object member (property or spread).
pub fn member_span(p: &ObjectPropertyKind) -> Span {
    p.span()
}

/// Whether `inner` lies within `outer` (`isInRange`).
pub fn in_range(outer: Span, inner: Span) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}

/// Parent kind of a node, looking through parentheses and `TSAsExpression` (`getParent`).
pub fn parent_skip_ts_as<'a>(sem: &Semantic<'a>, id: NodeId) -> (NodeId, AstKind<'a>) {
    let nodes = sem.nodes();
    let mut cur = nodes.parent_id(id);
    loop {
        match nodes.kind(cur) {
            AstKind::ParenthesizedExpression(_) | AstKind::TSAsExpression(_) => {
                cur = nodes.parent_id(cur)
            }
            k => return (cur, k),
        }
    }
}

/// Parent kind of a node, looking through parentheses only (ESTree parent).
pub fn parent<'a>(sem: &Semantic<'a>, id: NodeId) -> (NodeId, AstKind<'a>) {
    let nodes = sem.nodes();
    let mut cur = nodes.parent_id(id);
    while let AstKind::ParenthesizedExpression(_) = nodes.kind(cur) {
        cur = nodes.parent_id(cur);
    }
    (cur, nodes.kind(cur))
}

/// Symbol an identifier reference resolves to.
pub fn ref_symbol(sem: &Semantic, id: &IdentifierReference) -> Option<SymbolId> {
    sem.scoping().get_reference(id.reference_id()).symbol_id()
}

/// Declaration node of a symbol, only when it has a single definition (`variable.defs.length === 1`).
pub fn single_declaration<'a>(sem: &Semantic<'a>, sym: SymbolId) -> Option<AstKind<'a>> {
    let s = sem.scoping();
    if !s.symbol_redeclarations(sym).is_empty() {
        return None;
    }
    Some(sem.nodes().kind(s.symbol_declaration(sym)))
}

/// Node id of the declaration of a symbol.
pub fn declaration_id(sem: &Semantic, sym: SymbolId) -> NodeId {
    sem.scoping().symbol_declaration(sym)
}

/// The `VariableDeclaration` that owns a `VariableDeclarator` node.
pub fn declarator_kind(sem: &Semantic, declarator: NodeId) -> Option<VariableDeclarationKind> {
    match sem.nodes().parent_kind(declarator) {
        AstKind::VariableDeclaration(d) => Some(d.kind),
        _ => None,
    }
}

/// `getObjectOrArray`: the object/array literal itself, or the init of a single `const x = …`.
pub enum ObjectOrArray<'a> {
    Object(&'a ObjectExpression<'a>),
    Array(&'a ArrayExpression<'a>),
}

pub fn object_or_array<'a>(
    file: &LintFile<'a>,
    e: &'a Expression<'a>,
) -> Option<ObjectOrArray<'a>> {
    match skip_parens(e) {
        Expression::ObjectExpression(o) => Some(ObjectOrArray::Object(o)),
        Expression::ArrayExpression(a) => Some(ObjectOrArray::Array(a)),
        Expression::Identifier(id) => {
            let sem = file.semantic?;
            let sym = ref_symbol(sem, id)?;
            let decl = declaration_id(sem, sym);
            let AstKind::VariableDeclarator(d) = single_declaration(sem, sym)? else {
                return None;
            };
            if declarator_kind(sem, decl) != Some(VariableDeclarationKind::Const)
                || !d.id.is_binding_identifier()
            {
                return None;
            }
            object_or_array(file, d.init.as_ref()?)
        }
        _ => None,
    }
}

/// Levenshtein distance (`editDistance`), over UTF-16 units like JS.
pub fn edit_distance(a: &str, b: &str) -> usize {
    if a == b {
        return 0;
    }
    let a: Vec<u16> = a.encode_utf16().collect();
    let b: Vec<u16> = b.encode_utf16().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            cur[j] = if a[i - 1] == b[j - 1] {
                prev[j - 1]
            } else {
                prev[j].min(cur[j - 1]).min(prev[j - 1]) + 1
            };
        }
        prev = cur;
    }
    prev[b.len()]
}

/// JSON string literal (`JSON.stringify(str)`).
pub fn json_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_default()
}

/// Whether a function node (`Function` / arrow) — helper to get params of either.
pub fn function_params<'b, 'a>(kind: &AstKind<'a>) -> Option<&'b FormalParameters<'a>>
where
    'a: 'b,
{
    match kind {
        AstKind::Function(f) => Some(&f.params),
        AstKind::ArrowFunctionExpression(f) => Some(&f.params),
        _ => None,
    }
}

/// ESTree span of a binding pattern including its type annotation (typescript-eslint puts the
/// annotation inside the identifier's range), for a declarator id.
pub fn declarator_id_span(d: &VariableDeclarator) -> Span {
    let s = d.id.span();
    d.type_annotation
        .as_ref()
        .map_or(s, |t| Span::new(s.start, t.span.end))
}
