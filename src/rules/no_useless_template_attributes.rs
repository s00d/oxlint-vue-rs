//! vue/no-useless-template-attributes

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::tpl_misc::{is_fragment_template_attribute, template_key_name};
use crate::utils::tpl_valid::body_elements;

struct R;

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for e in body_elements(doc) {
            if e.name != "template" || e.parent.is_none() {
                continue;
            }
            let attrs = e.attributes();
            if !attrs.iter().any(is_fragment_template_attribute) {
                continue;
            }
            for a in attrs {
                if is_fragment_template_attribute(a) || template_key_name(a) == Some("key") {
                    continue;
                }
                ctx.report(
                    a.span,
                    if a.is_directive() {
                        "Unexpected useless directive on `<template>`."
                    } else {
                        "Unexpected useless attribute on `<template>`."
                    },
                );
            }
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(R))
}
