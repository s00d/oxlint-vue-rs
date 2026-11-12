//! vue/no-deprecated-slot-scope-attribute

use crate::ast::*;
use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::template::value_span;
use crate::utils::tpl_misc::{can_convert_to_v_slot, token_before};
use crate::utils::tpl_valid::body_directives;

struct R;

/// `slot-scope-attribute.ts` fix (`fixToUpgrade: true`).
pub fn fix_slot_scope(doc: &Document, e: &Element, scope: &Attribute) -> Option<Fix> {
    if !can_convert_to_v_slot(doc, e) || e.attr("slot").is_some() || e.directive("bind", Some("slot")).is_some() {
        return None;
    }
    let scope_value = value_span(doc, scope).map(|v| format!("={}", doc.slice(v))).unwrap_or_default();
    let replace = format!("v-slot{scope_value}");
    if e.name == "template" {
        return Some(Fix::replace(scope.span, replace));
    }
    let before = token_before(doc, scope.span.start)?;
    Fix::merge(
        doc.source,
        vec![
            Fix::remove(oxc::span::Span::new(before.span.end, scope.span.end)),
            Fix::insert(e.span.start, format!("<template {replace}>\n")),
            Fix::insert(e.span.end, "\n</template>"),
        ],
    )
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (e, _, a, k) in body_directives(doc, "slot-scope") {
            let fix = fix_slot_scope(doc, e, a);
            ctx.report_fix(k.span, None, "`slot-scope` are deprecated.", fix);
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(R))
}
