//! vue/use-v-on-exact

use oxc::span::Span;

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::template::{directive_key, is_custom_component};
use crate::utils::tpl_valid::{arg_text, body_elements};

const SYSTEM_MODIFIERS: [&str; 4] = ["ctrl", "shift", "alt", "meta"];
const GLOBAL_MODIFIERS: [&str; 7] = [
    "stop", "prevent", "capture", "self", "once", "passive", "native",
];
const KEYBOARD_EVENTS: [&str; 3] = ["keydown", "keypress", "keyup"];

struct Event<'d> {
    name: &'d str,
    span: Span,
    modifiers: Vec<&'d str>,
    has_handler: bool,
}

fn is_system(m: &str) -> bool {
    SYSTEM_MODIFIERS.contains(&m)
}

fn joined(mods: &[&str], f: fn(&str) -> bool) -> String {
    let mut v: Vec<&str> = mods.iter().copied().filter(|m| f(m)).collect();
    v.sort_unstable();
    v.join(",")
}

fn is_global_only_keyboard_stop(e: &Event) -> bool {
    KEYBOARD_EVENTS.contains(&e.name)
        && e.modifiers.contains(&"stop")
        && !e.has_handler
        && e.modifiers.iter().all(|m| GLOBAL_MODIFIERS.contains(m))
}

fn conflicted(base: &Event, event: &Event) -> bool {
    if event.span == base.span
        || event.modifiers.contains(&"exact")
        || (base.modifiers.contains(&"exact") && is_global_only_keyboard_stop(event))
    {
        return false;
    }
    let key = |m: &str| !GLOBAL_MODIFIERS.contains(&m) && !is_system(m);
    let (ek, bk) = (joined(&event.modifiers, key), joined(&base.modifiers, key));
    if !ek.is_empty() && !bk.is_empty() && ek != bk {
        return false;
    }
    let (es, bs) = (
        joined(&event.modifiers, is_system),
        joined(&base.modifiers, is_system),
    );
    !base.modifiers.is_empty() && bs != es && bs.contains(es.as_str())
}

struct UseVOnExact;

impl Rule for UseVOnExact {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for el in body_elements(doc) {
            let custom = is_custom_component(el, false);
            let mut events: Vec<Event> = el
                .attributes()
                .iter()
                .filter_map(|a| {
                    let k = directive_key(a).filter(|k| k.name.name == "on")?;
                    Some(Event {
                        name: arg_text(doc, k).unwrap_or(""),
                        span: k.span,
                        modifiers: k.modifiers.iter().map(|m| m.name.as_str()).collect(),
                        has_handler: a.value.is_some(),
                    })
                })
                .collect();
            if custom {
                events.retain(|e| e.modifiers.contains(&"native"));
            }
            // Group by event name, in first-seen order.
            let mut names: Vec<&str> = Vec::new();
            for e in &events {
                if !names.contains(&e.name) {
                    names.push(e.name);
                }
            }
            for name in names {
                let group: Vec<&Event> = events.iter().filter(|e| e.name == name).collect();
                if !group
                    .iter()
                    .any(|e| e.modifiers.iter().any(|m| is_system(m)))
                {
                    continue;
                }
                let mut hit: Vec<usize> = Vec::new();
                for base in &group {
                    for (i, e) in group.iter().enumerate() {
                        if !hit.contains(&i) && conflicted(base, e) {
                            hit.push(i);
                        }
                    }
                }
                for i in hit {
                    ctx.report(group[i].span, "Consider to use '.exact' modifier.");
                }
            }
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(UseVOnExact))
}
