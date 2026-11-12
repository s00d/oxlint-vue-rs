//! vue/no-v-text-v-html-on-component

use crate::rule::{Ctx, Rule, RuleOptions, fmt_msg};
use crate::utils::casing::{kebab_case, pascal_case};
use crate::utils::template::{directive_key, is_custom_component};
use crate::utils::tpl_misc::body_attributes;

struct R {
    allow: Vec<String>,
    ignore_ns: bool,
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (e, a) in body_attributes(doc) {
            let Some(k) = directive_key(a) else { continue };
            if !matches!(k.name.name.as_str(), "text" | "html") {
                continue;
            }
            let n = &e.raw_name;
            let allowed = self.allow.contains(n)
                || self.allow.contains(&pascal_case(n))
                || self.allow.contains(&kebab_case(n));
            if is_custom_component(e, self.ignore_ns) && !allowed {
                let d = format!("v-{}", k.name.name);
                ctx.report(
                    a.span,
                    fmt_msg(
                        "Using {{directiveName}} on component may break component's content.",
                        &[("directiveName", &d)],
                    ),
                );
            }
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let o = opts.get(0);
    Some(Box::new(R {
        allow: o["allow"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
            .unwrap_or_default(),
        ignore_ns: o["ignoreElementNamespaces"].as_bool() == Some(true),
    }))
}
