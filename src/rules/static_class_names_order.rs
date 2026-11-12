//! vue/static-class-names-order

use std::cmp::Ordering;

use crate::ast::{AttributeKey, AttributeValue};
use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::tpl_misc::{body_attributes, js_split_ws};

struct R;


/// `String.prototype.localeCompare` (ICU root collation, non-ignorable punctuation) via the UCA
/// implementation in `feruca`; matches Node on randomized Unicode samples.
fn locale_compare(a: &str, b: &str) -> Ordering {
    thread_local! {
        static COLLATOR: std::cell::RefCell<feruca::Collator> =
            std::cell::RefCell::new(feruca::Collator::new(feruca::Tailoring::default(), false, true));
    }
    COLLATOR.with(|c| c.borrow_mut().collate(a, b))
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (_, a) in body_attributes(doc) {
            let AttributeKey::Plain(k) = &a.key else { continue };
            if k.name != "class" {
                continue;
            }
            let Some(AttributeValue::Literal(v)) = &a.value else { continue };
            let parts = js_split_ws(&v.value);
            let divider = parts.get(1).cloned().unwrap_or_default();
            let mut names: Vec<&str> = parts.iter().map(String::as_str).filter(|s| !s.trim().is_empty()).collect();
            names.sort_by(|x, y| locale_compare(x, y));
            let sorted = names.join(&divider);
            if v.value != sorted {
                ctx.report_fix(
                    a.span,
                    None,
                    "Classes should be ordered alphabetically.",
                    Some(Fix::replace(v.span, format!("\"{sorted}\""))),
                );
            }
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(R))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn order() {
        assert_eq!(locale_compare("a", "B"), Ordering::Less);
        assert_eq!(locale_compare("a", "A"), Ordering::Less);
        assert_eq!(locale_compare("a-b", "ab"), Ordering::Less);
        assert_eq!(locale_compare("b", "a"), Ordering::Greater);
    }
}
