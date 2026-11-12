//! vue/no-multiple-template-root

use crate::ast::Child;
use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::js::in_range;
use crate::utils::tpl_valid::child_span;

struct NoMultipleTemplateRoot {
    disallow_comments: bool,
}

impl Rule for NoMultipleTemplateRoot {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        let Some(root) = doc.template_body.map(|id| doc.element(id)) else {
            return;
        };
        if self.disallow_comments {
            for c in &doc.comments {
                if in_range(root.span, c.span)
                    && root
                        .children
                        .iter()
                        .all(|ch| !in_range(child_span(doc, *ch), c.span))
                {
                    ctx.report(c.span, "The template root disallows comments.");
                }
            }
        }
        let mut roots = Vec::new();
        let mut extra_text = None;
        let mut extra_element = None;
        let mut is_vif = false;
        for c in &root.children {
            match c {
                Child::Element(id) => {
                    let e = doc.element(*id);
                    if roots.is_empty() {
                        roots.push(e);
                        is_vif = e.has_directive("if");
                    } else if is_vif && e.has_directive("else-if") {
                        roots.push(e);
                    } else if is_vif && e.has_directive("else") {
                        roots.push(e);
                        is_vif = false;
                    } else {
                        extra_element = Some(e.span);
                    }
                }
                _ => {
                    let s = child_span(doc, *c);
                    if !doc.slice(s).trim().is_empty() {
                        extra_text = Some(s);
                    }
                }
            }
        }
        if let Some(s) = extra_text {
            ctx.report(
                s,
                "The template root requires an element rather than texts.",
            );
        } else if let Some(s) = extra_element {
            ctx.report(s, "The template root requires exactly one element.");
        } else {
            for e in roots {
                if e.name == "template" || e.name == "slot" {
                    ctx.report(
                        e.start_tag.span,
                        format!("The template root disallows '<{}>' elements.", e.name),
                    );
                }
                if e.has_directive("for") {
                    ctx.report(
                        e.start_tag.span,
                        "The template root disallows 'v-for' directives.",
                    );
                }
            }
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(NoMultipleTemplateRoot {
        disallow_comments: opts.get(0)["disallowComments"].as_bool() == Some(true),
    }))
}
