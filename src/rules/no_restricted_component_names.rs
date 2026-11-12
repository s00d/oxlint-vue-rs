//! vue/no-restricted-component-names

use oxc::ast::ast::{Expression, ObjectExpression};
use oxc::span::GetSpan;
use crate::utils::regexp::JsRegex as Regex;

use crate::rule::{Ctx, Fix, Rule, RuleOptions, Suggestion};
use crate::utils::vue_component::{self as vc, MacroKind};
use crate::utils::{casing, js, regexp};

enum Matcher {
    Re(Option<Regex>),
    Name(String),
}

struct Opt {
    test: Matcher,
    message: Option<String>,
    suggest: Option<String>,
}

impl Opt {
    fn matches(&self, s: &str) -> bool {
        match &self.test {
            Matcher::Re(r) => r.as_ref().is_some_and(|r| r.is_match(s)),
            Matcher::Name(n) => s == casing::pascal_case(n) || s == casing::kebab_case(n),
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let parse = |name: &str| {
        if regexp::is_regexp(name) {
            Matcher::Re(regexp::to_regexp(name))
        } else {
            Matcher::Name(name.to_string())
        }
    };
    let list = opts
        .options
        .iter()
        .filter_map(|o| match o {
            serde_json::Value::String(s) => Some(Opt {
                test: parse(s),
                message: None,
                suggest: None,
            }),
            serde_json::Value::Object(m) => Some(Opt {
                test: parse(m.get("name")?.as_str()?),
                message: m
                    .get("message")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                suggest: m
                    .get("suggest")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
            }),
            _ => None,
        })
        .collect();
    Some(Box::new(NoRestrictedComponentNames { opts: list }))
}

struct NoRestrictedComponentNames {
    opts: Vec<Opt>,
}

impl NoRestrictedComponentNames {
    fn verify(&self, ctx: &mut Ctx<'_, '_>, obj: &ObjectExpression) {
        let Some(p) = js::find_property(obj, "name") else {
            return;
        };
        let value = js::skip_parens(&p.value);
        let name = match value {
            Expression::NullLiteral(_) => return,
            Expression::StringLiteral(_)
            | Expression::NumericLiteral(_)
            | Expression::BooleanLiteral(_)
            | Expression::BigIntLiteral(_)
            | Expression::RegExpLiteral(_) => js::string_literal_value(value),
            _ => None,
        };
        let Some(name) = name.filter(|n| !n.is_empty()) else {
            return;
        };
        for o in &self.opts {
            if !o.matches(&name) {
                continue;
            }
            let msg = o
                .message
                .clone()
                .unwrap_or_else(|| format!("Using component name `{name}` is not allowed."));
            let sugg = o
                .suggest
                .iter()
                .map(|s| Suggestion {
                    message_id: Some("suggest"),
                    desc: format!("Instead, change to `{s}`."),
                    fix: Fix::replace(p.value.span(), js::json_string(s)),
                })
                .collect();
            ctx.push(value.span(), Some("disallow"), msg, None, sugg);
        }
    }
}

impl Rule for NoRestrictedComponentNames {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let file = ctx.file;
        for o in vc::vue_objects(file) {
            self.verify(ctx, o.obj);
        }
        for m in vc::macros_of(file, MacroKind::DefineOptions) {
            if let Some(Expression::ObjectExpression(o)) = m.arg(0).map(js::skip_parens) {
                self.verify(ctx, o);
            }
        }
    }
}
