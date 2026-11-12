//! vue/no-deprecated-router-link-tag-prop

use crate::ast::AttributeKey;
use crate::rule::{Ctx, Rule, RuleOptions, fmt_msg};
use crate::utils::casing::{kebab_case, pascal_case};
use crate::utils::tpl_misc::static_arg;
use crate::utils::tpl_valid::body_elements;

struct R {
    names: Vec<String>,
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for e in body_elements(doc) {
            if !self.names.contains(&e.raw_name) {
                continue;
            }
            let key = match e.attr("tag") {
                Some(a) => match &a.key {
                    AttributeKey::Plain(k) => Some(k.span),
                    _ => None,
                },
                None => e.directive("bind", Some("tag")).and_then(|d| match &d.key {
                    AttributeKey::Directive(k) => static_arg(k).map(|i| i.span),
                    _ => None,
                }),
            };
            if let Some(s) = key {
                ctx.report(
                    s,
                    fmt_msg(
                        "'tag' property on '{{element}}' component is deprecated. Use scoped slots instead.",
                        &[("element", &e.raw_name)],
                    ),
                );
            }
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let comps: Vec<String> = opts.get(0)["components"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_else(|| vec!["RouterLink".into()]);
    let names = comps.iter().flat_map(|c| [kebab_case(c), pascal_case(c)]).collect();
    Some(Box::new(R { names }))
}
