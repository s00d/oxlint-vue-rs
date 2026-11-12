//! vue/sort-keys

use std::cmp::Ordering;

use oxc::ast::AstKind;
use oxc::ast::ast::*;
use oxc::ast_visit::Visit;
use oxc::span::{GetSpan, Span};
use serde_json::Value;

use crate::rule::{Ctx, LintFile, Rule, RuleOptions, fmt_msg};
use crate::utils::{js, tokens, vue_component as vc};

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let o = opts.get(1);
    let strs = |k: &str, d: &[&str]| -> Vec<String> {
        match o.get(k).and_then(Value::as_array) {
            Some(a) => a
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect(),
            None => d.iter().map(|s| s.to_string()).collect(),
        }
    };
    Some(Box::new(SortKeys {
        desc: opts.get(0).as_str() == Some("desc"),
        ignore_grandchildren_of: strs(
            "ignoreGrandchildrenOf",
            &["computed", "directives", "inject", "props", "watch"],
        ),
        ignore_children_of: strs("ignoreChildrenOf", &["model"]),
        insensitive: o.get("caseSensitive").and_then(Value::as_bool) == Some(false),
        min_keys: o.get("minKeys").and_then(Value::as_u64).unwrap_or(2) as usize,
        natural: o.get("natural").and_then(Value::as_bool) == Some(true),
        allow_line_separated_groups: o.get("allowLineSeparatedGroups").and_then(Value::as_bool)
            == Some(true),
    }))
}

struct SortKeys {
    desc: bool,
    ignore_grandchildren_of: Vec<String>,
    ignore_children_of: Vec<String>,
    insensitive: bool,
    min_keys: usize,
    natural: bool,
    allow_line_separated_groups: bool,
}

/// `natural-compare@1.4.0` over UTF-16 code units.
fn natural_compare(a: &str, b: &str) -> Ordering {
    if a == b {
        return Ordering::Equal;
    }
    let a: Vec<u16> = a.encode_utf16().collect();
    let b: Vec<u16> = b.encode_utf16().collect();
    fn code(s: &[u16], pos: usize) -> f64 {
        let c = s.get(pos).copied().unwrap_or(0) as f64;
        if !(45.0..=127.0).contains(&c) {
            c
        } else if c < 46.0 {
            65.0
        } else if c < 48.0 {
            c - 1.0
        } else if c < 58.0 {
            c + 18.0
        } else if c < 65.0 {
            c - 11.0
        } else if c < 91.0 {
            c + 11.0
        } else if c < 97.0 {
            c - 37.0
        } else if c < 123.0 {
            c + 5.0
        } else {
            c - 63.0
        }
    }
    // Numeric run starting at pos-1: (value, end index).
    fn number(s: &[u16], pos: usize) -> (f64, usize) {
        let mut i = pos;
        while {
            let c = code(s, i);
            c < 76.0 && c > 65.0
        } {
            i += 1;
        }
        let txt = String::from_utf16_lossy(&s[(pos - 1).min(s.len())..i.min(s.len())]);
        (txt.parse::<f64>().unwrap_or(f64::NAN), i)
    }
    let (mut pos_a, mut pos_b) = (0usize, 0usize);
    let mut code_b = 1.0;
    while code_b != 0.0 {
        let mut code_a = code(&a, pos_a);
        pos_a += 1;
        code_b = code(&b, pos_b);
        pos_b += 1;
        if code_a < 76.0 && code_b < 76.0 && code_a > 66.0 && code_b > 66.0 {
            let (va, ia) = number(&a, pos_a);
            let (vb, ib) = number(&b, pos_b);
            code_a = va;
            code_b = vb;
            pos_a = ia;
            pos_b = ib;
        }
        if code_a != code_b {
            return if code_a < code_b {
                Ordering::Less
            } else {
                Ordering::Greater
            };
        }
    }
    Ordering::Equal
}

impl SortKeys {
    fn valid_order(&self, prev: &str, this: &str) -> bool {
        let (a, b) = if self.desc {
            (this, prev)
        } else {
            (prev, this)
        };
        let (a, b) = if self.insensitive {
            (a.to_lowercase(), b.to_lowercase())
        } else {
            (a.to_string(), b.to_string())
        };
        if self.natural {
            natural_compare(&a, &b) != Ordering::Greater
        } else {
            a.encode_utf16().le(b.encode_utf16())
        }
    }
}

#[derive(Default, Clone)]
struct VueState {
    current_property: Option<Span>,
    current_property_name: Option<String>,
    is_vue_object: bool,
    within: bool,
    prop_name: Option<String>,
    chain_level: u32,
    ignore: bool,
}

struct Frame {
    span: Span,
    prev_name: Option<String>,
    prev_node: Option<Span>,
    prev_blank_line: bool,
    num_keys: usize,
    vue: VueState,
}

struct V<'r, 'c, 'f, 'a> {
    rule: &'r SortKeys,
    ctx: &'c mut Ctx<'f, 'a>,
    file: &'f LintFile<'a>,
    stack: Vec<Frame>,
}

