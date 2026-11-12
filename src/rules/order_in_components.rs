//! vue/order-in-components

use std::collections::HashMap;

use oxc::ast::AstKind;
use oxc::ast::ast::*;
use oxc::ast_visit::Visit;
use oxc::span::{GetSpan, Span};

use crate::rule::{Ctx, Fix, LintFile, Rule, RuleOptions, Suggestion, fmt_msg};
use crate::utils::vue_component::{self as vc, MacroKind};
use crate::utils::{js, tokens};

const DEFAULT_ORDER: &[&[&str]] = &[
    &["el"],
    &["name"],
    &["key"],
    &["parent"],
    &["functional"],
    &["delimiters", "comments"],
    &["components", "directives", "filters"],
    &["extends"],
    &["mixins"],
    &["provide", "inject"],
    &["ROUTER_GUARDS"],
    &["layout"],
    &["middleware"],
    &["validate"],
    &["scrollToTop"],
    &["transition"],
    &["loading"],
    &["inheritAttrs"],
    &["model"],
    &["props", "propsData"],
    &["emits"],
    &["slots"],
    &["expose"],
    &["setup"],
    &["asyncData"],
    &["data"],
    &["fetch"],
    &["head"],
    &["computed"],
    &["watch"],
    &["watchQuery"],
    &["LIFECYCLE_HOOKS"],
    &["methods"],
    &["template", "render"],
    &["renderError"],
];

fn group(name: &str) -> Option<&'static [&'static str]> {
    match name {
        "LIFECYCLE_HOOKS" => Some(&[
            "beforeCreate",
            "created",
            "beforeMount",
            "mounted",
            "beforeUpdate",
            "updated",
            "activated",
            "deactivated",
            "beforeUnmount",
            "unmounted",
            "beforeDestroy",
            "destroyed",
            "renderTracked",
            "renderTriggered",
            "errorCaptured",
        ]),
        "ROUTER_GUARDS" => Some(&["beforeRouteEnter", "beforeRouteUpdate", "beforeRouteLeave"]),
        _ => None,
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let mut order: HashMap<String, usize> = HashMap::new();
    // A string entry may name a group; names inside an array entry are taken literally.
    let mut set = |i: usize, name: &str, entry_is_string: bool| match group(name)
        .filter(|_| entry_is_string)
    {
        Some(g) => g.iter().for_each(|p| {
            order.insert(p.to_string(), i);
        }),
        None => {
            order.insert(name.to_string(), i);
        }
    };
    match opts.get(0).get("order").and_then(|v| v.as_array()) {
        Some(list) => {
            for (i, entry) in list.iter().enumerate() {
                match entry {
                    serde_json::Value::String(s) => set(i, s, true),
                    serde_json::Value::Array(a) => a
                        .iter()
                        .filter_map(|v| v.as_str())
                        .for_each(|s| set(i, s, false)),
                    _ => {}
                }
            }
        }
        None => {
            for (i, entry) in DEFAULT_ORDER.iter().enumerate() {
                if entry.len() == 1 {
                    set(i, entry[0], true);
                } else {
                    entry.iter().for_each(|s| set(i, s, false));
                }
            }
        }
    }
    Some(Box::new(OrderInComponents { order }))
}

struct OrderInComponents {
    order: HashMap<String, usize>,
}

const ORDER_MSG: &str = "The \"{{name}}\" property should be above the \"{{firstUnorderedPropertyName}}\" property on line {{line}}.";
const SIDE_EFFECTS_MSG: &str = "Manually move \"{{name}}\" property above \"{{firstUnorderedPropertyName}}\" property on line {{line}} (might break side effects).";

impl Rule for OrderInComponents {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let file = ctx.file;
        for o in vc::vue_objects(file) {
            self.check(ctx, &o.obj.properties);
        }
        for m in vc::macros_of(file, MacroKind::DefineOptions) {
            if let Some(Expression::ObjectExpression(o)) = m.arg(0).map(js::skip_parens) {
                self.check(ctx, &o.properties);
            }
        }
    }
}

impl OrderInComponents {
    fn pos(&self, name: &str) -> Option<usize> {
        self.order.get(name).copied()
    }

