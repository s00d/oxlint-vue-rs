//! vue/max-template-depth

use crate::ast::Child;
use crate::rule::{Ctx, Rule, RuleOptions};

struct R {
    max: Option<u64>,
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        let (Some(root), Some(max)) = (doc.template_body, self.max) else {
            return;
        };
        let mut stack = vec![(root, 0u64)];
        let mut found = Vec::new();
        while let Some((id, depth)) = stack.pop() {
            let e = doc.element(id);
            if depth > max {
                found.push((e.span, depth));
            }
            for c in e.children.iter().rev() {
                if let Child::Element(c) = c {
                    stack.push((*c, depth + 1));
                }
            }
        }
        for (span, depth) in found {
            ctx.report_id(
                span,
                "templateTooDeep",
                format!(
                    "Element is nested too deeply (depth of {depth}, maximum allowed is {max})."
                ),
            );
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(R {
        max: opts.get(0)["maxDepth"].as_u64(),
    }))
}
