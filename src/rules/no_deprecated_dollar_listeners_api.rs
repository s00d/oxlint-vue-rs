//! vue/no-deprecated-dollar-listeners-api

#[allow(unused_imports)]
use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::tpl_misc::{body_exprs, this_member_props};

struct R;

const NAME: &str = "$listeners";
const MSG: &str = "The `$listeners` is deprecated.";

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let mut spans = Vec::new();
        if let Some(doc) = ctx.doc() {
            for c in body_exprs(doc) {
                for r in &c.references {
                    if r.variable.is_none() && r.name == NAME {
                        spans.push(r.span);
                    }
                }
            }
        }
        spans.extend(this_member_props(ctx, NAME));
        for s in spans {
            ctx.report_fix(s, None, MSG, None);
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(R))
}
