//! Port of eslint-plugin-vue `lib/utils/property-references.ts`: which properties of an object
//! (component instance, props, a function parameter, …) an expression / pattern reads.
//!
//! Nodes are addressed by [`NodeRef`]: script nodes through the [`Semantic`] tree, template
//! nodes through per-container [`Nodes`] lists. `getNestNodes` is not ported (only
//! `ref-object-references` uses it, and that has its own extraction).

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use oxc::ast::AstKind;
use oxc::ast::ast::*;
use oxc::semantic::{NodeId, Semantic, SymbolId};
use oxc::span::{GetSpan, Span};

use crate::ast::Document;
use crate::rule::LintFile;
use crate::utils::js;
use crate::utils::ref_object_references::COMPOSITION_API_MODULES;
use crate::utils::reference_tracker::{TraceMap, esm_calls};
use crate::utils::tpl_analysis::{Nodes, is_global_name};

/// A node of the script program or of a template expression container.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeRef {
    Script(NodeId),
    /// (index into `Document::exprs`, index into that container's [`Nodes`]).
    Template(u32, u32),
}

/// `hasProperty` options.
#[derive(Default, Clone, Copy)]
pub struct HasOpts {
    pub unknown_call_as_any: bool,
    pub external_call_as_any: bool,
}

/// An object-pattern property value (`getNestFromPattern` input).
#[derive(Clone, Copy)]
pub enum PatVal<'a> {
    Binding(&'a BindingPattern<'a>),
    TargetIdent(&'a IdentifierReference<'a>),
    TargetMaybeDefault(&'a AssignmentTargetMaybeDefault<'a>),
    Target(&'a AssignmentTarget<'a>),
}

#[derive(Clone)]
pub enum Nest {
    Any,
    Never,
    Expr(NodeRef, bool),
    /// `extractFromExpression(toRefCall).getNest('value')`.
    ToRefValue(NodeRef),
}

