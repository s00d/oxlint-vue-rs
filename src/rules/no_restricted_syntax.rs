//! vue/no-restricted-syntax (core `no-restricted-syntax` over the whole SFC document: template,
//! `<style>` `v-bind()` and the other top-level blocks; script contents are not visited).
//! `wrapCoreRule` moves the core rule's `Program` handler (the selector `Program`) to
//! `VDocumentFragment`.

use oxc::span::Span;

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::esquery::{self, Matcher, Selector};
use crate::utils::indent::tree::{Builder, F, NONE, Ty};
use crate::utils::tpl_core::{DOCUMENT, for_each_root};

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
            // ESLint strips `:exit` from a handler key (the report happens on exit instead).
            let query = match sel.strip_suffix(":exit").unwrap_or(sel) {
                "Program" => "VDocumentFragment",
                s => s,
            };
            Some((esquery::parse(query), message))
        })
        .collect();
    Some(Box::new(NoRestrictedSyntax { selectors }))
}

struct NoRestrictedSyntax {
    selectors: Vec<(Vec<Selector>, String)>,
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
        let mut top = Vec::new();
        for c in &doc.children {
            match c {
                crate::ast::Child::Element(e) => {
                    let id = b.element(doc, *e);
                    if doc.element(*e).name == "style" {
                        styles.push(id);
                    }
                    top.push(id);
                }
                crate::ast::Child::Text(t) => {
                    top.push(b.leaf_node(Ty::VText, doc.text(*t).span));
                }
                crate::ast::Child::Expr(_) => {}
            }
        }
        b.fragment(Span::new(0, src.len() as u32), &top);
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
        let m = Matcher::new(&tree, src, Some(doc));
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
