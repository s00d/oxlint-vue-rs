//! vue/valid-template-root

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::tpl_valid::child_span;

struct ValidTemplateRoot;

impl Rule for ValidTemplateRoot {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        let Some(root) = doc.template_body.map(|id| doc.element(id)) else {
            return;
        };
        let has_src = root.attr("src").is_some();
        let roots: Vec<_> = root
            .children
            .iter()
            .map(|c| child_span(doc, *c))
            .filter(|s| !doc.slice(*s).trim().is_empty())
            .collect();
        if has_src {
            for s in roots {
                ctx.report(
                    s,
                    "The template root with 'src' attribute is required to be empty.",
                );
            }
        } else if roots.is_empty() {
            ctx.report(root.span, "The template requires child element.");
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(ValidTemplateRoot))
}
