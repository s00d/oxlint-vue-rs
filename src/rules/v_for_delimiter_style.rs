//! vue/v-for-delimiter-style

use oxc::span::GetSpan;

use crate::ast::{TokenKind, VExpression};
use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::tpl_misc::body_exprs;
use crate::utils::tpl_style::first_token;

struct R {
    preferred: String,
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for c in body_exprs(doc) {
            let Some(VExpression::For {
                span, left, rest, ..
            }) = &c.expression
            else {
                continue;
            };
            let from = match rest {
                Some(r) => Some(r.span.end),
                None => left.iter().rev().flatten().next().map(|p| p.span().end),
            }
            .or_else(|| first_token(doc, *span).map(|t| t.span.end));
            let Some(from) = from else { continue };
            let i = doc.tokens.partition_point(|t| t.span.start < from);
            let Some(delim) = doc.tokens[i..]
                .iter()
                .find(|t| t.kind != TokenKind::Punctuator)
            else {
                continue;
            };
            let used = doc.slice(delim.span);
            if used == self.preferred {
                continue;
            }
            let p = &self.preferred;
            ctx.report_fix(
                *span,
                Some("expected"),
                format!("Expected '{p}' instead of '{used}' in 'v-for'."),
                Some(Fix::replace(delim.span, p.clone())),
            );
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(R {
        preferred: opts.get(0).as_str().unwrap_or("in").to_string(),
    }))
}
