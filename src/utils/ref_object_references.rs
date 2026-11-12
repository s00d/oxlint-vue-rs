//! Port of eslint-plugin-vue `lib/utils/ref-object-references.ts`: where ref objects
//! (`ref()`, `computed()`, `toRef(s)()`, `shallowRef()`, `customRef()`, `useTemplateRef()`,
//! `defineModel()`) flow to.
//!
//! ponytail: the `toRefs` property extraction follows only member accesses and destructuring
//! (`property-references.ts` is not ported in full); extend when a rule needs more.

use std::collections::HashMap;

use oxc::ast::AstKind;
use oxc::ast::ast::*;
use oxc::semantic::{NodeId, Semantic, SymbolId};

use super::js;
use super::reference_tracker::{TraceMap, esm_calls};
use crate::rule::LintFile;

/// Kind of a ref object usage node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    /// An expression evaluating to the ref object (identifier, member, call).
    Expression,
    /// A pattern the ref object is destructured / assigned into.
    Pattern,
}

/// One usage of a ref object.
#[derive(Debug, Clone)]
pub struct RefObjectReference {
    pub kind: RefKind,
    /// The defining API (`ref`, `computed`, `toRefs`, `defineModel`, …).
    pub method: String,
    /// Node id of the defining call.
    pub define: NodeId,
    /// `defineChain`: the initialization path (this assignment source first, the call last).
    pub chain: Vec<NodeId>,
    /// `variableDeclarator` of an identifier reference whose variable has a single
    /// `const/let/var` definition.
    pub declarator: Option<NodeId>,
}

/// Ref object usages by node id (identifier references, member/call expressions, object patterns
/// and object assignment targets).
#[derive(Default)]
pub struct RefObjectReferences {
    map: HashMap<NodeId, RefObjectReference>,
}

impl RefObjectReferences {
    pub fn get(&self, id: NodeId) -> Option<&RefObjectReference> {
        self.map.get(&id)
    }
}

/// Composition API modules (`createCompositionApiTraceMap`).
pub const COMPOSITION_API_MODULES: [&str; 4] = [
    "vue",
    "@vue/composition-api",
    "@nuxtjs/composition-api",
    "#imports",
];

/// A pattern side: a binding pattern or an assignment target.
#[derive(Clone, Copy)]
enum Pat<'a> {
    Binding(&'a BindingPattern<'a>),
    Target(&'a AssignmentTarget<'a>),
}

#[derive(Clone)]
struct RefCtx {
    method: String,
    define: NodeId,
    chain: Vec<NodeId>,
}

impl RefCtx {
    fn through(&self, id: NodeId) -> RefCtx {
        let mut chain = vec![id];
        chain.extend(self.chain.iter().copied());
        RefCtx {
            method: self.method.clone(),
            define: self.define,
            chain,
        }
    }
}

/// `extractRefObjectReferences(context)`.
pub fn extract_ref_object_references<'a>(file: &LintFile<'a>) -> RefObjectReferences {
    let Some(sem) = file.semantic else {
        return RefObjectReferences::default();
    };
    let mut ex = Extractor {
        sem,
        refs: RefObjectReferences::default(),
        processed: Vec::new(),
    };
    let map = TraceMap::calls(&[
        "ref",
        "computed",
        "toRef",
        "customRef",
        "shallowRef",
        "useTemplateRef",
        "toRefs",
    ]);
    let modules: Vec<(&str, &TraceMap)> =
        COMPOSITION_API_MODULES.iter().map(|m| (*m, &map)).collect();
    for hit in esm_calls(file, &modules) {
        let method = hit.path.last().cloned().unwrap_or_default();
        ex.define_ref(hit.call, &method);
    }
    // `tracker.iterateGlobalReferences(map)`: calls of the declared globals.
    for (name, c) in crate::utils::tpl_analysis::global_calls(
        file,
        &[
            "ref",
            "computed",
            "toRef",
            "customRef",
            "shallowRef",
            "useTemplateRef",
            "toRefs",
        ],
    ) {
        ex.define_ref(c, &name);
    }
    // `defineModel()` calls (an unresolved / auto-defined global).
    for n in sem.nodes().iter() {
        if let AstKind::CallExpression(c) = n.kind()
            && let Expression::Identifier(id) = &c.callee
            && id.name == "defineModel"
            && js::ref_symbol(sem, id).is_none()
        {
            ex.define_model(c);
        }
    }
    ex.refs
}

