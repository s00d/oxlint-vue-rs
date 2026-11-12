//! vue/prefer-true-attribute-shorthand

use oxc::ast::ast::Expression;

use crate::ast::AttributeKey;
use crate::rule::{Ctx, Fix, Rule, RuleOptions, Suggestion};
use crate::utils::js::skip_parens;
use crate::utils::regexp::GroupMatcher;
use crate::utils::template::{directive_expression, is_custom_component};
use crate::utils::tpl_misc::{body_attributes, static_arg};

struct R {
    always: bool,
    except: GroupMatcher,
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (e, a) in body_attributes(doc) {
            if !is_custom_component(e, false) {
                continue;
            }
            let name = match &a.key {
                AttributeKey::Plain(k) => k.raw_name.as_str(),
                AttributeKey::Directive(k) if matches!(k.name.name.as_str(), "bind" | "model") => {
                    match static_arg(k) {
                        Some(i) => i.raw_name.as_str(),
                        None => continue,
                    }
                }
                AttributeKey::Directive(_) => continue,
            };
            let excepted = self.except.is_match(name);
            match &a.key {
                AttributeKey::Plain(k) if a.value.is_none() && (if self.always { excepted } else { !excepted }) => {
                    let n = &k.raw_name;
                    ctx.push(
                        a.span,
                        Some("expectLong"),
                        "Boolean prop with 'true' value should be written in long form.".into(),
                        None,
                        vec![
                            Suggestion {
                                message_id: Some("rewriteIntoLongVueProp"),
                                desc: "Rewrite this prop into long-form Vue component prop.".into(),
                                fix: Fix::replace(a.span, format!(":{n}=\"true\"")),
                            },
                            Suggestion {
                                message_id: Some("rewriteIntoLongHtmlAttr"),
                                desc: "Rewrite this prop into long-form HTML attribute.".into(),
                                fix: Fix::replace(a.span, format!("{n}=\"{n}\"")),
                            },
                        ],
                    );
                }
                AttributeKey::Directive(k) => {
                    let lit_true = directive_expression(doc, a)
                        .is_some_and(|x| matches!(skip_parens(x), Expression::BooleanLiteral(b) if b.value));
                    if lit_true && (if self.always { !excepted } else { excepted }) {
                        ctx.push(
                            a.span,
                            Some("expectShort"),
                            "Boolean prop with 'true' value should be written in shorthand form.".into(),
                            None,
                            vec![Suggestion {
                                message_id: Some("rewriteIntoShort"),
                                desc: "Rewrite this prop into shorthand form.".into(),
                                fix: Fix::replace(a.span, static_arg(k).unwrap().raw_name.clone()),
                            }],
                        );
                    }
                }
                _ => {}
            }
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let except: Vec<&str> = opts.get(1)["except"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    Some(Box::new(R { always: opts.get(0).as_str() != Some("never"), except: GroupMatcher::new(except) }))
}
