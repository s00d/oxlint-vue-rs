//! Helpers for the deprecation / template-structure / misc template rules (`no-deprecated-*`,
//! `no-unsupported-features`, `no-useless-*`, class rules, …).

use oxc::ast::AstKind;
use oxc::ast::ast::*;
use oxc::semantic::Semantic;
use oxc::span::{GetSpan, Span};

use crate::ast::*;
use crate::rule::{Ctx, Fix};
use crate::utils::js::{self, skip_parens};
use crate::utils::template::{child_element_chains, directive_key, is_custom_component};
use crate::utils::tpl_valid::{body_elements, equal_tokens, same_type};

/// Every attribute of every template-body element (root `<template>` included), in document order.
pub fn body_attributes<'d>(doc: &'d Document) -> Vec<(&'d Element, &'d Attribute)> {
    body_elements(doc)
        .into_iter()
        .flat_map(|e| e.attributes().iter().map(move |a| (e, a)))
        .collect()
}

/// Template-body expression containers in document order.
pub fn body_exprs<'d, 'a>(doc: &'d Document<'a>) -> impl Iterator<Item = &'d ExprContainer<'a>> {
    crate::utils::template::template_expr_containers(doc)
}

/// `utils.isThis(node, context)`: `this`, or an identifier bound once by `const x = this`.
pub fn is_this(sem: &Semantic, e: &Expression) -> bool {
    match skip_parens(e) {
        Expression::ThisExpression(_) => true,
        Expression::Identifier(id) => {
            let Some(sym) = js::ref_symbol(sem, id) else {
                return false;
            };
            let Some(AstKind::VariableDeclarator(d)) = js::single_declaration(sem, sym) else {
                return false;
            };
            js::declarator_kind(sem, js::declaration_id(sem, sym))
                == Some(VariableDeclarationKind::Const)
                && matches!(d.id, BindingPattern::BindingIdentifier(_))
                && d.init
                    .as_ref()
                    .is_some_and(|i| matches!(skip_parens(i), Expression::ThisExpression(_)))
        }
        _ => false,
    }
}

/// Script-side `this.<name>` member properties inside a Vue object (`defineVueVisitor` +
/// `MemberExpression` with an `Identifier` property): spans of the property identifiers.
pub fn this_member_props(ctx: &Ctx, name: &str) -> Vec<Span> {
    let file = ctx.file;
    let Some(sem) = file.semantic else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for n in sem.nodes().iter() {
        let (object, prop) = match n.kind() {
            AstKind::StaticMemberExpression(m) if m.property.name == name => {
                (&m.object, m.property.span)
            }
            AstKind::ComputedMemberExpression(m) => match &m.expression {
                Expression::Identifier(id) if id.name == name => (&m.object, id.span),
                _ => continue,
            },
            _ => continue,
        };
        if is_this(sem, object)
            && crate::utils::vue_component::enclosing_vue_object(file, n.id()).is_some()
        {
            out.push(prop);
        }
    }
    out
}

/// JS `Number(str)` (string → number conversion); `None` = NaN.
pub fn js_to_number(s: &str) -> Option<f64> {
    let t = s.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    if t.is_empty() {
        return Some(0.0);
    }
    let radix = |p: &str, r: u32| {
        if p.is_empty() {
            return None;
        }
        u128::from_str_radix(p, r).ok().map(|v| v as f64)
    };
    let lower = t.to_ascii_lowercase();
    if let Some(p) = lower.strip_prefix("0x") {
        return radix(p, 16);
    }
    if let Some(p) = lower.strip_prefix("0b") {
        return radix(p, 2);
    }
    if let Some(p) = lower.strip_prefix("0o") {
        return radix(p, 8);
    }
    match t {
        "Infinity" | "+Infinity" => return Some(f64::INFINITY),
        "-Infinity" => return Some(f64::NEG_INFINITY),
        _ => {}
    }
    if !t
        .bytes()
        .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'+' | b'-'))
    {
        return None;
    }
    t.parse::<f64>().ok()
}

/// `Number.isSafeInteger(n)`.
pub fn is_safe_integer(n: f64) -> bool {
    n.is_finite() && n.trunc() == n && n.abs() <= 9_007_199_254_740_991.0
}

