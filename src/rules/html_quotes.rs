//! vue/html-quotes

use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::template::{is_vbind_same_name_shorthand, value_span};
use crate::utils::tpl_style::has_invalid_eof;
use crate::utils::tpl_valid::body_elements;

struct R {
    double: bool,
    avoid_escape: bool,
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        if has_invalid_eof(doc) {
            return;
        }
        let q = if self.double { '"' } else { '\'' };
        let kind = if self.double {
            "double quotes"
        } else {
            "single quotes"
        };
        for e in body_elements(doc) {
            for a in e.attributes() {
                let Some(span) = value_span(doc, a) else {
                    continue;
                };
                if is_vbind_same_name_shorthand(doc, a) {
                    continue;
                }
                let text = doc.slice(span);
                let first = text.chars().next();
                if first == Some(q) {
                    continue;
                }
                let quoted = matches!(first, Some('"' | '\''));
                let content = if quoted && text.len() >= 2 {
                    &text[1..text.len() - 1]
                } else if quoted {
                    ""
                } else {
                    text
                };
                if self.avoid_escape && quoted && content.contains(q) {
                    continue;
                }
                let mut to_double = self.double;
                if self.avoid_escape && !quoted && content.contains(q) {
                    to_double = if self.double {
                        content.contains('\'')
                    } else {
                        !content.contains('"')
                    };
                }
                let rep = if to_double {
                    format!("\"{}\"", content.replace('"', "&quot;"))
                } else {
                    format!("'{}'", content.replace('\'', "&apos;"))
                };
                ctx.report_fix(
                    span,
                    Some("expected"),
                    format!("Expected to be enclosed by {kind}."),
                    Some(Fix::replace(span, rep)),
                );
            }
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(R {
        double: opts.get(0).as_str() != Some("single"),
        avoid_escape: opts.get(1)["avoidEscape"].as_bool() == Some(true),
    }))
}
