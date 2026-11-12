//! vue/mustache-interpolation-spacing

use oxc::span::Span;

use crate::ast::{ExprParent, TokenKind};
use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::tpl_misc::body_exprs;
use crate::utils::tpl_style::{first_token, last_token, token_after, token_before};

struct R {
    always: bool,
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for c in body_exprs(doc) {
            if c.expression.is_none() || !matches!(c.parent, ExprParent::Element(_)) {
                continue;
            }
            let (Some(open), Some(close)) = (first_token(doc, c.span), last_token(doc, c.span, 0))
            else {
                continue;
            };
            if open.kind != TokenKind::VExpressionStart || close.kind != TokenKind::VExpressionEnd {
                continue;
            }
            let (Some(first), Some(last)) = (
                token_after(doc, open.span.end, true),
                token_before(doc, close.span.start, true),
            ) else {
                continue;
            };
            if self.always {
                if open.span.end == first.span.start {
                    ctx.report_fix(
                        open.span,
                        Some("expectedSpaceAfter"),
                        "Expected 1 space after '{{', but not found.",
                        Some(Fix::insert(open.span.end, " ")),
                    );
                }
                if close.span.start == last.span.end {
                    ctx.report_fix(
                        close.span,
                        Some("expectedSpaceBefore"),
                        "Expected 1 space before '}}', but not found.",
                        Some(Fix::insert(close.span.start, " ")),
                    );
                }
            } else {
                if open.span.end != first.span.start {
                    ctx.report_fix(
                        Span::new(open.span.start, first.span.start),
                        Some("unexpectedSpaceAfter"),
                        "Expected no space after '{{', but found.",
                        Some(Fix::remove(Span::new(open.span.end, first.span.start))),
                    );
                }
                if close.span.start != last.span.end {
                    ctx.report_fix(
                        Span::new(last.span.end, close.span.end),
                        Some("unexpectedSpaceBefore"),
                        "Expected no space before '}}', but found.",
                        Some(Fix::remove(Span::new(last.span.end, close.span.start))),
                    );
                }
            }
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(R {
        always: opts.get(0).as_str().unwrap_or("always") == "always",
    }))
}
