//! vue/valid-attribute-name

use crate::ast::{AttributeKey, DirectiveArgument};
use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::template::is_custom_component;
use crate::utils::tpl_valid::{body_elements, is_xml_name};

struct ValidAttributeName;

impl Rule for ValidAttributeName {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for e in body_elements(doc) {
            if is_custom_component(e, false) {
                continue;
            }
            for a in e.attributes() {
                let bad = match &a.key {
                    AttributeKey::Plain(k) => Some(&k.name).filter(|n| !is_xml_name(n)),
                    AttributeKey::Directive(k) => match &k.argument {
                        Some(DirectiveArgument::Static(id))
                            if k.name.name == "bind" && !is_xml_name(&id.name) =>
                        {
                            Some(&id.name)
                        }
                        _ => None,
                    },
                };
                if let Some(n) = bad {
                    ctx.report(a.span, format!("Attribute name {n} is not valid."));
                }
            }
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(ValidAttributeName))
}
