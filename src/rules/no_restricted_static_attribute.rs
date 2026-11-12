//! vue/no-restricted-static-attribute

use crate::utils::regexp::JsRegex as Regex;

use crate::ast::{Attribute, AttributeKey, AttributeValue, Element};
use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::regexp::to_regexp;
use crate::utils::tpl_valid::body_elements;

enum ValueTest {
    Any,
    /// `value: true` — no value or a value equal to the key.
    True,
    Matches(Regex),
}

struct Opt {
    key: Regex,
    value: ValueTest,
    element: Option<Regex>,
    message: Option<String>,
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let options = opts
        .options
        .iter()
        .filter_map(|o| {
            if let Some(s) = o.as_str() {
                return Some(Opt {
                    key: to_regexp(s)?,
                    value: ValueTest::Any,
                    element: None,
                    message: None,
                });
            }
            let value = match o.get("value") {
                Some(serde_json::Value::Bool(true)) => ValueTest::True,
                Some(serde_json::Value::String(s)) if !s.is_empty() => {
                    ValueTest::Matches(to_regexp(s)?)
                }
                _ => ValueTest::Any,
            };
            let element = match o.get("element").and_then(|v| v.as_str()) {
                Some(s) if !s.is_empty() => Some(to_regexp(s)?),
                _ => None,
            };
            Some(Opt {
                key: to_regexp(o.get("key")?.as_str()?)?,
                value,
                element,
                message: o.get("message").and_then(|m| m.as_str()).map(String::from),
            })
        })
        .collect();
    Some(Box::new(NoRestrictedStaticAttribute { options }))
}

struct NoRestrictedStaticAttribute {
    options: Vec<Opt>,
}

fn value_of(a: &Attribute) -> Option<&str> {
    match &a.value {
        Some(AttributeValue::Literal(l)) => Some(&l.value),
        _ => None,
    }
}

impl Opt {
    fn test(&self, e: &Element, raw: &str, a: &Attribute) -> bool {
        self.key.is_match(raw)
            && match &self.value {
                ValueTest::Any => true,
                ValueTest::True => value_of(a).is_none_or(|v| v == raw),
                ValueTest::Matches(r) => value_of(a).is_some_and(|v| r.is_match(v)),
            }
            && self.element.as_ref().is_none_or(|r| r.is_match(&e.raw_name))
    }
}

impl Rule for NoRestrictedStaticAttribute {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        if self.options.is_empty() {
            return;
        }
        let Some(doc) = ctx.doc() else { return };
        for e in body_elements(doc) {
            for a in e.attributes() {
                let AttributeKey::Plain(k) = &a.key else { continue };
                let Some(o) = self.options.iter().find(|o| o.test(e, &k.raw_name, a)) else {
                    continue;
                };
                let message = o.message.clone().unwrap_or_else(|| {
                    let value = match (&o.value, value_of(a)) {
                        (ValueTest::Any, _) => String::new(),
                        (_, None) => "` set to `true".to_string(),
                        (_, Some(v)) => format!("=\"{v}\""),
                    };
                    let on = if o.element.is_some() {
                        format!(" on `<{}>`", e.raw_name)
                    } else {
                        String::new()
                    };
                    format!("Using `{}{value}`{on} is not allowed.", k.raw_name)
                });
                ctx.report_id(a.span, "restrictedAttr", message);
            }
        }
    }
}
