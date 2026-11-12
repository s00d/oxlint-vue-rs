//! vue/html-closing-bracket-newline

use oxc::span::Span;

use crate::ast::TokenKind;
use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::tpl_style::{document_tags, last_token, token_before};

struct R {
    singleline: bool,
    multiline: bool,
    self_singleline: Option<bool>,
    self_multiline: Option<bool>,
}

fn phrase(n: usize) -> String {
    match n {
        0 => "no line breaks".into(),
        1 => "1 line break".into(),
        n => format!("{n} line breaks"),
    }
}

fn always(v: &serde_json::Value) -> Option<bool> {
    v.as_str().map(|s| s == "always")
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        let lines = ctx.file.lines;
        for (e, is_end, span) in document_tags(doc) {
            let Some(close) = last_token(doc, span, 0) else {
                continue;
            };
            if !matches!(
                close.kind,
                TokenKind::HTMLSelfClosingTagClose | TokenKind::HTMLTagClose
            ) {
                continue;
            }
            let Some(prev) = token_before(doc, close.span.start, false) else {
                continue;
            };
            let prev_line = lines.line_of(prev.span.end);
            let single = lines.line_of(span.start) == prev_line;
            let self_closing = !is_end && e.start_tag.self_closing;
            let self_opt = if single {
                self.self_singleline
            } else {
                self.self_multiline
            };
            let want = match self_opt {
                Some(v) if self_closing => v,
                _ if single => self.singleline,
                _ => self.multiline,
            };
            let expected = usize::from(want);
            let actual = lines.line_of(close.span.start).saturating_sub(prev_line);
            if actual != expected {
                let range = Span::new(prev.span.end, close.span.start);
                ctx.report_fix(
                    range,
                    Some("expectedBeforeClosingBracket"),
                    format!(
                        "Expected {} before closing bracket, but {} found.",
                        phrase(expected),
                        phrase(actual)
                    ),
                    Some(Fix::replace(range, "\n".repeat(expected))),
                );
            }
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let o = opts.get(0);
    Some(Box::new(R {
        singleline: always(&o["singleline"]).unwrap_or(false),
        multiline: always(&o["multiline"]).unwrap_or(true),
        self_singleline: always(&o["selfClosingTag"]["singleline"]),
        self_multiline: always(&o["selfClosingTag"]["multiline"]),
    }))
}
