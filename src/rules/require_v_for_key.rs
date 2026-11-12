//! vue/require-v-for-key

use crate::ast::{Child, Document, Element};
use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::template::is_custom_component;
use crate::utils::tpl_valid::body_directives;

fn check_key(ctx: &mut Ctx, doc: &Document, e: &Element) {
    if e.directive("bind", Some("key")).is_some() {
        return;
    }
    if e.name == "template" || e.name == "slot" {
        for c in &e.children {
            if let Child::Element(id) = c {
                check_key(ctx, doc, doc.element(*id));
            }
        }
    } else if !is_custom_component(e, false) {
        ctx.report(
            e.start_tag.span,
            "Elements in iteration expect to have 'v-bind:key' directives.",
        );
    }
}

struct RequireVForKey;

impl Rule for RequireVForKey {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (e, _, _, _) in body_directives(doc, "for") {
            check_key(ctx, doc, e);
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(RequireVForKey))
}
