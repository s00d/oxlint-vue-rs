//! Helpers for the script ↔ template analysis rules (`no-unused-*`, `no-undef-*`, emits, props, …).
//!
//! Template expressions are separate oxc trees outside [`oxc::semantic::Semantic`], so
//! [`Nodes`] flattens one container into a node list with parent links (what ESLint's `parent`
//! gives the upstream rules).

use std::collections::{HashMap, HashSet};

use oxc::ast::AstKind;
use oxc::ast::ast::*;
use oxc::ast_visit::Visit;
use oxc::semantic::{NodeId, Semantic, SymbolId};
use oxc::span::{GetSpan, Span};

use crate::ast::*;
use crate::rule::LintFile;
use crate::utils::js;
use crate::utils::vue_component::{self as vc, MacroKind};

/// Pre-order node list of a template expression tree with parent links.
#[derive(Default)]
pub struct Nodes<'a> {
    pub kinds: Vec<AstKind<'a>>,
    /// Parent index (`None` = a root of the container).
    pub parents: Vec<Option<usize>>,
    stack: Vec<usize>,
}

impl<'a> Visit<'a> for Nodes<'a> {
    fn enter_node(&mut self, kind: AstKind<'a>) {
        self.kinds.push(kind);
        self.parents.push(self.stack.last().copied());
        self.stack.push(self.kinds.len() - 1);
    }
    fn leave_node(&mut self, _kind: AstKind<'a>) {
        self.stack.pop();
    }
}

impl<'a> Nodes<'a> {
    /// Nodes of one expression container (every part: filters, v-for, v-on statements, slot params).
    pub fn of(c: &ExprContainer<'a>) -> Self {
        let mut n = Nodes::default();
        visit_container(&mut n, c);
        n
    }
    pub fn parent(&self, i: usize) -> Option<usize> {
        self.parents[i]
    }
    /// ESTree parent: parentheses (and `TSAsExpression`-free) skipped.
    pub fn parent_skip_parens(&self, mut i: usize) -> Option<usize> {
        loop {
            let p = self.parents[i]?;
            if matches!(self.kinds[p], AstKind::ParenthesizedExpression(_)) {
                i = p;
                continue;
            }
            return Some(p);
        }
    }
}

/// Visit every part of a container's expression.
pub fn visit_container<'a, V: Visit<'a>>(v: &mut V, c: &ExprContainer<'a>) {
    match &c.expression {
        Some(VExpression::Expression(e)) => v.visit_expression(e),
        Some(VExpression::FilterSequence {
            expression,
            filters,
            ..
        }) => {
            v.visit_expression(expression);
            for f in filters {
                for a in &f.arguments {
                    v.visit_argument(a);
                }
            }
        }
        Some(VExpression::For {
            left, rest, right, ..
        }) => {
            for p in left.iter().flatten() {
                v.visit_binding_pattern(p);
            }
            if let Some(r) = rest {
                v.visit_binding_rest_element(r);
            }
            v.visit_expression(right);
        }
        Some(VExpression::On { body, .. }) => body.iter().for_each(|s| v.visit_statement(s)),
        Some(VExpression::SlotScope { params, .. }) => v.visit_formal_parameters(params),
        _ => {}
    }
}

/// Element that owns a container (the mustache parent or the directive's element).
pub fn container_element(c: &ExprContainer) -> Option<ElementId> {
    match c.parent {
        ExprParent::Element(e) => e,
        ExprParent::DirectiveValue(e, _) | ExprParent::DirectiveArgument(e, _) => Some(e),
    }
}

/// Names of the template variables in scope at an element (its own + its ancestors').
pub fn scope_variable_names(doc: &Document, el: ElementId) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let e = doc.element(el);
    for v in e
        .variables
        .iter()
        .chain(doc.ancestors(el).flat_map(|a| a.variables.iter()))
    {
        if !out.contains(&v.name) {
            out.push(v.name.clone());
        }
    }
    out
}