/// `IPropertyReferences`.
pub enum PRef<'a> {
    Any,
    Never,
    Member {
        node: NodeRef,
        name: String,
        prop_span: Span,
        tpl: bool,
    },
    Object(Vec<(String, Vec<(Span, PatVal<'a>)>)>),
    UnknownCall {
        external: bool,
    },
    /// `extractFromPath` (one span per segment).
    Path(Vec<(String, Span)>),
    NameLit {
        name: String,
        span: Span,
    },
    Name {
        name: String,
        span: Span,
        nest: Nest,
    },
    ToRefs(Rc<PRef<'a>>),
    Merge(Vec<Rc<PRef<'a>>>),
}

pub type Refs<'a> = Rc<PRef<'a>>;

/// `mergePropertyReferences`.
pub fn merge<'a>(mut v: Vec<Refs<'a>>) -> Refs<'a> {
    match v.len() {
        0 => Rc::new(PRef::Never),
        1 => v.pop().unwrap(),
        _ => Rc::new(PRef::Merge(v)),
    }
}

impl<'a> PRef<'a> {
    pub fn has_property(&self, name: &str, o: HasOpts) -> bool {
        match self {
            PRef::Any => true,
            PRef::Never => false,
            PRef::Member { name: n, .. }
            | PRef::NameLit { name: n, .. }
            | PRef::Name { name: n, .. } => n == name,
            PRef::Object(props) => props.iter().any(|(n, _)| n == name),
            PRef::UnknownCall { external } => {
                o.unknown_call_as_any || (o.external_call_as_any && *external)
            }
            PRef::Path(segs) => segs[0].0 == name,
            PRef::ToRefs(b) => b.has_property(name, o),
            PRef::Merge(v) => v.iter().any(|r| r.has_property(name, o)),
        }
    }

    /// `allProperties()`: names in insertion order with their nodes.
    pub fn all_properties(&self) -> Vec<(String, Vec<Span>)> {
        match self {
            PRef::Any | PRef::Never | PRef::UnknownCall { .. } => Vec::new(),
            PRef::Member {
                name, prop_span, ..
            } => vec![(name.clone(), vec![*prop_span])],
            PRef::Object(props) => props
                .iter()
                .map(|(n, v)| (n.clone(), v.iter().map(|(s, _)| *s).collect()))
                .collect(),
            PRef::Path(segs) => vec![(segs[0].0.clone(), vec![segs[0].1])],
            PRef::NameLit { name, span } | PRef::Name { name, span, .. } => {
                vec![(name.clone(), vec![*span])]
            }
            PRef::ToRefs(b) => b.all_properties(),
            PRef::Merge(v) => {
                let mut out: Vec<(String, Vec<Span>)> = Vec::new();
                for r in v {
                    for (n, nodes) in r.all_properties() {
                        if let Some((_, e)) = out.iter_mut().find(|(k, _)| *k == n) {
                            for s in nodes {
                                if !e.contains(&s) {
                                    e.push(s);
                                }
                            }
                        } else {
                            out.push((n, nodes));
                        }
                    }
                }
                out
            }
        }
    }

    pub fn get_nest(&self, ex: &Extractor<'_, 'a>, name: &str) -> Refs<'a> {
        match self {
            PRef::Any | PRef::UnknownCall { .. } => Rc::new(PRef::Any),
            PRef::Never => Rc::new(PRef::Never),
            PRef::Member {
                node, name: n, tpl, ..
            } => {
                if n == name {
                    ex.extract_from_expression(*node, *tpl)
                } else {
                    Rc::new(PRef::Never)
                }
            }
            PRef::Object(props) => match props.iter().find(|(n, _)| n == name) {
                Some((_, v)) => merge(v.iter().map(|(_, p)| ex.nest_from_pattern(*p)).collect()),
                None => Rc::new(PRef::Never),
            },
            PRef::Path(segs) => {
                if segs[0].0 != name {
                    Rc::new(PRef::Never)
                } else if segs.len() == 1 {
                    Rc::new(PRef::Any)
                } else {
                    Rc::new(PRef::Path(segs[1..].to_vec()))
                }
            }
            PRef::NameLit { name: n, .. } => {
                Rc::new(if n == name { PRef::Any } else { PRef::Never })
            }
            PRef::Name { name: n, nest, .. } => {
                if n != name {
                    return Rc::new(PRef::Never);
                }
                match nest {
                    Nest::Any => Rc::new(PRef::Any),
                    Nest::Never => Rc::new(PRef::Never),
                    Nest::Expr(r, tpl) => ex.extract_from_expression(*r, *tpl),
                    Nest::ToRefValue(r) => {
                        ex.extract_from_expression(*r, false).get_nest(ex, "value")
                    }
                }
            }
            PRef::ToRefs(b) => b.get_nest(ex, name).get_nest(ex, "value"),
            PRef::Merge(v) => merge(
                v.iter()
                    .filter(|r| r.has_property(name, HasOpts::default()))
                    .map(|r| r.get_nest(ex, name))
                    .collect(),
            ),
        }
    }
}

/// ESTree-shaped parent of a node.
enum Parent<'a> {
    Node(NodeRef, AstKind<'a>),
    /// The template `VExpressionContainer`.
    Container,
    None,
}

/// `definePropertyReferenceExtractor(context, options)`.
pub struct Extractor<'f, 'a> {
    file: &'f LintFile<'a>,
    sem: Option<&'f Semantic<'a>>,
    doc: Option<&'f Document<'a>>,
    unknown_member_as_unreferenced: bool,
    return_as_unreferenced: bool,
    functional_template: bool,
    tpl_nodes: RefCell<HashMap<u32, Rc<Nodes<'a>>>>,
    cache_expr: RefCell<HashMap<NodeRef, Refs<'a>>>,
    cache_pat: RefCell<HashMap<usize, Refs<'a>>>,
    cache_fn: RefCell<HashMap<(u32, usize), Refs<'a>>>,
    to_ref: RefCell<Option<(HashSet<Span>, HashSet<Span>)>>,
}

impl<'f, 'a> Extractor<'f, 'a> {
    pub fn new(
        file: &'f LintFile<'a>,
        unknown_member_as_unreferenced: bool,
        return_as_unreferenced: bool,
    ) -> Self {
        let doc = file.doc.as_ref();
        let functional_template = doc
            .and_then(|d| {
                d.template_body
                    .map(|t| d.element(t).attr("functional").is_some())
            })
            .unwrap_or(false);
        Extractor {
            file,
            sem: file.semantic,
            doc,
            unknown_member_as_unreferenced,
            return_as_unreferenced,
            functional_template,
            tpl_nodes: RefCell::default(),
            cache_expr: RefCell::default(),
            cache_pat: RefCell::default(),
            cache_fn: RefCell::default(),
            to_ref: RefCell::default(),
        }
    }

    fn nodes(&self, ci: u32) -> Rc<Nodes<'a>> {
        self.tpl_nodes
            .borrow_mut()
            .entry(ci)
            .or_insert_with(|| Rc::new(Nodes::of(&self.doc.unwrap().exprs[ci as usize])))
            .clone()
    }

    /// Node index of the template identifier reference at `span` in container `ci`.
    pub fn template_ident(&self, ci: u32, span: Span) -> Option<NodeRef> {
        let n = self.nodes(ci);
        n.kinds
            .iter()
            .position(|k| matches!(k, AstKind::IdentifierReference(id) if id.span == span))
            .map(|i| NodeRef::Template(ci, i as u32))
    }

    fn kind(&self, r: NodeRef) -> AstKind<'a> {
        match r {
            NodeRef::Script(id) => self.sem.unwrap().nodes().kind(id),
            NodeRef::Template(ci, ni) => self.nodes(ci).kinds[ni as usize],
        }
    }

    fn raw_parent(&self, r: NodeRef) -> Parent<'a> {
        match r {
            NodeRef::Script(id) => {
                let sem = self.sem.unwrap();
                if matches!(sem.nodes().kind(id), AstKind::Program(_)) {
                    return Parent::None;
                }
                let p = sem.nodes().parent_id(id);
                Parent::Node(NodeRef::Script(p), sem.nodes().kind(p))
            }
            NodeRef::Template(ci, ni) => match self.nodes(ci).parents[ni as usize] {
                Some(p) => Parent::Node(NodeRef::Template(ci, p as u32), self.nodes(ci).kinds[p]),
                None => Parent::Container,
            },
        }
    }

    /// ESTree parent: parentheses skipped.
    fn parent(&self, mut r: NodeRef) -> Parent<'a> {
        loop {
            match self.raw_parent(r) {
                Parent::Node(p, AstKind::ParenthesizedExpression(_)) => r = p,
                other => return other,
            }
        }
    }

    fn span(&self, r: NodeRef) -> Span {
        self.kind(r).span()
    }

    fn symbol_refs(&self, sym: SymbolId) -> Vec<NodeId> {
        self.sem
            .map(|s| {
                s.scoping()
                    .get_resolved_references(sym)
                    .map(|r| r.node_id())
                    .collect()
            })
            .unwrap_or_default()
    }

    // ---------------------------------------------------------------------------------------

    /// `extractFromExpression(node, isWithinTemplate)`.
    pub fn extract_from_expression(&self, node: NodeRef, tpl: bool) -> Refs<'a> {
        if let Some(r) = self.cache_expr.borrow().get(&node) {
            return r.clone();
        }
        self.cache_expr
            .borrow_mut()
            .insert(node, Rc::new(PRef::Any));
        let r = self.extract_expr_uncached(node, tpl);
        self.cache_expr.borrow_mut().insert(node, r.clone());
        r
    }

    fn extract_expr_uncached(&self, node: NodeRef, tpl: bool) -> Refs<'a> {
        let nspan = self.span(node);
        let is = |e: &Expression| js::skip_parens(e).span() == nspan;
        let never = || Rc::new(PRef::Never);
        let unknown = || {
            Rc::new(if self.unknown_member_as_unreferenced {
                PRef::Never
            } else {
                PRef::Any
            })
        };
        match self.parent(node) {
            Parent::Node(_, AstKind::AssignmentExpression(a)) => {
                if !tpl && is(&a.right) && a.operator == AssignmentOperator::Assign {
                    self.extract_from_pattern(PatVal::Target(&a.left))
                } else {
                    never()
                }
            }
            Parent::Node(_, AstKind::VariableDeclarator(d)) => {
                if !tpl && d.init.as_ref().is_some_and(is) {
                    self.extract_from_pattern(PatVal::Binding(&d.id))
                } else {
                    never()
                }
            }
            Parent::Node(
                p,
                k @ (AstKind::StaticMemberExpression(_)
                | AstKind::ComputedMemberExpression(_)
                | AstKind::PrivateFieldExpression(_)),
            ) => {
                let obj = match k {
                    AstKind::StaticMemberExpression(m) => &m.object,
                    AstKind::ComputedMemberExpression(m) => &m.object,
                    AstKind::PrivateFieldExpression(m) => &m.object,
                    _ => unreachable!(),
                };
                if !is(obj) {
                    return never();
                }
                let name = member_name(&k);
                if name.as_deref() == Some("$props")
                    && let Parent::Node(
                        gp,
                        gk @ (AstKind::StaticMemberExpression(_)
                        | AstKind::ComputedMemberExpression(_)
                        | AstKind::PrivateFieldExpression(_)),
                    ) = self.parent(p)
                {
                    return match member_name(&gk) {
                        Some(pn) => Rc::new(PRef::Member {
                            node: gp,
                            name: pn,
                            prop_span: prop_span(&gk),
                            tpl,
                        }),
                        None => unknown(),
                    };
                }
                match name {
                    Some(n) => Rc::new(PRef::Member {
                        node: p,
                        name: n,
                        prop_span: prop_span(&k),
                        tpl,
                    }),
                    None => unknown(),
                }
            }
            Parent::Node(_, AstKind::CallExpression(c)) => {
                let idx = c
                    .arguments
                    .iter()
                    .position(|a| a.as_expression().is_some_and(is));
                match idx {
                    Some(i) if !tpl => self.extract_from_call(c, i),
                    _ => never(),
                }
            }
            Parent::Node(p, AstKind::ChainExpression(_)) => self.extract_from_expression(p, tpl),
            Parent::Node(_, AstKind::ReturnStatement(_)) => {
                Rc::new(if self.return_as_unreferenced {
                    PRef::Never
                } else {
                    PRef::Any
                })
            }
            Parent::Container => Rc::new(PRef::Any),
            parent @ Parent::Node(
                _,
                AstKind::ArrowFunctionExpression(_)
                | AstKind::ObjectProperty(_)
                | AstKind::ArrayExpression(_),
            ) => Rc::new(if self.maybe_externally_used(&parent, nspan) {
                PRef::Any
            } else {
                PRef::Never
            }),
            _ => never(),
        }
    }

    /// `isMaybeExternallyUsed(parent)` (`node_span` = the original node).
    fn maybe_externally_used(&self, p: &Parent<'a>, node_span: Span) -> bool {
        match p {
            Parent::Container | Parent::Node(_, AstKind::ReturnStatement(_)) => true,
            Parent::Node(r, AstKind::ArrayExpression(_)) => {
                self.maybe_externally_used(&self.parent(*r), node_span)
            }
            Parent::Node(r, AstKind::ObjectProperty(_)) => match self.parent(*r) {
                Parent::Node(o, _) => self.maybe_externally_used(&self.parent(o), node_span),
                _ => false,
            },
            Parent::Node(_, AstKind::ArrowFunctionExpression(f)) => f
                .body
                .as_expression()
                .is_some_and(|e| js::skip_parens(e).span() == node_span),
            _ => false,
        }
    }

    /// `extractFromPattern(node)`.
    fn extract_from_pattern(&self, p: PatVal<'a>) -> Refs<'a> {
        let k = pat_key(p);
        if let Some(r) = self.cache_pat.borrow().get(&k) {
            return r.clone();
        }
        self.cache_pat.borrow_mut().insert(k, Rc::new(PRef::Any));
        let r = self.extract_pattern_uncached(p);
        self.cache_pat.borrow_mut().insert(k, r.clone());
        r
    }

    fn extract_pattern_uncached(&self, p: PatVal<'a>) -> Refs<'a> {
        match p {
            PatVal::Binding(b) => match b {
                BindingPattern::AssignmentPattern(a) => {
                    self.extract_pattern_uncached(PatVal::Binding(&a.left))
                }
                BindingPattern::ArrayPattern(_) => Rc::new(PRef::Never),
                BindingPattern::ObjectPattern(_) | BindingPattern::BindingIdentifier(_) => {
                    self.nest_from_pattern(p)
                }
            },
            PatVal::TargetMaybeDefault(
                AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(d),
            ) => self.extract_pattern_uncached(PatVal::Target(&d.binding)),
            PatVal::TargetMaybeDefault(t) => match t.as_assignment_target() {
                Some(t) => self.extract_pattern_uncached(PatVal::Target(t)),
                None => Rc::new(PRef::Never),
            },
            PatVal::Target(t) => match t {
                AssignmentTarget::ObjectAssignmentTarget(_)
                | AssignmentTarget::AssignmentTargetIdentifier(_) => self.nest_from_pattern(p),
                _ => Rc::new(PRef::Never),
            },
            PatVal::TargetIdent(_) => self.nest_from_pattern(p),
        }
    }

    /// `getNestFromPattern(pattern)`.
    fn nest_from_pattern(&self, p: PatVal<'a>) -> Refs<'a> {
        match p {
            PatVal::Binding(BindingPattern::ObjectPattern(o)) => {
                if o.rest.is_some() {
                    return Rc::new(PRef::Any);
                }
                let mut props: Vec<(String, Vec<(Span, PatVal<'a>)>)> = Vec::new();
                for prop in &o.properties {
                    let Some(name) = js::binding_prop_name(prop) else {
                        return Rc::new(PRef::Any);
                    };
                    add_prop(
                        &mut props,
                        name,
                        prop.key.span(),
                        PatVal::Binding(&prop.value),
                    );
                }
                Rc::new(PRef::Object(props))
            }
            PatVal::Target(AssignmentTarget::ObjectAssignmentTarget(o)) => {
                if o.rest.is_some() {
                    return Rc::new(PRef::Any);
                }
                let mut props: Vec<(String, Vec<(Span, PatVal<'a>)>)> = Vec::new();
                for prop in &o.properties {
                    match prop {
                        AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(i) => {
                            add_prop(
                                &mut props,
                                i.binding.name.to_string(),
                                i.binding.span,
                                PatVal::TargetIdent(&i.binding),
                            );
                        }
                        AssignmentTargetProperty::AssignmentTargetPropertyProperty(pp) => {
                            let name = match &pp.name {
                                PropertyKey::StaticIdentifier(id) if !pp.computed => {
                                    Some(id.name.to_string())
                                }
                                k => js::key_name(k),
                            };
                            let Some(name) = name else {
                                return Rc::new(PRef::Any);
                            };
                            add_prop(
                                &mut props,
                                name,
                                pp.name.span(),
                                PatVal::TargetMaybeDefault(&pp.binding),
                            );
                        }
                    }
                }
                Rc::new(PRef::Object(props))
            }
            PatVal::Binding(BindingPattern::BindingIdentifier(b)) => match b.symbol_id.get() {
                Some(sym) => self.extract_from_symbol(sym),
                None => Rc::new(PRef::Never),
            },
            PatVal::Binding(BindingPattern::AssignmentPattern(a)) => {
                self.nest_from_pattern(PatVal::Binding(&a.left))
            }
            PatVal::TargetIdent(id) => self.ident_target(id),
            PatVal::Target(AssignmentTarget::AssignmentTargetIdentifier(id)) => {
                self.ident_target(id)
            }
            PatVal::TargetMaybeDefault(
                AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(d),
            ) => self.nest_from_pattern(PatVal::Target(&d.binding)),
            PatVal::TargetMaybeDefault(t) => match t.as_assignment_target() {
                Some(t) => self.nest_from_pattern(PatVal::Target(t)),
                None => Rc::new(PRef::Any),
            },
            _ => Rc::new(PRef::Any),
        }
    }

    fn ident_target(&self, id: &IdentifierReference) -> Refs<'a> {
        match self.sem.and_then(|s| js::ref_symbol(s, id)) {
            Some(sym) => self.extract_from_symbol(sym),
            None => Rc::new(PRef::Never),
        }
    }

    /// `extractFromIdentifier`: merge of every reference of the variable.
    fn extract_from_symbol(&self, sym: SymbolId) -> Refs<'a> {
        merge(
            self.symbol_refs(sym)
                .into_iter()
                .map(|id| self.extract_from_expression(NodeRef::Script(id), false))
                .collect(),
        )
    }

    /// `extractFromCall(node, argIndex)`.
    fn extract_from_call(&self, call: &'a CallExpression<'a>, idx: usize) -> Refs<'a> {
        let Expression::Identifier(callee) = &call.callee else {
            return self.unknown_call(call);
        };
        match self.find_function(callee, 0) {
            Some(f) => self.extract_from_function_param(f, idx),
            None => {
                if idx == 0 {
                    let (to_ref, to_refs) = self.to_ref_sets();
                    if to_ref.contains(&call.span) {
                        return self.extract_from_to_ref(call);
                    }
                    if to_refs.contains(&call.span) {
                        return Rc::new(PRef::ToRefs(
                            self.extract_from_expression(NodeRef::Script(call.node_id()), false),
                        ));
                    }
                }
                self.unknown_call(call)
            }
        }
    }

    fn unknown_call(&self, call: &CallExpression) -> Refs<'a> {
        // `isExternalCallee`
        let mut e = &call.callee;
        while let Some(m) = js::skip_parens(e).as_member_expression() {
            e = m.object();
        }
        let external = match js::skip_parens(e) {
            Expression::Identifier(id) => match self.sem.and_then(|s| js::ref_symbol(s, id)) {
                None => true,
                Some(sym) => {
                    let sem = self.sem.unwrap();
                    let decl = sem.nodes().kind(sem.scoping().symbol_declaration(sym));
                    matches!(
                        decl,
                        AstKind::ImportSpecifier(_)
                            | AstKind::ImportDefaultSpecifier(_)
                            | AstKind::ImportNamespaceSpecifier(_)
                    )
                }
            },
            _ => false,
        };
        Rc::new(PRef::UnknownCall { external })
    }

    /// `findFunction(context, id)`.
    fn find_function(&self, id: &IdentifierReference, depth: u32) -> Option<FnRef<'a>> {
        let sem = self.sem?;
        if depth > 16 {
            return None;
        }
        let sym = js::ref_symbol(sem, id)?;
        match js::single_declaration(sem, sym)? {
            AstKind::Function(f) if f.is_declaration() => Some(FnRef::Function(f)),
            AstKind::VariableDeclarator(d)
                if js::declarator_kind(sem, js::declaration_id(sem, sym))
                    == Some(VariableDeclarationKind::Const) =>
            {
                match js::skip_parens(d.init.as_ref()?) {
                    Expression::FunctionExpression(f) => Some(FnRef::Function(f)),
                    Expression::ArrowFunctionExpression(f) => Some(FnRef::Arrow(f)),
                    Expression::Identifier(i) => self.find_function(i, depth + 1),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// `extractFromFunctionParam(node, argIndex)`.
    pub fn extract_from_function_param(&self, f: FnRef<'a>, idx: usize) -> Refs<'a> {
        let params = f.params();
        let key = (params.span.start, idx);
        if let Some(r) = self.cache_fn.borrow().get(&key) {
            return r.clone();
        }
        self.cache_fn.borrow_mut().insert(key, Rc::new(PRef::Never));
        let r = match params.items.get(idx) {
            Some(p) if p.initializer.is_some() => {
                self.extract_from_pattern(PatVal::Binding(&p.pattern))
            }
            Some(p) => self.extract_from_pattern(PatVal::Binding(&p.pattern)),
            None => Rc::new(PRef::Never),
        };
        self.cache_fn.borrow_mut().insert(key, r.clone());
        r
    }

    fn extract_from_to_ref(&self, call: &'a CallExpression<'a>) -> Refs<'a> {
        let Some(n) = call.arguments.get(1).and_then(|a| a.as_expression()) else {
            return Rc::new(PRef::Any);
        };
        if !matches!(
            n,
            Expression::StringLiteral(_) | Expression::TemplateLiteral(_)
        ) {
            return Rc::new(PRef::Any);
        }
        match js::string_literal_value(n).filter(|s| !s.is_empty()) {
            Some(name) => Rc::new(PRef::Name {
                name,
                span: n.span(),
                nest: Nest::ToRefValue(NodeRef::Script(call.node_id())),
            }),
            None => Rc::new(PRef::Any),
        }
    }

    fn to_ref_sets(&self) -> (HashSet<Span>, HashSet<Span>) {
        if let Some(s) = &*self.to_ref.borrow() {
            return s.clone();
        }
        let mut to_ref = HashSet::new();
        let mut to_refs = HashSet::new();
        let map = TraceMap::calls(&["toRef", "toRefs"]);
        let modules: Vec<(&str, &TraceMap)> =
            COMPOSITION_API_MODULES.iter().map(|m| (*m, &map)).collect();
        for hit in esm_calls(self.file, &modules) {
            if hit.path.last().map(String::as_str) == Some("toRef") {
                to_ref.insert(hit.call.span);
            } else {
                to_refs.insert(hit.call.span);
            }
        }
        for (name, call) in
            crate::utils::tpl_analysis::global_calls(self.file, &["toRef", "toRefs"])
        {
            if name == "toRef" {
                to_ref.insert(call.span);
            } else {
                to_refs.insert(call.span);
            }
        }
        *self.to_ref.borrow_mut() = Some((to_ref.clone(), to_refs.clone()));
        (to_ref, to_refs)
    }

    /// `extractFromPath(pathString, node)`.
    pub fn extract_from_path(&self, path: &str, span: Span) -> Refs<'a> {
        Rc::new(PRef::Path(
            path.split('.').map(|p| (p.to_string(), span)).collect(),
        ))
    }

    /// `extractFromNameLiteral(node)`.
    pub fn extract_from_name_literal(&self, e: &Expression) -> Refs<'a> {
        let e = js::skip_parens(e);
        if !matches!(
            e,
            Expression::StringLiteral(_) | Expression::TemplateLiteral(_)
        ) {
            return Rc::new(PRef::Never);
        }
        match js::string_literal_value(e).filter(|s| !s.is_empty()) {
            Some(name) => Rc::new(PRef::NameLit {
                name,
                span: e.span(),
            }),
            None => Rc::new(PRef::Never),
        }
    }

    /// `extractFromName(name, node)` (nest = ANY).
    pub fn extract_from_name(&self, name: &str, span: Span) -> Refs<'a> {
        Rc::new(PRef::Name {
            name: name.to_string(),
            span,
            nest: Nest::Any,
        })
    }

    /// `extractFromVExpressionContainer(node, { ignoreGlobals })`.
    pub fn extract_from_container(&self, ci: u32, ignore_globals: bool) -> Refs<'a> {
        let Some(doc) = self.doc else {
            return Rc::new(PRef::Never);
        };
        let c = &doc.exprs[ci as usize];
        let mut out = Vec::new();
        for r in &c.references {
            // `x as const` is no reference; `<script setup generic="T">` resolves type `T`.
            let generic = r.is_type_reference
                && (r.name == "const"
                    || doc.top_level().any(|e| {
                        e.variables.iter().any(|v| {
                            v.kind == crate::ast::VariableKind::Generic && v.name == r.name
                        })
                    }));
            if r.variable.is_some()
                || generic
                || (ignore_globals && is_global_name(self.file, &r.name))
            {
                continue;
            }
            let Some(id) = self.template_ident(ci, r.span) else {
                continue;
            };
            if self.functional_template {
                if r.name == "props" {
                    out.push(self.extract_from_expression(id, true));
                }
                continue;
            }
            if r.name == "$props"
                && let Parent::Node(_, k) = self.parent(id)
            {
                let prop = match k {
                    AstKind::StaticMemberExpression(m)
                        if js::skip_parens(&m.object).span() == r.span =>
                    {
                        Some((m.property.name.to_string(), m.property.span))
                    }
                    AstKind::ComputedMemberExpression(m)
                        if js::skip_parens(&m.object).span() == r.span =>
                    {
                        match &m.expression {
                            Expression::Identifier(i) => Some((i.name.to_string(), i.span)),
                            _ => None,
                        }
                    }
                    _ => None,
                };
                if let Some((name, span)) = prop {
                    out.push(Rc::new(PRef::Name {
                        name,
                        span,
                        nest: Nest::Never,
                    }));
                    continue;
                }
            }
            out.push(Rc::new(PRef::Name {
                name: r.name.clone(),
                span: r.span,
                nest: Nest::Expr(id, true),
            }));
        }
        merge(out)
    }

    /// `extractFromStyleVariablesContext(ctx)`: the member paths of `<style>` `v-bind()`s.
    pub fn extract_from_style_vars(&self, paths: &[Vec<(String, Span)>]) -> Refs<'a> {
        merge(
            paths
                .iter()
                .filter(|p| !p.is_empty())
                .map(|p| Rc::new(PRef::Path(p.clone())))
                .collect(),
        )
    }
}

