//! vue/no-shadow-native-events

use std::collections::HashSet;

use oxc::ast::AstKind;
use oxc::semantic::NodeId;
use oxc::span::Span;

use crate::rule::{Ctx, Rule, RuleOptions, fmt_msg};
use crate::utils::template::DOM_EVENTS;
use crate::utils::tpl_analysis::{
    TemplateDefine, callee_ident, emit_setup, innermost_vue, member_call_object, name_param,
    template_calls, template_define,
};
use crate::utils::tpl_misc::is_this;
use crate::utils::vue_component::{self as vc, ComponentEmit, MacroKind};

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(NoShadowNativeEvents))
}

struct NoShadowNativeEvents;

fn report(ctx: &mut Ctx, name: &str, span: Span) {
    ctx.report_id(
        span,
        "violation",
        fmt_msg("Avoid shadowing the native event \"{{ name }}\".", &[("name", name)]),
    );
}

fn verify(ctx: &mut Ctx, name: &str, span: Span) {
    if DOM_EVENTS.contains(&name.to_lowercase()) {
        report(ctx, name, span);
    }
}

fn verify_decls(ctx: &mut Ctx, emits: &[ComponentEmit]) {
    for e in emits {
        if let (Some(n), Some(s)) = (e.name(), e.node_span())
            && !n.is_empty()
            && DOM_EVENTS.contains(&n.to_lowercase())
        {
            report(ctx, &n, s);
        }
    }
}

impl Rule for NoShadowNativeEvents {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let file = ctx.file;
        let setup = emit_setup(file, true, false);
        let mut define_emits = None;
        for m in vc::macros_of(file, MacroKind::DefineEmits) {
            verify_decls(ctx, &vc::emits_from_define_emits(file, m.call));
            define_emits = Some(m);
        }
        let objs = vc::vue_objects(file);
        for o in &objs {
            verify_decls(ctx, &vc::emits_from_options(o.obj));
        }
        if let Some(sem) = file.semantic {
            let vue_ids: HashSet<NodeId> = objs.iter().map(|o| o.node_id()).collect();
            for n in sem.nodes().iter() {
                let AstKind::CallExpression(call) = n.kind() else {
                    continue;
                };
                let Some((name, span)) = name_param(call) else {
                    continue;
                };
                let this_emit = member_call_object(call, "$emit").is_some_and(|o| is_this(sem, o));
                if vc::in_script_setup(file, call.span) {
                    let c = setup.program.as_ref().filter(|(s, _)| call.span.start >= *s);
                    if c.is_some_and(|(_, c)| c.matches(call)) {
                        verify(ctx, &name, span);
                    }
                    if this_emit {
                        verify(ctx, &name, span);
                    }
                }
                if let Some(v) = innermost_vue(sem, &vue_ids, n.id()) {
                    if setup.vue.get(&v).is_some_and(|c| c.matches(call)) {
                        verify(ctx, &name, span);
                    }
                    if this_emit {
                        verify(ctx, &name, span);
                    }
                }
            }
        }
        let Some(doc) = file.doc.as_ref() else { return };
        let define = template_define(file, &objs, |_| true);
        // `isEmitVariableName`: only with `<script setup>` and `const <id> = defineEmits()`.
        let emit_var = match define {
            Some(TemplateDefine::Setup) => define_emits
                .and_then(|m| m.declarator.filter(|_| m.with_defaults.is_none()))
                .and_then(|d| d.id.get_binding_identifier())
                .map(|id| id.name.to_string()),
            _ => None,
        };
        for call in template_calls(doc) {
            let Some((name, span)) = name_param(call) else {
                continue;
            };
            if callee_ident(call).is_some_and(|c| c == "$emit" || emit_var.as_deref() == Some(c)) {
                verify(ctx, &name, span);
            }
        }
    }
}
