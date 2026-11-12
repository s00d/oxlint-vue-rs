//! vue/no-unused-properties

use std::collections::{HashMap, HashSet};

use oxc::ast::AstKind;
use oxc::ast::ast::*;
use oxc::semantic::{NodeId, Semantic};
use oxc::span::{GetSpan, Span};

use crate::ast::{AttributeKey, AttributeValue};
use crate::rule::{Ctx, Rule, RuleOptions, fmt_msg};
use crate::utils::js;
use crate::utils::property_references::{
    Extractor, HasOpts, NodeRef, Refs, merge, store_helper_names, vue_param_functions, watch_handler_values, FnRef,
};
use crate::utils::tokens;
use crate::utils::tpl_analysis::{innermost_vue, style_var_paths};
use crate::utils::tpl_misc::body_attributes;
use crate::utils::vue_component::{self as vc, ComponentProp, MacroKind};

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let o = opts.get(0);
    let strs = |k: &str| -> Option<Vec<String>> {
        o[k].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
    };
    let unref = strs("unreferencedOptions").unwrap_or_default();
    Some(Box::new(NoUnusedProperties {
        groups: strs("groups").unwrap_or_else(|| vec!["props".into()]),
        deep_data: o["deepData"].as_bool() == Some(true),
        ignore_public: o["ignorePublicMembers"].as_bool() == Some(true),
        unknown_member: unref.iter().any(|s| s == "unknownMemberAsUnreferenced"),
        return_unref: unref.iter().any(|s| s == "returnAsUnreferenced"),
    }))
}

struct NoUnusedProperties {
    groups: Vec<String>,
    deep_data: bool,
    ignore_public: bool,
    unknown_member: bool,
    return_unref: bool,
}

fn label(g: &str) -> &'static str {
    match g {
        "props" => "property",
        "data" => "data",
        "asyncData" => "async data",
        "computed" => "computed property",
        "methods" => "method",
        "setup" => "property returned from `setup()`",
        "inject" => "inject",
        "watch" => "watch",
        "provide" => "provide",
        _ => "expose",
    }
}

struct Prop<'a> {
    name: String,
    group: &'static str,
    span: Span,
    prop: Option<&'a ObjectProperty<'a>>,
}

#[derive(Default)]
struct Container<'a> {
    props: Vec<Prop<'a>>,
    refs: Vec<Refs<'a>>,
    refs_for_props: Vec<Refs<'a>>,
    /// `extends` / `mixins` may read any property (upstream reports those as unused).
    inherits: bool,
}

fn is_jsdoc(src: &str, s: Span) -> bool {
    let t = &src[s.start as usize..s.end as usize];
    t.starts_with("/**") && !t.starts_with("/***")
}

/// `findJSDocComment(node)`.
fn jsdoc_before(ctx: &Ctx, start: u32) -> Option<Span> {
    let file = ctx.file;
    let src = ctx.source();
    let mut pos = start;
    loop {
        let t = tokens::token_or_comment_before(file, pos)?;
        if !tokens::is_comment(file, t) {
            return None;
        }
        if src[t.start as usize..].starts_with("//") {
            pos = t.start;
            continue;
        }
        return is_jsdoc(src, t).then_some(t);
    }
}

fn is_public(ctx: &Ctx, p: &ObjectProperty) -> bool {
    let src = ctx.source();
    let jsdoc = jsdoc_before(ctx, p.span.start).or_else(|| match &p.value {
        Expression::FunctionExpression(f) => jsdoc_before(ctx, f.span.start),
        Expression::ArrowFunctionExpression(f) => jsdoc_before(ctx, f.span.start),
        _ => None,
    });
    jsdoc.is_some_and(|s| {
        // `/(?:^|\s|\*)@public\b/u` over the comment value
        let v = &src[s.start as usize + 2..s.end as usize - 2];
        v.match_indices("@public").any(|(i, _)| {
            let before_ok = i == 0 || v[..i].ends_with(|c: char| c.is_whitespace() || c == '*');
            let after = &v[i + 7..];
            let after_ok = !after.starts_with(|c: char| c.is_alphanumeric() || c == '_');
            before_ok && after_ok
        })
    })
}