/// `getNameParamNode(call)`: a string-literal first argument → (value, ESTree node span).
pub fn name_param(call: &CallExpression) -> Option<(String, Span)> {
    let e = call.arguments.first()?.as_expression()?;
    if !js::is_string_literal(e) {
        return None;
    }
    js::string_literal_value(e).map(|n| (n, e.span()))
}

/// `skipChainExpression(e)` when it is a `MemberExpression` (parentheses skipped).
pub fn chain_member<'b, 'a>(e: &'b Expression<'a>) -> Option<&'b MemberExpression<'a>> {
    match js::skip_parens(e) {
        Expression::ChainExpression(c) => c.expression.as_member_expression(),
        e => e.as_member_expression(),
    }
}

/// Identifier node ids of the references of a symbol (`variable.references`).
pub fn reference_ids(sem: &Semantic, sym: SymbolId, read_only: bool) -> Vec<NodeId> {
    sem.scoping()
        .get_resolved_references(sym)
        .filter(|r| !read_only || r.is_read())
        .map(|r| r.node_id())
        .collect()
}

/// Every call expression of the template body.
pub fn template_calls<'a>(doc: &Document<'a>) -> Vec<&'a CallExpression<'a>> {
    struct C<'a>(Vec<&'a CallExpression<'a>>);
    impl<'a> Visit<'a> for C<'a> {
        fn enter_node(&mut self, kind: AstKind<'a>) {
            if let AstKind::CallExpression(c) = kind {
                self.0.push(c);
            }
        }
    }
    let mut v = C(Vec::new());
    for c in crate::utils::template::template_expr_containers(doc) {
        visit_container(&mut v, c);
    }
    v.0
}

/// Setup-context reference ids of `onSetupFunctionEnter` / `onDefineEmitsEnter` (emit rules).
#[derive(Default)]
pub struct EmitCtx {
    /// References of `setup(props, context)`'s `context`.
    pub ctx_ids: HashSet<NodeId>,
    /// References of `setup(props, { emit })`'s `emit` / `const emit = defineEmits()`.
    pub emit_ids: HashSet<NodeId>,
}

impl EmitCtx {
    /// `emit(…)` with a tracked emit reference, or `context.emit(…)` with a tracked context
    /// (`skip_chain`: `skipChainExpression` on the member object).
    pub fn matches(&self, call: &CallExpression) -> bool {
        if let Expression::Identifier(id) = js::skip_parens(&call.callee)
            && self.emit_ids.contains(&id.node_id())
        {
            return true;
        }
        if let Some(m) = chain_member(&call.callee)
            && js::member_name(m).as_deref() == Some("emit")
            && let Expression::Identifier(o) = js::skip_parens(m.object())
        {
            return self.ctx_ids.contains(&o.node_id());
        }
        false
    }
}

/// What the emit rules collect from the script: `<script setup>` `defineEmits` contexts and
/// per-Vue-object `setup()` contexts.
#[derive(Default)]
pub struct EmitSetup {
    /// `const <name> = defineEmits()` (template `emitParamName`).
    pub param_name: Option<String>,
    /// Program-level context and the offset from which it is active (the `defineEmits` call).
    pub program: Option<(u32, EmitCtx)>,
    /// Vue object node id → its `setup()` context.
    pub vue: HashMap<NodeId, EmitCtx>,
}

/// Build [`EmitSetup`]. `read_only`: only read references; `skip_default`: `skipDefaultParamValue`
/// on the context parameter (otherwise a defaulted parameter yields an empty context).
pub fn emit_setup(file: &LintFile, read_only: bool, skip_default: bool) -> EmitSetup {
    let mut out = EmitSetup::default();
    let Some(sem) = file.semantic else {
        return out;
    };
    for m in vc::macros_of(file, MacroKind::DefineEmits) {
        if m.with_defaults.is_some() {
            continue;
        }
        let Some(d) = m.declarator else { continue };
        let BindingPattern::BindingIdentifier(id) = &d.id else {
            continue;
        };
        out.param_name = Some(id.name.to_string());
        out.program = Some((
            m.call.span.start,
            EmitCtx {
                ctx_ids: HashSet::new(),
                emit_ids: reference_ids(sem, id.symbol_id(), read_only)
                    .into_iter()
                    .collect(),
            },
        ));
    }
    for o in vc::vue_objects(file) {
        for f in vc::setup_functions(o.obj) {
            if let Some(c) = setup_param_context(sem, f.params(), read_only, skip_default) {
                out.vue.insert(o.node_id(), c);
            }
        }
    }
    out
}