    fn check(&self, ctx: &mut Ctx<'_, '_>, nodes: &[ObjectPropertyKind]) {
        let file = ctx.file;
        let props: Vec<(usize, &ObjectProperty, String)> = nodes
            .iter()
            .enumerate()
            .filter_map(|(i, p)| match p {
                ObjectPropertyKind::ObjectProperty(p) => {
                    Some((i, &**p, js::prop_name_or_ident(p).unwrap_or_default()))
                }
                ObjectPropertyKind::SpreadProperty(_) => None,
            })
            .collect();
        for (k, (node_idx, prop, name)) in props.iter().enumerate() {
            let Some(pos) = self.pos(name) else { continue };
            // JS `sort((a, b) => pa > pb ? 1 : -1)` on V8 puts later equal items first.
            let first = props[..k]
                .iter()
                .enumerate()
                .filter_map(|(j, p)| {
                    self.pos(&p.2)
                        .filter(|pp| *pp > pos)
                        .map(|pp| (pp, std::cmp::Reverse(j), p))
                })
                .min_by_key(|(pp, j, _)| (*pp, *j))
                .map(|(_, _, p)| p);
            let Some((first_idx, first_prop, first_name)) = first else {
                continue;
            };
            let line = (file.lines.line_of(first_prop.span.start) + 1).to_string();
            let data = [
                ("name", name.as_str()),
                ("firstUnorderedPropertyName", first_name.as_str()),
                ("line", line.as_str()),
            ];
            let side_effects = nodes[*first_idx..=*node_idx]
                .iter()
                .any(|p| !no_side_effects(p));
            let fix = handle_fix(file, prop.span, first_prop.span);
            if side_effects {
                let sugg = fix.map(|fix| Suggestion {
                    message_id: Some("reorderWithSideEffects"),
                    desc: fmt_msg(SIDE_EFFECTS_MSG, &data),
                    fix,
                });
                ctx.push(
                    prop.span,
                    Some("order"),
                    fmt_msg(ORDER_MSG, &data),
                    None,
                    sugg.into_iter().collect(),
                );
            } else {
                ctx.push(
                    prop.span,
                    Some("order"),
                    fmt_msg(ORDER_MSG, &data),
                    fix,
                    Vec::new(),
                );
            }
        }
    }
}

fn handle_fix(file: &LintFile, prop: Span, unordered: Span) -> Option<Fix> {
    let after = tokens::token_after(file, prop.end);
    let has_after_comma = after.is_some_and(|t| file.slice(t) == ",");
    let before = tokens::token_before(file, prop.start)?;
    let code_start = before.end;
    let code_end = match after {
        Some(a) if has_after_comma => a.end,
        _ => prop.end,
    };
    let remove_start = if has_after_comma {
        code_start
    } else {
        before.start
    };
    let mut code = file.source[code_start as usize..code_end as usize].to_string();
    if !has_after_comma {
        code.push(',');
    }
    let insert_after = tokens::token_before(file, unordered.start)?;
    Fix::merge(
        file.source,
        vec![
            Fix::remove(Span::new(remove_start, code_end)),
            Fix::insert(insert_after.end, code),
        ],
    )
}

/// `isNotSideEffectsNode`.
fn no_side_effects(p: &ObjectPropertyKind) -> bool {
    let mut v = SideEffects {
        valid: true,
        skip: None,
    };
    v.visit_object_property_kind(p);
    v.valid
}

struct SideEffects {
    valid: bool,
    skip: Option<Span>,
}

impl<'a> Visit<'a> for SideEffects {
    fn enter_node(&mut self, kind: AstKind<'a>) {
        if !self.valid || self.skip.is_some() {
            return;
        }
        // Known side-effect-free leaves: their subtree is not inspected.
        let leaf = matches!(
            kind,
            AstKind::Function(_)
                | AstKind::IdentifierReference(_)
                | AstKind::IdentifierName(_)
                | AstKind::BindingIdentifier(_)
                | AstKind::StringLiteral(_)
                | AstKind::NumericLiteral(_)
                | AstKind::BooleanLiteral(_)
                | AstKind::NullLiteral(_)
                | AstKind::BigIntLiteral(_)
                | AstKind::RegExpLiteral(_)
                | AstKind::ArrowFunctionExpression(_)
                | AstKind::TemplateElement(_)
                | AstKind::TSAsExpression(_)
        );
        if leaf {
            self.skip = Some(kind.span());
            return;
        }
        let ok = match kind {
            AstKind::ObjectProperty(_)
            | AstKind::ObjectExpression(_)
            | AstKind::ArrayExpression(_)
            | AstKind::Elision(_)
            | AstKind::ParenthesizedExpression(_)
            | AstKind::StaticMemberExpression(_)
            | AstKind::ComputedMemberExpression(_)
            | AstKind::PrivateFieldExpression(_)
            | AstKind::ConditionalExpression(_)
            | AstKind::SpreadElement(_)
            | AstKind::TemplateLiteral(_)
            | AstKind::ChainExpression(_)
            | AstKind::LogicalExpression(_)
            | AstKind::BinaryExpression(_) => true,
            AstKind::UnaryExpression(u) => {
                !matches!(u.operator, UnaryOperator::Delete | UnaryOperator::Void)
            }
            _ => false,
        };
        if !ok {
            self.valid = false;
        }
    }
    fn leave_node(&mut self, kind: AstKind<'a>) {
        if self.skip == Some(kind.span()) {
            self.skip = None;
        }
    }
}
