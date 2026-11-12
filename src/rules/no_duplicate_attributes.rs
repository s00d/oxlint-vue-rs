//! vue/no-duplicate-attributes

use std::collections::HashSet;

use crate::ast::{AttributeKey, DirectiveArgument};
use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::tpl_valid::body_elements;

struct NoDuplicateAttributes {
    allow_style: bool,
    allow_class: bool,
}

impl Rule for NoDuplicateAttributes {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for e in body_elements(doc) {
            let mut directives: HashSet<&str> = HashSet::new();
            let mut attrs: HashSet<&str> = HashSet::new();
            for a in e.attributes() {
                let (name, is_dir) = match &a.key {
                    AttributeKey::Plain(k) => (k.name.as_str(), false),
                    AttributeKey::Directive(k) => match &k.argument {
                        Some(DirectiveArgument::Static(id)) if k.name.name == "bind" => {
                            (id.name.as_str(), true)
                        }
                        _ => continue,
                    },
                };
                let coexist =
                    (self.allow_style && name == "style") || (self.allow_class && name == "class");
                let dup = if coexist {
                    if is_dir {
                        directives.contains(name)
                    } else {
                        attrs.contains(name)
                    }
                } else {
                    directives.contains(name) || attrs.contains(name)
                };
                if dup {
                    ctx.report(a.span, format!("Duplicate attribute '{name}'."));
                }
                if is_dir {
                    directives.insert(name);
                } else {
                    attrs.insert(name);
                }
            }
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let o = opts.get(0);
    Some(Box::new(NoDuplicateAttributes {
        allow_style: o["allowCoexistStyle"].as_bool() != Some(false),
        allow_class: o["allowCoexistClass"].as_bool() != Some(false),
    }))
}
