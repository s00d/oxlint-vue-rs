//! vue/v-on-event-hyphenation

use crate::ast::DirectiveArgument;
use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::casing::converter;
use crate::utils::regexp::GroupMatcher;
use crate::utils::template::is_custom_component;
use crate::utils::tpl_valid::body_directives;

struct R {
    hyphenated: bool,
    ignore: Vec<String>,
    ignore_tags: GroupMatcher,
    autofix: bool,
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (e, _, a, k) in body_directives(doc, "on") {
            if !is_custom_component(e, false) || self.ignore_tags.is_match(&e.raw_name) {
                continue;
            }
            let Some(DirectiveArgument::Static(arg)) = &k.argument else {
                continue;
            };
            let name = arg.raw_name.as_str();
            if name.is_empty()
                || self.ignore.iter().any(|i| name.contains(i.as_str()))
                || if self.hyphenated {
                    name.to_lowercase() == name
                } else {
                    !name.contains('-')
                }
            {
                continue;
            }
            let text = doc.slice(k.span);
            let case = if self.hyphenated {
                "kebab-case"
            } else {
                "camelCase"
            };
            let fix = (self.autofix && !name.contains('_'))
                .then(|| Fix::replace(arg.span, converter(case)(name)));
            let (id, msg) = if self.hyphenated {
                ("mustBeHyphenated", "must be hyphenated")
            } else {
                ("cannotBeHyphenated", "can't be hyphenated")
            };
            ctx.report_fix(a.span, Some(id), format!("v-on event '{text}' {msg}."), fix);
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let o = opts.get(1);
    let strs = |v: &serde_json::Value| -> Vec<String> {
        v.as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str())
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default()
    };
    let tags = strs(&o["ignoreTags"]);
    Some(Box::new(R {
        hyphenated: opts.get(0).as_str() != Some("never"),
        ignore: strs(&o["ignore"]),
        ignore_tags: GroupMatcher::new(tags.iter().map(String::as_str)),
        autofix: o["autofix"].as_bool() == Some(true),
    }))
}
