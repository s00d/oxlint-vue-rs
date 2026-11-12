//! vue/no-restricted-syntax (core `no-restricted-syntax` over the whole SFC document: template,
//! `<style>` `v-bind()` and the other top-level blocks; script contents are not visited).
//! ponytail: the `VDocumentFragment` root itself is not a node (selectors naming it never match).

use std::collections::HashMap;

use oxc::span::Span;

use crate::ast::{AttributeKey, AttributeValue, DirectiveArgument, Document};
use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::esquery::{self, Matcher, Selector, Val};
use crate::utils::indent::tree::{Builder, F, NONE, Tree, Ty, flag};
use crate::utils::tpl_core::{DOCUMENT, for_each_root};
use crate::utils::tpl_misc::js_to_number;

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let selectors = opts
        .options
        .iter()
        .filter_map(|o| {
            let (sel, msg) = match o.as_str() {
                Some(s) => (s, None),
                None => (
                    o.get("selector")?.as_str()?,
                    o.get("message")
                        .and_then(|m| m.as_str())
                        .filter(|m| !m.is_empty())
                        .map(String::from),
                ),
            };
            let message = msg.unwrap_or_else(|| format!("Using '{sel}' is not allowed."));
            Some((esquery::parse(sel), message))
        })
        .collect();
    Some(Box::new(NoRestrictedSyntax { selectors }))
}

struct NoRestrictedSyntax {
    selectors: Vec<(Vec<Selector>, String)>,
}

/// Template node properties read from the document, by range.
#[derive(Default)]
struct VProps {
    /// VElement / VIdentifier: (name, rawName).
    names: HashMap<(u32, u32), (String, String)>,
    /// VLiteral / VText: value.
    values: HashMap<(u32, u32), String>,
}

impl VProps {
    fn new(doc: &Document) -> Self {
        let mut p = VProps::default();
        let r = |s: Span| (s.start, s.end);
        for e in &doc.elements {
            p.names
                .insert(r(e.span), (e.name.clone(), e.raw_name.clone()));
            for a in e.attributes() {
                let mut ids = Vec::new();
                match &a.key {
                    AttributeKey::Plain(k) => ids.push(k),
                    AttributeKey::Directive(k) => {
                        ids.push(&k.name);
                        if let Some(DirectiveArgument::Static(i)) = &k.argument {
                            ids.push(i);
                        }
                        ids.extend(k.modifiers.iter().filter(|m| !m.raw_name.is_empty()));
                    }
                }
                for i in ids {
                    p.names
                        .insert(r(i.span), (i.name.clone(), i.raw_name.clone()));
                }
                if let Some(AttributeValue::Literal(l)) = &a.value {
                    p.values.insert(r(l.span), l.value.clone());
                }
            }
        }
        for t in &doc.texts {
            p.values.insert(r(t.span), t.value.clone());
        }
        p
    }
}

/// JS string literal value (common escapes).
fn unquote(raw: &str) -> String {
    let body = &raw[1..raw.len().saturating_sub(1).max(1)];
    let mut out = String::new();
    let mut it = body.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('b') => out.push('\u{8}'),
            Some('f') => out.push('\u{c}'),
            Some('v') => out.push('\u{b}'),
            Some('0') => out.push('\0'),
            Some('u') => {
                let hex: String = it.by_ref().take(4).collect();
                if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    out.push(ch);
                }
            }
            Some('\n') | None => {}
            Some(o) => out.push(o),
        }
    }
    out
}

fn leaf(tree: &Tree, src: &str, vp: &VProps, n: u32, name: &str) -> Val {
    let node = tree.node(n);
    let text = &src[node.start as usize..node.end as usize];
    let key = (node.start, node.end);
    let ident = |t: &str| {
        let t = t.trim_start_matches('#');
        t.chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
            .collect::<String>()
    };
    let child_range = |f: F| {
        let c = tree.get(n, f);
        (c != NONE).then(|| tree.range(c))
    };
    match (node.ty, name) {
        (Ty::Identifier | Ty::PrivateIdentifier, "name") => Val::Str(ident(text)),
        (Ty::Literal, "raw") => Val::Str(text.to_string()),
        (Ty::Literal, "value") => match text {
            "true" => Val::Bool(true),
            "false" => Val::Bool(false),
            "null" => Val::Null,
            t if t.starts_with(['"', '\'']) => Val::Str(unquote(t)),
            t if t.starts_with('/') => Val::Str(t.to_string()),
            t if t.ends_with('n') => Val::Str(t[..t.len() - 1].replace('_', "")),
            t => js_to_number(&t.replace('_', "")).map_or(Val::Undefined, Val::Num),
        },
        (
            Ty::BinaryExpression | Ty::LogicalExpression | Ty::AssignmentExpression,
            "operator",
        ) => {
            let (Some(l), Some(r)) = (child_range(F::Left), child_range(F::Right)) else {
                return Val::Undefined;
            };
            let between = &src[l.1 as usize..r.0 as usize];
            Val::Str(
                between
                    .chars()
                    .filter(|c| !c.is_whitespace() && *c != '(' && *c != ')')
                    .collect(),
            )
        }
        (Ty::UnaryExpression | Ty::UpdateExpression, "operator" | "prefix") => {
            let Some(a) = child_range(F::Argument) else {
                return Val::Undefined;
            };
            let pre: String = src[node.start as usize..a.0 as usize]
                .chars()
                .filter(|c| !c.is_whitespace() && *c != '(')
                .collect();
            let prefix = node.ty == Ty::UnaryExpression || !pre.is_empty();
            if name == "prefix" {
                return Val::Bool(prefix);
            }
            if prefix {
                Val::Str(pre)
            } else {
                Val::Str(
                    src[a.1 as usize..node.end as usize]
                        .chars()
                        .filter(|c| !c.is_whitespace() && *c != ')')
                        .collect(),
                )
            }
        }
        (Ty::VariableDeclaration, "kind") => Val::Str(ident(text)),
        (_, "computed") => Val::Bool(tree.has(n, flag::COMPUTED)),
        (_, "optional") => Val::Bool(tree.has(n, flag::OPTIONAL)),
        (_, "async") => Val::Bool(tree.has(n, flag::ASYNC)),
        (Ty::VAttribute, "directive") => {
            let k = tree.get(n, F::Key);
            Val::Bool(k != NONE && tree.ty(k) == Ty::VDirectiveKey)
        }
        (Ty::VElement | Ty::VIdentifier, "name") => vp
            .names
            .get(&key)
            .map_or(Val::Undefined, |v| Val::Str(v.0.clone())),
        (Ty::VElement | Ty::VIdentifier, "rawName") => vp
            .names
            .get(&key)
            .map_or(Val::Undefined, |v| Val::Str(v.1.clone())),
        (Ty::VLiteral | Ty::VText, "value") => vp
            .values
            .get(&key)
            .map_or(Val::Undefined, |v| Val::Str(v.clone())),
        _ => Val::Undefined,
    }
}