/// A function node for [`Extractor::extract_from_function_param`].
#[derive(Clone, Copy)]
pub enum FnRef<'a> {
    Function(&'a Function<'a>),
    Arrow(&'a ArrowFunctionExpression<'a>),
}

impl<'a> FnRef<'a> {
    fn params(&self) -> &'a FormalParameters<'a> {
        match self {
            FnRef::Function(f) => &f.params,
            FnRef::Arrow(f) => &f.params,
        }
    }
}

fn add_prop<'a>(
    props: &mut Vec<(String, Vec<(Span, PatVal<'a>)>)>,
    name: String,
    span: Span,
    v: PatVal<'a>,
) {
    if let Some((_, e)) = props.iter_mut().find(|(n, _)| *n == name) {
        e.push((span, v));
    } else {
        props.push((name, vec![(span, v)]));
    }
}

fn pat_key(p: PatVal) -> usize {
    match p {
        PatVal::Binding(b) => b as *const _ as usize,
        PatVal::TargetIdent(i) => i as *const _ as usize,
        PatVal::TargetMaybeDefault(t) => t as *const _ as usize,
        PatVal::Target(t) => t as *const _ as usize,
    }
}

fn member_name(k: &AstKind) -> Option<String> {
    crate::utils::tpl_analysis::member_kind_name(k)
}

/// Span of a member expression's `property` node.
fn prop_span(k: &AstKind) -> Span {
    match k {
        AstKind::StaticMemberExpression(m) => m.property.span,
        AstKind::ComputedMemberExpression(m) => m.expression.span(),
        AstKind::PrivateFieldExpression(m) => m.field.span,
        _ => k.span(),
    }
}

impl<'f, 'a> Extractor<'f, 'a> {
    /// `extractFromPattern(pattern)` for a binding pattern.
    pub fn extract_from_binding(&self, p: &'a BindingPattern<'a>) -> Refs<'a> {
        self.extract_from_pattern(PatVal::Binding(p))
    }
}

