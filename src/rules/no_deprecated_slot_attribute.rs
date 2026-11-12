//! vue/no-deprecated-slot-attribute

use crate::ast::*;
use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::casing::{kebab_case, pascal_case};
use crate::utils::regexp::GroupMatcher;
use crate::utils::tpl_misc::{body_attributes, can_convert_to_v_slot, static_arg};
use crate::utils::tpl_valid::expr_span;

struct R {
    ignore: GroupMatcher,
    ignore_parents: GroupMatcher,
}

/// `fixSlotToVSlot` (`syntaxes/slot-attribute.ts`).
pub fn fix_slot_to_v_slot(
    doc: &Document,
    e: &Element,
    slot: &Attribute,
    slot_name: Option<&str>,
    is_vbind: bool,
) -> Option<Fix> {
    let scope = e.attributes().iter().find(|a| {
        matches!(&a.key, AttributeKey::Directive(k) if k.name.name == "slot-scope" || k.name.name == "scope")
    });
    let name_arg = match slot_name {
        Some(n) if !n.is_empty() => {
            if is_vbind {
                format!(":[{n}]")
            } else {
                format!(":{n}")
            }
        }
        _ => String::new(),
    };
    let scope_value = scope
        .and_then(|s| crate::utils::template::value_span(doc, s))
        .map(|v| format!("={}", doc.slice(v)))
        .unwrap_or_default();
    let replace = format!("v-slot{name_arg}{scope_value}");
    let mut edits = Vec::new();
    if e.name == "template" {
        edits.push(Fix::replace(slot.span, replace));
        if let Some(s) = scope {
            edits.push(Fix::remove(s.span));
        }
    } else {
        edits.push(Fix::remove(slot.span));
        if let Some(s) = scope {
            edits.push(Fix::remove(s.span));
        }
        let vfor = e.directive("for", None);
        let vfor_text = vfor.map(|v| format!("{} ", doc.slice(v.span))).unwrap_or_default();
        if let Some(v) = vfor {
            edits.push(Fix::remove(v.span));
        }
        edits.push(Fix::insert(e.span.start, format!("<template {vfor_text}{replace}>\n")));
        edits.push(Fix::insert(e.span.end, "\n</template>"));
    }
    Fix::merge(doc.source, edits)
}

impl R {
    fn is_ignored(&self, doc: &Document, e: &Element) -> bool {
        let n = &e.raw_name;
        if self.ignore.is_match(n) || self.ignore.is_match(&pascal_case(n)) || self.ignore.is_match(&kebab_case(n)) {
            return true;
        }
        e.parent.is_some_and(|p| self.ignore_parents.is_match(&doc.element(p).raw_name))
    }
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (e, a) in body_attributes(doc) {
            let (key_span, fix) = match &a.key {
                AttributeKey::Plain(k) if k.name == "slot" => {
                    if self.is_ignored(doc, e) {
                        continue;
                    }
                    let value = crate::utils::template::literal_value(a);
                    let ok = can_convert_to_v_slot(doc, e)
                        && value.is_none_or(|v| v.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'));
                    (k.span, if ok { fix_slot_to_v_slot(doc, e, a, value, false) } else { None })
                }
                AttributeKey::Directive(k)
                    if k.name.name == "bind" && static_arg(k).is_some_and(|i| i.name == "slot") =>
                {
                    if self.is_ignored(doc, e) {
                        continue;
                    }
                    let expr = crate::utils::template::directive_expression(doc, a);
                    let ok = can_convert_to_v_slot(doc, e)
                        && (a.value.is_none()
                            || expr.is_some_and(|x| {
                                matches!(crate::utils::js::skip_parens(x), oxc::ast::ast::Expression::Identifier(_))
                            }));
                    let name = expr.map(|x| doc.slice(expr_span(x)).trim());
                    (k.span, if ok { fix_slot_to_v_slot(doc, e, a, name, true) } else { None })
                }
                _ => continue,
            };
            ctx.report_fix(key_span, None, "`slot` attributes are deprecated.", fix);
        }
    }
}

fn strings(v: &serde_json::Value) -> Vec<&str> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_default()
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let o = opts.get(0);
    Some(Box::new(R {
        ignore: GroupMatcher::new(strings(&o["ignore"])),
        ignore_parents: GroupMatcher::new(strings(&o["ignoreParents"])),
    }))
}
