//! vue/no-root-v-if

use crate::ast::Child;
use crate::rule::{Ctx, Rule, RuleOptions};

struct R;

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        let Some(root) = doc.template_body else { return };
        let root = doc.element(root);
        let els: Vec<_> = root
            .children
            .iter()
            .filter_map(|c| match c {
                Child::Element(id) => Some(doc.element(*id)),
                _ => None,
            })
            .collect();
        if els.len() == 1 && els[0].has_directive("if") {
            ctx.report(root.span, "`v-if` should not be used on root element without `v-else`.");
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(R))
}
