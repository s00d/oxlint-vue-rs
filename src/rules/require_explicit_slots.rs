//! vue/require-explicit-slots

use std::collections::HashSet;

use oxc::ast::ast::{Expression, PropertyKey, TSSignature, TSType, TSTypeName};
use oxc::span::Span;

use crate::ast::{AttributeKey, AttributeValue, VExpression};
use crate::rule::{Ctx, Rule, RuleOptions, fmt_msg};
use crate::utils::js;
use crate::utils::template::{directive_value, literal_value_of};
use crate::utils::tpl_valid::body_elements;
use crate::utils::vue_component::{self as vc, MacroKind};

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(RequireExplicitSlots))
}

struct RequireExplicitSlots;

/// `(slotName, node span)` list.
type Slots = Vec<(Option<String>, Span)>;

/// `slots: Object as SlotsType<{ … }>` members: `(name, member span)`.
fn options_slots<'a>(obj: &'a oxc::ast::ast::ObjectExpression<'a>) -> Vec<(Option<String>, Span)> {
    let Some(p) = js::find_property(obj, "slots") else { return Vec::new() };
    let Expression::TSAsExpression(a) = js::skip_parens(&p.value) else { return Vec::new() };
    let TSType::TSTypeReference(r) = &a.type_annotation else { return Vec::new() };
    if !matches!(&r.type_name, TSTypeName::IdentifierReference(id) if id.name == "SlotsType") {
        return Vec::new();
    }
    let Some(TSType::TSTypeLiteral(l)) = r.type_arguments.as_ref().and_then(|t| t.params.first())
    else {
        return Vec::new();
    };
    l.members
        .iter()
        .filter_map(|m| {
            let (key, span) = match m {
                TSSignature::TSPropertySignature(p) => (&p.key, p.span),
                TSSignature::TSMethodSignature(p) => (&p.key, p.span),
                _ => return None,
            };
            let name = match key {
                PropertyKey::StaticIdentifier(i) => Some(i.name.to_string()),
                PropertyKey::Identifier(i) => Some(i.name.to_string()),
                PropertyKey::StringLiteral(s) => Some(s.value.display().to_string()),
                _ => None,
            };
            Some((name, span))
        })
        .collect()
}

impl Rule for RequireExplicitSlots {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let file = ctx.file;
        let Some(doc) = ctx.doc() else { return };
        if !doc
            .top_level()
            .any(|e| e.name == "script" && literal_value_of(e, "lang") == Some("ts"))
        {
            return;
        }
        let mut groups: Vec<(u32, Slots)> = Vec::new();
        for m in vc::macros_of(file, MacroKind::DefineSlots) {
            groups.push((m.call.span.start, vc::slots_from_define_slots(file, m.call)));
        }
        for o in vc::vue_objects(file) {
            groups.push((o.obj.span.start, options_slots(o.obj)));
        }
        groups.sort_by_key(|g| g.0);
        let mut defined: HashSet<String> = HashSet::new();
        for (_, slots) in groups {
            for (name, span) in slots {
                let Some(name) = name.filter(|n| !n.is_empty()) else { continue };
                if defined.contains(&name) {
                    ctx.report_id(
                        span,
                        "alreadyDefinedSlot",
                        fmt_msg("Slot {{slotName}} is already defined.", &[("slotName", &name)]),
                    );
                } else {
                    defined.insert(name);
                }
            }
        }
        for e in body_elements(doc) {
            if e.name != "slot" {
                continue;
            }
            let name_attr = e.attributes().iter().find(|a| match &a.key {
                AttributeKey::Plain(k) => k.name == "name",
                AttributeKey::Directive(k) => k.name.name == "bind" && k.static_argument() == Some("name"),
            });
            let name: Option<String> = match name_attr {
                None => Some("default".into()),
                Some(a) if a.is_directive() => {
                    let Some(VExpression::Expression(x)) =
                        directive_value(doc, a).and_then(|c| c.expression.as_ref())
                    else {
                        continue;
                    };
                    let x = js::skip_parens(x);
                    if !js::is_string_literal(x) {
                        continue;
                    }
                    js::string_literal_value(x).filter(|n| !n.is_empty())
                }
                Some(a) => match &a.value {
                    Some(AttributeValue::Literal(l)) => Some(l.value.clone()),
                    _ => None,
                },
            };
            if !name.is_some_and(|n| defined.contains(&n)) {
                ctx.report_id(e.span, "requireExplicitSlots", "Slots must be explicitly defined.");
            }
        }
    }
}
