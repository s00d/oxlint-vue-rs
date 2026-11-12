//! vue/html-comment-content-newline

use oxc::span::Span;

use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::tpl_style::html_comments;

#[derive(Clone, Copy, PartialEq)]
enum Opt {
    Always,
    Never,
    Ignore,
}

fn opt(v: &serde_json::Value, d: Opt) -> Opt {
    match v.as_str() {
        Some("always") => Opt::Always,
        Some("never") => Opt::Never,
        Some("ignore") => Opt::Ignore,
        _ => d,
    }
}

struct R {
    singleline: Opt,
    multiline: Opt,
    exceptions: Vec<String>,
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        let lines = ctx.file.lines;
        let line = |o: u32| lines.line_of(o);
        for c in html_comments(doc, &self.exceptions, false) {
            let Some(value) = c.value else { continue };
            let start_line = line(c.open_decoration.map_or(value.start, |d| d.end));
            let end_line = line(c.close_decoration.map_or(value.end, |d| d.start));
            let ty = if start_line == end_line {
                self.singleline
            } else {
                self.multiline
            };
            if ty == Opt::Ignore {
                continue;
            }
            let req = ty != Opt::Never;
            // open
            let before = c.open_decoration.unwrap_or(c.open);
            let range = Span::new(before.end, value.start);
            if req {
                if line(before.end) >= line(value.start) {
                    let (id, msg, fix) = if c.open_decoration.is_some() {
                        (
                            "expectedAfterExceptionBlock",
                            "Expected line break after exception block.",
                            None,
                        )
                    } else {
                        (
                            "expectedAfterHTMLCommentOpen",
                            "Expected line break after '<!--'.",
                            Some(Fix::insert(before.end, "\n")),
                        )
                    };
                    ctx.report_fix(range, Some(id), msg, fix);
                }
            } else if line(before.end) != line(value.start) {
                ctx.report_fix(
                    range,
                    Some("unexpectedAfterHTMLCommentOpen"),
                    "Unexpected line breaks after '<!--'.",
                    Some(Fix::replace(range, " ")),
                );
            }
            // close
            let after = c.close_decoration.unwrap_or(c.close);
            let range = Span::new(value.end, after.start);
            if req {
                if line(value.end) >= line(after.start) {
                    let (id, msg, fix) = if c.close_decoration.is_some() {
                        (
                            "expectedBeforeExceptionBlock",
                            "Expected line break before exception block.",
                            None,
                        )
                    } else {
                        (
                            "expectedBeforeHTMLCommentOpen",
                            "Expected line break before '-->'.",
                            Some(Fix::insert(after.start, "\n")),
                        )
                    };
                    ctx.report_fix(range, Some(id), msg, fix);
                }
            } else if line(value.end) != line(after.start) {
                ctx.report_fix(
                    range,
                    Some("unexpectedBeforeHTMLCommentOpen"),
                    "Unexpected line breaks before '-->'.",
                    Some(Fix::replace(range, " ")),
                );
            }
        }
    }
}

pub fn exceptions(v: &serde_json::Value) -> Vec<String> {
    v["exceptions"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let o = opts.get(0);
    let (singleline, multiline) = match o.as_str() {
        Some(s) if !s.is_empty() => (opt(o, Opt::Never), opt(o, Opt::Always)),
        _ => (
            opt(&o["singleline"], Opt::Never),
            opt(&o["multiline"], Opt::Always),
        ),
    };
    Some(Box::new(R {
        singleline,
        multiline,
        exceptions: exceptions(opts.get(1)),
    }))
}
