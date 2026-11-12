//! vue/prefer-single-event-payload

use std::collections::HashSet;

use oxc::ast::AstKind;
use oxc::ast::ast::{BindingPattern, CallExpression, Expression, FormalParameters, TSType};
use oxc::ast_visit::{Visit, walk};
use oxc::semantic::{NodeId, Semantic, SymbolId};
use oxc::span::{GetSpan, Span};

use crate::ast::{ExprParent, VExpression};
use crate::rule::{Ctx, LintFile, Rule, RuleOptions, fmt_msg};
use crate::utils::js;
use crate::utils::vue_component::{self as vc, ComponentEmit, MacroKind, TypeEmitNode};

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(PreferSingleEventPayload))
}

struct PreferSingleEventPayload;

fn verify_call(ctx: &mut Ctx, call: &CallExpression) {
    if call.arguments.len() <= 2 {
        return;
    }
    let name = call.arguments[0]
        .as_expression()
        .filter(|e| js::is_string_literal(e))
        .and_then(js::string_literal_value)
        .unwrap_or_else(|| "unknown".into());
    let span = Span::new(
        call.arguments[2].span().start,
        call.arguments.last().unwrap().span().end,
    );
    ctx.report_id(span, "preferSinglePayload", fmt_msg("Pass a single payload object instead of multiple arguments when emitting the \"{{name}}\" event.", &[("name", &name)]));
}

fn param_spans(p: &FormalParameters) -> Vec<Span> {
    p.items
        .iter()
        .map(|i| i.span)
        .chain(p.rest.as_ref().map(|r| r.span))
        .collect()
}

fn verify_declaration(ctx: &mut Ctx, emit: &ComponentEmit) {
    let (params, first) = match emit {
        ComponentEmit::Object { value, .. } => match value {
            Expression::ArrowFunctionExpression(f) => (param_spans(&f.params), 1),
            Expression::FunctionExpression(f) => (param_spans(&f.params), 1),
            _ => return,
        },
        ComponentEmit::Type { node, .. } => match node {
            TypeEmitNode::CallSignature(c) => (param_spans(&c.params), 2),
            TypeEmitNode::FunctionType(f) => (param_spans(&f.params), 2),
            TypeEmitNode::Method(m) => (param_spans(&m.params), 1),
            TypeEmitNode::Property(p) => {
                match p.type_annotation.as_ref().map(|a| &a.type_annotation) {
                    Some(TSType::TSTupleType(t)) => {
                        (t.element_types.iter().map(|e| e.span()).collect(), 1)
                    }
                    _ => return,
                }
            }
        },
        _ => return,
    };
    if params.len() < first + 1 {
        return;
    }
    let name = emit.name().unwrap_or_else(|| "unknown".into());
    let span = Span::new(params[first].start, params.last().unwrap().end);
    ctx.report_id(span, "preferSinglePayloadInDeclaration", fmt_msg("Declare a single payload parameter instead of multiple parameters for the \"{{name}}\" event.", &[("name", &name)]));
}

/// Reference identifier node ids of a symbol (`variable.references`), optionally reads only.
fn refs(sem: &Semantic, sym: SymbolId, read_only: bool) -> HashSet<NodeId> {
    sem.scoping()
        .get_resolved_references(sym)
        .filter(|r| !read_only || r.is_read())
        .map(|r| r.node_id())
        .collect()
}

fn callee_ident_id(call: &CallExpression) -> Option<NodeId> {
    match js::skip_parens(&call.callee) {
        Expression::Identifier(id) => Some(id.node_id()),
        _ => None,
    }
}

/// `getCalleeMemberNode`: (property name, object identifier node id).
fn callee_member(call: &CallExpression) -> Option<(String, Option<NodeId>)> {
    let m = js::skip_parens(&call.callee).as_member_expression()?;
    let name = js::member_name(m)?;
    let obj = match m.object() {
        Expression::Identifier(id) => Some(id.node_id()),
        _ => None,
    };
    Some((name, obj))
}

/// Template `$emit(…)` / `emit(…)` calls with more than two arguments: (event name, report span).
struct TemplateCalls<'c> {
    names: &'c HashSet<String>,
    out: Vec<(Option<String>, Span)>,
}

impl<'a> Visit<'a> for TemplateCalls<'_> {
    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        if let Expression::Identifier(id) = js::skip_parens(&call.callee)
            && (id.name == "$emit" || self.names.contains(id.name.as_str()))
            && call.arguments.len() > 2
        {
            let name = call.arguments[0]
                .as_expression()
                .filter(|e| js::is_string_literal(e))
                .and_then(js::string_literal_value);
            let span = Span::new(
                call.arguments[2].span().start,
                call.arguments.last().unwrap().span().end,
            );
            self.out.push((name, span));
        }
        walk::walk_call_expression(self, call);
    }
}

