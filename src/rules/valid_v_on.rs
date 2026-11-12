//! vue/valid-v-on

use std::collections::HashSet;

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::template::{KEY_ALIASES, directive_value, is_empty_value_directive};
use crate::utils::tpl_valid::body_directives;

const VALID_MODIFIERS: [&str; 22] = [
    "stop", "prevent", "capture", "self", "ctrl", "shift", "alt", "meta", "native", "once", "left",
    "right", "middle", "passive", "esc", "tab", "enter", "space", "up", "down", "delete", "exact",
];
const VERB_MODIFIERS: [&str; 2] = ["stop", "prevent"];

/// `Number.isSafeInteger(Number.parseInt(s, 10))`.
fn parses_to_safe_int(s: &str) -> bool {
    let t = s.trim_start();
    let t = t.strip_prefix(['+', '-']).unwrap_or(t);
    let digits = t.bytes().take_while(u8::is_ascii_digit).count();
    // Leading zeros do not change the value; more than 16 significant digits is beyond 2^53.
    let sig = t[..digits].trim_start_matches('0');
    digits > 0
        && (sig.len() < 16
            || (sig.len() == 16 && sig.parse::<u64>().is_ok_and(|v| v < (1u64 << 53))))
}

struct ValidVOn {
    custom: HashSet<String>,
}

impl Rule for ValidVOn {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (_, _, a, k) in body_directives(doc, "on") {
            for m in &k.modifiers {
                let n = m.name.as_str();
                let ok = VALID_MODIFIERS.contains(&n)
                    || parses_to_safe_int(n)
                    || n.chars().count() == 1
                    || KEY_ALIASES.contains(n)
                    || self.custom.contains(n);
                if !ok {
                    ctx.report(
                        m.span,
                        format!("'v-on' directives don't support the modifier '{n}'."),
                    );
                }
            }
            let has_expr = directive_value(doc, a).is_some_and(|c| c.expression.is_some());
            if has_expr
                || k.modifiers
                    .iter()
                    .any(|m| VERB_MODIFIERS.contains(&m.name.as_str()))
            {
                continue;
            }
            match crate::utils::template::value_span(doc, a) {
                Some(vs) if !is_empty_value_directive(doc, a) => {
                    let text = doc.slice(vs);
                    let b = text.as_bytes();
                    let inner = if !b.is_empty() && (b[0] == b'"' || b[0] == b'\'') && b[0] == b[b.len() - 1] {
                        text.get(1..text.len() - 1).unwrap_or("")
                    } else {
                        text
                    };
                    if !inner.is_empty() && inner.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_') {
                        ctx.report(vs, format!("Avoid using JavaScript keyword as \"v-on\" value: {text}."));
                    }
                }
                _ => ctx.report(a.span, "'v-on' directives require a value or verb modifier (like 'stop' or 'prevent')."),
            }
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let custom = opts.get(0)["modifiers"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    Some(Box::new(ValidVOn { custom }))
}
