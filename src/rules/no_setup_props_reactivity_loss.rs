//! vue/no-setup-props-reactivity-loss

use std::collections::HashMap;

use oxc::ast::AstKind;
use oxc::ast::ast::*;
use oxc::semantic::{NodeId, ScopeId, Semantic};
use oxc::span::{GetSpan, Span};

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::js;
use crate::utils::vue_component as vc;

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(NoSetupPropsReactivityLoss))
}

struct NoSetupPropsReactivityLoss;

const DESTRUCTURING: &str = "Destructuring the `props` will cause the value to lose reactivity.";

/// Props references collected for one setup scope (`None` = program / `<script setup>`).
#[derive(Default)]
struct ScopeRefs {
    /// References to the whole props object.
    props: Vec<(NodeId, Span)>,
    /// References to a single destructured prop.
    prop: Vec<(NodeId, Span)>,
    name: &'static str,
}

/// A prop id pattern: whole `props` or one destructured `prop`.
#[derive(Clone, Copy)]
enum Kind {
    Props,
    Prop,
}

fn unwrap_assignment<'b, 'a>(p: &'b BindingPattern<'a>) -> &'b BindingPattern<'a> {
    match p {
        BindingPattern::AssignmentPattern(a) => &a.left,
        p => p,
    }
}

struct Collector<'s, 'a> {
    sem: &'s Semantic<'a>,
    scopes: HashMap<Option<NodeId>, ScopeRefs>,
    reports: Vec<(Span, &'static str, String)>,
}

impl<'a> Collector<'_, 'a> {
    fn prop_id(
        &mut self,
        node: &BindingPattern<'a>,
        decl_span: Span,
        kind: Kind,
        scope_node: Option<NodeId>,
        scope: ScopeId,
        name: &'static str,
    ) {
        let sym = match node {
            BindingPattern::ObjectPattern(_) | BindingPattern::ArrayPattern(_) => {
                self.reports
                    .push((decl_span, "destructuring", DESTRUCTURING.into()));
                return;
            }
            BindingPattern::AssignmentPattern(_) => return,
            BindingPattern::BindingIdentifier(b) => b.symbol_id(),
        };
        let nodes = self.sem.nodes();
        let entry = self.scopes.entry(scope_node).or_insert_with(|| ScopeRefs {
            name,
            ..ScopeRefs::default()
        });
        let list = match kind {
            Kind::Props => &mut entry.props,
            Kind::Prop => &mut entry.prop,
        };
        for r in self.sem.scoping().get_resolved_references(sym) {
            if r.scope_id() != scope || !r.is_read() {
                continue;
            }
            list.push((r.node_id(), nodes.kind(r.node_id()).span()));
        }
    }
}

/// Innermost function containing the node (`None` = program).
fn enclosing_fn(sem: &Semantic, id: NodeId) -> Option<NodeId> {
    sem.nodes()
        .ancestors(id)
        .find(|n| {
            matches!(
                n.kind(),
                AstKind::Function(_) | AstKind::ArrowFunctionExpression(_)
            )
        })
        .map(|n| n.id())
}

fn is_props_member_accessed(sem: &Semantic, span: Span, refs: &ScopeRefs) -> bool {
    refs.props.iter().any(|(id, s)| {
        js::in_range(span, *s)
            && match js::parent(sem, *id).1 {
                AstKind::StaticMemberExpression(m) => js::skip_parens(&m.object).span() == *s,
                AstKind::ComputedMemberExpression(m) => js::skip_parens(&m.object).span() == *s,
                AstKind::PrivateFieldExpression(m) => js::skip_parens(&m.object).span() == *s,
                _ => false,
            }
    }) || refs.prop.iter().any(|(_, s)| js::in_range(span, *s))
}

/// `skipChainExpression` (+ parentheses): the member inside `a?.b`, else the expression itself.
enum Node<'b, 'a> {
    Expr(&'b Expression<'a>),
    Member(&'b MemberExpression<'a>),
    Other,
}

fn skip_chain<'b, 'a>(e: &'b Expression<'a>) -> Node<'b, 'a> {
    match js::skip_parens(e) {
        Expression::ChainExpression(c) => c
            .expression
            .as_member_expression()
            .map_or(Node::Other, Node::Member),
        e => match e.as_member_expression() {
            Some(m) => Node::Member(m),
            None => Node::Expr(e),
        },
    }
}

