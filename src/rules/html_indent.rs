//! vue/html-indent

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::indent::{Opts, run_html};

struct HtmlIndent(Opts);

impl Rule for HtmlIndent {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        run_html(ctx, &self.0);
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(HtmlIndent(Opts::parse(opts, 1))))
}