/// `utils.iterateWatchHandlerValues(property)`.
pub fn watch_handler_values<'a>(value: &'a Expression<'a>, out: &mut Vec<&'a Expression<'a>>) {
    match js::skip_parens(value) {
        Expression::ObjectExpression(o) => {
            if let Some(h) = js::find_property(o, "handler") {
                out.push(&h.value);
            }
        }
        Expression::ArrayExpression(a) => {
            for el in &a.elements {
                if let Some(e) = el.as_expression() {
                    watch_handler_values(e, out);
                }
            }
        }
        _ => out.push(value),
    }
}

/// Functions with parameters handled by `'ObjectExpression > Property > :function[params.length>0]'`:
/// `data(vm)`, `computed: { x(vm) }`, `computed: { x: { get(vm) } }` — `(function, is data)`.
pub fn vue_param_functions<'a>(obj: &'a ObjectExpression<'a>) -> Vec<(FnRef<'a>, bool)> {
    let fn_of = |e: &'a Expression<'a>| -> Option<FnRef<'a>> {
        match e {
            Expression::FunctionExpression(f)
                if !f.params.items.is_empty() || f.params.rest.is_some() =>
            {
                Some(FnRef::Function(f))
            }
            Expression::ArrowFunctionExpression(f)
                if !f.params.items.is_empty() || f.params.rest.is_some() =>
            {
                Some(FnRef::Arrow(f))
            }
            _ => None,
        }
    };
    let mut out = Vec::new();
    for p in js::properties(obj) {
        match js::prop_name(p).as_deref() {
            Some("data") => {
                if let Some(f) = fn_of(&p.value) {
                    out.push((f, true));
                }
            }
            Some("computed") => {
                let Expression::ObjectExpression(c) = &p.value else {
                    continue;
                };
                for cp in js::properties(c) {
                    if let Some(f) = fn_of(&cp.value) {
                        out.push((f, false));
                    } else if let Expression::ObjectExpression(g) = &cp.value {
                        for gp in js::properties(g) {
                            if js::prop_name(gp).as_deref() == Some("get")
                                && let Some(f) = fn_of(&gp.value)
                            {
                                out.push((f, false));
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Names (+ nodes) defined by a Vuex / Pinia `map*` helper call: `(group, name, span)`.
pub fn store_helper_names<'a>(
    call: &'a CallExpression<'a>,
    any_literal: bool,
) -> Vec<(&'static str, String, Span)> {
    let Expression::Identifier(callee) = &call.callee else {
        return Vec::new();
    };
    let n = callee.name.as_str();
    // `/^mapMutations|mapActions$/u`, `/^mapState|mapGetters|mapWritableState$/u`
    let group = if n.starts_with("mapMutations") || n.ends_with("mapActions") {
        "methods"
    } else if n.starts_with("mapState")
        || n.contains("mapGetters")
        || n.ends_with("mapWritableState")
    {
        "computed"
    } else {
        return Vec::new();
    };
    let arg = match call.arguments.len() {
        0 => return Vec::new(),
        2 => &call.arguments[1],
        _ => &call.arguments[0],
    };
    let mut out = Vec::new();
    match arg.as_expression() {
        Some(Expression::ObjectExpression(o)) => {
            for p in js::properties(o) {
                if let Some(name) = js::prop_name(p).filter(|n| !n.is_empty()) {
                    out.push((group, name, p.span));
                }
            }
        }
        Some(Expression::ArrayExpression(a)) => {
            for el in &a.elements {
                let Some(e) = el.as_expression() else {
                    continue;
                };
                let ok = if any_literal {
                    js::is_string_literal(e)
                        || matches!(
                            e,
                            Expression::NumericLiteral(_)
                                | Expression::BooleanLiteral(_)
                                | Expression::NullLiteral(_)
                                | Expression::RegExpLiteral(_)
                                | Expression::BigIntLiteral(_)
                        )
                } else {
                    js::is_string_literal(e)
                };
                if !ok {
                    continue;
                }
                if let Some(name) = js::string_literal_value(e).filter(|n| !n.is_empty()) {
                    out.push((group, name, e.span()));
                }
            }
        }
        _ => {}
    }
    out
}