/// The "useful escape" scan of `no-useless-mustaches` / `no-useless-v-bind`.
pub fn has_useful_escape(raw: &str, cooked: &str) -> bool {
    if raw == cooked {
        return false;
    }
    let mut it = raw.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                None => return true,
                Some(n) if "nrvtbfux".contains(n) => return true,
                _ => {}
            }
        }
    }
    false
}

/// Raw value (between the quotes) of a string literal / expression-less template literal.
pub fn string_raw<'a>(doc: &Document<'a>, e: &Expression) -> Option<&'a str> {
    match skip_parens(e) {
        Expression::StringLiteral(s) => {
            let t = doc.slice(s.span);
            Some(&t[1..t.len() - 1])
        }
        Expression::TemplateLiteral(t) if t.expressions.is_empty() => {
            let q = t.quasis.first()?;
            Some(doc.slice(q.span))
        }
        _ => None,
    }
}

/// Whether the template tokens of `span` include a JS comment.
pub fn has_js_comment(doc: &Document, span: Span) -> bool {
    doc.comments.iter().any(|c| {
        matches!(c.kind, TokenKind::LineComment | TokenKind::BlockComment)
            && span.start <= c.span.start
            && c.span.end <= span.end
    })
}

/// Template token before `pos` (no whitespace / comments).
pub fn token_before<'d>(doc: &'d Document, pos: u32) -> Option<&'d Token> {
    let i = doc.tokens.partition_point(|t| t.span.end <= pos);
    doc.tokens[..i].iter().rev().find(|t| {
        !matches!(
            t.kind,
            TokenKind::HTMLWhitespace
                | TokenKind::HTMLComment
                | TokenKind::LineComment
                | TokenKind::BlockComment
        )
    })
}

/// Template tokens after `pos` (no whitespace / comments).
pub fn tokens_after<'d>(doc: &'d Document, pos: u32) -> impl Iterator<Item = &'d Token> {
    let i = doc.tokens.partition_point(|t| t.span.start < pos);
    doc.tokens[i..].iter().filter(|t| {
        !matches!(
            t.kind,
            TokenKind::HTMLWhitespace
                | TokenKind::HTMLComment
                | TokenKind::LineComment
                | TokenKind::BlockComment
        )
    })
}

/// Static argument name of a directive key (`key.argument.name`).
pub fn static_arg(k: &DirectiveKey) -> Option<&VIdentifier> {
    match &k.argument {
        Some(DirectiveArgument::Static(id)) => Some(id),
        _ => None,
    }
}

/// `getKeyName(attr)` of `no-lone-template` / `no-useless-template-attributes`.
pub fn template_key_name(a: &Attribute) -> Option<&str> {
    match &a.key {
        AttributeKey::Plain(k) => Some(&k.name),
        AttributeKey::Directive(k) if k.name.name == "bind" => {
            static_arg(k).map(|i| i.name.as_str())
        }
        AttributeKey::Directive(_) => None,
    }
}

/// `SPECIAL_TEMPLATE_DIRECTIVES` + `slot-scope` / `scope` / `slot` key of a `<template>` attribute.
pub fn is_fragment_template_attribute(a: &Attribute) -> bool {
    if let Some(k) = directive_key(a)
        && matches!(
            k.name.name.as_str(),
            "slot-scope" | "scope" | "if" | "else" | "else-if" | "for" | "slot"
        )
    {
        return true;
    }
    template_key_name(a) == Some("slot")
}

// ---------------------------------------------------------------------------------------------
// `syntaxes/utils/can-convert-to-v-slot.ts`
// ---------------------------------------------------------------------------------------------

type VForVars<'d, 'a> = (&'d ExprContainer<'a>, Vec<(String, Span)>);

struct SlotCtx<'d, 'a> {
    element: ElementId,
    has_vfor: bool,
    /// (v-for expression container, used variables `(name, span)`).
    vfor_vars: Option<VForVars<'d, 'a>>,
    normalized: String,
}

