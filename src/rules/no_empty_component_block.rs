//! vue/no-empty-component-block

use crate::ast::{AttributeKey, Child};
use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::template::literal_value;

struct R;

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for b in doc.top_level() {
            if !matches!(b.name.as_str(), "template" | "script" | "style") {
                continue;
            }
            let attrs = b.attributes();
            let has_src = attrs.iter().any(|a| {
                matches!(&a.key, AttributeKey::Plain(k) if k.name == "src")
                    && literal_value(a).is_some_and(|v| !v.is_empty())
            });
            if !attrs.is_empty() && has_src {
                continue;
            }
            let empty = match b.children.as_slice() {
                [] => true,
                [Child::Text(t)] => doc.text(*t).value.trim().is_empty(),
                _ => false,
            };
            if empty {
                ctx.report_fix(
                    b.span,
                    Some("unexpected"),
                    format!("`<{}>` is empty. Empty block is not allowed.", b.name),
                    Some(Fix::remove(b.span)),
                );
            }
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(R))
}
