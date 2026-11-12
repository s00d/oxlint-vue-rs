//! vue/valid-v-pre

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::tpl_valid::{ValueRule, run_simple};

struct ValidV;

impl Rule for ValidV {
    fn run(&self, ctx: &mut Ctx) {
        run_simple(ctx, "pre", ValueRule::Forbidden);
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(ValidV))
}
