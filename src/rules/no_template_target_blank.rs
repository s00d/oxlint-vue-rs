//! vue/no-template-target-blank

use crate::ast::{Attribute, AttributeKey, AttributeValue, Element};
use crate::rule::{Ctx, Fix, Rule, RuleOptions, Suggestion};
use crate::utils::template::literal_value;
use crate::utils::tpl_misc::{body_attributes, static_arg};

struct R {
    allow_referrer: bool,
    dynamic_always: bool,
}

fn plain<'e>(e: &'e Element, name: &str) -> impl Iterator<Item = &'e Attribute> {
    let name = name.to_string();
    e.attributes()
        .iter()
        .filter(move |a| matches!(&a.key, AttributeKey::Plain(k) if k.name == name))
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (e, a) in body_attributes(doc) {
            let AttributeKey::Plain(k) = &a.key else { continue };
            if k.name != "target" || literal_value(a) != Some("_blank") {
                continue;
            }
            let secure = plain(e, "rel").any(|r| {
                literal_value(r).is_some_and(|v| {
                    let v = v.to_lowercase();
                    let tags: Vec<&str> = v.split(' ').collect();
                    tags.contains(&"noopener") && (self.allow_referrer || tags.contains(&"noreferrer"))
                })
            });
            if secure {
                continue;
            }
            let external = plain(e, "href").any(|h| {
                matches!(&h.value, Some(AttributeValue::Literal(l)) if {
                    let v = l.value.as_str();
                    v.starts_with("//") || {
                        let n = v.bytes().take_while(|b| b.is_ascii_alphanumeric() || *b == b'_').count();
                        n > 0 && v.as_bytes().get(n) == Some(&b':')
                    }
                })
            });
            let dynamic = e.attributes().iter().any(|h| {
                matches!(&h.key, AttributeKey::Directive(k) if k.name.name == "bind" && static_arg(k).is_some_and(|i| i.name == "href"))
            });
            if !(external || (self.dynamic_always && dynamic)) {
                continue;
            }
            let sugg = match plain(e, "rel").next() {
                Some(rel) => Suggestion {
                    message_id: None,
                    desc: "Change `rel` attribute value to `noopener noreferrer`.".into(),
                    fix: Fix::replace(rel.span, "rel=\"noopener noreferrer\""),
                },
                None => Suggestion {
                    message_id: None,
                    desc: "Add `rel=\"noopener noreferrer\"`.".into(),
                    fix: Fix::insert(a.span.end, " rel=\"noopener noreferrer\""),
                },
            };
            ctx.push(
                a.span,
                Some("missingRel"),
                "Using target=\"_blank\" without rel=\"noopener noreferrer\" is a security risk.".into(),
                None,
                vec![sugg],
            );
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let o = opts.get(0);
    Some(Box::new(R {
        allow_referrer: o["allowReferrer"].as_bool().unwrap_or(false),
        dynamic_always: o["enforceDynamicLinks"].as_str().unwrap_or("always") == "always",
    }))
}
