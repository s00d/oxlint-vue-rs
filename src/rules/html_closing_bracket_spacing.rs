//! vue/html-closing-bracket-spacing

use oxc::span::Span;

use crate::ast::TokenKind;
use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::tpl_style::{document_tags, first_token, last_token};

struct R {
    start_tag: Option<bool>,
    end_tag: Option<bool>,
    self_closing_tag: Option<bool>,
}

/// `"always"` → Some(true), `"never"` → Some(false), anything else → None (no check).
fn opt(v: &serde_json::Value, default: &str) -> Option<bool> {
    match v.as_str().unwrap_or(if v.is_null() { default } else { "" }) {
        "always" => Some(true),
        "never" => Some(false),
        _ => None,
    }
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        let lines = ctx.file.lines;
        for (_, _, span) in document_tags(doc) {
            let (Some(first), Some(last)) = (first_token(doc, span), last_token(doc, span, 0))
            else {
                continue;
            };
            let ty = match (first.kind, last.kind) {
                (TokenKind::HTMLEndTagOpen, TokenKind::HTMLTagClose) => self.end_tag,
                (TokenKind::HTMLTagOpen, TokenKind::HTMLTagClose) => self.start_tag,
                (TokenKind::HTMLTagOpen, TokenKind::HTMLSelfClosingTagClose) => {
                    self.self_closing_tag
                }
                _ => None,
            };
            let Some(always) = ty else { continue };
            let Some(prev) = last_token(doc, span, 1) else {
                continue;
            };
            if lines.line_of(prev.span.end) != lines.line_of(last.span.start) {
                continue;
            }
            let has_space = prev.span.end != last.span.start;
            let bracket = doc.slice(last.span);
            if always && !has_space {
                ctx.report_fix(
                    last.span,
                    Some("missing"),
                    format!("Expected a space before '{bracket}', but not found."),
                    Some(Fix::insert(last.span.start, " ")),
                );
            } else if !always && has_space {
                ctx.report_fix(
                    Span::new(prev.span.end, last.span.end),
                    Some("unexpected"),
                    format!("Expected no space before '{bracket}', but found."),
                    Some(Fix::remove(Span::new(prev.span.end, last.span.start))),
                );
            }
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let o = opts.get(0);
    Some(Box::new(R {
        start_tag: opt(&o["startTag"], "never"),
        end_tag: opt(&o["endTag"], "never"),
        self_closing_tag: opt(&o["selfClosingTag"], "always"),
    }))
}
