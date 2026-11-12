//! vue/no-deprecated-v-bind-sync

use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::tpl_valid::{arg_text, body_directives};

struct R;

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (_, _, a, k) in body_directives(doc, "bind") {
            if !k.has_modifier("sync") {
                continue;
            }
            let fix = if k.modifiers.len() > 1 {
                None
            } else {
                arg_text(doc, k).map(|t| Fix::replace(k.span, format!("v-model:{t}")))
            };
            ctx.report_fix(
                a.span,
                None,
                "'.sync' modifier on 'v-bind' directive is deprecated. Use 'v-model:propName' instead.",
                fix,
            );
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(R))
}
