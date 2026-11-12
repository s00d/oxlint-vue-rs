//! vue/enforce-style-attribute

use crate::ast::Element;
use crate::rule::{Ctx, Rule, RuleOptions};

struct R {
    allowed: Vec<String>,
}

fn has(e: &Element, name: &str) -> bool {
    e.attr(name).is_some()
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        let allow = |s: &str| self.allowed.iter().any(|a| a == s);
        let mut sorted = self.allowed.clone();
        sorted.sort();
        let list = sorted.join(", ");
        for e in doc.top_level().filter(|e| e.raw_name == "style") {
            let (scoped, module) = (has(e, "scoped"), has(e, "module"));
            let (id, msg) = if !allow("plain") && !scoped && !module {
                ("notAllowedPlain", "Plain <style> tags are not allowed.")
            } else if !allow("scoped") && scoped {
                ("notAllowedScoped", "The scoped attribute is not allowed.")
            } else if !allow("module") && module {
                ("notAllowedModule", "The module attribute is not allowed.")
            } else {
                continue;
            };
            ctx.report_id(e.span, id, format!("{msg} Allowed: {list}."));
            return;
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let mut allowed: Vec<String> = opts.get(0)["allow"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();
    if allowed.is_empty() {
        allowed.push("scoped".into());
    }
    Some(Box::new(R { allowed }))
}
