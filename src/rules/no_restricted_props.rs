//! vue/no-restricted-props

use oxc::ast::ast::{Expression, PropertyKey};
use oxc::span::{GetSpan, Span};
use crate::utils::regexp::JsRegex as Regex;
use serde_json::Value;

use crate::rule::{Ctx, Fix, Rule, RuleOptions, Suggestion, fmt_msg};
use crate::utils::js;
use crate::utils::regexp::to_regexp;
use crate::utils::vue_component::{self as vc, ComponentProp, Macro, MacroKind};

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let options = opts
        .options
        .iter()
        .filter_map(|o| {
            let (name, message, suggest) = match o {
                Value::String(s) => (s.as_str(), None, None),
                _ => (
                    o["name"].as_str()?,
                    o["message"].as_str().map(str::to_string),
                    o["suggest"].as_str().map(str::to_string),
                ),
            };
            Some(Opt {
                test: to_regexp(name)?,
                message,
                suggest,
            })
        })
        .collect();
    Some(Box::new(NoRestrictedProps { options }))
}

struct Opt {
    test: Regex,
    message: Option<String>,
    suggest: Option<String>,
}

struct NoRestrictedProps {
    options: Vec<Opt>,
}

enum KeyKind {
    Literal,
    Ident,
    Other,
}

fn key_kind(p: &ComponentProp) -> KeyKind {
    let lit = |e: &Expression| match js::skip_parens(e) {
        Expression::TemplateLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::BigIntLiteral(_)
        | Expression::RegExpLiteral(_) => KeyKind::Literal,
        Expression::Identifier(_) => KeyKind::Ident,
        _ => KeyKind::Other,
    };
    let pk = |k: &PropertyKey| match k {
        PropertyKey::StaticIdentifier(_) => KeyKind::Ident,
        PropertyKey::PrivateIdentifier(_) => KeyKind::Other,
        _ => k.as_expression().map_or(KeyKind::Other, lit),
    };
    match p {
        ComponentProp::Array { key: Some(k), .. } => lit(k),
        ComponentProp::Object { key: Some(k), .. } => pk(k),
        ComponentProp::Type(t) => pk(t.key),
        _ => KeyKind::Other,
    }
}

impl NoRestrictedProps {
    fn process(&self, ctx: &mut Ctx, props: &[ComponentProp], m: Option<&Macro>) {
        for p in props {
            let Some(name) = p.name().filter(|n| !n.is_empty()) else {
                continue;
            };
            let Some(opt) = self.options.iter().find(|o| o.test.is_match(&name)) else {
                continue;
            };
            let Some(key) = p.key_span() else { continue };
            let message = opt
                .message
                .clone()
                .unwrap_or_else(|| format!("Using `{name}` props is not allowed."));
            let mut suggestions = Vec::new();
            if let Some(s) = &opt.suggest {
                let replace = match key_kind(p) {
                    KeyKind::Literal => Some(js::json_string(s)),
                    KeyKind::Ident => Some(if is_simple_ident(s) {
                        s.clone()
                    } else {
                        js::json_string(s)
                    }),
                    KeyKind::Other => None,
                };
                if let Some(replace) = replace {
                    let mut fixes = vec![Fix::replace(key, replace.clone())];
                    if let Some(m) = m {
                        let mut targets: Vec<(bool, Span, Span)> = Vec::new();
                        if let Some(wd) = vc::with_defaults_props(m).get(&name) {
                            targets.push((wd.shorthand, wd.key.span(), wd.value.span()));
                        }
                        if let Some(d) = vc::props_destructure(m).get(&name) {
                            targets.push((d.shorthand, d.key.span(), d.value.span()));
                        }
                        for (shorthand, k, v) in targets {
                            fixes.push(if shorthand {
                                Fix::insert(v.start, format!("{replace}:"))
                            } else {
                                Fix::replace(k, replace.clone())
                            });
                        }
                    }
                    if let Some(fix) = Fix::merge(ctx.source(), fixes) {
                        suggestions.push(Suggestion {
                            message_id: Some("instead"),
                            desc: fmt_msg("Instead, change to `{{suggest}}`.", &[("suggest", s)]),
                            fix,
                        });
                    }
                }
            }
            ctx.push(key, Some("restrictedProp"), message, None, suggestions);
        }
    }
}

/// `/^[a-z]\w*$/iu`
fn is_simple_ident(s: &str) -> bool {
    let mut it = s.chars();
    it.next().is_some_and(|c| c.is_ascii_alphabetic())
        && it.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

impl Rule for NoRestrictedProps {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let file = ctx.file;
        for m in vc::macros_of(file, MacroKind::DefineProps) {
            self.process(ctx, &vc::props_from_define_props(file, m.call), Some(&m));
        }
        for o in vc::vue_objects(file) {
            self.process(ctx, &vc::props_from_options(o.obj), None);
        }
    }
}
