//! vue/no-v-for-template-key-on-child

use crate::ast::{Attribute, Child, Document, Element, VariableKind};
use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::template::directive_value;
use crate::utils::tpl_valid::body_directives;

fn uses_iteration_var(doc: &Document, for_el: &Element, key: &Attribute) -> bool {
    directive_value(doc, key).is_some_and(|c| {
        c.references.iter().any(|r| {
            for_el
                .variables
                .iter()
                .any(|v| v.kind == VariableKind::VFor && v.name == r.name)
        })
    })
}

struct NoVForTemplateKeyOnChild;

impl Rule for NoVForTemplateKeyOnChild {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (t, _, _, _) in body_directives(doc, "for") {
            if t.name != "template" {
                continue;
            }
            if t.directive("bind", Some("key"))
                .is_some_and(|k| uses_iteration_var(doc, t, k))
            {
                continue;
            }
            for c in &t.children {
                let Child::Element(id) = c else { continue };
                let child = doc.element(*id);
                if ["if", "else-if", "else", "for"]
                    .iter()
                    .any(|d| child.has_directive(d))
                {
                    continue;
                }
                if let Some(k) = child.directive("bind", Some("key"))
                    && uses_iteration_var(doc, t, k)
                {
                    ctx.report(
                        k.span,
                        "`<template v-for>` key should be placed on the `<template>` tag.",
                    );
                }
            }
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(NoVForTemplateKeyOnChild))
}
