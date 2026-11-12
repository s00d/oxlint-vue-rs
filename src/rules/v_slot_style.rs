//! vue/v-slot-style

use oxc::span::Span;

use crate::ast::DirectiveArgument;
use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::tpl_valid::{arg_span, body_directives};

#[derive(Clone, Copy, PartialEq)]
enum Style {
    Shorthand,
    Longform,
    VSlot,
}

fn style(v: &serde_json::Value) -> Option<Style> {
    match v.as_str()? {
        "shorthand" => Some(Style::Shorthand),
        "longform" => Some(Style::Longform),
        "v-slot" => Some(Style::VSlot),
        _ => None,
    }
}

struct R {
    at_component: Style,
    default: Style,
    named: Style,
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (e, _, a, k) in body_directives(doc, "slot") {
            let is_default = match &k.argument {
                None => true,
                Some(DirectiveArgument::Static(id)) => id.name == "default",
                _ => false,
            };
            let expected = if is_default {
                if e.name == "template" {
                    self.default
                } else {
                    self.at_component
                }
            } else {
                self.named
            };
            let actual = if k.name.raw_name == "#" {
                Style::Shorthand
            } else if k.argument.is_none() {
                Style::VSlot
            } else {
                Style::Longform
            };
            if actual == expected {
                continue;
            }
            let arg = arg_span(doc, k);
            let range = Span::new(k.name.span.start, arg.unwrap_or(k.name.span).end);
            let arg_text = arg.map_or("default", |s| doc.slice(s));
            let actual_text = doc.slice(range);
            let (id, msg, text) = match expected {
                Style::Shorthand => (
                    "expectedShorthand",
                    format!("Expected '#{arg_text}' instead of '{actual_text}'."),
                    format!("#{arg_text}"),
                ),
                Style::Longform => (
                    "expectedLongform",
                    format!("Expected 'v-slot:{arg_text}' instead of '{actual_text}'."),
                    format!("v-slot:{arg_text}"),
                ),
                Style::VSlot => (
                    "expectedVSlot",
                    format!("Expected 'v-slot' instead of '{actual_text}'."),
                    "v-slot".to_string(),
                ),
            };
            ctx.report_fix(a.span, Some(id), msg, Some(Fix::replace(range, text)));
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let o = opts.get(0);
    let mut r = R {
        at_component: Style::VSlot,
        default: Style::Shorthand,
        named: Style::Shorthand,
    };
    if let Some(s) = style(o) {
        r = R {
            at_component: s,
            default: s,
            named: s,
        };
    } else if o.is_object() {
        r.at_component = style(&o["atComponent"]).unwrap_or(r.at_component);
        r.default = style(&o["default"]).unwrap_or(r.default);
        r.named = style(&o["named"]).unwrap_or(r.named);
    }
    Some(Box::new(r))
}