/// `findExpression(context, id)`: follow `const x = <init>` chains.
fn find_expression<'a>(sem: Option<&Semantic<'a>>, e: &'a Expression<'a>) -> &'a Expression<'a> {
    let mut cur = e;
    for _ in 0..16 {
        let Expression::Identifier(id) = js::skip_parens(cur) else { return cur };
        let Some(sem) = sem else { return cur };
        let Some(sym) = js::ref_symbol(sem, id) else { return cur };
        let Some(AstKind::VariableDeclarator(d)) = js::single_declaration(sem, sym) else { return cur };
        if js::declarator_kind(sem, js::declaration_id(sem, sym)) != Some(VariableDeclarationKind::Const) {
            return cur;
        }
        let Some(init) = &d.init else { return cur };
        cur = init;
    }
    cur
}

impl NoUnusedProperties {
    fn has_group(&self, g: &str) -> bool {
        self.groups.iter().any(|x| x == g)
    }

    fn verify_deep<'a>(
        &self,
        ctx: &mut Ctx<'_, 'a>,
        ex: &Extractor<'_, 'a>,
        segs: &[String],
        value: &'a Expression<'a>,
        refs: Refs<'a>,
    ) {
        let sem = ctx.file.semantic;
        let target = find_expression(sem, value);
        let Expression::ObjectExpression(o) = js::skip_parens(target) else { return };
        for p in js::properties(o) {
            let Some(name) = js::prop_name(p) else { continue };
            let mut path = segs.to_vec();
            path.push(name.clone());
            if !refs.has_property(&name, HasOpts { unknown_call_as_any: true, ..HasOpts::default() }) {
                ctx.report_id(
                    p.key.span(),
                    "unused",
                    fmt_msg(
                        "'{{name}}' of {{group}} found, but never used.",
                        &[("group", "data"), ("name", &path.join("."))],
                    ),
                );
                continue;
            }
            let nest = refs.get_nest(ex, &name);
            self.verify_deep(ctx, ex, &path, &p.value, nest);
        }
    }
}

impl Rule for NoUnusedProperties {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let file = ctx.file;
        let sem = file.semantic;
        let ex = Extractor::new(file, self.unknown_member, self.return_unref);
        let program_key = NodeId::DUMMY;
        let mut order: Vec<NodeId> = Vec::new();
        let mut containers: HashMap<NodeId, Container> = HashMap::new();
        let get = |k: NodeId, order: &mut Vec<NodeId>| -> NodeId {
            if !order.contains(&k) {
                order.push(k);
            }
            k
        };
        let mut props_pattern_name: Option<String> = None;

        // <script setup>
        if self.has_group("props") {
            for m in vc::script_setup_macros(file) {
                match m.kind {
                    MacroKind::DefineProps => {
                        let k = get(program_key, &mut order);
                        let c = containers.entry(k).or_default();
                        for p in vc::props_from_define_props(file, m.call) {
                            let Some(name) = p.name().filter(|n| !n.is_empty()) else { continue };
                            let never = matches!(&p, ComponentProp::Type(t) if matches!(t.node,
                                TSSignature::TSPropertySignature(s) if matches!(s.type_annotation.as_ref().map(|a| &a.type_annotation), Some(TSType::TSNeverKeyword(_)))));
                            if never {
                                continue;
                            }
                            let prop = match &p {
                                ComponentProp::Object { prop, .. } => Some(&**prop),
                                _ => None,
                            };
                            let Some(span) = p.key_span() else { continue };
                            c.props.push(Prop { name, group: "props", span, prop });
                        }
                        if let Some(d) = m.declarator {
                            props_pattern_name = d.id.get_binding_identifier().map(|i| i.name.to_string())
                                .filter(|_| matches!(d.id, BindingPattern::BindingIdentifier(_)));
                            c.refs_for_props.push(ex.extract_from_binding(&d.id));
                        }
                    }
                    MacroKind::DefineModel => {
                        let k = get(program_key, &mut order);
                        let c = containers.entry(k).or_default();
                        let model = vc::model_from_define_model(m.call);
                        let span = model.name_node.unwrap_or(m.call.span);
                        if m.declarator.is_some() && m.with_defaults.is_none() {
                            c.refs.push(ex.extract_from_name(&model.name, span));
                        } else {
                            c.props.push(Prop { name: model.name, group: "props", span, prop: None });
                        }
                    }
                    _ => {}
                }
            }
        }