impl Rule for NoRestrictedSyntax {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        if self.selectors.is_empty() {
            return;
        }
        let Some(doc) = ctx.doc() else { return };
        let src = ctx.source();
        let mut b = Builder::new(src);
        let mut styles = Vec::new();
        for c in &doc.children {
            match c {
                crate::ast::Child::Element(e) => {
                    let id = b.element(doc, *e);
                    if doc.element(*e).name == "style" {
                        styles.push(id);
                    }
                }
                crate::ast::Child::Text(t) => {
                    b.leaf_node(Ty::VText, doc.text(*t).span);
                }
                crate::ast::Child::Expr(_) => {}
            }
        }
        let mut containers: Vec<(u32, Span)> = Vec::new();
        for_each_root(ctx, DOCUMENT, |_, root| {
            if !root.style {
                return;
            }
            let crate::ast::VExpression::Expression(x) = root.expr else {
                return;
            };
            let id = b.style_container(root.container, x);
            let parent = styles.iter().copied().find(|&s| {
                let (a, z) = b.t.range(s);
                a <= root.container.start && root.container.end <= z
            });
            if let Some(p) = parent {
                b.t.nodes[id as usize].parent = p;
                containers.push((p, root.container));
            }
        });
        // vue-eslint-parser splits the `<style>` text around each `v-bind()` container.
        for &s in &styles {
            let mut kids = Vec::new();
            b.t.children(s, &mut kids);
            let Some(&text) = kids.iter().find(|&&k| b.t.ty(k) == Ty::VText) else {
                continue;
            };
            let end = b.t.node(text).end;
            let mut cs: Vec<Span> = containers
                .iter()
                .filter(|(p, _)| *p == s)
                .map(|(_, c)| *c)
                .collect();
            cs.sort_by_key(|c| c.start);
            for (i, c) in cs.iter().enumerate() {
                if i == 0 {
                    b.t.nodes[text as usize].end = c.start;
                }
                let next = cs.get(i + 1).map_or(end, |n| n.start);
                let id = b.leaf_node(Ty::VText, Span::new(c.end, next));
                b.t.nodes[id as usize].parent = s;
            }
        }
        let tree = b.t;
        let vp = VProps::new(doc);
        let leaf_fn = |n: u32, name: &str| leaf(&tree, src, &vp, n, name);
        let m = Matcher {
            tree: &tree,
            leaf: &leaf_fn,
        };
        // ESTree visitor keys: the TS-only fields of ESTree nodes are not visited (TS nodes
        // themselves fall back to all keys).
        let is_type = |n: u32| {
            let p = tree.parent(n);
            p != NONE
                && !tree.ty(p).name().starts_with("TS")
                && matches!(
                    tree.field_of(n),
                    Some(
                        F::TypeAnnotation
                            | F::TypeArguments
                            | F::TypeParameters
                            | F::ReturnType
                            | F::SuperTypeArguments
                            | F::Implements
                    )
                )
        };
        for n in 0..tree.nodes.len() as u32 {
            if tree.ty(n) == Ty::Unknown {
                continue;
            }
            let mut p = n;
            let mut in_type = false;
            while p != NONE {
                if is_type(p) {
                    in_type = true;
                    break;
                }
                p = tree.parent(p);
            }
            if in_type {
                continue;
            }
            for (sel, msg) in &self.selectors {
                if m.matches_any(n, sel) {
                    let (s, e) = tree.range(n);
                    ctx.report_id(Span::new(s, e), "restrictedSyntax", msg.clone());
                }
            }
        }
    }
}
