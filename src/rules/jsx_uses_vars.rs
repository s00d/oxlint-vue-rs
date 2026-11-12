//! vue/jsx-uses-vars
//!
//! Upstream only marks JSX identifiers as used for ESLint's core `no-unused-vars`; it never reports.
//! oxc's semantic analysis already counts JSX element names as references, so this is a no-op.

use crate::rule::{Ctx, Rule, RuleOptions};

struct JsxUsesVars;

impl Rule for JsxUsesVars {
    fn run(&self, _ctx: &mut Ctx) {}
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(JsxUsesVars))
}
