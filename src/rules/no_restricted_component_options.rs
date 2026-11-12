//! vue/no-restricted-component-options

use oxc::ast::ast::{Expression, ObjectExpression, ObjectPropertyKind};
use oxc::span::GetSpan;
use crate::utils::regexp::JsRegex as Regex;

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::vue_component::{self as vc, MacroKind};
use crate::utils::{js, regexp};

/// One path step: `*` (any member, spreads included) or a name pattern.
enum Step {
    Wildcard,
    Test(Option<Regex>),
}

struct Opt {
    steps: Vec<Step>,
    message: Option<String>,
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let names = |v: &serde_json::Value| -> Vec<String> {
        match v {
            serde_json::Value::String(s) => vec![s.clone()],
            serde_json::Value::Array(a) => a
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect(),
            _ => Vec::new(),
        }
    };
    let list: Vec<Opt> = opts
        .options
        .iter()
        .map(|o| {
            let (n, message) = match o {
                serde_json::Value::Object(m) => (
                    names(m.get("name").unwrap_or(&serde_json::Value::Null)),
                    m.get("message")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                ),
                v => (names(v), None),
            };
            let steps = n
                .iter()
                .map(|s| {
                    if s == "*" {
                        Step::Wildcard
                    } else {
                        Step::Test(regexp::to_regexp(s))
                    }
                })
                .collect();
            Opt { steps, message }
        })
        .collect();
    Some(Box::new(NoRestrictedComponentOptions { opts: list }))
}

struct NoRestrictedComponentOptions {
    opts: Vec<Opt>,
}

fn verify(
    ctx: &mut Ctx<'_, '_>,
    obj: &ObjectExpression,
    steps: &[Step],
    message: Option<&str>,
    path: &mut Vec<String>,
) {
    let Some((step, rest)) = steps.split_first() else {
        return;
    };
    for p in &obj.properties {
        let key = match (step, p) {
            (Step::Wildcard, _) => "*".to_string(),
            (Step::Test(re), ObjectPropertyKind::ObjectProperty(prop)) => {
                match js::prop_name(prop).filter(|n| !n.is_empty()) {
                    Some(n) if re.as_ref().is_some_and(|r| r.is_match(&n)) => n,
                    _ => continue,
                }
            }
            (Step::Test(_), ObjectPropertyKind::SpreadProperty(_)) => continue,
        };
        if !rest.is_empty() {
            if let ObjectPropertyKind::ObjectProperty(prop) = p
                && let Expression::ObjectExpression(inner) = &prop.value
            {
                path.push(key);
                verify(ctx, inner, rest, message, path);
                path.pop();
            }
        } else {
            let msg = message.map(str::to_string).unwrap_or_else(|| {
                let mut full = path.clone();
                full.push(key);
                format!("Using `{}` is not allowed.", full.join("."))
            });
            let span = match p {
                ObjectPropertyKind::ObjectProperty(prop) => prop.key.span(),
                ObjectPropertyKind::SpreadProperty(s) => s.span,
            };
            ctx.report_id(span, "restrictedOption", msg);
        }
    }
}

impl Rule for NoRestrictedComponentOptions {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        if self.opts.is_empty() {
            return;
        }
        let file = ctx.file;
        let mut objs: Vec<&ObjectExpression> =
            vc::vue_objects(file).into_iter().map(|o| o.obj).collect();
        for m in vc::macros_of(file, MacroKind::DefineOptions) {
            if let Some(Expression::ObjectExpression(o)) = m.arg(0).map(js::skip_parens) {
                objs.push(o);
            }
        }
        for obj in objs {
            for o in &self.opts {
                verify(ctx, obj, &o.steps, o.message.as_deref(), &mut Vec::new());
            }
        }
    }
}
