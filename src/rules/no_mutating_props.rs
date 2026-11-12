//! vue/no-mutating-props

use std::collections::{HashMap, HashSet};

use oxc::ast::AstKind;
use oxc::ast::ast::*;
use oxc::semantic::{NodeId, Semantic};
use oxc::span::{GetSpan, Span};

use crate::ast::{AttributeKey, AttributeValue, DirectiveArgument};
use crate::rule::{Ctx, Rule, RuleOptions, fmt_msg};
use crate::utils::js;
use crate::utils::template::template_expr_containers;
use crate::utils::tpl_analysis::{
    Mutation, MutationKind, Nodes, Tree, find_mutating, innermost_vue, member_kind_name,
    module_bindings, vue_define,
};
use crate::utils::tpl_misc::{body_attributes, is_this};
use crate::utils::vue_component::{self as vc, MacroKind};

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(NoMutatingProps {
        shallow_only: opts.get(0)["shallowOnly"].as_bool() == Some(true),
    }))
}

struct NoMutatingProps {
    shallow_only: bool,
}

const GLOBALS_WHITE_LISTED: &[&str] = &[
    "Infinity", "undefined", "NaN", "isFinite", "isNaN", "parseFloat", "parseInt", "decodeURI",
    "decodeURIComponent", "encodeURI", "encodeURIComponent", "Math", "Number", "Date", "Array",
    "Object", "Boolean", "String", "RegExp", "Map", "Set", "JSON", "Intl", "BigInt",
];

#[derive(Default)]
struct PropsInfo {
    name: Option<String>,
    set: HashSet<String>,
}

fn report(ctx: &mut Ctx, span: Span, name: &str) {
    ctx.report_id(
        span,
        "unexpectedMutation",
        fmt_msg("Unexpected mutation of \"{{key}}\" prop.", &[("key", name)]),
    );
}

/// `getPropertyNameText` for a member expression kind.
fn member_text(src: &str, k: &AstKind) -> String {
    if let Some(n) = member_kind_name(k) {
        return n;
    }
    match k {
        AstKind::ComputedMemberExpression(m) => {
            let s = m.expression.span();
            format!("[{}]", &src[s.start as usize..s.end as usize])
        }
        _ => "?unknown?".into(),
    }
}

fn binding_prop_text(src: &str, p: &BindingProperty) -> String {
    if let Some(n) = js::binding_prop_name(p) {
        return n;
    }
    if p.computed {
        let s = p.key.span();
        return format!("[{}]", &src[s.start as usize..s.end as usize]);
    }
    "?unknown?".into()
}

fn pattern_props<'a>(src: &str, p: &'a BindingPattern<'a>, path: Vec<String>, out: &mut Vec<(&'a BindingIdentifier<'a>, Vec<String>)>) {
    match p {
        BindingPattern::BindingIdentifier(id) => out.push((id, path)),
        BindingPattern::AssignmentPattern(a) => pattern_props(src, &a.left, path, out),
        BindingPattern::ObjectPattern(o) => {
            for prop in &o.properties {
                let mut p2 = path.clone();
                p2.push(binding_prop_text(src, prop));
                pattern_props(src, &prop.value, p2, out);
            }
            if let Some(r) = &o.rest {
                pattern_props(src, &r.argument, path, out);
            }
        }
        BindingPattern::ArrayPattern(a) => {
            for (i, el) in a.elements.iter().enumerate() {
                if let Some(el) = el {
                    let mut p2 = path.clone();
                    p2.push(i.to_string());
                    pattern_props(src, el, p2, out);
                }
            }
            if let Some(r) = &a.rest {
                let mut p2 = path.clone();
                p2.push(a.elements.len().to_string());
                pattern_props(src, &r.argument, p2, out);
            }
        }
    }
}

impl NoMutatingProps {
    fn shallow_invalid<I>(&self, m: &Mutation<I>, root: bool) -> bool {
        !self.shallow_only
            || (m.path.len() == usize::from(root)
                && matches!(m.kind, MutationKind::Assignment | MutationKind::Update))
    }

    fn verify_mutating<'a, T: Tree<'a>>(&self, ctx: &mut Ctx, t: &T, start: T::Id, name: &str, root: bool) {
        if let Some(m) = find_mutating(t, start)
            && self.shallow_invalid(&m, root)
        {
            report(ctx, t.span_of(m.node), name);
        }
    }

