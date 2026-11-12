//! vue/no-template-key

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::tpl_valid::body_elements;

struct NoTemplateKey;

impl Rule for NoTemplateKey {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for e in body_elements(doc) {
            if e.name != "template" || e.has_directive("for") {
                continue;
            }
            if let Some(k) = e.attr("key").or_else(|| e.directive("bind", Some("key"))) {
                ctx.report(
                    k.span,
                    "'<template>' cannot be keyed. Place the key on real elements instead.",
                );
            }
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(NoTemplateKey))
}
