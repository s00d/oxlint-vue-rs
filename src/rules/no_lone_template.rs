//! vue/no-lone-template

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::tpl_misc::{is_fragment_template_attribute, template_key_name};
use crate::utils::tpl_valid::body_elements;

struct R {
    ignore_accessible: bool,
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for e in body_elements(doc) {
            if e.name != "template" || e.parent.is_none() {
                continue;
            }
            let attrs = e.attributes();
            if attrs.iter().any(is_fragment_template_attribute)
                || (self.ignore_accessible
                    && attrs.iter().any(|a| matches!(template_key_name(a), Some("id" | "ref"))))
            {
                continue;
            }
            ctx.report(e.start_tag.span, "`<template>` require directive.");
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(R { ignore_accessible: opts.get(0)["ignoreAccessible"].as_bool() == Some(true) }))
}
