//! vue/attributes-order

use std::collections::HashMap;

use crate::ast::*;
use crate::rule::{Ctx, Fix, Rule, RuleOptions, Suggestion};
use crate::utils::tpl_style::key_span;
use crate::utils::tpl_valid::{arg_text, body_elements};

const OTHER_ATTRS: [&str; 3] = ["ATTR_DYNAMIC", "ATTR_STATIC", "ATTR_SHORTHAND_BOOL"];

struct R {
    position: HashMap<String, usize>,
    alphabetical: bool,
    sort_line_length: bool,
    ignore_vbind_object: bool,
}

fn dir_name(a: &Attribute) -> Option<&str> {
    match &a.key {
        AttributeKey::Directive(k) => Some(&k.name.name),
        AttributeKey::Plain(_) => None,
    }
}
fn is_vbind(a: &Attribute) -> bool {
    dir_name(a) == Some("bind")
}
fn is_attr_or_bind_or_model(a: Option<&Attribute>) -> bool {
    a.is_some_and(|a| !a.is_directive() || matches!(dir_name(a), Some("bind" | "model")))
}
fn is_vbind_object(a: Option<&Attribute>) -> bool {
    a.is_some_and(|a| matches!(&a.key, AttributeKey::Directive(k) if k.name.name == "bind" && k.argument.is_none()))
}

fn attribute_name(doc: &Document, a: &Attribute) -> String {
    match &a.key {
        AttributeKey::Plain(k) => k.name.clone(),
        AttributeKey::Directive(k) if k.name.name == "bind" => {
            arg_text(doc, k).unwrap_or("").to_string()
        }
        AttributeKey::Directive(k) => {
            let mut t = format!("v-{}", k.name.name);
            if let Some(arg) = arg_text(doc, k) {
                t.push(':');
                t.push_str(arg);
            }
            for m in &k.modifiers {
                t.push('.');
                t.push_str(&m.name);
            }
            t
        }
    }
}

fn attribute_type(a: &Attribute) -> &'static str {
    let prop = match &a.key {
        AttributeKey::Directive(k) if k.name.name != "bind" => {
            return match k.name.name.as_str() {
                "for" => "LIST_RENDERING",
                "if" | "else-if" | "else" | "show" | "cloak" => "CONDITIONALS",
                "pre" | "once" => "RENDER_MODIFIERS",
                "model" => "TWO_WAY_BINDING",
                "on" => "EVENTS",
                "html" | "text" => "CONTENT",
                "slot" => "SLOT",
                "is" => "DEFINITION",
                _ => "OTHER_DIRECTIVES",
            };
        }
        AttributeKey::Directive(k) => match &k.argument {
            Some(DirectiveArgument::Static(id)) => id.raw_name.as_str(),
            _ => "",
        },
        AttributeKey::Plain(k) => k.name.as_str(),
    };
    match prop {
        "is" => "DEFINITION",
        "id" => "GLOBAL",
        "ref" | "key" => "UNIQUE",
        "slot" | "slot-scope" => "SLOT",
        _ if is_vbind(a) => "ATTR_DYNAMIC",
        _ if a.value.is_none() => "ATTR_SHORTHAND_BOOL",
        _ => "ATTR_STATIC",
    }
}

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

impl R {
    fn position(&self, a: &Attribute) -> Option<usize> {
        self.position.get(attribute_type(a)).copied()
    }

    fn is_alphabetical(doc: &Document, prev: &Attribute, cur: &Attribute) -> bool {
        let (p, c) = (attribute_name(doc, prev), attribute_name(doc, cur));
        if p == c {
            return is_vbind(prev) <= is_vbind(cur);
        }
        p.encode_utf16().lt(c.encode_utf16())
    }

