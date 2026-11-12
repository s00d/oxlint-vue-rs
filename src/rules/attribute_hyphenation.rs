//! vue/attribute-hyphenation

use crate::ast::*;
use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::casing::exact_convert;
use crate::utils::regexp::GroupMatcher;
use crate::utils::template::{SVG_ATTRIBUTES_WEIRD_CASE, is_custom_component};
use crate::utils::tpl_style::key_span;
use crate::utils::tpl_valid::body_elements;

struct R {
    hyphenated: bool,
    ignore: Vec<String>,
    ignore_tags: GroupMatcher,
}

fn attribute_name(a: &Attribute) -> Option<&str> {
    match &a.key {
        AttributeKey::Plain(k) => Some(&k.raw_name),
        AttributeKey::Directive(k) if matches!(k.name.name.as_str(), "bind" | "model") => {
            match &k.argument {
                Some(DirectiveArgument::Static(id)) => Some(&id.raw_name),
                _ => None,
            }
        }
        AttributeKey::Directive(_) => None,
    }
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for e in body_elements(doc) {
            if (!is_custom_component(e, false) && e.name != "slot")
                || self.ignore_tags.is_match(&e.raw_name)
            {
                continue;
            }
            for a in e.attributes() {
                let Some(name) = attribute_name(a) else {
                    continue;
                };
                if self.ignore.iter().any(|i| name.contains(i.as_str()))
                    || SVG_ATTRIBUTES_WEIRD_CASE
                        .iter()
                        .any(|i| name.contains(i.as_str()))
                    || if self.hyphenated {
                        name.to_lowercase() == name
                    } else {
                        !name.contains('-')
                    }
                {
                    continue;
                }
                let text = doc.slice(key_span(a));
                let fix = (!(text.contains('_')
                    || text.ends_with(".sync")
                    || name.starts_with(|c: char| c.is_ascii_uppercase())))
                .then(|| {
                    let case = if self.hyphenated {
                        "kebab-case"
                    } else {
                        "camelCase"
                    };
                    Fix::replace(
                        key_span(a),
                        text.replacen(name, &exact_convert(case, name), 1),
                    )
                });
                let (id, msg) = if self.hyphenated {
                    ("mustBeHyphenated", "must be hyphenated")
                } else {
                    ("cannotBeHyphenated", "can't be hyphenated")
                };
                ctx.report_fix(a.span, Some(id), format!("Attribute '{text}' {msg}."), fix);
            }
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let o = opts.get(1);
    let mut ignore: Vec<String> = ["data-", "aria-", "slot-scope"].map(String::from).to_vec();
    if let Some(a) = o["ignore"].as_array() {
        ignore.extend(a.iter().filter_map(|v| v.as_str()).map(String::from));
    }
    let tags: Vec<&str> = o["ignoreTags"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    Some(Box::new(R {
        hyphenated: opts.get(0).as_str() != Some("never"),
        ignore,
        ignore_tags: GroupMatcher::new(tags),
    }))
}
