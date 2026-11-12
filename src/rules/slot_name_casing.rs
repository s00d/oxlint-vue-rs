//! vue/slot-name-casing

use crate::rule::{Ctx, Rule, RuleOptions, fmt_msg};
use crate::utils::casing;
use crate::utils::template::literal_value;
use crate::utils::tpl_valid::body_elements;

struct R {
    case: &'static str,
    check: fn(&str) -> bool,
}

fn is_single_word(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_lowercase())
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for e in body_elements(doc) {
            if e.name != "slot" {
                continue;
            }
            let Some(a) = e.attr("name") else { continue };
            if let Some(name) = literal_value(a).filter(|n| !n.is_empty())
                && !(self.check)(name)
            {
                ctx.report(
                    a.span,
                    fmt_msg("Slot name \"{{name}}\" is not {{caseType}}.", &[("name", name), ("caseType", self.case)]),
                );
            }
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let case = match opts.get(0).as_str() {
        Some("kebab-case") => "kebab-case",
        Some("singleword") => "singleword",
        _ => "camelCase",
    };
    let check = if case == "singleword" { is_single_word } else { casing::checker(case) };
    Some(Box::new(R { case, check }))
}
