//! vue/no-undef-properties

use std::collections::{HashMap, HashSet};

use oxc::ast::AstKind;
use oxc::ast::ast::*;
use oxc::semantic::{NodeId, Semantic};
use oxc::span::{GetSpan, Span};

use crate::rule::{Ctx, Rule, RuleOptions, fmt_msg};
use crate::utils::js;
use crate::utils::property_references::{
    Extractor, FnRef, NodeRef, Refs, store_helper_names, vue_param_functions, watch_handler_values,
};
use crate::utils::reference_tracker::{TraceMap, esm_calls};
use crate::utils::ref_object_references::COMPOSITION_API_MODULES;
use crate::utils::regexp::GroupMatcher;
use crate::utils::template::VUE_RESERVED;
use crate::utils::tpl_analysis::{container_element, global_names, global_calls, innermost_vue, module_bindings, style_var_paths};
use crate::utils::vue_component::{self as vc, ComponentProp, MacroKind};

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let ignores: Vec<String> = opts.get(0)["ignores"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_else(|| vec!["/^\\$/".to_string()]);
    Some(Box::new(NoUndefProperties {
        ignores: GroupMatcher::new(ignores.iter().map(String::as_str)),
    }))
}

struct NoUndefProperties {
    ignores: GroupMatcher,
}

/// `PropertyData`.
#[derive(Clone, Default)]
struct PData<'a> {
    is_props: bool,
    /// Nested object properties (`hasNestProperty` when `Some`).
    nest: Option<&'a ObjectExpression<'a>>,
}

/// `getObjectPropertyMap(object)` → whether every member is a statically named property.
fn object_map_ok(o: &ObjectExpression) -> bool {
    o.properties.iter().all(|p| match p {
        ObjectPropertyKind::ObjectProperty(p) => js::prop_name(p).is_some(),
        _ => false,
    })
}

fn nested<'a>(o: &'a ObjectExpression<'a>, name: &str) -> Option<PData<'a>> {
    // Map semantics: the last property with that name wins.
    let p = js::properties(o).filter(|p| js::prop_name(p).as_deref() == Some(name)).last()?;
    Some(PData {
        is_props: false,
        nest: match &p.value {
            Expression::ObjectExpression(v) if object_map_ok(v) => Some(v),
            _ => None,
        },
    })
}

struct CompCtx<'a> {
    defs: HashMap<String, PData<'a>>,
    reported_nodes: HashSet<Span>,
    reported_names: HashSet<String>,
    unknown: bool,
}

impl<'a> CompCtx<'a> {
    fn new() -> Self {
        CompCtx { defs: HashMap::new(), reported_nodes: HashSet::new(), reported_names: HashSet::new(), unknown: false }
    }
}

struct Checker<'r, 'f, 'a> {
    rule: &'r NoUndefProperties,
    ex: &'r Extractor<'f, 'a>,
    store: &'r HashSet<String>,
}

impl<'a> Checker<'_, '_, 'a> {
    fn verify(&self, ctx: &mut Ctx, c: &mut CompCtx<'a>, refs: Refs<'a>, is_props: bool) {
        if c.unknown {
            return;
        }
        self.verify_level(ctx, c, None, refs, None, is_props);
    }

    fn verify_level(
        &self,
        ctx: &mut Ctx,
        c: &mut CompCtx<'a>,
        level: Option<&'a ObjectExpression<'a>>,
        refs: Refs<'a>,
        path: Option<&str>,
        is_props: bool,
    ) {
        for (name, nodes) in refs.all_properties() {
            let full = match path {
                Some(p) => format!("{p}.{name}"),
                None => name.clone(),
            };
            let prop = match level {
                None => c.defs.get(&name).cloned(),
                Some(o) => nested(o, &name),
            };
            let Some(prop) = prop else {
                self.report(ctx, c, nodes[0], &full, false);
                continue;
            };
            if is_props && !prop.is_props {
                self.report(ctx, c, nodes[0], &full, true);
                continue;
            }
            if let Some(o) = prop.nest {
                let nest = refs.get_nest(self.ex, &name);
                self.verify_level(ctx, c, Some(o), nest, Some(&full), is_props);
            }
        }
    }

