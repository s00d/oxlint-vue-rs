//! vue/restricted-component-names

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::regexp::GroupMatcher;
use crate::utils::template::{
    self as tpl, DEPRECATED_HTML_ELEMENT_NAMES, HTML_ELEMENT_NAMES, SVG_ELEMENT_NAMES,
};

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let allow = opts
        .get(0)
        .get("allow")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
        .unwrap_or_default();
    Some(Box::new(RestrictedComponentNames {
        allow: GroupMatcher::new(allow),
    }))
}

struct RestrictedComponentNames {
    allow: GroupMatcher,
}

fn reserved(name: &str) -> bool {
    HTML_ELEMENT_NAMES.contains(name)
        || DEPRECATED_HTML_ELEMENT_NAMES.contains(name)
        || SVG_ELEMENT_NAMES.contains(name)
        || tpl::VUE2_BUILTIN_COMPONENT_NAMES.contains(&name)
        || tpl::VUE3_BUILTIN_COMPONENT_NAMES.contains(&name)
}

impl Rule for RestrictedComponentNames {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let Some(doc) = ctx.doc() else { return };
        let ids = doc.template_body.into_iter().chain(doc.template_elements());
        for id in ids {
            let e = doc.element(id);
            if reserved(&e.raw_name) || self.allow.is_match(&e.raw_name) {
                continue;
            }
            ctx.report_id(
                e.span,
                "invalidName",
                format!("Component name \"{}\" is not allowed.", e.raw_name),
            );
        }
    }
}
