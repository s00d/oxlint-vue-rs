//! vue/no-ref-object-reactivity-loss

use oxc::ast::AstKind;
use oxc::ast::ast::*;
use oxc::semantic::{NodeId, Semantic};
use oxc::span::{GetSpan, Span};

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::js;
use crate::utils::ref_object_references::extract_ref_object_references;

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(NoRefObjectReactivityLoss))
}

struct NoRefObjectReactivityLoss;

const MSG: &str = "Getting a value from the ref object in the same scope will cause the value to lose reactivity.";

/// `isUpdate(node)`: the node is written to (assignment / update / destructuring target).
fn is_update(sem: &Semantic, id: NodeId) -> bool {
    let span = sem.nodes().kind(id).span();
    let (pid, parent) = js::parent(sem, id);
    match parent {
        AstKind::UpdateExpression(_) => true,
        AstKind::AssignmentExpression(a) => a.left.span() == span,
        AstKind::AssignmentTargetWithDefault(d) => d.binding.span() == span && is_update(sem, pid),
        AstKind::AssignmentTargetPropertyProperty(_)
        | AstKind::ObjectAssignmentTarget(_)
        | AstKind::ArrayAssignmentTarget(_)
        | AstKind::AssignmentTargetRest(_) => is_update(sem, pid),
        AstKind::StaticMemberExpression(m) => {
            js::skip_parens(&m.object).span() == span && is_update(sem, pid)
        }
        AstKind::ComputedMemberExpression(m) => {
            js::skip_parens(&m.object).span() == span && is_update(sem, pid)
        }
        _ => false,
    }
}

/// Function nodes that are immediately invoked (sync, non-generator) do not open a scope.
fn is_iife(sem: &Semantic, id: NodeId, kind: AstKind) -> bool {
    let ok = match kind {
        AstKind::Function(f) => {
            f.r#type == FunctionType::FunctionExpression && !f.r#async && !f.generator
        }
        AstKind::ArrowFunctionExpression(f) => !f.r#async,
        _ => return false,
    };
    ok && matches!(js::parent(sem, id).1, AstKind::CallExpression(c) if js::skip_parens(&c.callee).span() == kind.span())
}

/// Innermost non-IIFE function (or the program) containing `id`.
fn scope_of(sem: &Semantic, id: NodeId) -> Option<NodeId> {
    sem.nodes()
        .ancestors(id)
        .find(|n| match n.kind() {
            k @ (AstKind::Function(_) | AstKind::ArrowFunctionExpression(_)) => {
                !is_iife(sem, n.id(), k)
            }
            _ => false,
        })
        .map(|n| n.id())
}

impl Rule for NoRefObjectReactivityLoss {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let file = ctx.file;
        let Some(sem) = file.semantic else { return };
        let refs = extract_ref_object_references(file);
        let mut hits: Vec<Span> = Vec::new();
        let verify = |id: NodeId, span: Span, hits: &mut Vec<Span>| {
            if let Some(r) = refs.get(id)
                && scope_of(sem, r.define) == scope_of(sem, id)
            {
                hits.push(span);
            }
        };
        for n in sem.nodes().iter() {
            match n.kind() {
                AstKind::StaticMemberExpression(m)
                    if m.property.name == "value" && !is_update(sem, n.id()) =>
                {
                    let o = js::skip_parens(&m.object);
                    verify(o.node_id(), o.span(), &mut hits);
                }
                AstKind::ComputedMemberExpression(m)
                    if js::string_literal_value(&m.expression).as_deref() == Some("value")
                        && !is_update(sem, n.id()) =>
                {
                    let o = js::skip_parens(&m.object);
                    verify(o.node_id(), o.span(), &mut hits);
                }
                AstKind::ObjectPattern(p) if js::find_binding_property(p, "value").is_some() => {
                    verify(n.id(), p.span, &mut hits)
                }
                AstKind::ObjectAssignmentTarget(p)
                    if p.properties.iter().any(|q| match q {
                        AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(i) => {
                            i.binding.name == "value"
                        }
                        AssignmentTargetProperty::AssignmentTargetPropertyProperty(pp) => {
                            js::key_name(&pp.name).as_deref() == Some("value")
                        }
                    }) =>
                {
                    verify(n.id(), p.span, &mut hits)
                }
                _ => {}
            }
        }
        for s in hits {
            ctx.report_id(s, "getValueInSameScope", MSG);
        }
    }
}
