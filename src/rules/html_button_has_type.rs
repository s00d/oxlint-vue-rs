//! vue/html-button-has-type

use crate::ast::AttributeValue;
use crate::rule::{Ctx, Rule, RuleOptions, fmt_msg};
use crate::utils::template::{directive_value, value_span};
use crate::utils::tpl_valid::body_elements;

struct R {
    button: bool,
    submit: bool,
    reset: bool,
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for e in body_elements(doc) {
            if e.raw_name != "button" {
                continue;
            }
            if let Some(a) = e.attr("type") {
                match &a.value {
                    Some(AttributeValue::Literal(l)) if !l.value.is_empty() => {
                        let v = l.value.as_str();
                        let allowed = match v {
                            "button" => Some(self.button),
                            "submit" => Some(self.submit),
                            "reset" => Some(self.reset),
                            _ => None,
                        };
                        match allowed {
                            None => ctx.report(
                                l.span,
                                fmt_msg("{{value}} is an invalid value for button type attribute.", &[("value", v)]),
                            ),
                            Some(false) => ctx.report(
                                l.span,
                                fmt_msg("{{value}} is a forbidden value for button type attribute.", &[("value", v)]),
                            ),
                            Some(true) => {}
                        }
                    }
                    _ => ctx.report(
                        value_span(doc, a).unwrap_or(a.span),
                        "A value must be set for button type attribute.",
                    ),
                }
                continue;
            }
            if let Some(d) = e.directive("bind", Some("type")) {
                if directive_value(doc, d).is_none_or(|c| c.expression.is_none()) {
                    ctx.report(
                        value_span(doc, d).unwrap_or(d.span),
                        "A value must be set for button type attribute.",
                    );
                }
                continue;
            }
            ctx.report(e.start_tag.span, "Missing an explicit type attribute for button.");
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let o = opts.get(0);
    let b = |k: &str| o[k].as_bool().unwrap_or(true);
    Some(Box::new(R { button: b("button"), submit: b("submit"), reset: b("reset") }))
}
