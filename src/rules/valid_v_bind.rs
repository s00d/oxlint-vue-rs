//! vue/valid-v-bind

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::tpl_valid::{body_directives, missing_value};

const VALID_MODIFIERS: [&str; 4] = ["prop", "camel", "sync", "attr"];

struct ValidVBind;

impl Rule for ValidVBind {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (_, _, a, k) in body_directives(doc, "bind") {
            for m in &k.modifiers {
                if !VALID_MODIFIERS.contains(&m.name.as_str()) {
                    ctx.report(
                        m.span,
                        format!(
                            "'v-bind' directives don't support the modifier '{}'.",
                            m.name
                        ),
                    );
                }
            }
            if missing_value(doc, a) {
                ctx.report(a.span, "'v-bind' directives require an attribute value.");
            }
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(ValidVBind))
}
