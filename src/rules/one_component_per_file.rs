//! vue/one-component-per-file

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::vue_component::{self as vc, DefinitionType, VueObjectType};

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(OneComponentPerFile))
}

struct OneComponentPerFile;

impl Rule for OneComponentPerFile {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let file = ctx.file;
        let components: Vec<_> = vc::vue_objects(file)
            .into_iter()
            .filter(|o| o.ty.is_component())
            .filter(|o| {
                o.ty != VueObjectType::Definition
                    || vc::definition_type(file, o.obj) != Some(DefinitionType::Mixin)
            })
            .collect();
        if components.len() > 1 {
            for c in components {
                ctx.report_id(
                    c.obj.span,
                    "tooManyComponents",
                    "There is more than one component in this file.",
                );
            }
        }
    }
}