fn slot_ctx<'d, 'a>(doc: &'d Document<'a>, e: &Element) -> SlotCtx<'d, 'a> {
    let slot = e.attr("slot").or_else(|| e.directive("bind", Some("slot")));
    let vfor = e.directive("for", None);
    let mut vfor_vars = None;
    if let Some(s) = slot
        && s.is_directive()
        && let Some(vf) = vfor
        && let Some(AttributeValue::Expr(vid)) = &vf.value
        && doc.expr(*vid).expression.is_some()
    {
        let mut vars = Vec::new();
        if let Some(AttributeValue::Expr(sid)) = &s.value {
            for r in &doc.expr(*sid).references {
                if let Some((eid, idx)) = r.variable {
                    let v = &doc.element(eid).variables[idx as usize];
                    let st = e.start_tag.span;
                    if v.kind == VariableKind::VFor
                        && v.span.start > st.start
                        && v.span.end < st.end
                    {
                        vars.push((v.name.clone(), v.span));
                    }
                }
            }
        }
        if !vars.is_empty() {
            vfor_vars = Some((doc.expr(*vid), vars));
        }
    }
    let normalized = match slot {
        None => "default".to_string(),
        Some(a) => match (&a.key, &a.value) {
            (AttributeKey::Plain(_), Some(AttributeValue::Literal(l))) => l.value.clone(),
            (AttributeKey::Plain(_), _) => "default".to_string(),
            (_, Some(AttributeValue::Expr(id))) => format!("[{}]", doc.slice(doc.expr(*id).span)),
            (_, _) => "[null]".to_string(),
        },
    };
    SlotCtx {
        element: e.id,
        has_vfor: vfor.is_some(),
        vfor_vars,
        normalized,
    }
}

#[allow(clippy::type_complexity)]
fn vfor_parts<'d, 'a>(
    c: &'d ExprContainer<'a>,
) -> Option<(
    &'d oxc::allocator::Vec<'a, Option<BindingPattern<'a>>>,
    &'d Expression<'a>,
)> {
    match &c.expression {
        Some(VExpression::For { left, right, .. }) => Some((left, right)),
        _ => None,
    }
}

fn slot_vfor_vars_equal(doc: &Document, a: &VForVars, b: &VForVars) -> bool {
    let (Some((al, ar)), Some((bl, br))) = (vfor_parts(a.0), vfor_parts(b.0)) else {
        return false;
    };
    if a.1.len() != b.1.len()
        || !(same_type(ar, br)
            && equal_tokens(doc, js::skip_parens(ar).span(), js::skip_parens(br).span()))
    {
        return false;
    }
    let mut checked: Vec<&str> = Vec::new();
    for (ap, bp) in al.iter().zip(bl.iter()) {
        let find = |p: &Option<BindingPattern>, vars: &'_ [(String, Span)]| -> Option<usize> {
            let p = p.as_ref()?;
            vars.iter().position(|(_, s)| js::in_range(p.span(), *s))
        };
        let (av, bv) = (find(ap, &a.1), find(bp, &b.1));
        match (av, bv) {
            (Some(x), Some(y)) => {
                let (ap, bp) = (ap.as_ref().unwrap(), bp.as_ref().unwrap());
                if a.1[x].0 != b.1[y].0
                    || std::mem::discriminant(ap) != std::mem::discriminant(bp)
                    || !equal_tokens(doc, ap.span(), bp.span())
                {
                    return false;
                }
                checked.push(&a.1[x].0);
            }
            (None, None) => {}
            _ => return false,
        }
    }
    a.1.iter()
        .all(|(n, _)| checked.contains(&n.as_str()) || b.1.iter().any(|(m, _)| m == n))
}

/// `canConvertToVSlot(element)`.
pub fn can_convert_to_v_slot(doc: &Document, e: &Element) -> bool {
    let Some(owner) = e.parent.map(|p| doc.element(p)) else {
        return false;
    };
    if !is_custom_component(owner, false) || owner.name == "component" {
        return false;
    }
    let target = slot_ctx(doc, e);
    if target.has_vfor && target.vfor_vars.is_none() {
        return false;
    }
    for group in child_element_chains(doc, &owner.children) {
        if group.contains(&target.element) {
            continue;
        }
        for c in group {
            let s = slot_ctx(doc, doc.element(c));
            let same = match (&target.vfor_vars, &s.vfor_vars) {
                (Some(a), Some(b)) => slot_vfor_vars_equal(doc, a, b),
                (None, None) => target.normalized == s.normalized,
                _ => false,
            };
            if same {
                return false;
            }
        }
    }
    true
}

/// Text of a fix that removes an attribute (`fixer.remove(attr)`).
pub fn remove(a: &Attribute) -> Fix {
    Fix::remove(a.span)
}