        // Vue objects
        let objs = vc::vue_objects(file);
        let groups: Vec<&'static str> = ["props", "data", "asyncData", "computed", "methods", "setup", "inject"]
            .into_iter()
            .filter(|g| self.has_group(g))
            .collect();
        if let Some(sem) = sem {
            let vue_ids: HashSet<NodeId> = objs.iter().map(|o| o.node_id()).collect();
            // Per-object setup: watchers / exposes / properties, function params.
            let mut param_fns: HashMap<u32, (NodeId, FnRef, bool)> = HashMap::new();
            let mut setup_fns: HashMap<u32, NodeId> = HashMap::new();
            let mut render_fns: HashMap<u32, (NodeId, bool)> = HashMap::new();
            for o in &objs {
                for (f, _) in vue_param_functions(o.obj) {
                    let start = match f { FnRef::Function(x) => x.span.start, FnRef::Arrow(x) => x.span.start };
                    param_fns.insert(start, (o.node_id(), f, false));
                }
                for f in vc::setup_functions(o.obj) {
                    setup_fns.insert(f.span().start, o.node_id());
                }
                for f in vc::render_functions(o.obj) {
                    render_fns.insert(f.span().start, (o.node_id(), o.functional()));
                }
            }
            for n in sem.nodes().iter() {
                match n.kind() {
                    AstKind::ObjectExpression(o) if vue_ids.contains(&o.node_id()) => {
                        let k = get(o.node_id(), &mut order);
                        let c = containers.entry(k).or_default();
                        c.inherits |= vc::inherits_options(o);
                        for w in vc::iterate_properties_full(o, &["watch", "expose"]) {
                            if w.group == "watch" {
                                c.refs.push(ex.extract_from_path(&w.name, w.span));
                                if let Some(p) = w.prop
                                    && p.kind == PropertyKind::Init
                                {
                                    let mut vals = Vec::new();
                                    watch_handler_values(&p.value, &mut vals);
                                    for v in vals {
                                        c.refs.push(ex.extract_from_name_literal(v));
                                    }
                                }
                            } else {
                                c.refs.push(ex.extract_from_name(&w.name, w.span));
                            }
                        }
                        for p in vc::iterate_properties_full(o, &groups) {
                            c.props.push(Prop { name: p.name, group: p.group, span: p.span, prop: p.prop });
                        }
                    }
                    AstKind::CallExpression(call) => {
                        if let Some(v) = innermost_vue(sem, &vue_ids, n.id()) {
                            let names = store_helper_names(call, true);
                            if !names.is_empty() {
                                let k = get(v, &mut order);
                                let c = containers.entry(k).or_default();
                                for (g, name, span) in names {
                                    c.props.push(Prop { name, group: g, span, prop: None });
                                }
                            }
                        }
                    }
                    AstKind::Function(_) | AstKind::ArrowFunctionExpression(_) => {
                        let start = n.kind().span().start;
                        if let Some((v, f, _)) = param_fns.get(&start)
                            && innermost_vue(sem, &vue_ids, n.id()) == Some(*v)
                        {
                            let r = ex.extract_from_function_param(*f, 0);
                            let k = get(*v, &mut order);
                            containers.entry(k).or_default().refs.push(r);
                        }
                        if let Some(v) = setup_fns.get(&start)
                            && let Some(f) = vc::FnNode::from_kind(n.kind())
                        {
                            let r = ex.extract_from_function_param(fn_ref(f), 0);
                            let k = get(*v, &mut order);
                            containers.entry(k).or_default().refs_for_props.push(r);
                        }
                        if let Some((v, functional)) = render_fns.get(&start)
                            && let Some(f) = vc::FnNode::from_kind(n.kind())
                        {
                            let r = ex.extract_from_function_param(fn_ref(f), 0);
                            let k = get(*v, &mut order);
                            containers.entry(k).or_default().refs_for_props.push(r);
                            if *functional {
                                let r2 = ex.extract_from_function_param(fn_ref(f), 1).get_nest(&ex, "props");
                                containers.entry(k).or_default().refs_for_props.push(r2);
                            }
                        }
                    }
                    AstKind::ThisExpression(_) | AstKind::IdentifierReference(_) => {
                        let is_t = match n.kind() {
                            AstKind::ThisExpression(_) => true,
                            AstKind::IdentifierReference(id) => is_this_ident(sem, id),
                            _ => false,
                        };
                        if !is_t {
                            continue;
                        }
                        if let Some(v) = innermost_vue(sem, &vue_ids, n.id()) {
                            let r = ex.extract_from_expression(NodeRef::Script(n.id()), false);
                            let k = get(v, &mut order);
                            containers.entry(k).or_default().refs.push(r);
                        }
                    }
                    _ => {}
                }
            }
        }