    fn report(&self, ctx: &mut Ctx, c: &mut CompCtx<'a>, span: Span, name: &str, props: bool) {
        let reserved = VUE_RESERVED.as_array().is_some_and(|a| a.iter().any(|v| v.as_str() == Some(name)));
        if reserved
            || self.rule.ignores.is_match(name)
            || self.store.contains(name)
            || c.reported_nodes.contains(&span)
            || c.reported_names.contains(name)
        {
            return;
        }
        c.reported_nodes.insert(span);
        c.reported_names.insert(name.to_string());
        let (id, msg) = if props {
            ("undefProps", "'{{name}}' is not defined in props.")
        } else {
            ("undef", "'{{name}}' is not defined.")
        };
        ctx.report_id(span, id, fmt_msg(msg, &[("name", name)]));
    }
}

/// `reactive()` / `toRefs()` call spans (ESM + global).
fn reactive_sets(file: &crate::rule::LintFile) -> (HashSet<Span>, HashSet<Span>) {
    let map = TraceMap::calls(&["reactive", "toRefs"]);
    let modules: Vec<(&str, &TraceMap)> = COMPOSITION_API_MODULES.iter().map(|m| (*m, &map)).collect();
    let (mut reactive, mut to_refs) = (HashSet::new(), HashSet::new());
    for hit in esm_calls(file, &modules) {
        if hit.path.last().map(String::as_str) == Some("reactive") {
            reactive.insert(hit.call.span);
        } else {
            to_refs.insert(hit.call.span);
        }
    }
    for (n, c) in global_calls(file, &["reactive", "toRefs"]) {
        if n == "reactive" {
            reactive.insert(c.span);
        } else {
            to_refs.insert(c.span);
        }
    }
    (reactive, to_refs)
}

/// `iterateSetupToRefsNames(node)`.
fn setup_to_refs_names(
    sem: &Semantic,
    obj: &ObjectExpression,
    sets: &(HashSet<Span>, HashSet<Span>),
) -> Vec<String> {
    let mut out = Vec::new();
    let Some(setup) = js::find_property(obj, "setup") else { return out };
    let mut returned: Vec<&ObjectExpression> = Vec::new();
    match &setup.value {
        Expression::FunctionExpression(f) => {
            for s in f.body.iter().flat_map(|b| b.statements.iter()) {
                if let Statement::ReturnStatement(r) = s
                    && let Some(Expression::ObjectExpression(o)) = r.argument.as_ref().map(js::skip_parens)
                {
                    returned.push(o);
                }
            }
        }
        Expression::ArrowFunctionExpression(f) => match &f.body {
            ArrowFunctionBody::FunctionBody(b) => {
                for s in &b.statements {
                    if let Statement::ReturnStatement(r) = s
                        && let Some(Expression::ObjectExpression(o)) = r.argument.as_ref().map(js::skip_parens)
                    {
                        returned.push(o);
                    }
                }
            }
            body => {
                if let Some(Expression::ObjectExpression(o)) = body.as_expression().map(js::skip_parens) {
                    returned.push(o);
                }
            }
        },
        _ => return out,
    }
    for o in returned {
        for p in &o.properties {
            let ObjectPropertyKind::SpreadProperty(sp) = p else { continue };
            let Expression::CallExpression(c) = &sp.argument else { continue };
            if !sets.1.contains(&c.span) {
                continue;
            }
            let mut arg = c.arguments.first().and_then(|a| a.as_expression());
            if let Some(Expression::Identifier(id)) = arg {
                let Some(sym) = js::ref_symbol(sem, id) else { continue };
                let Some(AstKind::VariableDeclarator(d)) = js::single_declaration(sem, sym) else { continue };
                if js::declarator_kind(sem, js::declaration_id(sem, sym)) != Some(VariableDeclarationKind::Const) {
                    continue;
                }
                arg = d.init.as_ref();
            }
            let Some(Expression::CallExpression(r)) = arg else { continue };
            if !sets.0.contains(&r.span) {
                continue;
            }
            if let Some(Expression::ObjectExpression(ro)) = r.arguments.first().and_then(|a| a.as_expression()) {
                out.extend(js::properties(ro).filter_map(js::prop_name).filter(|n| !n.is_empty()));
            }
        }
    }
    out
}

