//! vue/block-tag-newline

use oxc::span::Span;

use crate::ast::Element;
use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::tpl_style::{has_invalid_eof, is_js_ws, js_trim, linebreak_count};

#[derive(Clone, Copy, PartialEq)]
enum Opt {
    Always,
    Never,
    Consistent,
    Ignore,
}

#[derive(Clone, Copy)]
struct Contents {
    singleline: Opt,
    multiline: Opt,
    max_empty_lines: usize,
}

struct R {
    base: Contents,
    blocks: Vec<(String, Contents)>,
}

fn opt(v: &serde_json::Value) -> Option<Opt> {
    match v.as_str()? {
        "always" => Some(Opt::Always),
        "never" => Some(Opt::Never),
        "consistent" => Some(Opt::Consistent),
        "ignore" => Some(Opt::Ignore),
        _ => None,
    }
}

fn phrase(n: usize) -> String {
    if n == 1 {
        "1 line break".into()
    } else {
        format!("{n} line breaks")
    }
}

fn verify(ctx: &mut Ctx, e: &Element, o: Contents) {
    let Some(end_tag) = e.end_tag else { return };
    if e.start_tag.self_closing {
        return;
    }
    let src = ctx.source();
    let (s, en) = (e.start_tag.span.end, end_tag.start);
    if s > en {
        return;
    }
    let text = &src[s as usize..en as usize];
    let trimmed = js_trim(text);
    if trimmed.is_empty() {
        return;
    }
    let option =
        if o.multiline != o.singleline && trimmed.contains(['\n', '\r', '\u{2028}', '\u{2029}']) {
            o.multiline
        } else {
            o.singleline
        };
    if option == Opt::Ignore {
        return;
    }
    let before = &text[..text.len() - text.trim_start_matches(is_js_ws).len()];
    let after = &text[text.trim_end_matches(is_js_ws).len()..];
    let (bc, ac) = (linebreak_count(before), linebreak_count(after));
    let always = match option {
        Opt::Always => true,
        Opt::Never => false,
        _ => {
            if (bc > 0) == (ac > 0) {
                return;
            }
            true
        }
    };
    let tag = &e.name;
    let max = o.max_empty_lines;
    let before_range = Span::new(s, s + before.len() as u32);
    let after_range = Span::new(en - after.len() as u32, en);
    if always {
        if bc == 0 {
            ctx.report_fix(
                Span::empty(s),
                Some("missingOpeningLinebreak"),
                format!("A line break is required after '<{tag}>'."),
                Some(Fix::insert(s, "\n")),
            );
        } else if max < bc - 1 {
            ctx.report_fix(
                before_range,
                Some("expectedOpeningLinebreak"),
                format!(
                    "Expected {} after '<{tag}>', but {} found.",
                    phrase(max + 1),
                    phrase(bc)
                ),
                Some(Fix::replace(before_range, "\n".repeat(max + 1))),
            );
        }
        if ac == 0 {
            ctx.report_fix(
                Span::empty(en),
                Some("missingClosingLinebreak"),
                format!("A line break is required before '</{tag}>'."),
                Some(Fix::insert(en, "\n")),
            );
        } else if max < ac - 1 {
            ctx.report_fix(
                after_range,
                Some("expectedClosingLinebreak"),
                format!(
                    "Expected {}  before '</{tag}>', but {} found.",
                    phrase(max + 1),
                    phrase(ac)
                ),
                Some(Fix::replace(after_range, "\n".repeat(max + 1))),
            );
        }
    } else {
        if bc > 0 {
            ctx.report_fix(
                before_range,
                Some("unexpectedOpeningLinebreak"),
                format!("There should be no line break after '<{tag}>'."),
                Some(Fix::remove(before_range)),
            );
        }
        if ac > 0 {
            ctx.report_fix(
                after_range,
                Some("unexpectedOpeningLinebreak"),
                format!("There should be no line break after '<{tag}>'."),
                Some(Fix::remove(after_range)),
            );
        }
    }
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        if has_invalid_eof(doc) {
            return;
        }
        for e in doc.top_level() {
            let o = self
                .blocks
                .iter()
                .find(|(n, _)| *n == e.name)
                .map_or(self.base, |b| b.1);
            verify(ctx, e, o);
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let o = opts.get(0);
    let num = |v: &serde_json::Value| v.as_f64().map(|n| n.max(0.0) as usize);
    let base = Contents {
        singleline: opt(&o["singleline"]).unwrap_or(Opt::Consistent),
        multiline: opt(&o["multiline"]).unwrap_or(Opt::Always),
        max_empty_lines: num(&o["maxEmptyLines"]).unwrap_or(0),
    };
    let blocks = o["blocks"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, v)| v.is_object())
        .map(|(k, v)| {
            (
                k.clone(),
                Contents {
                    singleline: opt(&v["singleline"]).unwrap_or(base.singleline),
                    multiline: opt(&v["multiline"]).unwrap_or(base.multiline),
                    max_empty_lines: num(&v["maxEmptyLines"]).unwrap_or(base.max_empty_lines),
                },
            )
        })
        .collect();
    Some(Box::new(R { base, blocks }))
}
