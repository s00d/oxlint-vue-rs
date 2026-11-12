//! vue/no-useless-v-bind

use oxc::span::Span;

use crate::ast::DirectiveArgument;
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
        for (e, _, a, k) in body_directives(doc, "bind") {
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
            // A dynamic name (`:[k]="'v'"`) has no static spelling, and a static attribute of the same
            // name next to it (`class="a" :class="'b'"`, merged by Vue) would become a duplicate.
            let name = match &k.argument {
                Some(DirectiveArgument::Static(id)) => Some(id.name.as_str()),
                _ => None,
            };
            let duplicate = name.is_some_and(|n| e.attr(n).is_some());
            let fix = if has_comment || has_escape || name.is_none() || duplicate {
                None
            } else {
                let text = doc.slice(c.span);
                let shorthand = k.name.raw_name == ":";
                let end = k.name.span.end + if shorthand { 0 } else { 1 };
                let v = match text.chars().next() {
                    Some('"') => format!("\"{}\"", value.replace('"', "&quot;")),
                    Some('\'') => format!("'{}'", value.replace('\'', "&apos;")),
                    // Unquoted: quote a value an unquoted attribute cannot hold.
                    _ if value.is_empty()
                        || value.contains([' ', '\t', '\n', '\r', '\x0c', '>', '=', '<', '`']) =>
                    {
                        format!("\"{}\"", value.replace('"', "&quot;"))
                    }
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
