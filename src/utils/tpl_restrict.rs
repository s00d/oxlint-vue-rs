//! Helpers for the `no-restricted-*` template rules.

use crate::utils::regexp::JsRegex as Regex;
use serde_json::Value;

use crate::ast::{DirectiveArgument, DirectiveKey, Element};
use crate::rule::Ctx;
use crate::utils::regexp::to_regexp;
use crate::utils::template::directive_key;
use crate::utils::tpl_valid::body_elements;

/// A parsed `no-restricted-v-bind` / `no-restricted-v-on` option.
pub struct DirectiveOpt {
    /// `None` = the option `null` (matches a directive without argument).
    pub argument: Option<Regex>,
    pub modifiers: Vec<String>,
    pub element: Option<Regex>,
    pub message: Option<String>,
}

impl DirectiveOpt {
    pub fn parse(o: &Value) -> Option<Self> {
        let arg = |v: &Value| match v {
            Value::Null => Some(None),
            Value::String(s) => to_regexp(s).map(Some),
            _ => None,
        };
        if !o.is_object() {
            return Some(Self {
                argument: arg(o)?,
                modifiers: Vec::new(),
                element: None,
                message: None,
            });
        }
        let modifiers = o
            .get("modifiers")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|m| m.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let element = match o.get("element").and_then(Value::as_str) {
            Some(s) if !s.is_empty() => Some(to_regexp(s)?),
            _ => None,
        };
        Some(Self {
            argument: arg(o.get("argument")?)?,
            modifiers,
            element,
            message: o.get("message").and_then(Value::as_str).map(String::from),
        })
    }

    pub fn test(&self, e: &Element, k: &DirectiveKey) -> bool {
        (match (&self.argument, &k.argument) {
            (None, arg) => arg.is_none(),
            (Some(r), Some(DirectiveArgument::Static(id))) => r.is_match(&id.raw_name),
            _ => false,
        }) && self
            .modifiers
            .iter()
            .all(|m| k.modifiers.iter().any(|x| &x.name == m))
            && self
                .element
                .as_ref()
                .is_none_or(|r| r.is_match(&e.raw_name))
    }
}

/// Report the key of every template directive `name` matched by an option; `message` builds the
/// default message.
pub fn check_directives(
    ctx: &mut Ctx,
    name: &str,
    options: &[DirectiveOpt],
    id: &'static str,
    message: impl Fn(&Element, &DirectiveKey, &DirectiveOpt) -> String,
) {
    let Some(doc) = ctx.doc() else { return };
    for e in body_elements(doc) {
        for a in e.attributes() {
            let Some(k) = directive_key(a).filter(|k| k.name.name == name) else {
                continue;
            };
            if let Some(o) = options.iter().find(|o| o.test(e, k)) {
                let m = o.message.clone().unwrap_or_else(|| message(e, k, o));
                ctx.report_id(k.span, id, m);
            }
        }
    }
}

/// `:arg` part of a default message (static argument only).
pub fn static_arg_raw(k: &DirectiveKey) -> Option<&str> {
    match &k.argument {
        Some(DirectiveArgument::Static(id)) => Some(&id.raw_name),
        _ => None,
    }
}

/// `.mod1.mod2` part of a default message.
pub fn mods_suffix(o: &DirectiveOpt) -> String {
    if o.modifiers.is_empty() {
        String::new()
    } else {
        format!(".{}", o.modifiers.join("."))
    }
}
