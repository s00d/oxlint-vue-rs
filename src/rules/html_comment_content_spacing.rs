//! vue/html-comment-content-spacing

use oxc::span::Span;

use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::rules::html_comment_content_newline::exceptions;
use crate::utils::tpl_style::html_comments;

struct R {
    space: bool,
    exceptions: Vec<String>,
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        let lines = ctx.file.lines;
        let line = |o: u32| lines.line_of(o);
        for c in html_comments(doc, &self.exceptions, true) {
            let Some(value) = c.value else { continue };
            let before = c.open_decoration.unwrap_or(c.open);
            let range = Span::new(before.end, value.start);
            if line(before.end) == line(value.start) {
                if self.space {
                    if before.end >= value.start {
                        let (id, msg, fix) = if c.open_decoration.is_some() {
                            (
                                "expectedAfterExceptionBlock",
                                "Expected space after exception block.",
                                None,
                            )
                        } else {
                            (
                                "expectedAfterHTMLCommentOpen",
                                "Expected space after '<!--'.",
                                Some(Fix::insert(before.end, " ")),
                            )
                        };
                        ctx.report_fix(range, Some(id), msg, fix);
                    }
                } else if c.open_decoration.is_none() && before.end != value.start {
                    ctx.report_fix(
                        range,
                        Some("unexpectedAfterHTMLCommentOpen"),
                        "Unexpected space after '<!--'.",
                        Some(Fix::remove(range)),
                    );
                }
            }
            let after = c.close_decoration.unwrap_or(c.close);
            let range = Span::new(value.end, after.start);
            if line(value.end) == line(after.start) {
                if self.space {
                    if value.end >= after.start {
                        let (id, msg, fix) = if c.close_decoration.is_some() {
                            (
                                "expectedBeforeExceptionBlock",
                                "Expected space before exception block.",
                                None,
                            )
                        } else {
                            (
                                "expectedBeforeHTMLCommentOpen",
                                "Expected space before '-->'.",
                                Some(Fix::insert(after.start, " ")),
                            )
                        };
                        ctx.report_fix(range, Some(id), msg, fix);
                    }
                } else if c.close_decoration.is_none() && value.end != after.start {
                    ctx.report_fix(
                        range,
                        Some("unexpectedBeforeHTMLCommentOpen"),
                        "Unexpected space before '-->'.",
                        Some(Fix::remove(range)),
                    );
                }
            }
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(R {
        space: opts.get(0).as_str() != Some("never"),
        exceptions: exceptions(opts.get(1)),
    }))
}
