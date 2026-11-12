//! vue/no-restricted-custom-event

use std::collections::HashSet;

use oxc::ast::AstKind;
use oxc::semantic::NodeId;
use oxc::span::Span;
use crate::utils::regexp::JsRegex as Regex;

use crate::rule::{Ctx, Fix, Rule, RuleOptions, Suggestion, fmt_msg};
use crate::utils::js;
use crate::utils::regexp::to_regexp;
use crate::utils::tpl_analysis::{
    callee_ident, emit_setup, innermost_vue, member_call_object, name_param, template_calls,
};
use crate::utils::vue_component as vc;

struct Opt {
    test: Option<Regex>,
    message: Option<String>,
    suggest: Option<String>,
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let options = opts
        .options
        .iter()
        .map(|o| match o.as_str() {
            Some(s) => Opt { test: to_regexp(s), message: None, suggest: None },
            None => Opt {
                test: o["event"].as_str().and_then(to_regexp),
                message: o["message"].as_str().map(str::to_string),
                suggest: o["suggest"].as_str().map(str::to_string),
            },
        })
        .collect();
    Some(Box::new(NoRestrictedCustomEvent { options }))
}

struct NoRestrictedCustomEvent {
    options: Vec<Opt>,
}

impl NoRestrictedCustomEvent {
    fn verify(&self, ctx: &mut Ctx, name: &str, span: Span) {
        let Some(o) = self.options.iter().find(|o| o.test.as_ref().is_some_and(|r| r.is_match(name)))
        else {
            return;
        };
        let message = o
            .message
            .clone()
            .unwrap_or_else(|| format!("Using `{name}` event is not allowed."));
        let src = ctx.source().as_bytes();
        let sugg = o
            .suggest
            .as_ref()
            .map(|s| {
                let q1 = src[span.start as usize] as char;
                let q2 = src[span.end as usize - 1] as char;
                let j = js::json_string(s);
                let body = j[1..j.len() - 1].replace('\'', "\\'");
                vec![Suggestion {
                    message_id: Some("instead"),
                    desc: fmt_msg("Instead, change to `{{suggest}}`.", &[("suggest", s)]),
                    fix: Fix::replace(span, format!("{q1}{body}{q2}")),
                }]
            })
            .unwrap_or_default();
        ctx.push(span, Some("restrictedEvent"), message, None, sugg);
    }
}

impl Rule for NoRestrictedCustomEvent {
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
            if callee_ident(call) == Some("$emit") {
                self.verify(ctx, &name, span);
            }
        }
    }
}
