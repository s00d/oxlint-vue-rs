//! vue/no-define-model-default

use oxc::ast::ast::Expression;

use crate::rule::{Ctx, Rule, RuleOptions, fmt_msg};
use crate::utils::js;
use crate::utils::vue_component::{self as vc, MacroKind};

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(NoDefineModelDefault))
}

struct NoDefineModelDefault;

impl Rule for NoDefineModelDefault {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        for m in vc::macros_of(ctx.file, MacroKind::DefineModel) {
            let model = vc::model_from_define_model(m.call);
            let Some(Expression::ObjectExpression(o)) = model.options.map(js::skip_ts_as) else {
                continue;
            };
            if let Some(d) = js::find_property(o, "default") {
                ctx.report_id(
                    d.span,
                    "unexpected",
                    fmt_msg(
                        "Unexpected default value for the \"{{name}}\" model.",
                        &[("name", &model.name)],
                    ),
                );
            }
        }
    }
}
