//! vue/custom-event-name-casing

use std::collections::HashSet;

use oxc::ast::AstKind;
use oxc::semantic::NodeId;
use oxc::span::Span;

use crate::rule::{Ctx, Rule, RuleOptions, fmt_msg};
use crate::utils::casing;
use crate::utils::regexp::GroupMatcher;
use crate::utils::tpl_analysis::{
    callee_ident, emit_setup, innermost_vue, member_call_object, name_param, template_calls,
};
use crate::utils::vue_component as vc;

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let case = opts.get(0).as_str().unwrap_or("camelCase").to_string();
    let ignores = opts.get(1)["ignores"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
        .unwrap_or_default();
    Some(Box::new(CustomEventNameCasing {
        checker: casing::checker(&case),
        case,
        ignores: GroupMatcher::new(ignores),
    }))
}

struct CustomEventNameCasing {
    case: String,
    checker: fn(&str) -> bool,
    ignores: GroupMatcher,
}

impl CustomEventNameCasing {
    fn verify(&self, ctx: &mut Ctx, name: &str, span: Span) {
        if name.split(':').all(self.checker) || self.ignores.is_match(name) {
            return;
        }
        ctx.report_id(
            span,
            "unexpected",
            fmt_msg(
                "Custom event name '{{name}}' must be {{caseType}}.",
                &[("name", name), ("caseType", &self.case)],
            ),
        );
    }
}

impl Rule for CustomEventNameCasing {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let file = ctx.file;
        let setup = emit_setup(file, false, true);
        if let Some(sem) = file.semantic {
            let vue_ids: HashSet<NodeId> =
                vc::vue_objects(file).iter().map(|o| o.node_id()).collect();
            for n in sem.nodes().iter() {
                let AstKind::CallExpression(call) = n.kind() else {
                    continue;
                };
                let Some((name, span)) = name_param(call) else {
                    continue;
                };
                if vc::in_script_setup(file, call.span)
                    && setup
                        .program
                        .as_ref()
                        .is_some_and(|(s, c)| call.span.start >= *s && c.matches(call))
                {
                    self.verify(ctx, &name, span);
                }
                if let Some(v) = innermost_vue(sem, &vue_ids, n.id())
                    && setup.vue.get(&v).is_some_and(|c| c.matches(call))
                {
                    self.verify(ctx, &name, span);
                }
                if member_call_object(call, "$emit").is_some() {
                    self.verify(ctx, &name, span);
                }
            }
        }
        let Some(doc) = file.doc.as_ref() else { return };
        for call in template_calls(doc) {
            let Some((name, span)) = name_param(call) else {
                continue;
            };
            if callee_ident(call)
                .is_some_and(|c| c == "$emit" || setup.param_name.as_deref() == Some(c))
            {
                self.verify(ctx, &name, span);
            }
        }
    }
}
