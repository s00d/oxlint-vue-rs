//! vue/script-indent

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::indent::{Opts, run_script};

struct ScriptIndent(Opts);

impl Rule for ScriptIndent {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        run_script(ctx, &self.0);
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(ScriptIndent(Opts::parse(opts, 0))))
}