    /// `node` / `prev` are indices into the start tag's attributes.
    fn report(ctx: &mut Ctx, doc: &Document, attrs: &[Attribute], node: usize, prev: usize) {
        let cur_text = doc.slice(key_span(&attrs[node]));
        let prev_text = doc.slice(key_span(&attrs[prev]));
        let n = &attrs[node];
        let is_move_up = |i: usize| -> bool {
            let a = Some(&attrs[i]);
            if is_vbind_object(Some(n)) {
                is_attr_or_bind_or_model(a)
            } else if is_attr_or_bind_or_model(Some(n)) {
                is_vbind_object(a)
            } else {
                false
            }
        };
        let previous: Vec<usize> = (prev..node).collect();
        let mut moves: Vec<usize> = vec![node];
        for &i in &previous {
            if is_move_up(i) {
                moves.insert(0, i);
            } else {
                moves.push(i);
            }
        }
        let edits: Vec<Fix> = moves
            .iter()
            .enumerate()
            .map(|(k, &m)| {
                let target = previous.get(k).copied().unwrap_or(node);
                Fix::replace(attrs[target].span, doc.slice(attrs[m].span))
            })
            .collect();
        let fix = Fix::merge(doc.source, edits);
        // isVBindObjectOverrideChanged(before, after)
        let mut before = previous.clone();
        before.push(node);
        let pos_after = |x: usize| moves.iter().position(|&m| m == x);
        let suggest = before.iter().enumerate().any(|(oi, &o)| {
            is_vbind_object(Some(&attrs[o]))
                && before.iter().enumerate().any(|(ai, &a)| {
                    is_attr_or_bind_or_model(Some(&attrs[a]))
                        && !is_vbind_object(Some(&attrs[a]))
                        && (ai < oi) != (pos_after(a) < pos_after(o))
                })
        });
        let msg = format!("Attribute \"{cur_text}\" should go before \"{prev_text}\".");
        if suggest {
            let sugg = fix
                .map(|fix| Suggestion {
                    message_id: Some("moveAttribute"),
                    desc: format!(
                        "Move \"{cur_text}\" before \"{prev_text}\", which changes what `v-bind=\"...\"` overrides."
                    ),
                    fix,
                })
                .into_iter()
                .collect();
            ctx.push(n.span, Some("expectedOrder"), msg, None, sugg);
        } else {
            ctx.report_fix(n.span, Some("expectedOrder"), msg, fix);
        }
    }
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for e in body_elements(doc) {
            let all = e.attributes();
            // indices into `all` after the v-bind-object filter
            let filtered: Vec<usize> = (0..all.len())
                .filter(|&i| {
                    let a = Some(&all[i]);
                    !(self.ignore_vbind_object && is_vbind_object(a))
                        && (!is_vbind_object(a)
                            || (!is_attr_or_bind_or_model(i.checked_sub(1).map(|j| &all[j]))
                                && !is_attr_or_bind_or_model(all.get(i + 1))))
                })
                .collect();
            let position_at = |fi: usize| -> Option<usize> {
                let a = &all[filtered[fi]];
                if is_vbind_object(Some(a)) {
                    for &ni in &filtered[fi + 1..] {
                        let next = &all[ni];
                        if is_attr_or_bind_or_model(Some(next)) && !is_vbind_object(Some(next)) {
                            return self.position(next);
                        }
                    }
                }
                self.position(a)
            };
            let list: Vec<(usize, usize)> = (0..filtered.len())
                .filter_map(|fi| position_at(fi).map(|p| (filtered[fi], p)))
                .collect();
            if list.len() <= 1 {
                continue;
            }
            let (mut prev, mut prev_pos) = list[0];
            for &(cur, pos) in &list[1..] {
                let mut valid = prev_pos <= pos;
                if valid && prev_pos == pos {
                    let mut by_len = false;
                    if self.sort_line_length {
                        let (pl, cl) = (
                            utf16_len(doc.slice(all[prev].span)),
                            utf16_len(doc.slice(all[cur].span)),
                        );
                        if pl != cl {
                            valid = pl < cl;
                            by_len = true;
                        }
                    }
                    if self.alphabetical && !by_len {
                        valid = R::is_alphabetical(doc, &all[prev], &all[cur]);
                    }
                }
                if valid {
                    prev = cur;
                    prev_pos = pos;
                } else {
                    R::report(ctx, doc, all, cur, prev);
                }
            }
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let o = opts.get(0);
    let default = serde_json::json!([
        "DEFINITION",
        "LIST_RENDERING",
        "CONDITIONALS",
        "RENDER_MODIFIERS",
        "GLOBAL",
        ["UNIQUE", "SLOT"],
        "TWO_WAY_BINDING",
        "OTHER_DIRECTIVES",
        OTHER_ATTRS,
        "EVENTS",
        "CONTENT"
    ]);
    let order = o["order"]
        .as_array()
        .unwrap_or_else(|| default.as_array().unwrap());
    let mut position = HashMap::new();
    for (i, item) in order.iter().enumerate() {
        let items: Vec<&str> = match item {
            serde_json::Value::Array(a) => a.iter().filter_map(|v| v.as_str()).collect(),
            serde_json::Value::String(s) => vec![s.as_str()],
            _ => Vec::new(),
        };
        for it in items {
            if it == "OTHER_ATTR" {
                for oa in OTHER_ATTRS {
                    position.insert(oa.to_string(), i);
                }
            } else {
                position.insert(it.to_string(), i);
            }
        }
    }
    Some(Box::new(R {
        position,
        alphabetical: o["alphabetical"].as_bool() == Some(true),
        sort_line_length: o["sortLineLength"].as_bool() == Some(true),
        ignore_vbind_object: o["ignoreVBindObject"].as_bool() == Some(true),
    }))
}