impl<'a> Visit<'a> for V<'_, '_, '_, 'a> {
    fn enter_node(&mut self, kind: AstKind<'a>) {
        match kind {
            AstKind::ObjectExpression(obj) => self.enter_object(obj),
            AstKind::SpreadElement(s) => {
                let Some(top) = self.stack.last_mut() else {
                    return;
                };
                if let Some(sem) = self.file.semantic
                    && matches!(sem.nodes().parent_kind(s.node_id()), AstKind::ObjectExpression(o) if o.span == top.span)
                {
                    top.prev_name = None;
                }
            }
            AstKind::ObjectProperty(p) => self.property(p),
            _ => {}
        }
    }
    fn leave_node(&mut self, kind: AstKind<'a>) {
        if let AstKind::ObjectExpression(_) = kind {
            self.stack.pop();
        }
    }
}

impl<'a> V<'_, '_, '_, 'a> {
    fn enter_object(&mut self, obj: &ObjectExpression<'a>) {
        let upper = self.stack.last().map(|f| f.vue.clone()).unwrap_or_default();
        let mut vue = VueState {
            is_vue_object: vc::vue_object_type(self.file, obj).is_some(),
            ..Default::default()
        };
        if vue.is_vue_object {
            vue.within = true;
            vue.ignore = true;
        } else if upper.within
            && let Some(cur) = upper.current_property
        {
            let is_chain = self.file.semantic.is_some_and(|sem| {
                matches!(js::parent_skip_ts_as(sem, obj.node_id()).1, AstKind::ObjectProperty(p) if p.span == cur && js::skip_ts_as(&p.value).span() == obj.span)
            });
            if is_chain {
                let (prop_name, level) = if upper.is_vue_object {
                    (upper.current_property_name.clone().unwrap_or_default(), 1)
                } else {
                    (
                        upper.prop_name.clone().unwrap_or_default(),
                        upper.chain_level + 1,
                    )
                };
                vue.within = true;
                vue.ignore = (level == 1 && self.rule.ignore_children_of.contains(&prop_name))
                    || (level == 2 && self.rule.ignore_grandchildren_of.contains(&prop_name));
                vue.prop_name = Some(prop_name);
                vue.chain_level = level;
            }
        }
        self.stack.push(Frame {
            span: obj.span,
            prev_name: None,
            prev_node: None,
            prev_blank_line: false,
            num_keys: obj.properties.len(),
            vue,
        });
    }

    fn property(&mut self, p: &ObjectProperty<'a>) {
        let file = self.file;
        let rule = self.rule;
        let Some(top) = self.stack.last_mut() else {
            return;
        };
        top.vue.current_property = Some(p.span);
        top.vue.current_property_name = js::prop_name(p);
        if top.vue.ignore {
            return;
        }
        let prev_name = top.prev_name.clone();
        let this_name = js::prop_name_or_ident(p);
        let mut blank = top.prev_blank_line;
        if let Some(prev) = top.prev_node {
            let mut last_end = prev.end;
            for t in tokens::tokens_between(file, prev.end, p.span.start, true)
                .into_iter()
                .chain(std::iter::once(p.span))
            {
                if tokens::line(file, t.start) > tokens::line(file, last_end) + 1 {
                    blank = true;
                }
                last_end = t.end;
            }
        }
        top.prev_node = Some(p.span);
        if this_name.is_some() {
            top.prev_name = this_name.clone();
        }
        if rule.allow_line_separated_groups && blank {
            top.prev_blank_line = this_name.is_none();
            return;
        }
        let (Some(prev), Some(this)) = (prev_name, this_name) else {
            return;
        };
        if top.num_keys < rule.min_keys || rule.valid_order(&prev, &this) {
            return;
        }
        let msg = fmt_msg(
            "Expected object keys to be in {{natural}}{{insensitive}}{{order}}ending order. '{{thisName}}' should be before '{{prevName}}'.",
            &[
                ("thisName", &this),
                ("prevName", &prev),
                ("order", if rule.desc { "desc" } else { "asc" }),
                (
                    "insensitive",
                    if rule.insensitive { "insensitive " } else { "" },
                ),
                ("natural", if rule.natural { "natural " } else { "" }),
            ],
        );
        self.ctx.report_id(p.key.span(), "sortKeys", msg);
    }
}

impl Rule for SortKeys {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let file = ctx.file;
        let Some(program) = file.program else { return };
        let mut v = V {
            rule: self,
            ctx,
            file,
            stack: Vec::new(),
        };
        v.visit_program(program);
    }
}

#[cfg(test)]
mod tests {
    use super::natural_compare;
    use std::cmp::Ordering::*;
    #[test]
    fn natural() {
        assert_eq!(natural_compare("a2", "a10"), Less);
        assert_eq!(natural_compare("a10", "a2"), Greater);
        assert_eq!(natural_compare("B", "a"), Less);
    }
}
