//! vue/no-potential-component-option-typo

use oxc::span::GetSpan;

use crate::rule::{Ctx, Fix, Rule, RuleOptions, Suggestion};
use crate::utils::js;
use crate::utils::template::VUE_COMPONENT_OPTIONS;
use crate::utils::vue_component as vc;

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let o = opts.get(0);
    let mut candidates: Vec<String> = Vec::new();
    let mut add = |s: &str| {
        if !candidates.iter().any(|c| c == s) {
            candidates.push(s.to_string());
        }
    };
    for c in o
        .get("custom")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str())
    {
        add(c);
    }
    let presets: Vec<&str> = match o.get("presets").and_then(|v| v.as_array()) {
        Some(a) => a.iter().filter_map(|v| v.as_str()).collect(),
        None => vec!["vue"],
    };
    for p in presets {
        // `Object.values(vueComponentOptions)` order.
        let keys: Vec<&str> = if p == "all" {
            vec!["nuxt", "vue-router", "vue"]
        } else {
            vec![p]
        };
        for k in keys {
            for opt in VUE_COMPONENT_OPTIONS[k]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str())
            {
                add(opt);
            }
        }
    }
    if candidates.is_empty() {
        return Some(Box::new(Typo {
            candidates,
            threshold: 0,
        }));
    }
    let threshold = o.get("threshold").and_then(|v| v.as_f64()).unwrap_or(1.0) as usize;
    Some(Box::new(Typo {
        candidates,
        threshold: threshold.max(1),
    }))
}

struct Typo {
    candidates: Vec<String>,
    threshold: usize,
}

impl Rule for Typo {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        if self.candidates.is_empty() {
            return;
        }
        for o in vc::vue_objects(ctx.file) {
            for p in js::properties(o.obj) {
                let Some(name) = js::prop_name(p) else {
                    continue;
                };
                if self.candidates.contains(&name) {
                    continue;
                }
                let mut list: Vec<(&String, usize)> = self
                    .candidates
                    .iter()
                    .map(|c| (c, js::edit_distance(c, &name)))
                    .filter(|(_, d)| *d <= self.threshold && *d > 0)
                    .collect();
                list.sort_by_key(|(_, d)| *d);
                if list.is_empty() {
                    continue;
                }
                let key = p.key.span();
                let opts: Vec<&str> = list.iter().map(|(c, _)| c.as_str()).collect();
                let sugg = opts
                    .iter()
                    .map(|c| Suggestion {
                        message_id: None,
                        desc: format!("Replace property '{name}' to '{c}'"),
                        fix: Fix::replace(key, c.to_string()),
                    })
                    .collect();
                ctx.push(
                    key,
                    Some("potentialTypo"),
                    format!(
                        "'{name}' may be a typo, which is similar to option [{}].",
                        opts.join(",")
                    ),
                    None,
                    sugg,
                );
            }
        }
    }
}