/// `verify(left, right)`; `left_pattern` = span of an object/array pattern on the left.
fn verify(
    sem: &Semantic,
    left_pattern: Option<Span>,
    right: &Expression,
    refs: &ScopeRefs,
    out: &mut Vec<(Span, &'static str, String)>,
) {
    let msg = || {
        format!(
            "Getting a value from the `props` in root scope of `{}` will cause the value to lose reactivity.",
            refs.name
        )
    };
    let mut node = skip_chain(right);
    if let Node::Expr(e @ (Expression::ArrayExpression(_) | Expression::ObjectExpression(_))) = node
        && is_props_member_accessed(sem, e.span(), refs)
    {
        out.push((e.span(), "getProperty", msg()));
        return;
    }
    let root = loop {
        match node {
            Node::Member(m) => node = skip_chain(m.object()),
            Node::Expr(e) => break Some(e),
            Node::Other => break None,
        }
    };
    let exprs: Vec<&Expression> = match root {
        Some(Expression::TemplateLiteral(t)) => t.expressions.iter().collect(),
        Some(Expression::ConditionalExpression(c)) => vec![&c.test, &c.consequent, &c.alternate],
        Some(e @ Expression::Identifier(_)) => vec![e],
        _ => Vec::new(),
    };
    if let Some(lp) = left_pattern
        && exprs.iter().any(|e| matches!(e, Expression::Identifier(i) if refs.props.iter().any(|(id, _)| *id == i.node_id())))
    {
        out.push((lp, "getProperty", msg()));
        return;
    }
    if let Some(e) = exprs
        .iter()
        .find(|e| is_props_member_accessed(sem, e.span(), refs))
    {
        out.push((e.span(), "getProperty", msg()));
    }
}

impl Rule for NoSetupPropsReactivityLoss {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let file = ctx.file;
        let Some(sem) = file.semantic else { return };
        let mut c = Collector {
            sem,
            scopes: HashMap::new(),
            reports: Vec::new(),
        };

        for m in vc::macros_of(file, vc::MacroKind::DefineProps) {
            let Some(d) = m.declarator else { continue };
            let scope = sem.nodes().get_node(m.call.node_id()).scope_id();
            match &d.id {
                BindingPattern::ObjectPattern(o) => {
                    for p in &o.properties {
                        let v = unwrap_assignment(&p.value);
                        c.prop_id(v, v.span(), Kind::Prop, None, scope, "<script setup>");
                    }
                    if let Some(rest) = &o.rest {
                        let v = unwrap_assignment(&rest.argument);
                        c.prop_id(v, v.span(), Kind::Props, None, scope, "<script setup>");
                    }
                }
                id => {
                    let v = unwrap_assignment(id);
                    c.prop_id(
                        v,
                        js::declarator_id_span(d),
                        Kind::Props,
                        None,
                        scope,
                        "<script setup>",
                    );
                }
            }
        }
        for o in vc::vue_objects(file) {
            for f in vc::setup_functions(o.obj) {
                let Some(param) = f.params().items.first() else {
                    continue;
                };
                let scope = match f {
                    vc::FnNode::Function(f) => f.scope_id(),
                    vc::FnNode::Arrow(f) => f.scope_id(),
                };
                let span = match &param.type_annotation {
                    Some(t) => Span::new(param.pattern.span().start, t.span.end),
                    None => param.pattern.span(),
                };
                c.prop_id(
                    &param.pattern,
                    span,
                    Kind::Props,
                    Some(f.node_id()),
                    scope,
                    "setup()",
                );
            }
        }

        let mut out = std::mem::take(&mut c.reports);
        if !c.scopes.is_empty() {
            for n in sem.nodes().iter() {
                let check = matches!(
                    n.kind(),
                    AstKind::CallExpression(_)
                        | AstKind::VariableDeclarator(_)
                        | AstKind::AssignmentExpression(_)
                );
                if !check {
                    continue;
                }
                let Some(refs) = c.scopes.get(&enclosing_fn(sem, n.id())) else {
                    continue;
                };
                match n.kind() {
                    AstKind::CallExpression(call) => {
                        if is_props_member_accessed(sem, call.span, refs) {
                            out.push((call.span, "getProperty", format!("Getting a value from the `props` in root scope of `{}` will cause the value to lose reactivity.", refs.name)));
                        }
                    }
                    AstKind::VariableDeclarator(d) => {
                        if let Some(init) = &d.init {
                            let lp = matches!(
                                d.id,
                                BindingPattern::ObjectPattern(_) | BindingPattern::ArrayPattern(_)
                            )
                            .then(|| js::declarator_id_span(d));
                            verify(sem, lp, init, refs, &mut out);
                        }
                    }
                    AstKind::AssignmentExpression(a) => {
                        let lp = matches!(
                            a.left,
                            AssignmentTarget::ObjectAssignmentTarget(_)
                                | AssignmentTarget::ArrayAssignmentTarget(_)
                        )
                        .then(|| a.left.span());
                        verify(sem, lp, &a.right, refs, &mut out);
                    }
                    _ => {}
                }
            }
        }
        for (span, id, msg) in out {
            ctx.report_id(span, id, msg);
        }
    }
}
