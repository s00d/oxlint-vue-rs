//! vue/valid-model-definition

use oxc::ast::ast::Expression;
use oxc::span::GetSpan;

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::js::{find_property, prop_name, properties};
use crate::utils::vue_component::vue_objects;

struct ValidModelDefinition;

impl Rule for ValidModelDefinition {
    fn run(&self, ctx: &mut Ctx) {
        for vo in vue_objects(ctx.file) {
            let Some(model) = find_property(vo.obj, "model") else {
                continue;
            };
            let Expression::ObjectExpression(o) = &model.value else {
                continue;
            };
            for p in properties(o) {
                let Some(name) = prop_name(p) else { continue };
                if name != "prop" && name != "event" {
                    ctx.report(p.span(), format!("Invalid key '{name}' in model option."));
                }
            }
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(ValidModelDefinition))
}
