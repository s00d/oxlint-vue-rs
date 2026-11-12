//! vue/no-custom-modifiers-on-v-model

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::template::is_custom_component;
use crate::utils::tpl_valid::body_directives;

const VALID_MODIFIERS: [&str; 3] = ["lazy", "number", "trim"];

struct NoCustomModifiersOnVModel;

impl Rule for NoCustomModifiersOnVModel {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (e, _, a, k) in body_directives(doc, "model") {
            if !is_custom_component(e, false) {
                continue;
            }
            for m in &k.modifiers {
                if !VALID_MODIFIERS.contains(&m.name.as_str()) {
                    ctx.report(
                        a.span,
                        format!(
                            "'v-model' directives don't support the modifier '{}'.",
                            m.name
                        ),
                    );
                }
            }
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(NoCustomModifiersOnVModel))
}