fn setup_param_context(
    sem: &Semantic,
    params: &FormalParameters,
    read_only: bool,
    skip_default: bool,
) -> Option<EmitCtx> {
    let p = params.items.get(1)?;
    if p.initializer.is_some() && !skip_default {
        return Some(EmitCtx::default());
    }
    let ids = |sym| reference_ids(sem, sym, read_only).into_iter().collect();
    match &p.pattern {
        BindingPattern::ObjectPattern(op) => {
            let ep = js::find_binding_property(op, "emit")?;
            let BindingPattern::BindingIdentifier(id) = &ep.value else {
                return None;
            };
            Some(EmitCtx {
                ctx_ids: HashSet::new(),
                emit_ids: ids(id.symbol_id()),
            })
        }
        BindingPattern::BindingIdentifier(id) => Some(EmitCtx {
            ctx_ids: ids(id.symbol_id()),
            emit_ids: HashSet::new(),
        }),
        BindingPattern::AssignmentPattern(_) => Some(EmitCtx::default()),
        BindingPattern::ArrayPattern(_) => None,
    }
}

/// `callee` is a member `<obj>.<name>(…)` (callee `skipChainExpression`): the object expression.
pub fn member_call_object<'b, 'a>(
    call: &'b CallExpression<'a>,
    name: &str,
) -> Option<&'b Expression<'a>> {
    let m = chain_member(&call.callee)?;
    (js::member_name(m).as_deref() == Some(name)).then(|| m.object())
}

/// `callee.type === 'Identifier'` (parentheses skipped) name.
pub fn callee_ident<'b>(call: &'b CallExpression) -> Option<&'b str> {
    match js::skip_parens(&call.callee) {
        Expression::Identifier(id) => Some(id.name.as_str()),
        _ => None,
    }
}

/// Innermost Vue object (by node id, from `vue_ids`) that contains node `id`.
pub fn innermost_vue(sem: &Semantic, vue_ids: &HashSet<NodeId>, id: NodeId) -> Option<NodeId> {
    sem.nodes().ancestor_ids(id).find(|a| vue_ids.contains(a))
}

/// The component whose declarations the template is checked against (`vueTemplateDefineData`):
/// `<script setup>` wins; otherwise, in `onVueObjectExit` (post-)order, the first `export`
/// object or else the last `mark` / `definition` object. `accept` filters objects (e.g. ones
/// whose declarations were collected).
pub enum TemplateDefine<'a> {
    Setup,
    Vue(vc::VueObject<'a>),
}

pub fn template_define<'a>(
    file: &LintFile<'a>,
    objs: &[vc::VueObject<'a>],
    accept: impl Fn(&vc::VueObject<'a>) -> bool,
) -> Option<TemplateDefine<'a>> {
    if vc::is_script_setup(file) {
        return Some(TemplateDefine::Setup);
    }
    vue_define(objs, accept).map(TemplateDefine::Vue)
}

/// The `onVueObjectExit` part of [`template_define`].
pub fn vue_define<'a>(
    objs: &[vc::VueObject<'a>],
    accept: impl Fn(&vc::VueObject<'a>) -> bool,
) -> Option<vc::VueObject<'a>> {
    let mut post: Vec<&vc::VueObject<'a>> = objs.iter().collect();
    post.sort_by_key(|o| (o.obj.span.end, std::cmp::Reverse(o.obj.span.start)));
    let mut cur: Option<vc::VueObject<'a>> = None;
    for o in post {
        if cur.is_some_and(|c| c.ty == vc::VueObjectType::Export) {
            break;
        }
        if o.ty != vc::VueObjectType::Instance && accept(o) {
            cur = Some(*o);
        }
    }
    cur
}

