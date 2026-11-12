//! vue/no-deprecated-html-element-is

use crate::ast::{AttributeKey, Element};
use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::template::*;
use crate::utils::tpl_misc::{body_attributes, static_arg};

struct R;

fn is_valid_element(e: &Element) -> bool {
    !is_html_well_known_element_name(&e.raw_name)
        && !is_svg_well_known_element_name(&e.raw_name)
        && !is_math_well_known_element_name(&e.raw_name)
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (e, a) in body_attributes(doc) {
            let hit = match &a.key {
                AttributeKey::Directive(k) => {
                    k.name.name == "bind" && static_arg(k).is_some_and(|i| i.name == "is")
                }
                AttributeKey::Plain(k) => {
                    k.name == "is" && !literal_value(a).is_some_and(|v| v.starts_with("vue:"))
                }
            };
            if hit && !is_valid_element(e) {
                ctx.report(a.span, "The `is` attribute on HTML element are deprecated.");
            }
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(R))
}