    fn verify_prop_variable(&self, ctx: &mut Ctx, sem: &Semantic, id: &BindingIdentifier, path: &[String]) {
        let src = ctx.source();
        let ids: Vec<NodeId> = sem
            .scoping()
            .get_resolved_references(id.symbol_id())
            .filter(|r| r.is_read())
            .map(|r| r.node_id())
            .collect();
        for rid in ids {
            let Some(m) = find_mutating(sem, rid) else { continue };
            if !self.shallow_invalid(&m, path.is_empty()) {
                continue;
            }
            let name = if path.is_empty() {
                let Some(first) = m.path.first() else { continue };
                member_text(src, &sem.nodes().kind(*first))
            } else {
                if m.path.is_empty() && m.kind != MutationKind::Call {
                    continue;
                }
                path[0].clone()
            };
            report(ctx, sem.span_of(m.node), &name);
        }
    }
}

impl Rule for NoMutatingProps {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let file = ctx.file;
        let src = file.source;
        let objs = vc::vue_objects(file);
        let mut vue_props: HashMap<NodeId, PropsInfo> = HashMap::new();
        for o in &objs {
            vue_props.insert(
                o.node_id(),
                PropsInfo {
                    name: None,
                    set: vc::props_from_options(o.obj).iter().filter_map(|p| p.name()).collect(),
                },
            );
        }
        let mut setup_props: Option<PropsInfo> = None;
        if let Some(sem) = file.semantic {
            let defined: HashSet<&str> = module_bindings(file).into_iter().map(|(n, _)| n).collect();
            for m in vc::macros_of(file, MacroKind::DefineProps) {
                let mut info = PropsInfo {
                    name: Some(String::new()),
                    set: vc::props_from_define_props(file, m.call)
                        .iter()
                        .filter_map(|p| p.name())
                        .filter(|n| !GLOBALS_WHITE_LISTED.contains(&n.as_str()) && !defined.contains(n.as_str()))
                        .collect(),
                };
                if let Some(d) = m.declarator {
                    let mut props = Vec::new();
                    pattern_props(src, &d.id, Vec::new(), &mut props);
                    for (id, path) in props {
                        if path.is_empty() {
                            info.name = Some(id.name.to_string());
                        } else {
                            info.set.insert(id.name.to_string());
                        }
                        self.verify_prop_variable(ctx, sem, id, &path);
                    }
                }
                setup_props = Some(info);
            }
            for o in &objs {
                for f in vc::setup_functions(o.obj) {
                    let Some(p) = f.params().items.first() else { continue };
                    if matches!(p.pattern, BindingPattern::ArrayPattern(_)) && p.initializer.is_none() {
                        continue;
                    }
                    let mut props = Vec::new();
                    pattern_props(src, &p.pattern, Vec::new(), &mut props);
                    for (id, path) in props {
                        self.verify_prop_variable(ctx, sem, id, &path);
                    }
                }
            }
            // `this.<prop>` mutations inside Vue objects.
            let vue_ids: HashSet<NodeId> = vue_props.keys().copied().collect();
            for n in sem.nodes().iter() {
                let obj: &Expression = match n.kind() {
                    AstKind::StaticMemberExpression(m) => &m.object,
                    AstKind::ComputedMemberExpression(m) => &m.object,
                    _ => continue,
                };
                if !matches!(js::skip_parens(obj), Expression::ThisExpression(_) | Expression::Identifier(_))
                    || !is_this(sem, obj)
                {
                    continue;
                }
                let Some(v) = innermost_vue(sem, &vue_ids, n.id()) else { continue };
                let Some(name) = member_kind_name(&n.kind()) else { continue };
                if vue_props[&v].set.contains(&name) {
                    self.verify_mutating(ctx, sem, n.id(), &name, false);
                }
            }
        }

