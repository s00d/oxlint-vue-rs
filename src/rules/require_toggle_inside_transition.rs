//! vue/require-toggle-inside-transition

use oxc::ast::ast::Expression;

use crate::ast::Child;
use crate::rule::{Ctx, Rule, RuleOptions, fmt_msg};
use crate::utils::js::skip_parens;
use crate::utils::template::{directive_expression, is_custom_component};
use crate::utils::tpl_valid::body_elements;

struct R {
    directives: Vec<String>,
    list: String,
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for parent in body_elements(doc) {
            if parent.name != "transition" {
                continue;
            }
            let Some(e) = parent.children.iter().find_map(|c| match c {
                Child::Element(id) => Some(doc.element(*id)),
                _ => None,
            }) else {
                continue;
            };
            if is_custom_component(e, false) {
                continue;
            }
            let appear = parent.directive("bind", Some("appear"));
            if parent.attr("appear").is_some()
                || appear.is_some_and(|a| {
                    !directive_expression(doc, a)
                        .is_some_and(|x| matches!(skip_parens(x), Expression::BooleanLiteral(b) if !b.value))
                })
            {
                continue;
            }
            if e.name != "slot"
                && self.directives.iter().all(|d| !e.has_directive(d))
                && e.directive("bind", Some("key")).is_none()
            {
                ctx.report(
                    e.start_tag.span,
                    fmt_msg(
                        "The element inside `<transition>` is expected to have a {{allowedDirectives}} directive.",
                        &[("allowedDirectives", &self.list)],
                    ),
                );
            }
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let mut directives = vec!["if".to_string(), "show".to_string()];
    if let Some(a) = opts.get(0)["additionalDirectives"].as_array() {
        directives.extend(a.iter().filter_map(|v| v.as_str().map(str::to_string)));
    }
    let n = directives.len();
    let mut list = String::new();
    for (i, d) in directives.iter().enumerate() {
        if i == 0 {
            list += &format!("`v-{d}`");
        } else if i < n - 1 {
            list += &format!(", `v-{d}`");
        } else {
            list += &format!(" or `v-{d}`");
        }
    }
    Some(Box::new(R { directives, list }))
}