/// Module-scope (root scope) bindings of the script program: `(name, symbol)`.
pub fn module_bindings<'a>(file: &LintFile<'a>) -> Vec<(&'a str, SymbolId)> {
    let Some(sem) = file.semantic else {
        return Vec::new();
    };
    let sc = sem.scoping();
    sc.get_bindings(sc.root_scope_id())
        .iter()
        .map(|(n, s)| (n.as_str(), *s))
        .collect()
}

/// `utils.getRegisteredComponents(obj)`.
pub fn registered_components<'a>(
    obj: &'a ObjectExpression<'a>,
) -> Vec<(&'a ObjectProperty<'a>, String)> {
    let Some(Expression::ObjectExpression(o)) = js::properties(obj)
        .find(|p| {
            js::prop_name(p).as_deref() == Some("components")
                && matches!(js::skip_parens(&p.value), Expression::ObjectExpression(_))
        })
        .map(|p| js::skip_parens(&p.value))
    else {
        return Vec::new();
    };
    js::properties(o)
        .filter_map(|p| js::prop_name(p).map(|n| (p, n)))
        .collect()
}

/// A tree with ESTree-like parent links: the script [`Semantic`] or a template [`Nodes`].
pub trait Tree<'a> {
    type Id: Copy + Eq;
    fn kind(&self, id: Self::Id) -> AstKind<'a>;
    /// Raw parent (`None` at a root).
    fn raw_parent(&self, id: Self::Id) -> Option<Self::Id>;
    /// ESTree parent: parentheses skipped.
    fn parent_of(&self, mut id: Self::Id) -> Option<Self::Id> {
        loop {
            let p = self.raw_parent(id)?;
            if matches!(self.kind(p), AstKind::ParenthesizedExpression(_)) {
                id = p;
                continue;
            }
            return Some(p);
        }
    }
    fn span_of(&self, id: Self::Id) -> Span {
        self.kind(id).span()
    }
}

impl<'a> Tree<'a> for Semantic<'a> {
    type Id = NodeId;
    fn kind(&self, id: NodeId) -> AstKind<'a> {
        self.nodes().kind(id)
    }
    fn raw_parent(&self, id: NodeId) -> Option<NodeId> {
        if matches!(self.nodes().kind(id), AstKind::Program(_)) {
            return None;
        }
        Some(self.nodes().parent_id(id))
    }
}

