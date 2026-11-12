//! vue/match-component-import-name

use oxc::ast::ast::Expression;

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::vue_component as vc;
use crate::utils::{casing, js};

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(MatchComponentImportName))
}

struct MatchComponentImportName;

impl Rule for MatchComponentImportName {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        for o in vc::vue_objects(ctx.file)
            .into_iter()
            .filter(|o| o.ty.is_component())
        {
            let Some(comps) = js::find_property(o.obj, "components") else {
                continue;
            };
            let Expression::ObjectExpression(comps) = &comps.value else {
                continue;
            };
            for p in js::properties(comps) {
                let Expression::Identifier(id) = &p.value else {
                    continue;
                };
                if p.computed {
                    continue;
                }
                let imported = js::prop_name(p).unwrap_or_default();
                let expected = [casing::pascal_case(&id.name), casing::kebab_case(&id.name)];
                if !expected.contains(&imported) {
                    let msg = format!(
                        "Component alias {imported} should be one of: {}.",
                        expected.join(", ")
                    );
                    ctx.report_id(p.span, "unexpected", msg);
                }
            }
        }
    }
}
