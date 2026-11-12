//! vue/no-unused-refs

use std::collections::HashSet;

use oxc::ast::AstKind;
use oxc::ast::ast::*;
use oxc::span::{GetSpan, Span};

use crate::ast::{AttributeKey, AttributeValue};
use crate::rule::{Ctx, Rule, RuleOptions, fmt_msg};
use crate::utils::js;
use crate::utils::template::template_expr_containers;
use crate::utils::tpl_analysis::{Nodes, module_bindings};
use crate::utils::tpl_misc::body_attributes;
use crate::utils::vue_component as vc;

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(NoUnusedRefs))
}

struct NoUnusedRefs;

#[derive(Default)]
struct Used {
    names: HashSet<String>,
    unknown: bool,
}

impl Used {
    fn object_pattern(&mut self, props: &[Option<String>], has_rest: bool) {
        for p in props {
            match p {
                Some(n) => {
                    self.names.insert(n.clone());
                }
                None => {
                    self.unknown = true;
                    return;
                }
            }
        }
        if has_rest {
            self.unknown = true;
        }
    }
    fn binding_pattern(&mut self, p: &BindingPattern) {
        match p {
            BindingPattern::ObjectPattern(o) => {
                let names: Vec<_> = o.properties.iter().map(js::binding_prop_name).collect();
                self.object_pattern(&names, o.rest.is_some());
            }
            BindingPattern::BindingIdentifier(_) => self.unknown = true,
            _ => {}
        }
    }
    fn assign_target(&mut self, t: &AssignmentTarget) {
        match t {
            AssignmentTarget::ObjectAssignmentTarget(o) => {
                let names: Vec<_> = o
                    .properties
                    .iter()
                    .map(|p| match p {
                        AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(i) => {
                            Some(i.binding.name.to_string())
                        }
                        AssignmentTargetProperty::AssignmentTargetPropertyProperty(p) => {
                            match &p.name {
                                PropertyKey::StaticIdentifier(id) if !p.computed => {
                                    Some(id.name.to_string())
                                }
                                k => js::key_name(k),
                            }
                        }
                    })
                    .collect();
                self.object_pattern(&names, o.rest.is_some());
            }
            AssignmentTarget::AssignmentTargetIdentifier(_) => self.unknown = true,
            _ => {}
        }
    }
    /// `extractUsedForPattern`: `node` = span of the `$refs` node, `parent` = its ESTree parent
    /// (chains and parentheses skipped).
    fn parent(&mut self, node: Span, parent: Option<AstKind>) {
        let is = |e: &Expression| js::skip_parens(e).span() == node;
        match parent {
            Some(AstKind::AssignmentExpression(a)) if is(&a.right) => self.assign_target(&a.left),
            Some(AstKind::VariableDeclarator(d)) if d.init.as_ref().is_some_and(is) => {
                self.binding_pattern(&d.id)
            }
            Some(AstKind::StaticMemberExpression(m)) if is(&m.object) => {
                self.names.insert(m.property.name.to_string());
            }
            Some(AstKind::ComputedMemberExpression(m)) if is(&m.object) => {
                match js::string_literal_value(&m.expression) {
                    Some(n) => {
                        self.names.insert(n);
                    }
                    None => self.unknown = true,
                }
            }
            Some(AstKind::PrivateFieldExpression(m)) if is(&m.object) => self.unknown = true,
            Some(AstKind::CallExpression(c))
                if c.arguments.iter().any(|a| a.as_expression().is_some_and(is)) =>
            {
                self.unknown = true
            }
            Some(AstKind::ForInStatement(_) | AstKind::ReturnStatement(_)) => self.unknown = true,
            _ => {}
        }
    }
}

fn is_transparent(k: &AstKind) -> bool {
    matches!(k, AstKind::ChainExpression(_) | AstKind::ParenthesizedExpression(_))
}

impl Rule for NoUnusedRefs {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let file = ctx.file;
        let Some(doc) = ctx.doc() else { return };
        if doc.template_body.is_none() {
            return;
        }
        let mut used = Used::default();
        if vc::is_script_setup(file) {
            used.names.extend(module_bindings(file).into_iter().map(|(n, _)| n.to_string()));
        }
        for o in vc::vue_objects(file) {
            used.names.extend(vc::iterate_properties(o.obj, &["setup"]).into_iter().map(|p| p.name));
        }
        if let Some(sem) = file.semantic {
            let nodes = sem.nodes();
            for n in nodes.iter() {
                let (refs_span, refs_id) = match n.kind() {
                    AstKind::IdentifierReference(id) if id.name == "$refs" => (id.span, n.id()),
                    AstKind::StaticMemberExpression(m) if m.property.name == "$refs" => (m.span, n.id()),
                    AstKind::CallExpression(c) => {
                        if let Some(a) = c.arguments.first().and_then(|a| a.as_expression())
                            && js::is_string_literal(a)
                            && matches!(&c.callee, Expression::Identifier(id) if id.name == "useTemplateRef")
                            && let Some(name) = js::string_literal_value(a)
                        {
                            used.names.insert(name);
                        }
                        continue;
                    }
                    _ => continue,
                };
                let mut cur = refs_id;
                let mut parent = nodes.parent_kind(cur);
                while is_transparent(&parent) {
                    cur = nodes.parent_id(cur);
                    parent = nodes.parent_kind(cur);
                }
                used.parent(refs_span, Some(parent));
            }
        }
        for c in template_expr_containers(doc) {
            if !c.references.iter().any(|r| r.variable.is_none() && r.name == "$refs") {
                continue;
            }
            let nodes = Nodes::of(c);
            for r in c.references.iter().filter(|r| r.variable.is_none() && r.name == "$refs") {
                let Some(i) = nodes.kinds.iter().position(
                    |k| matches!(k, AstKind::IdentifierReference(id) if id.span == r.span),
                ) else {
                    continue;
                };
                let mut cur = i;
                let mut p = nodes.parent(cur);
                while let Some(pi) = p.filter(|pi| is_transparent(&nodes.kinds[*pi])) {
                    cur = pi;
                    p = nodes.parent(cur);
                }
                used.parent(r.span, p.map(|pi| nodes.kinds[pi]));
            }
        }
        if used.unknown {
            return;
        }
        for (_, a) in body_attributes(doc) {
            if let (AttributeKey::Plain(k), Some(AttributeValue::Literal(l))) = (&a.key, &a.value)
                && k.name == "ref"
                && !used.names.contains(&l.value)
            {
                ctx.report_id(
                    l.span,
                    "unused",
                    fmt_msg("'{{name}}' is defined as ref, but never used.", &[("name", &l.value)]),
                );
            }
        }
    }
}
