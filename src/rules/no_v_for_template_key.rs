//! vue/no-v-for-template-key

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::tpl_valid::body_directives;

struct NoVForTemplateKey;

impl Rule for NoVForTemplateKey {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (e, _, _, _) in body_directives(doc, "for") {
            if e.name != "template" {
                continue;
            }
            if let Some(k) = e.attr("key").or_else(|| e.directive("bind", Some("key"))) {
                ctx.report(
                    k.span,
                    "'<template v-for>' cannot be keyed. Place the key on real elements instead.",
                );
            }
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(NoVForTemplateKey))
}