        // Template.
        let mut tpl_refs: Vec<Refs> = Vec::new();
        let mut ref_names: HashSet<String> = HashSet::new();
        if let Some(doc) = file.doc.as_ref() {
            let paths = style_var_paths(doc);
            if doc.top_level().any(|e| e.name == "style") {
                tpl_refs.push(ex.extract_from_style_vars(&paths));
            }
            for (ci, c) in doc.exprs.iter().enumerate() {
                let in_tpl = crate::utils::tpl_analysis::container_element(c).is_some_and(|e| doc.in_template(e));
                if !in_tpl {
                    continue;
                }
                let p = ex.extract_from_container(ci as u32, false);
                tpl_refs.push(p.clone());
                if let Some(pn) = &props_pattern_name {
                    for (k, _) in p.all_properties() {
                        if &k == pn {
                            tpl_refs.push(p.get_nest(&ex, &k));
                        }
                    }
                }
            }
            for (_, a) in body_attributes(doc) {
                if let (AttributeKey::Plain(k), Some(AttributeValue::Literal(l))) = (&a.key, &a.value)
                    && k.name == "ref"
                {
                    ref_names.insert(l.value.clone());
                }
            }
        }

        // Report.
        for k in order {
            let c = containers.remove(&k).unwrap_or_default();
            if c.inherits {
                continue;
            }
            let refs = merge(c.refs.iter().cloned().chain(tpl_refs.iter().cloned()).collect());
            let props_refs = merge(c.refs_for_props);
            for p in &c.props {
                if (p.group == "props"
                    && (props_refs.has_property(&p.name, HasOpts { external_call_as_any: true, ..HasOpts::default() })
                        || refs.has_property("$props", HasOpts::default())))
                    || (p.group == "setup" && ref_names.contains(&p.name))
                    || (self.ignore_public && p.group != "props" && p.prop.is_some_and(|op| is_public(ctx, op)))
                {
                    continue;
                }
                if refs.has_property(&p.name, HasOpts::default()) {
                    if self.deep_data
                        && matches!(p.group, "data" | "asyncData")
                        && let Some(op) = p.prop
                    {
                        let nest = refs.get_nest(&ex, &p.name);
                        self.verify_deep(ctx, &ex, std::slice::from_ref(&p.name), &op.value, nest);
                    }
                    continue;
                }
                ctx.report_id(
                    p.span,
                    "unused",
                    fmt_msg(
                        "'{{name}}' of {{group}} found, but never used.",
                        &[("group", label(p.group)), ("name", &p.name)],
                    ),
                );
            }
        }
    }
}

/// `utils.isThis` for an identifier reference (`const vm = this` alias).
fn is_this_ident(sem: &Semantic, id: &IdentifierReference) -> bool {
    let Some(sym) = js::ref_symbol(sem, id) else { return false };
    let Some(AstKind::VariableDeclarator(d)) = js::single_declaration(sem, sym) else { return false };
    js::declarator_kind(sem, js::declaration_id(sem, sym)) == Some(VariableDeclarationKind::Const)
        && matches!(d.id, BindingPattern::BindingIdentifier(_))
        && d.init.as_ref().is_some_and(|i| matches!(js::skip_parens(i), Expression::ThisExpression(_)))
}

fn fn_ref(f: vc::FnNode<'_>) -> FnRef<'_> {
    match f {
        vc::FnNode::Function(x) => FnRef::Function(x),
        vc::FnNode::Arrow(x) => FnRef::Arrow(x),
    }
}
