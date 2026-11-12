//! vue/padding-line-between-tags

use std::collections::HashMap;

use oxc::span::Span;

use crate::ast::{Child, Element, ElementId};
use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::tpl_style::{is_js_ws, split_lines};
use crate::utils::tpl_valid::body_elements;

#[derive(Clone, Copy, PartialEq)]
enum Blank {
    Always,
    Never,
    Consistent,
}

struct Conf {
    blank: Blank,
    prev: String,
    next: String,
}

struct R {
    confs: Vec<Conf>,
}

fn is_blank(s: &str) -> bool {
    s.chars().all(is_js_ws)
}

impl R {
    fn line_text<'a>(ctx: &Ctx<'_, 'a>, line: usize) -> &'a str {
        let lines = ctx.file.lines;
        let src = ctx.source();
        let Some(start) = lines.line_start(line) else {
            return "";
        };
        let end = lines.line_start(line + 1).unwrap_or(src.len() as u32) as usize;
        src[start as usize..end].trim_end_matches(['\n', '\r', '\u{2028}', '\u{2029}'])
    }
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        let lines = ctx.file.lines;
        let line = |o: u32| lines.line_of(o);
        let single = |e: &Element| {
            let end = e.end_tag.unwrap_or(e.start_tag.span);
            line(e.start_tag.span.start) == line(end.end)
        };
        let matches = |sel: &str, e: &Element| {
            if let Some(ci) = sel.rfind(':')
                && ci > 0
            {
                let (tag, pseudo) = sel.split_at(ci);
                let tag_ok = tag == "*" || e.name == tag;
                if pseudo == ":single-line" {
                    return tag_ok && single(e);
                }
                if pseudo == ":multi-line" {
                    return tag_ok && !single(e);
                }
            }
            sel == "*" || e.name == sel
        };
        let mut first_consistent: HashMap<ElementId, Blank> = HashMap::new();
        let src = ctx.source();
        for block in body_elements(doc) {
            let Some(parent) = block.parent else { continue };
            let end_tag = block.end_tag.unwrap_or(block.start_tag.span);
            let Some(sib) = doc.element(parent).children.iter().find_map(|c| match c {
                Child::Element(id)
                    if *id != block.id && doc.element(*id).span.start >= end_tag.end =>
                {
                    Some(doc.element(*id))
                }
                _ => None,
            }) else {
                continue;
            };
            let Some(conf) = self
                .confs
                .iter()
                .rev()
                .find(|c| matches(&c.prev, block) && matches(&c.next, sib))
            else {
                continue;
            };
            let diff = line(sib.span.start) as i64 - line(end_tag.end) as i64;
            let mut blank = conf.blank;
            if blank == Blank::Consistent {
                match first_consistent.get(&parent) {
                    None => {
                        first_consistent.insert(
                            parent,
                            if diff > 1 {
                                Blank::Always
                            } else {
                                Blank::Never
                            },
                        );
                        continue;
                    }
                    Some(b) => blank = *b,
                }
            }
            if blank == Blank::Always {
                let fix = match diff {
                    1 => Some(Fix::insert(block.span.end, "\n")),
                    0 => {
                        let t = R::line_text(ctx, line(end_tag.start));
                        let sp = &t[..t.len() - t.trim_start_matches(is_js_ws).len()];
                        Some(Fix::insert(end_tag.end, format!("\n\n{sp}")))
                    }
                    _ => None,
                };
                if let Some(fix) = fix {
                    ctx.report_fix(
                        sib.span,
                        Some("always"),
                        "Expected blank line before this tag.",
                        Some(fix),
                    );
                }
            } else {
                if diff <= 1 {
                    continue;
                }
                let mut only_text = true;
                let mut i = line(end_tag.start) + 1;
                while only_text && i < line(sib.span.start) {
                    only_text = !is_blank(R::line_text(ctx, i));
                    i += 1;
                }
                if only_text {
                    continue;
                }
                let range = Span::new(end_tag.end, sib.span.start);
                let mut parts = split_lines(&src[range.start as usize..range.end as usize]);
                let mut text = format!("\n{}", parts.pop().unwrap_or(""));
                for (i, p) in parts.iter().enumerate().rev() {
                    if !is_blank(p) {
                        text = format!("{}{p}{text}", if i == 0 { "" } else { "\n" });
                    }
                }
                ctx.report_fix(
                    sib.span,
                    Some("never"),
                    "Unexpected blank line before this tag.",
                    Some(Fix::replace(range, text)),
                );
            }
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let confs = match opts.get(0).as_array() {
        Some(a) => a
            .iter()
            .map(|c| Conf {
                blank: match c["blankLine"].as_str() {
                    Some("never") => Blank::Never,
                    Some("consistent") => Blank::Consistent,
                    _ => Blank::Always,
                },
                prev: c["prev"].as_str().unwrap_or("").to_string(),
                next: c["next"].as_str().unwrap_or("").to_string(),
            })
            .collect(),
        None => vec![Conf {
            blank: Blank::Always,
            prev: "*".into(),
            next: "*".into(),
        }],
    };
    Some(Box::new(R { confs }))
}