fn template_calls(file: &LintFile, names: &HashSet<String>) -> Vec<(Option<String>, Span)> {
    let Some(doc) = file.doc.as_ref() else {
        return Vec::new();
    };
    let mut v = TemplateCalls {
        names,
        out: Vec::new(),
    };
    for c in &doc.exprs {
        let el = match c.parent {
            ExprParent::Element(Some(e))
            | ExprParent::DirectiveValue(e, _)
            | ExprParent::DirectiveArgument(e, _) => e,
            ExprParent::Element(None) => continue,
        };
        if !doc.in_template(el) {
            continue;
        }
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
            Some(VExpression::For { right, .. }) => v.visit_expression(right),
            Some(VExpression::On { body, .. }) => body.iter().for_each(|s| v.visit_statement(s)),
            Some(VExpression::SlotScope { params, .. }) => v.visit_formal_parameters(params),
            _ => {}
        }
    }
    v.out
}

impl Rule for PreferSingleEventPayload {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let file = ctx.file;
        let Some(sem) = file.semantic else {
            report_template(ctx, &HashSet::new());
            return;
        };

        // <script setup>
        let mut setup_emit_ids: HashSet<NodeId> = HashSet::new();
        // vue-eslint-parser adds template references to the script variable, so template calls
        // match by the declared name.
        let mut setup_emit_names: HashSet<String> = HashSet::new();
        for m in vc::macros_of(file, MacroKind::DefineEmits) {
            for e in vc::emits_from_define_emits(file, m.call) {
                verify_declaration(ctx, &e);
            }
            if m.with_defaults.is_some() {
                continue;
            }
            if let Some(d) = m.declarator
                && let BindingPattern::BindingIdentifier(id) = &d.id
            {
                setup_emit_ids = refs(sem, id.symbol_id(), true);
                setup_emit_names = HashSet::from([id.name.to_string()]);
            }
        }

        // Options API declarations and setup(props, context)
        let mut contexts: Vec<(NodeId, HashSet<NodeId>, HashSet<NodeId>)> = Vec::new();
        for o in vc::vue_objects(file) {
            for e in vc::emits_from_options(o.obj) {
                verify_declaration(ctx, &e);
            }
            for f in vc::setup_functions(o.obj) {
                let Some(p) = f.params().items.get(1) else {
                    continue;
                };
                let pat = match &p.pattern {
                    BindingPattern::AssignmentPattern(a) => &a.left,
                    pat => pat,
                };
                match pat {
                    BindingPattern::ObjectPattern(op) => {
                        let Some(ep) = js::find_binding_property(op, "emit") else {
                            continue;
                        };
                        let BindingPattern::BindingIdentifier(id) = &ep.value else {
                            continue;
                        };
                        contexts.push((
                            o.node_id(),
                            HashSet::new(),
                            refs(sem, id.symbol_id(), false),
                        ));
                    }
                    BindingPattern::BindingIdentifier(id) => contexts.push((
                        o.node_id(),
                        refs(sem, id.symbol_id(), false),
                        HashSet::new(),
                    )),
                    _ => {}
                }
            }
        }

        for n in sem.nodes().iter() {
            let AstKind::CallExpression(call) = n.kind() else {
                continue;
            };
            let callee_id = callee_ident_id(call);
            let member = callee_member(call);
            if vc::in_script_setup(file, call.span)
                && callee_id.is_some_and(|id| setup_emit_ids.contains(&id))
            {
                verify_call(ctx, call);
            }
            if !contexts.is_empty()
                && let Some(vue) = vc::enclosing_vue_object(file, n.id())
            {
                for (vid, ctx_ids, emit_ids) in &contexts {
                    if *vid != vue.node_id() {
                        continue;
                    }
                    if callee_id.is_some_and(|id| emit_ids.contains(&id)) {
                        verify_call(ctx, call);
                    } else if let Some((name, Some(obj))) = &member
                        && name == "emit"
                        && ctx_ids.contains(obj)
                    {
                        verify_call(ctx, call);
                    }
                }
            }
            if member.as_ref().is_some_and(|(name, _)| name == "$emit") {
                verify_call(ctx, call);
            }
        }

        report_template(ctx, &setup_emit_names);
    }
}

fn report_template(ctx: &mut Ctx<'_, '_>, setup_emit_names: &HashSet<String>) {
    for (name, span) in template_calls(ctx.file, setup_emit_names) {
        let name = name.unwrap_or_else(|| "unknown".into());
        ctx.report_id(span, "preferSinglePayload", fmt_msg("Pass a single payload object instead of multiple arguments when emitting the \"{{name}}\" event.", &[("name", &name)]));
    }
}
