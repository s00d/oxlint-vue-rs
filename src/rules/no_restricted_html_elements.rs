//! vue/no-restricted-html-elements

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::template::{is_html_element, is_math_element, is_svg_element};
use crate::utils::tpl_valid::body_elements;

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let options = opts
        .options
        .iter()
        .map(|o| {
            let item = o.get("element").unwrap_or(o);
            let names = match item {
                serde_json::Value::Array(a) => {
                    a.iter().filter_map(|v| v.as_str().map(String::from)).collect()
                }
                v => v.as_str().map(String::from).into_iter().collect(),
            };
            (names, o.get("message").and_then(|m| m.as_str()).map(String::from))
        })
        .collect();
    Some(Box::new(NoRestrictedHtmlElements { options }))
}

struct NoRestrictedHtmlElements {
    options: Vec<(Vec<String>, Option<String>)>,
}

impl Rule for NoRestrictedHtmlElements {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let Some(doc) = ctx.doc() else { return };
        for e in body_elements(doc) {
            if !is_html_element(e) && !is_svg_element(e) && !is_math_element(e) {
                continue;
            }
            let Some((_, msg)) = self.options.iter().find(|(n, _)| n.contains(&e.raw_name)) else {
                continue;
            };
            match msg {
                Some(m) => ctx.report_id(e.start_tag.span, "customMessage", m.clone()),
                None => ctx.report_id(
                    e.start_tag.span,
                    "forbiddenElement",
                    format!("Unexpected use of forbidden element {}.", e.raw_name),
                ),
            }
        }
    }
}
