//! vue/no-unused-vars

use std::collections::HashMap;

use oxc::ast::ast::BindingPattern;
use oxc::span::Span;
use crate::utils::regexp::JsRegex as Regex;

use crate::ast::{Document, Element, ElementId, VExpression, VariableKind};
use crate::rule::{Ctx, Fix, Rule, RuleOptions, Suggestion, fmt_msg};
use crate::utils::tpl_valid::body_elements;

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let pattern = opts.get(0)["ignorePattern"].as_str().map(str::to_string);
    Some(Box::new(NoUnusedVars {
        re: pattern.as_deref().and_then(|p| Regex::new(p).ok()),
        pattern,
    }))
}

struct NoUnusedVars {
    pattern: Option<String>,
    re: Option<Regex>,
}

/// Spans of the variables declared directly (not by destructuring) on an element.
fn direct_ids(doc: &Document, e: &Element) -> Vec<Span> {
    let mut out = Vec::new();
    fn top(p: &BindingPattern, out: &mut Vec<Span>) {
        match p {
            BindingPattern::BindingIdentifier(id) => out.push(id.span),
            BindingPattern::AssignmentPattern(a) => top(&a.left, out),
            _ => {}
        }
    }
    for a in e.attributes() {
        let Some(crate::ast::AttributeValue::Expr(id)) = &a.value else {
            continue;
        };
        match &doc.expr(*id).expression {
            Some(VExpression::For { left, rest, .. }) => {
                for p in left.iter().flatten() {
                    top(p, &mut out);
                }
                if let Some(r) = rest {
                    top(&r.argument, &mut out);
                }
            }
            Some(VExpression::SlotScope { params, .. }) => {
                for p in &params.items {
                    top(&p.pattern, &mut out);
                }
                if let Some(r) = &params.rest {
                    top(&r.rest.argument, &mut out);
                }
            }
            _ => {}
        }
    }
    out
}

fn used_as_tag(doc: &Document, name: &str, e: &Element) -> bool {
    e.children.iter().any(|c| match c {
        crate::ast::Child::Element(id) => {
            let c = doc.element(*id);
            c.raw_name == name || used_as_tag(doc, name, c)
        }
        _ => false,
    })
}

impl Rule for NoUnusedVars {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let Some(doc) = ctx.doc() else { return };
        let mut refs: HashMap<(ElementId, u32), usize> = HashMap::new();
        for c in &doc.exprs {
            for r in &c.references {
                if let Some(v) = r.variable {
                    *refs.entry(v).or_default() += 1;
                }
            }
        }
        for e in body_elements(doc) {
            if e.variables.is_empty() {
                continue;
            }
            let direct = direct_ids(doc, e);
            for kind in [VariableKind::VFor, VariableKind::Scope, VariableKind::Generic] {
                let mut after_used = false;
                for (i, v) in e.variables.iter().enumerate().rev() {
                    if v.kind != kind {
                        continue;
                    }
                    if refs.get(&(e.id, i as u32)).is_some_and(|n| *n > 0) {
                        after_used = true;
                        continue;
                    }
                    if self.re.as_ref().is_some_and(|r| r.is_match(&v.name))
                        || (after_used && direct.contains(&v.span))
                        || used_as_tag(doc, &v.name, e)
                    {
                        continue;
                    }
                    let sugg = if self.pattern.as_deref() == Some("^_") {
                        vec![Suggestion {
                            message_id: Some("replaceWithUnderscore"),
                            desc: fmt_msg(
                                "Replace `{{name}}` with `_{{name}}` to ignore the unused variable.",
                                &[("name", &v.name)],
                            ),
                            fix: Fix::replace(v.span, format!("_{}", v.name)),
                        }]
                    } else {
                        Vec::new()
                    };
                    ctx.push(
                        v.span,
                        Some("unusedVariable"),
                        fmt_msg("'{{name}}' is defined but never used.", &[("name", &v.name)]),
                        None,
                        sugg,
                    );
                }
            }
        }
    }
}