struct Extractor<'s, 'a> {
    sem: &'s Semantic<'a>,
    refs: RefObjectReferences,
    processed: Vec<SymbolId>,
}

impl<'a> Extractor<'_, 'a> {
    fn set(&mut self, id: NodeId, kind: RefKind, ctx: &RefCtx) {
        self.set_with(id, kind, ctx, None);
    }

    fn set_with(&mut self, id: NodeId, kind: RefKind, ctx: &RefCtx, declarator: Option<NodeId>) {
        self.refs.map.insert(
            id,
            RefObjectReference {
                kind,
                method: ctx.method.clone(),
                define: ctx.define,
                chain: ctx.chain.clone(),
                declarator,
            },
        );
    }

    /// Pattern the expression `id` is assigned to (`x = <id>` / `const x = <id>`).
    fn assigned_pattern(&self, id: NodeId) -> Option<Pat<'a>> {
        let (_, parent) = js::parent(self.sem, id);
        match parent {
            AstKind::VariableDeclarator(d)
                if d.init
                    .as_ref()
                    .is_some_and(|i| js::skip_parens(i).node_id() == id) =>
            {
                Some(Pat::Binding(&d.id))
            }
            AstKind::AssignmentExpression(a)
                if a.operator == AssignmentOperator::Assign
                    && js::skip_parens(&a.right).node_id() == id =>
            {
                Some(Pat::Target(&a.left))
            }
            _ => None,
        }
    }

    fn define_ref(&mut self, call: &'a CallExpression<'a>, method: &str) {
        let ctx = RefCtx {
            method: method.to_string(),
            define: call.node_id(),
            chain: vec![call.node_id()],
        };
        let Some(pattern) = self.assigned_pattern(call.node_id()) else {
            if method != "toRefs" {
                self.set(call.node_id(), RefKind::Expression, &ctx);
            }
            return;
        };
        if method == "toRefs" {
            let mut nests = Vec::new();
            self.property_nests(pattern, &mut nests, 0);
            for n in nests {
                match n {
                    Nest::Expr(id) => self.member_expression(id, &ctx),
                    Nest::Pat(p) => self.pattern(p, &ctx),
                }
            }
        } else {
            self.pattern(pattern, &ctx);
        }
    }

    fn define_model(&mut self, call: &'a CallExpression<'a>) {
        let ctx = RefCtx {
            method: "defineModel".into(),
            define: call.node_id(),
            chain: vec![call.node_id()],
        };
        let Some(mut pattern) = self.assigned_pattern(call.node_id()) else {
            return;
        };
        // ponytail: `[m] = defineModel()` assignment targets are not unwrapped, only declarations.
        if let Pat::Binding(BindingPattern::ArrayPattern(a)) = pattern
            && let Some(Some(first)) = a.elements.first()
        {
            pattern = Pat::Binding(first);
        }
        self.pattern(pattern, &ctx);
    }

    /// `processExpression`: follow `x = <node>` / `const x = <node>`.
    fn expression(&mut self, id: NodeId, ctx: &RefCtx) -> bool {
        match self.assigned_pattern(id) {
            Some(p) => {
                self.pattern(p, &ctx.through(id));
                true
            }
            None => false,
        }
    }

    fn member_expression(&mut self, id: NodeId, ctx: &RefCtx) {
        if !self.expression(id, ctx) {
            self.set(id, RefKind::Expression, ctx);
        }
    }

    fn pattern(&mut self, p: Pat<'a>, ctx: &RefCtx) {
        match p {
            Pat::Binding(BindingPattern::BindingIdentifier(b)) => {
                self.identifier(b.symbol_id(), ctx)
            }
            Pat::Binding(BindingPattern::ObjectPattern(o)) => {
                self.set(o.node_id(), RefKind::Pattern, ctx)
            }
            Pat::Binding(BindingPattern::AssignmentPattern(a)) => {
                self.pattern(Pat::Binding(&a.left), ctx)
            }
            Pat::Binding(BindingPattern::ArrayPattern(_)) => {}
            Pat::Target(AssignmentTarget::AssignmentTargetIdentifier(id)) => {
                if let Some(sym) = js::ref_symbol(self.sem, id) {
                    self.identifier(sym, ctx);
                }
            }
            Pat::Target(AssignmentTarget::ObjectAssignmentTarget(o)) => {
                self.set(o.node_id(), RefKind::Pattern, ctx)
            }
            Pat::Target(_) => {}
        }
    }

    /// `processIdentifierPattern`.
    fn identifier(&mut self, sym: SymbolId, ctx: &RefCtx) {
        if self.processed.contains(&sym) {
            return;
        }
        self.processed.push(sym);
        let refs: Vec<(NodeId, bool, bool)> = self
            .sem
            .scoping()
            .get_resolved_references(sym)
            .map(|r| (r.node_id(), r.is_read(), r.is_write()))
            .collect();
        let declarator = match js::single_declaration(self.sem, sym) {
            Some(AstKind::VariableDeclarator(d)) => Some(d.node_id()),
            _ => None,
        };
        for (id, read, write) in refs {
            if read && self.expression(id, ctx) {
                continue;
            }
            self.set_with(
                id,
                if write {
                    RefKind::Pattern
                } else {
                    RefKind::Expression
                },
                ctx,
                declarator,
            );
        }
    }

    /// Nested nodes for each property of the object a pattern receives (`toRefs` results).
    fn property_nests(&self, p: Pat<'a>, out: &mut Vec<Nest<'a>>, depth: u32) {
        if depth > 16 {
            return;
        }
        match p {
            Pat::Binding(BindingPattern::ObjectPattern(o)) => {
                for prop in &o.properties {
                    if js::binding_prop_name(prop).is_some() {
                        out.push(Nest::Pat(Pat::Binding(&prop.value)));
                    }
                }
            }
            Pat::Binding(BindingPattern::AssignmentPattern(a)) => {
                self.property_nests(Pat::Binding(&a.left), out, depth + 1)
            }
            Pat::Binding(BindingPattern::BindingIdentifier(b)) => {
                for r in self
                    .sem
                    .scoping()
                    .get_resolved_references(b.symbol_id())
                    .filter(|r| r.is_read())
                {
                    let id = r.node_id();
                    let (pid, parent) = js::parent(self.sem, id);
                    match parent {
                        AstKind::StaticMemberExpression(m)
                            if js::skip_parens(&m.object).node_id() == id =>
                        {
                            out.push(Nest::Expr(pid))
                        }
                        AstKind::ComputedMemberExpression(m)
                            if js::skip_parens(&m.object).node_id() == id
                                && js::string_literal_value(&m.expression).is_some() =>
                        {
                            out.push(Nest::Expr(pid))
                        }
                        _ => {
                            if let Some(next) = self.assigned_pattern(id) {
                                self.property_nests(next, out, depth + 1);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

enum Nest<'a> {
    Expr(NodeId),
    Pat(Pat<'a>),
}

/// `ReferenceTracker.iterateGlobalReferences` only sees globals declared in the config
/// (`globalScope.set`), not every unresolved name (e.g. Nuxt auto-imports stay invisible).
pub(crate) fn is_declared_global(file: &LintFile, name: &str) -> bool {
    file.globals
        .get(name)
        .is_some_and(|v| !matches!(v.as_str(), Some("off")) && !v.is_null())
}
