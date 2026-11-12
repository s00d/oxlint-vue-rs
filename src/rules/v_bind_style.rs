//! vue/v-bind-style

use oxc::ast::ast::Expression;
use oxc::span::Span;

use crate::ast::*;
use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::casing::{camel_case, is_kebab_case};
use crate::utils::js::skip_parens;
use crate::utils::template::{directive_expression, is_vbind_same_name_shorthand};
use crate::utils::tpl_valid::body_directives;

#[derive(PartialEq)]
enum Same {
    Always,
    Never,
    Ignore,
}

struct R {
    shorthand: bool,
    same: Same,
}

fn kebab_to_camel(s: &str) -> String {
    if is_kebab_case(s) {
        camel_case(s)
    } else {
        s.to_string()
    }
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        let src = ctx.source();
        for (_, _, a, k) in body_directives(doc, "bind") {
            let Some(arg) = &k.argument else { continue };
            // checkAttributeSameName
            if self.same != Same::Ignore {
                let attr = match arg {
                    DirectiveArgument::Static(id) => Some(id.raw_name.as_str()),
                    DirectiveArgument::Dynamic(_) => None,
                };
                let value = match directive_expression(doc, a).map(skip_parens) {
                    Some(Expression::Identifier(id)) => Some(id.name.as_str()),
                    _ => None,
                };
                if let (Some(attr), Some(value)) = (attr, value)
                    && kebab_to_camel(attr) == kebab_to_camel(value)
                {
                    let prefer = self.same == Same::Always;
                    if is_vbind_same_name_shorthand(doc, a) != prefer {
                        let (id, msg) = if prefer {
                            ("expectedShorthand", "Expected same-name shorthand.")
                        } else {
                            ("unexpectedShorthand", "Unexpected same-name shorthand.")
                        };
                        let fix = if prefer {
                            let cut = k.modifiers.last().map_or_else(
                                || match arg {
                                    DirectiveArgument::Static(i) => i.span.end,
                                    DirectiveArgument::Dynamic(e) => doc.expr(*e).span.end,
                                },
                                |m| m.span.end,
                            );
                            Some(Fix::remove(Span::new(cut, a.span.end)))
                        } else {
                            Some(Fix::insert(
                                a.span.end,
                                format!("=\"{}\"", kebab_to_camel(attr)),
                            ))
                        };
                        ctx.report_fix(a.span, Some(id), msg, fix);
                    }
                }
            }
            // checkAttributeStyle
            let prop = k.name.raw_name == ".";
            let is_short = k.name.raw_name == ":" || prop;
            if is_short == self.shorthand {
                continue;
            }
            let (id, msg) = if self.shorthand {
                ("unexpectedLonghand", "Unexpected 'v-bind' before ':'.")
            } else if prop {
                (
                    "expectedLonghandForProp",
                    "Expected 'v-bind:' instead of '.'.",
                )
            } else {
                ("expectedLonghand", "Expected 'v-bind' before ':'.")
            };
            let fix = if self.shorthand {
                Some(Fix::remove(k.name.span))
            } else {
                let mut edits = vec![Fix::insert(a.span.start, "v-bind")];
                if prop {
                    edits.push(Fix::replace(k.name.span, ":"));
                    if let Some(m) = k.modifiers.first()
                        && m.name == "prop"
                        && m.raw_name.is_empty()
                    {
                        edits.push(Fix::insert(m.span.start, ".prop"));
                    }
                }
                Fix::merge(src, edits)
            };
            ctx.report_fix(a.span, Some(id), msg, fix);
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(R {
        shorthand: opts.get(0).as_str() != Some("longform"),
        same: match opts.get(1)["sameNameShorthand"].as_str() {
            Some("always") => Same::Always,
            Some("never") => Same::Never,
            _ => Same::Ignore,
        },
    }))
}