/// JS `s.split(/(\s+)/)`: pieces with the whitespace runs kept (`""` at the ends when the string
/// starts / ends with whitespace).
pub fn js_split_ws(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut ws = false;
    for c in s.chars() {
        let w = c.is_whitespace();
        if w != ws {
            out.push(std::mem::take(&mut cur));
            ws = w;
        }
        cur.push(c);
    }
    out.push(cur);
    if out.len() > 1 && out[0].is_empty() && !ws && s.is_empty() {
        out.truncate(1);
    }
    out
}

/// ESTree (espree) range of `tl.quasis[i]`: includes the delimiters (`` ` `` / `}` before,
/// `` ` `` / `${` after).
pub fn quasi_span(tl: &TemplateLiteral, i: usize) -> Span {
    let q = &tl.quasis[i];
    Span::new(
        q.span.start.saturating_sub(1),
        q.span.end + if q.tail { 1 } else { 2 },
    )
}

/// `v-bind()` calls in top-level `<style>` blocks (vue-eslint-parser `src/style`): the spans of
/// `v-bind( … )` (the `VExpressionContainer`s of `getStyleVariablesContext().vBinds`).
pub fn style_v_binds(doc: &Document) -> Vec<Span> {
    #[derive(PartialEq)]
    enum K {
        Word,
        Punct(u8),
        Quoted,
        Comment,
    }
    let mut out = Vec::new();
    for style in doc.top_level().filter(|e| e.name == "style") {
        let [Child::Text(t)] = style.children.as_slice() else {
            continue;
        };
        let base = doc.text(*t).span.start;
        let code = doc.slice(doc.text(*t).span);
        let inline =
            crate::utils::template::literal_value_of(style, "lang").is_some_and(|l| l != "css");
        let b = code.as_bytes();
        let punct = |c: u8| {
            matches!(
                c,
                b':' | b';' | b'(' | b')' | b'{' | b'}' | b'[' | b']' | b'/' | b'*'
            )
        };
        let mut toks: Vec<(K, usize, usize)> = Vec::new();
        let mut i = 0;
        while i < b.len() {
            let c = b[i];
            if c.is_ascii_whitespace() {
                i += 1;
                continue;
            }
            let s = i;
            if c == b'/' && b.get(i + 1) == Some(&b'*') {
                i += 2;
                while i < b.len() && !(b[i] == b'*' && b.get(i + 1) == Some(&b'/')) {
                    i += 1;
                }
                i = (i + 2).min(b.len());
                toks.push((K::Comment, s, i));
            } else if c == b'/' && inline && b.get(i + 1) == Some(&b'/') {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
                toks.push((K::Comment, s, i));
            } else if c == b'"' || c == b'\'' {
                i += 1;
                while i < b.len() && b[i] != c {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
                i = (i + 1).min(b.len());
                toks.push((K::Quoted, s, i));
            } else if punct(c) {
                i += 1;
                toks.push((K::Punct(c), s, i));
            } else {
                while i < b.len()
                    && !b[i].is_ascii_whitespace()
                    && !punct(b[i])
                    && b[i] != b'"'
                    && b[i] != b'\''
                {
                    i += 1;
                }
                toks.push((K::Word, s, i));
            }
        }
        let mut j = 0;
        while j < toks.len() {
            let (k, s, e) = &toks[j];
            if *k != K::Word || &code[*s..*e] != "v-bind" {
                j += 1;
                continue;
            }
            let mut p = j + 1;
            while p < toks.len() && toks[p].0 == K::Comment {
                p += 1;
            }
            if p >= toks.len() || toks[p].0 != K::Punct(b'(') {
                j += 1;
                continue;
            }
            let mut depth = 0usize;
            let mut close = None;
            for (q, t) in toks.iter().enumerate().skip(p + 1) {
                match t.0 {
                    K::Punct(b')') if depth == 0 => {
                        close = Some(q);
                        break;
                    }
                    K::Punct(b')') => depth -= 1,
                    K::Punct(b'(') => depth += 1,
                    _ => {}
                }
            }
            match close {
                Some(q) => {
                    out.push(Span::new(base + *s as u32, base + toks[q].2 as u32));
                    j = q + 1;
                }
                None => j += 1,
            }
        }
    }
    out
}
