//! vue/block-order

use crate::ast::{AttributeKey, AttributeValue, Element};
use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::tpl_style::{Simple, has_invalid_eof, parse_selector, selector_matches};

struct R {
    /// (selector, order index)
    orders: Vec<(Vec<Vec<Simple>>, usize)>,
}

fn attribute_string(e: &Element) -> String {
    e.attributes()
        .iter()
        .map(|a| match &a.value {
            Some(AttributeValue::Expr(_)) => String::new(),
            v => {
                let name = match &a.key {
                    AttributeKey::Plain(k) => k.name.as_str(),
                    AttributeKey::Directive(_) => "[object Object]",
                };
                match v {
                    Some(AttributeValue::Literal(l)) if !l.value.is_empty() => {
                        format!("{name}={}", l.value)
                    }
                    _ => name.to_string(),
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        if has_invalid_eof(doc) {
            return;
        }
        let lines = ctx.file.lines;
        let src = ctx.source();
        let elements: Vec<&Element> = doc.top_level().collect();
        let with_order: Vec<(usize, &Element)> = elements
            .iter()
            .filter_map(|e| {
                self.orders
                    .iter()
                    .find(|(s, _)| selector_matches(s, e))
                    .map(|(_, i)| (*i, *e))
            })
            .collect();
        for (idx, (expected, element)) in with_order.iter().enumerate() {
            let Some((_, first)) = with_order[..idx]
                .iter()
                .filter(|(o, _)| expected < o)
                .min_by_key(|(o, _)| *o)
            else {
                continue;
            };
            let fa = attribute_string(first);
            let ea = attribute_string(element);
            let sp = |s: &str| {
                if s.is_empty() {
                    String::new()
                } else {
                    format!(" {s}")
                }
            };
            let msg = format!(
                "'<{}{}>' should be above '<{}{}>' on line {}.",
                element.name,
                sp(&ea),
                first.name,
                sp(&fa),
                lines.line_of(first.span.start) + 1
            );
            let fixed: Vec<&Element> = elements
                .iter()
                .flat_map(|it| {
                    if it.id == first.id {
                        vec![*element, *it]
                    } else if it.id == element.id {
                        vec![]
                    } else {
                        vec![*it]
                    }
                })
                .collect();
            let edits: Vec<Fix> = elements
                .iter()
                .zip(&fixed)
                .filter(|(a, b)| a.id != b.id)
                .map(|(a, b)| Fix::replace(a.span, doc.slice(b.span)))
                .collect();
            ctx.report_fix(
                element.span,
                Some("unexpected"),
                msg,
                Fix::merge(src, edits),
            );
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let default = serde_json::json!([["script", "template"], "style"]);
    let order = opts.get(0)["order"]
        .as_array()
        .unwrap_or_else(|| default.as_array().unwrap());
    let mut orders = Vec::new();
    for (i, o) in order.iter().enumerate() {
        match o {
            serde_json::Value::Array(a) => {
                for s in a.iter().filter_map(|v| v.as_str()) {
                    orders.push((parse_selector(s), i));
                }
            }
            serde_json::Value::String(s) => orders.push((parse_selector(s), i)),
            _ => {}
        }
    }
    Some(Box::new(R { orders }))
}
