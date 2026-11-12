//! vue/valid-v-for

use oxc::ast::ast::BindingPattern;
use oxc::span::{GetSpan, Span};

use crate::ast::{Attribute, Child, Document, Element, VExpression, VariableKind};
use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::template::{directive_value, is_custom_component};
use crate::utils::tpl_valid::{ValueRule, body_directives, check_simple, missing_value};

/// Whether `attr`'s value references a `v-for` variable of `for_el`.
fn uses_iteration_var(doc: &Document, for_el: &Element, attr: &Attribute) -> bool {
    directive_value(doc, attr).is_some_and(|c| {
        c.references.iter().any(|r| {
            for_el
                .variables
                .iter()
                .any(|v| v.kind == VariableKind::VFor && v.name == r.name)
        })
    })
}

fn check_key(ctx: &mut Ctx, doc: &Document, for_el: &Element, el: &Element) {
    let key = el.directive("bind", Some("key"));
    match key {
        None if el.name == "template" => {
            for c in &el.children {
                if let Child::Element(id) = c {
                    let child = doc.element(*id);
                    if let Some(child_for) = child.directive("for", None)
                        && uses_iteration_var(doc, for_el, child_for)
                    {
                        continue;
                    }
                    check_key(ctx, doc, for_el, child);
                }
            }
        }
        None => {
            if is_custom_component(el, false) {
                ctx.report(
                    el.start_tag.span,
                    "Custom elements in iteration require 'v-bind:key' directives.",
                );
            }
        }
        Some(k) => {
            if !uses_iteration_var(doc, for_el, k) {
                ctx.report(
                    k.span,
                    "Expected 'v-bind:key' directive to use the variables which are defined by the 'v-for' directive.",
                );
            }
        }
    }
}

struct ValidVFor {
    allow_empty_alias: bool,
}

impl Rule for ValidVFor {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (e, _, a, k) in body_directives(doc, "for") {
            check_key(ctx, doc, e, e);
            check_simple(ctx, doc, a, k, ValueRule::Required);
            if missing_value(doc, a) {
                continue;
            }
            let Some(c) = directive_value(doc, a) else {
                continue;
            };
            let Some(expr) = &c.expression else { continue };
            let VExpression::For {
                span, left, rest, ..
            } = expr
            else {
                ctx.report(
                    c.span,
                    "'v-for' directives require the special syntax '<alias> in <expression>'.",
                );
                continue;
            };
            // ESTree `left`: aliases (null = hole) followed by the rest element.
            // (span, is plain identifier) per alias; `None` = empty alias.
            let mut aliases: Vec<Option<(Span, bool)>> = left
                .iter()
                .map(|p| {
                    p.as_ref()
                        .map(|p| (p.span(), matches!(p, BindingPattern::BindingIdentifier(_))))
                })
                .collect();
            if let Some(r) = rest {
                aliases.push(Some((r.span, false)));
            }
            if matches!(aliases.first(), Some(None)) && !self.allow_empty_alias {
                ctx.report(*span, "Invalid empty alias.");
            }
            for alias in aliases.iter().skip(1).take(2) {
                match alias {
                    Some((_, true)) => {}
                    None if self.allow_empty_alias => {}
                    None => ctx.report(*span, "Invalid empty alias."),
                    Some((s, false)) => {
                        ctx.report(*s, format!("Invalid alias '{}'.", doc.slice(*s)))
                    }
                }
            }
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(ValidVFor {
        allow_empty_alias: opts.get(0)["allowEmptyAlias"].as_bool() == Some(true),
    }))
}