fn fn_ref(f: vc::FnNode<'_>) -> FnRef<'_> {
    match f {
        vc::FnNode::Function(x) => FnRef::Function(x),
        vc::FnNode::Arrow(x) => FnRef::Arrow(x),
    }
}

impl Rule for NoUndefProperties {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let file = ctx.file;
        let ex = Extractor::new(file, false, false);
        let objs = vc::vue_objects(file);
        let vue_ids: HashSet<NodeId> = objs.iter().map(|o| o.node_id()).collect();

        // ponytail: store helper names are collected up front (upstream adds them while
        // traversing, so a report before a later helper call outside computed/methods differs).
        let mut store: HashSet<String> = HashSet::new();
        if let Some(sem) = file.semantic {
            for n in sem.nodes().iter() {
                if let AstKind::CallExpression(c) = n.kind()
                    && innermost_vue(sem, &vue_ids, n.id()).is_some()
                {
                    store.extend(store_helper_names(c, false).into_iter().map(|(_, n, _)| n));
                }
            }
        }
        let ck = Checker { rule: self, ex: &ex, store: &store };

        // Contexts in creation order: Program (script setup) first, then Vue objects.
        let mut order: Vec<Option<NodeId>> = Vec::new();
        let mut ctxs: HashMap<Option<NodeId>, CompCtx> = HashMap::new();
        let setup = vc::is_script_setup(file);
        if setup {
            order.push(None);
            let mut c = CompCtx::new();
            for g in global_names(file) {
                c.defs.insert(g, PData::default());
            }
            for (n, _) in module_bindings(file) {
                c.defs.insert(n.to_string(), PData::default());
            }
            ctxs.insert(None, c);
        }

        if let Some(sem) = file.semantic {
            let macros: HashMap<Span, vc::Macro> =
                vc::script_setup_macros(file).into_iter().map(|m| (m.call.span, m)).collect();
            let mut param_fns: HashMap<u32, (NodeId, FnRef, bool)> = HashMap::new();
            let mut setup_fns: HashMap<u32, NodeId> = HashMap::new();
            let mut render_fns: HashMap<u32, (NodeId, bool)> = HashMap::new();
            for o in &objs {
                for (f, is_data) in vue_param_functions(o.obj) {
                    let start = match f {
                        FnRef::Function(x) => x.span.start,
                        FnRef::Arrow(x) => x.span.start,
                    };
                    param_fns.insert(start, (o.node_id(), f, is_data));
                }
                for f in vc::setup_functions(o.obj) {
                    setup_fns.insert(f.span().start, o.node_id());
                }
                for f in vc::render_functions(o.obj) {
                    render_fns.insert(f.span().start, (o.node_id(), o.functional()));
                }
            }
            let mut sets: Option<(HashSet<Span>, HashSet<Span>)> = None;
            for n in sem.nodes().iter() {
                match n.kind() {
                    AstKind::CallExpression(call) if setup && macros.contains_key(&call.span) => {
                        let m = &macros[&call.span];
                        let c = ctxs.get_mut(&None).unwrap();
                        match m.kind {
                            MacroKind::DefineProps => {
                                let mut unknown = false;
                                for p in vc::props_from_define_props(file, m.call) {
                                    if matches!(p, ComponentProp::Unknown { .. }) {
                                        c.unknown = true;
                                        unknown = true;
                                        break;
                                    }
                                    if let Some(name) = p.name().filter(|n| !n.is_empty()) {
                                        c.defs.insert(name, PData { is_props: true, nest: None });
                                    }
                                }
                                if !unknown && let Some(d) = m.declarator {
                                    let r = ex.extract_from_binding(&d.id);
                                    ck.verify(ctx, c, r, false);
                                }
                            }
                            MacroKind::DefineModel => {
                                let name = vc::model_from_define_model(m.call).name;
                                c.defs.insert(name, PData { is_props: true, nest: None });
                            }
                            _ => {}
                        }
                    }
                    AstKind::ObjectExpression(o) if vue_ids.contains(&o.node_id()) => {
                        let key = Some(o.node_id());
                        if !order.contains(&key) {
                            order.push(key);
                        }
                        let mut c = ctxs.remove(&key).unwrap_or_else(CompCtx::new);
                        // Members of `extends` / `mixins` are defined: nothing to report (upstream reports them).
                        c.unknown |= vc::inherits_options(o);
                        for p in vc::iterate_properties_full(
                            o,
                            &["props", "asyncData", "data", "computed", "setup", "methods", "inject"],
                        ) {
                            let nest = match (p.group, p.prop.map(|x| &x.value)) {
                                ("data" | "asyncData", Some(Expression::ObjectExpression(v))) if object_map_ok(v) => Some(&**v),
                                _ => None,
                            };
                            c.defs.insert(p.name, PData { is_props: p.group == "props", nest });
                        }
                        let sets = sets.get_or_insert_with(|| reactive_sets(file));
                        for name in setup_to_refs_names(sem, o, sets) {
                            c.defs.entry(name).or_default();
                        }
                        for w in vc::iterate_properties_full(o, &["watch", "expose"]) {
                            if w.group == "watch" {
                                let r = ex.extract_from_path(&w.name, w.span);
                                ck.verify(ctx, &mut c, r, false);
                                if let Some(p) = w.prop
                                    && p.kind == PropertyKind::Init
                                {
                                    let mut vals = Vec::new();
                                    watch_handler_values(&p.value, &mut vals);
                                    for v in vals {
                                        let r = ex.extract_from_name_literal(v);
                                        ck.verify(ctx, &mut c, r, false);
                                    }
                                }
                            } else {
                                let r = ex.extract_from_name(&w.name, w.span);
                                ck.verify(ctx, &mut c, r, false);
                            }
                        }
                        ctxs.insert(key, c);
                    }
                    AstKind::Function(_) | AstKind::ArrowFunctionExpression(_) => {
                        let start = n.kind().span().start;
                        if let Some((v, f, is_data)) = param_fns.get(&start)
                            && innermost_vue(sem, &vue_ids, n.id()) == Some(*v)
                            && let Some(c) = ctxs.get_mut(&Some(*v))
                        {
                            let r = ex.extract_from_function_param(*f, 0);
                            ck.verify(ctx, c, r, *is_data);
                        }
                        if let Some(v) = setup_fns.get(&start)
                            && let Some(f) = vc::FnNode::from_kind(n.kind())
                            && let Some(c) = ctxs.get_mut(&Some(*v))
                        {
                            let r = ex.extract_from_function_param(fn_ref(f), 0);
                            ck.verify(ctx, c, r, true);
                        }
                        if let Some((v, functional)) = render_fns.get(&start)
                            && let Some(f) = vc::FnNode::from_kind(n.kind())
                            && let Some(c) = ctxs.get_mut(&Some(*v))
                        {
                            let r = ex.extract_from_function_param(fn_ref(f), 0);
                            ck.verify(ctx, c, r, false);
                            if *functional {
                                let r2 = ex.extract_from_function_param(fn_ref(f), 1).get_nest(&ex, "props");
                                ck.verify(ctx, c, r2, true);
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
                        if let Some(v) = innermost_vue(sem, &vue_ids, n.id())
                            && let Some(c) = ctxs.get_mut(&Some(v))
                        {
                            let r = ex.extract_from_expression(NodeRef::Script(n.id()), false);
                            ck.verify(ctx, c, r, false);
                        }
                    }
                    _ => {}
                }
            }
        }

        // Template context: `<script setup>` or the first Vue object inside `export default`.
        let key = order.iter().copied().find(|k| match k {
            None => true,
            Some(id) => file
                .semantic
                .is_some_and(|s| s.nodes().ancestor_kinds(*id).any(|k| matches!(k, AstKind::ExportDefaultDeclaration(_)))),
        });
        let Some(key) = key else { return };
        let Some(doc) = file.doc.as_ref() else { return };
        let mut c = ctxs.remove(&key).unwrap();
        if doc.top_level().any(|e| e.name == "style") {
            let r = ex.extract_from_style_vars(&style_var_paths(doc));
            ck.verify(ctx, &mut c, r, false);
        }
        // Template traversal order (containers are not stored in document order).
        let mut cis: Vec<usize> = (0..doc.exprs.len())
            .filter(|&ci| container_element(&doc.exprs[ci]).is_some_and(|el| doc.in_template(el)))
            .collect();
        cis.sort_by_key(|&ci| doc.exprs[ci].span.start);
        for ci in cis {
            let r = ex.extract_from_container(ci as u32, true);
            ck.verify(ctx, &mut c, r, false);
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