        // Template.
        let Some(doc) = file.doc.as_ref() else { return };
        let info = match setup_props {
            Some(i) => i,
            None => match vue_define(&objs, |_| true) {
                Some(o) => vue_props.remove(&o.node_id()).unwrap_or_default(),
                None => return,
            },
        };
        for c in template_expr_containers(doc) {
            let nodes = Nodes::of(c);
            let vm_ref = |i: usize| -> bool {
                matches!(nodes.kinds[i], AstKind::IdentifierReference(id)
                    if c.references.iter().any(|r| r.variable.is_none() && r.span == id.span))
            };
            for i in 0..nodes.kinds.len() {
                match nodes.kinds[i] {
                    AstKind::ThisExpression(_) => {
                        let Some(p) = nodes.parent_of(i) else { continue };
                        let k = nodes.kinds[p];
                        let is_obj = match k {
                            AstKind::StaticMemberExpression(m) => matches!(js::skip_parens(&m.object), Expression::ThisExpression(_)),
                            AstKind::ComputedMemberExpression(m) => matches!(js::skip_parens(&m.object), Expression::ThisExpression(_)),
                            _ => false,
                        };
                        if !is_obj {
                            continue;
                        }
                        if let Some(name) = member_kind_name(&k)
                            && info.set.contains(&name)
                        {
                            self.verify_mutating(ctx, &nodes, p, &name, false);
                        }
                    }
                    AstKind::IdentifierReference(id) if vm_ref(i) => {
                        let root = info.name.as_deref() == Some(id.name.as_str());
                        let parent_name =
                            nodes.parent_of(i).and_then(|p| member_kind_name(&nodes.kinds[p]));
                        let name = if root { parent_name.unwrap_or_else(|| id.name.to_string()) } else { id.name.to_string() };
                        if !name.is_empty() && (root || info.set.contains(&name)) {
                            self.verify_mutating(ctx, &nodes, i, &name, root);
                        }
                    }
                    _ => {}
                }
            }
        }
        // `v-model` / `v-bind.sync` targets.
        for (_, a) in body_attributes(doc) {
            let AttributeKey::Directive(k) = &a.key else { continue };
            match k.name.name.as_str() {
                "model" => {}
                "bind" if k.has_modifier("sync") => {}
                _ => continue,
            }
            let mut cids = Vec::new();
            if let Some(DirectiveArgument::Dynamic(id)) = &k.argument {
                cids.push(*id);
            }
            if let Some(AttributeValue::Expr(id)) = &a.value {
                cids.push(*id);
            }
            for cid in cids {
                let c = doc.expr(cid);
                let Some(crate::ast::VExpression::Expression(e)) = &c.expression else { continue };
                let e = js::skip_parens(e);
                // `getMemberChaining`
                let mut chain: Vec<&MemberExpression> = Vec::new();
                let mut cur: &Expression = e;
                let n = loop {
                    let x = js::skip_parens(cur);
                    let m = match x {
                        Expression::ChainExpression(ch) => ch.expression.as_member_expression(),
                        x => x.as_member_expression(),
                    };
                    match m {
                        Some(m) => {
                            chain.push(m);
                            cur = m.object();
                        }
                        None => break x,
                    }
                };
                chain.reverse();
                let mem_name = |m: &MemberExpression| -> String {
                    match m {
                        MemberExpression::StaticMemberExpression(s) => s.property.name.to_string(),
                        MemberExpression::ComputedMemberExpression(s) => js::string_literal_value(&s.expression)
                            .unwrap_or_else(|| {
                                let sp = s.expression.span();
                                format!("[{}]", &src[sp.start as usize..sp.end as usize])
                            }),
                        _ => "?unknown?".into(),
                    }
                };
                let name = match n {
                    Expression::Identifier(id)
                        if c.references.iter().any(|r| r.variable.is_none() && r.span == id.span) =>
                    {
                        if info.name.as_deref() == Some(id.name.as_str()) {
                            if self.shallow_only && chain.len() > 1 {
                                continue;
                            }
                            chain.first().map(|m| mem_name(m)).filter(|s| !s.is_empty()).unwrap_or_else(|| id.name.to_string())
                        } else {
                            if self.shallow_only && !chain.is_empty() {
                                continue;
                            }
                            if !info.set.contains(id.name.as_str()) {
                                continue;
                            }
                            id.name.to_string()
                        }
                    }
                    Expression::ThisExpression(_) => {
                        if self.shallow_only && chain.len() > 1 {
                            continue;
                        }
                        let Some(m) = chain.first() else { continue };
                        let Some(name) = js::member_name(m) else { continue };
                        if !info.set.contains(&name) {
                            continue;
                        }
                        name
                    }
                    _ => continue,
                };
                report(ctx, e.span(), &name);
            }
        }
    }
}

