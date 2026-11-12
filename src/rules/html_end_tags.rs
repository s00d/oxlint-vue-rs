//! vue/html-end-tags

use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::template::is_html_void_element_name;
use crate::utils::tpl_style::has_invalid_eof;
use crate::utils::tpl_valid::body_elements;

struct R;

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        if has_invalid_eof(doc) {
            return;
        }
        for e in body_elements(doc) {
            if !is_html_void_element_name(&e.name)
                && e.end_tag.is_none()
                && !e.start_tag.self_closing
            {
                ctx.report_fix(
                    e.start_tag.span,
                    Some("missingEndTag"),
                    format!("'<{}>' should have end tag.", e.name),
                    Some(Fix::insert(e.span.end, format!("</{}>", e.name))),
                );
            }
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(R))
}
