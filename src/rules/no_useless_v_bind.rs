//! vue/no-useless-v-bind

use oxc::span::Span;

use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::js::{is_string_literal, string_literal_value};
use crate::utils::template::{directive_expression, directive_value};
use crate::utils::tpl_misc::{has_js_comment, has_useful_escape, string_raw};
use crate::utils::tpl_valid::body_directives;

struct R {
    ignore_comment: bool,
    ignore_escape: bool,
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (_, _, a, k) in body_directives(doc, "bind") {
            if k.argument.is_none() || !k.modifiers.is_empty() {
                continue;
            }
            let (Some(c), Some(x)) = (directive_value(doc, a), directive_expression(doc, a)) else { continue };
            if !is_string_literal(x) {
                continue;
            }
            let Some(value) = string_literal_value(x) else { continue };
            let raw = string_raw(doc, x).unwrap_or("");
            let has_comment = has_js_comment(doc, c.span);
            if self.ignore_comment && has_comment {
                continue;
            }
            let has_escape = has_useful_escape(raw, &value);
            if self.ignore_escape && has_escape {
                continue;
            }
            let fix = if has_comment || has_escape {
                None
            } else {
                let text = doc.slice(c.span);
                let shorthand = k.name.raw_name == ":";
                let end = k.name.span.end + if shorthand { 0 } else { 1 };
                let v = match text.chars().next() {
                    Some('"') => format!("\"{}\"", value.replace('"', "&quot;")),
                    Some('\'') => format!("'{}'", value.replace('\'', "&apos;")),
                    _ => value.replace('"', "&quot;").replace('\'', "&apos;"),
                };
                Fix::merge(doc.source, vec![Fix::remove(Span::new(k.name.span.start, end)), Fix::replace(c.span, v)])
            };
            ctx.report_fix(a.span, None, "Unexpected `v-bind` with a string literal value.", fix);
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let o = opts.get(0);
    Some(Box::new(R {
        ignore_comment: o["ignoreIncludesComment"].as_bool() == Some(true),
        ignore_escape: o["ignoreStringEscape"].as_bool() == Some(true),
    }))
}
