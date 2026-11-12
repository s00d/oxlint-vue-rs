//! vue/no-ref-as-operand

use std::collections::HashSet;

use oxc::ast::AstKind;
use oxc::ast::ast::*;
use oxc::semantic::{NodeId, Semantic};
use oxc::span::{GetSpan, Span};

use crate::rule::{Ctx, Fix, Rule, RuleOptions, fmt_msg};
use crate::utils::js;
use crate::utils::ref_object_references::{RefObjectReferences, extract_ref_object_references};
use crate::utils::tpl_analysis::{emit_setup, innermost_vue, name_param};
use crate::utils::vue_component as vc;

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(NoRefAsOperand))
}

struct NoRefAsOperand;

/// ESTree parent (parentheses skipped): (id, kind, the child's node id as seen by the parent).
fn parent<'a>(sem: &Semantic<'a>, mut id: NodeId) -> (NodeId, AstKind<'a>) {
    loop {
        let p = sem.nodes().parent_id(id);
        let k = sem.nodes().kind(p);
        if matches!(k, AstKind::ParenthesizedExpression(_)) {
            id = p;
            continue;
        }
        return (p, k);
    }
}

fn is(e: &Expression, span: Span) -> bool {
    js::skip_parens(e).span() == span
}

fn logical_used_as_operand(sem: &Semantic, mut id: NodeId) -> bool {
    let (mut pid, mut pk) = parent(sem, id);
    while let AstKind::LogicalExpression(_) = pk {
        id = pid;
        (pid, pk) = parent(sem, id);
    }
    let span = sem.nodes().kind(id).span();
    match pk {
        AstKind::IfStatement(_) | AstKind::SwitchStatement(_) | AstKind::UnaryExpression(_) | AstKind::BinaryExpression(_) => true,
        AstKind::ConditionalExpression(c) => is(&c.test, span),
        AstKind::TemplateLiteral(_) => !matches!(parent(sem, pid).1, AstKind::TaggedTemplateExpression(_)),
        _ => false,
    }
}

struct Checker<'r, 's, 'a> {
    refs: &'r RefObjectReferences,
    sem: &'s Semantic<'a>,
}

impl Checker<'_, '_, '_> {
    /// `isRefInit` → the defining method.
    fn ref_init(&self, id: NodeId) -> Option<String> {
        let data = self.refs.get(id)?;
        let AstKind::VariableDeclarator(d) = self.sem.nodes().kind(data.declarator?) else {
            return None;
        };
        let init = js::skip_parens(d.init.as_ref()?);
        let init_id = match init {
            Expression::CallExpression(c) => c.node_id(),
            Expression::Identifier(i) => i.node_id(),
            Expression::StaticMemberExpression(m) => m.node_id(),
            Expression::ComputedMemberExpression(m) => m.node_id(),
            Expression::PrivateFieldExpression(m) => m.node_id(),
            _ => return None,
        };
        data.chain.contains(&init_id).then(|| data.method.clone())
    }
    fn report(&self, ctx: &mut Ctx, id: NodeId, span: Span) {
        if let Some(method) = self.ref_init(id) {
            ctx.report_fix(
                span,
                Some("requireDotValue"),
                fmt_msg(
                    "Must use `.value` to read or write the value wrapped by `{{method}}()`.",
                    &[("method", &method)],
                ),
                Some(Fix::insert(span.end, ".value")),
            );
        }
    }
}

impl Rule for NoRefAsOperand {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let file = ctx.file;
        let Some(sem) = file.semantic else { return };
        let refs = extract_ref_object_references(file);
        let c = Checker { refs: &refs, sem };
        let setup = emit_setup(file, false, true);
        let vue_ids: HashSet<NodeId> = vc::vue_objects(file).iter().map(|o| o.node_id()).collect();
        for n in sem.nodes().iter() {
            match n.kind() {
                AstKind::IdentifierReference(id) => {
                    let span = id.span;
                    let (pid, pk) = parent(sem, n.id());
                    let check = match pk {
                        AstKind::IfStatement(s) => is(&s.test, span),
                        AstKind::SwitchStatement(s) => is(&s.discriminant, span),
                        AstKind::UnaryExpression(_) | AstKind::UpdateExpression(_) | AstKind::BinaryExpression(_) => true,
                        AstKind::AssignmentExpression(a) => {
                            // `=`: only the left side (and no type check).
                            a.operator != AssignmentOperator::Assign || a.left.span() == span
                        }
                        AstKind::LogicalExpression(l) => {
                            (is(&l.left, span) || logical_used_as_operand(sem, pid))
                                && refs.get(n.id()).and_then(|d| d.declarator).is_some_and(|d| {
                                    js::declarator_kind(sem, d) == Some(VariableDeclarationKind::Const)
                                })
                        }
                        AstKind::ConditionalExpression(e) => is(&e.test, span),
                        AstKind::TemplateLiteral(_) => {
                            !matches!(parent(sem, pid).1, AstKind::TaggedTemplateExpression(_))
                        }
                        AstKind::StaticMemberExpression(m) => {
                            is(&m.object, span) && !matches!(m.property.name.as_str(), "value" | "effect")
                        }
                        AstKind::ComputedMemberExpression(m) => {
                            is(&m.object, span)
                                && js::string_literal_value(&m.expression)
                                    .is_some_and(|n| n != "value" && n != "effect")
                        }
                        _ => false,
                    };
                    if check {
                        c.report(ctx, n.id(), span);
                    }
                }
                AstKind::CallExpression(call) => {
                    if name_param(call).is_none() {
                        continue;
                    }
                    let mut hit = vc::in_script_setup(file, call.span)
                        && setup.program.as_ref().is_some_and(|(s, e)| call.span.start >= *s && e.matches(call));
                    if !hit
                        && let Some(v) = innermost_vue(sem, &vue_ids, n.id())
                    {
                        hit = setup.vue.get(&v).is_some_and(|e| e.matches(call));
                    }
                    if hit {
                        for a in &call.arguments {
                            if let Some(Expression::Identifier(id)) = a.as_expression().map(js::skip_parens) {
                                c.report(ctx, id.node_id(), id.span);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
}