impl<'a> Tree<'a> for Nodes<'a> {
    type Id = usize;
    fn kind(&self, id: usize) -> AstKind<'a> {
        self.kinds[id]
    }
    fn raw_parent(&self, id: usize) -> Option<usize> {
        self.parents[id]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationKind {
    Assignment,
    Update,
    Call,
}

/// `utils.findMutating(node)`: the mutation the member chain starting at `start` ends in.
pub struct Mutation<I> {
    pub kind: MutationKind,
    /// The mutating node (assignment / update / call).
    pub node: I,
    /// The member expressions walked through (`pathNodes`).
    pub path: Vec<I>,
}

/// Static name of a member expression kind (`getStaticPropertyName`).
pub fn member_kind_name(k: &AstKind) -> Option<String> {
    match k {
        AstKind::StaticMemberExpression(m) => Some(m.property.name.to_string()),
        AstKind::ComputedMemberExpression(m) => js::string_literal_value(&m.expression),
        _ => None,
    }
}

fn member_object<'b, 'a>(k: &AstKind<'a>) -> Option<&'b Expression<'a>>
where
    'a: 'b,
{
    match k {
        AstKind::StaticMemberExpression(m) => Some(&m.object),
        AstKind::ComputedMemberExpression(m) => Some(&m.object),
        AstKind::PrivateFieldExpression(m) => Some(&m.object),
        _ => None,
    }
}

pub fn find_mutating<'a, T: Tree<'a>>(t: &T, start: T::Id) -> Option<Mutation<T::Id>> {
    let mut path: Vec<T::Id> = Vec::new();
    let mut node = start;
    loop {
        let target = t.parent_of(node)?;
        let nspan = t.span_of(node);
        let is = |e: &Expression| js::skip_parens(e).span() == nspan;
        match t.kind(target) {
            AstKind::AssignmentExpression(a) => {
                return (a.left.span() == nspan).then_some(Mutation {
                    kind: MutationKind::Assignment,
                    node: target,
                    path,
                });
            }
            AstKind::UpdateExpression(_) => {
                return Some(Mutation {
                    kind: MutationKind::Update,
                    node: target,
                    path,
                });
            }
            AstKind::UnaryExpression(u) => {
                return (u.operator == UnaryOperator::Delete).then_some(Mutation {
                    kind: MutationKind::Update,
                    node: target,
                    path,
                });
            }
            AstKind::CallExpression(c) => {
                if let Some(mem) = path.last()
                    && is(&c.callee)
                    && member_kind_name(&t.kind(*mem)).is_some_and(|n| {
                        matches!(
                            n.as_str(),
                            "push"
                                | "pop"
                                | "shift"
                                | "unshift"
                                | "reverse"
                                | "splice"
                                | "sort"
                                | "copyWithin"
                                | "fill"
                        )
                    })
                {
                    path.pop();
                    return Some(Mutation {
                        kind: MutationKind::Call,
                        node: target,
                        path,
                    });
                }
                let assign = c
                    .arguments
                    .first()
                    .and_then(|a| a.as_expression())
                    .is_some_and(is)
                    && match js::skip_parens(&c.callee) {
                        Expression::StaticMemberExpression(m) => {
                            m.property.name == "assign"
                                && matches!(&m.object, Expression::Identifier(o) if o.name == "Object")
                        }
                        Expression::ComputedMemberExpression(m) => {
                            matches!(&m.expression, Expression::Identifier(p) if p.name == "assign")
                                && matches!(&m.object, Expression::Identifier(o) if o.name == "Object")
                        }
                        _ => false,
                    };
                return assign.then_some(Mutation {
                    kind: MutationKind::Call,
                    node: target,
                    path,
                });
            }
            k @ (AstKind::StaticMemberExpression(_)
            | AstKind::ComputedMemberExpression(_)
            | AstKind::PrivateFieldExpression(_)) => {
                if !member_object(&k).is_some_and(is) {
                    return None;
                }
                path.push(target);
                node = target;
            }
            AstKind::ChainExpression(_) => node = target,
            _ => return None,
        }
    }
}

/// ESLint's `es2026` builtin globals (the global scope of `ecmaVersion: "latest"`).
pub const ES_GLOBALS: &[&str] = &[
    "Array",
    "Boolean",
    "constructor",
    "Date",
    "decodeURI",
    "decodeURIComponent",
    "encodeURI",
    "encodeURIComponent",
    "Error",
    "escape",
    "eval",
    "EvalError",
    "Function",
    "hasOwnProperty",
    "Infinity",
    "isFinite",
    "isNaN",
    "isPrototypeOf",
    "Math",
    "NaN",
    "Number",
    "Object",
    "parseFloat",
    "parseInt",
    "propertyIsEnumerable",
    "RangeError",
    "ReferenceError",
    "RegExp",
    "String",
    "SyntaxError",
    "toLocaleString",
    "toString",
    "TypeError",
    "undefined",
    "unescape",
    "URIError",
    "valueOf",
    "JSON",
    "ArrayBuffer",
    "DataView",
    "Float32Array",
    "Float64Array",
    "Int16Array",
    "Int32Array",
    "Int8Array",
    "Intl",
    "Map",
    "Promise",
    "Proxy",
    "Reflect",
    "Set",
    "Symbol",
    "Uint16Array",
    "Uint32Array",
    "Uint8Array",
    "Uint8ClampedArray",
    "WeakMap",
    "WeakSet",
    "Atomics",
    "SharedArrayBuffer",
    "BigInt",
    "BigInt64Array",
    "BigUint64Array",
    "globalThis",
    "AggregateError",
    "FinalizationRegistry",
    "WeakRef",
    "Float16Array",
    "Iterator",
    "AsyncDisposableStack",
    "DisposableStack",
    "SuppressedError",
    "Temporal",
];

/// `tracker.iterateGlobalReferences({ name: { [CALL]: true } })`: calls of unresolved globals.
// ponytail: `window.x()` / `globalThis.x()` member forms are not followed.
pub fn global_calls<'a>(
    file: &LintFile<'a>,
    names: &[&str],
) -> Vec<(String, &'a CallExpression<'a>)> {
    let Some(sem) = file.semantic else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (name, ids) in sem.scoping().root_unresolved_references() {
        if !names.contains(&name.as_str()) {
            continue;
        }
        for rid in ids {
            let id = sem.scoping().get_reference(*rid).node_id();
            if let Some(p) = sem.parent_of(id)
                && let AstKind::CallExpression(c) = sem.nodes().kind(p)
                && js::skip_parens(&c.callee).span() == sem.nodes().kind(id).span()
            {
                out.push((name.to_string(), c));
            }
        }
    }
    out.sort_by_key(|(_, c)| c.span.start);
    out
}

/// Member paths read by `<style>` `v-bind()`s (`getStyleVariablesContext().references`):
/// `v-bind(a.b)` / `v-bind('a.b')` → `[(a, span), (b, span)]`.
// ponytail: only identifier paths are understood; another expression contributes its leading
// identifier (parse it with oxc when a corpus needs more).
pub fn style_var_paths(doc: &Document) -> Vec<Vec<(String, Span)>> {
    let mut out = Vec::new();
    for vb in crate::utils::tpl_misc::style_v_binds(doc) {
        let text = doc.slice(vb);
        let (Some(open), Some(close)) = (text.find('('), text.rfind(')')) else {
            continue;
        };
        let mut start = open + 1;
        let mut inner = &text[start..close];
        let lead = inner.len() - inner.trim_start().len();
        start += lead;
        inner = inner.trim();
        if inner.len() >= 2
            && (inner.starts_with('\'') || inner.starts_with('"'))
            && inner.ends_with(&inner[..1])
        {
            start += 1;
            inner = &inner[1..inner.len() - 1];
        }
        let mut path = Vec::new();
        let mut pos = 0usize;
        for seg in inner.split('.') {
            let ok = !seg.is_empty()
                && !seg.starts_with(|c: char| c.is_ascii_digit())
                && seg
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '$');
            if !ok {
                if path.is_empty() {
                    let ident: String = seg
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
                        .collect();
                    if !ident.is_empty() && !ident.starts_with(|c: char| c.is_ascii_digit()) {
                        let s = vb.start + (start + pos) as u32;
                        path.push((ident.clone(), Span::new(s, s + ident.len() as u32)));
                    }
                }
                break;
            }
            let s = vb.start + (start + pos) as u32;
            path.push((seg.to_string(), Span::new(s, s + seg.len() as u32)));
            pos += seg.len() + 1;
        }
        out.push(path);
    }
    out
}

/// `@typescript-eslint/scope-manager` implicit `esnext` lib globals (types and values).
pub static TS_LIB_GLOBALS: std::sync::LazyLock<HashSet<String>> = std::sync::LazyLock::new(|| {
    serde_json::from_str::<Vec<String>>(include_str!("data/ts-lib-esnext.json"))
        .expect("valid name list")
        .into_iter()
        .collect()
});

/// Whether `name` is a variable of the ESLint global scope: the ES builtins, plus the
/// TypeScript lib globals when the script is parsed as TypeScript.
pub fn is_global_name(file: &LintFile, name: &str) -> bool {
    ES_GLOBALS.contains(&name) || (file.ts && TS_LIB_GLOBALS.contains(name))
}

/// Every global-scope variable name (see [`is_global_name`]).
pub fn global_names(file: &LintFile) -> Vec<String> {
    let mut v: Vec<String> = ES_GLOBALS.iter().map(|s| s.to_string()).collect();
    if file.ts {
        v.extend(TS_LIB_GLOBALS.iter().cloned());
    }
    v
}
