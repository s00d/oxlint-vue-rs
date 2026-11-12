//! vue/no-unused-emit-declarations

use std::collections::{HashMap, HashSet};

use oxc::ast::AstKind;
use oxc::ast::ast::{ArrayExpressionElement, Argument, CallExpression, Expression, ObjectPropertyKind};
use oxc::semantic::{NodeId, Semantic};
use oxc::span::Span;

use crate::rule::{Ctx, Rule, RuleOptions, fmt_msg};
use crate::utils::js;
use crate::utils::tpl_analysis::{
    EmitCtx, callee_ident, chain_member, emit_setup, innermost_vue, member_call_object,
    name_param, template_calls,
};
use crate::utils::tpl_misc::is_this;
use crate::utils::vue_component::{self as vc, ComponentEmit, MacroKind};

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(NoUnusedEmitDeclarations))
}

struct NoUnusedEmitDeclarations;

#[derive(Default)]
struct State {
    /// name → declaration node span (insertion order kept by `order`).
    decls: HashMap<String, Option<Span>>,
    order: Vec<String>,
    calls: HashSet<String>,
}

impl State {
    fn declare(&mut self, emits: &[ComponentEmit]) {
        for e in emits {
            if let Some(n) = e.name().filter(|n| !n.is_empty()) {
                if !self.decls.contains_key(&n) {
                    self.order.push(n.clone());
                }
                self.decls.insert(n, e.node_span());
            }
        }
    }
    fn clear(&mut self) {
        self.decls.clear();
        self.order.clear();
        self.calls.clear();
    }
    fn add_call(&mut self, call: &CallExpression) {
        if let Some((n, _)) = name_param(call) {
            self.calls.insert(n);
        }
    }
}

fn is_ref(e: &Expression, c: &EmitCtx) -> bool {
    matches!(js::skip_parens(e), Expression::Identifier(id)
        if c.emit_ids.contains(&id.node_id()) || c.ctx_ids.contains(&id.node_id()))
}

fn has_expr_ref(e: &Expression, c: &EmitCtx) -> bool {
    if let Some(m) = js::skip_parens(e).as_member_expression() {
        let obj = chain_member(m.object()).map_or(m.object(), |_| m.object());
        if is_ref(obj, c) {
            return true;
        }
    }
    is_ref(e, c)
}

fn has_args_refs(args: &[Argument], c: &EmitCtx) -> bool {
    for a in args {
        let Some(e) = a.as_expression() else { continue };
        match js::skip_parens(e) {
            Expression::ObjectExpression(o) => {
                if o.properties.iter().any(|p| {
                    matches!(p, ObjectPropertyKind::ObjectProperty(p) if has_expr_ref(&p.value, c))
                }) {
                    return true;
                }
            }
            Expression::ArrayExpression(arr)
                if arr.elements.iter().any(|el| match el {
                    ArrayExpressionElement::SpreadElement(_) | ArrayExpressionElement::Elision(_) => false,
                    el => el.as_expression().is_some_and(|e| has_expr_ref(e, c)),
                }) =>
            {
                return true;
            }
            _ => {}
        }
        if has_expr_ref(e, c) {
            return true;
        }
    }
    false
}

/// The shared `callVisitor`.
fn visit_call(
    st: &mut State,
    sem: Option<&Semantic>,
    call: &CallExpression,
    c: Option<&EmitCtx>,
    param_name: Option<&str>,
) {
    if let Some(c) = c {
        if has_args_refs(&call.arguments, c) {
            st.clear();
            return;
        }
        if c.matches(call) {
            st.add_call(call);
        }
    }
    if member_call_object(call, "$emit").is_some_and(|o| match sem {
        Some(sem) => is_this(sem, o),
        None => matches!(js::skip_parens(o), Expression::ThisExpression(_)),
    }) {
        st.add_call(call);
    }
    if callee_ident(call).is_some_and(|n| n == "$emit" || param_name == Some(n)) {
        st.add_call(call);
    }
}

impl Rule for NoUnusedEmitDeclarations {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let file = ctx.file;
        let setup = emit_setup(file, false, false);
        let param = setup.param_name.as_deref();
        // `emitParamName` is only known once `defineEmits` has been visited.
        let param_at = |pos: u32| param.filter(|_| setup.program.as_ref().is_some_and(|(s, _)| pos >= *s));
        let mut st = State::default();
        if let Some(sem) = file.semantic {
            let objs = vc::vue_objects(file);
            let vue_ids: HashSet<NodeId> = objs.iter().map(|o| o.node_id()).collect();
            let macros: HashMap<Span, vc::Macro> =
                vc::script_setup_macros(file).into_iter().map(|m| (m.call.span, m)).collect();
            for n in sem.nodes().iter() {
                match n.kind() {
                    AstKind::ObjectExpression(o) if vue_ids.contains(&o.node_id()) => {
                        st.declare(&vc::emits_from_options(o));
                    }
                    AstKind::CallExpression(call) => {
                        let in_setup = vc::in_script_setup(file, call.span);
                        if let Some(v) = innermost_vue(sem, &vue_ids, n.id()) {
                            visit_call(&mut st, Some(sem), call, setup.vue.get(&v), param_at(call.span.start));
                        }
                        if in_setup {
                            if let Some(m) = macros.get(&call.span) {
                                match m.kind {
                                    MacroKind::DefineEmits => {
                                        st.declare(&vc::emits_from_define_emits(file, m.call))
                                    }
                                    MacroKind::DefineModel if m.declarator.is_some() && m.with_defaults.is_none() => {
                                        st.calls.insert(format!(
                                            "update:{}",
                                            vc::model_from_define_model(m.call).name
                                        ));
                                    }
                                    _ => {}
                                }
                            }
                            let c = setup
                                .program
                                .as_ref()
                                .filter(|(s, _)| call.span.start >= *s)
                                .map(|(_, c)| c);
                            visit_call(&mut st, Some(sem), call, c, param_at(call.span.start));
                        }
                    }
                    _ => {}
                }
            }
        }
        if let Some(doc) = file.doc.as_ref() {
            let c = setup.program.as_ref().map(|(_, c)| c);
            for call in template_calls(doc) {
                visit_call(&mut st, None, call, c, param);
            }
        }
        for name in &st.order {
            if st.calls.contains(name) {
                continue;
            }
            if let Some(Some(span)) = st.decls.get(name) {
                ctx.report_id(
                    *span,
                    "unused",
                    fmt_msg("`{{name}}` is defined as emit but never used.", &[("name", name)]),
                );
            }
        }
    }
}
