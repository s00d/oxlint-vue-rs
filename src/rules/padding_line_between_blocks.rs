//! vue/padding-line-between-blocks

use oxc::span::Span;

use crate::ast::{Element, TokenKind};
use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::tpl_style::{has_invalid_eof, split_lines};

struct R {
    always: bool,
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        if has_invalid_eof(doc) {
            return;
        }
        let lines = ctx.file.lines;
        let line = |o: u32| lines.line_of(o);
        let src = ctx.source();
        let elements: Vec<&Element> = doc.top_level().collect();
        for w in elements.windows(2) {
            let (prev, next) = (w[0], w[1]);
            let between = Span::new(prev.span.end, next.span.start);
            let mut spans: Vec<Span> =
                doc.tokens_in(between)
                    .iter()
                    .filter(|t| t.kind != TokenKind::HTMLWhitespace)
                    .chain(doc.comments.iter().filter(|c| {
                        prev.span.end <= c.span.start && c.span.start < next.span.start
                    }))
                    .map(|t| t.span)
                    .collect();
            spans.sort_by_key(|s| s.start);
            spans.push(next.span);
            let mut p = prev.span;
            if self.always {
                let mut linebreak: Option<Span> = None;
                let mut padded = false;
                for s in &spans {
                    let n = line(s.start).saturating_sub(line(p.end));
                    if n > 1 {
                        padded = true;
                        break;
                    }
                    if linebreak.is_none() && n > 0 {
                        linebreak = Some(p);
                    }
                    p = *s;
                }
                if padded {
                    continue;
                }
                let fix = match linebreak {
                    Some(l) => Fix::insert(l.end, "\n"),
                    None => Fix::insert(prev.span.end, "\n\n"),
                };
                ctx.report_fix(
                    next.span,
                    Some("always"),
                    "Expected blank line before this block.",
                    Some(fix),
                );
            } else {
                if line(prev.span.end) == line(next.span.start) {
                    continue;
                }
                let mut edits = Vec::new();
                for s in &spans {
                    if line(s.start).saturating_sub(line(p.end)) > 1 {
                        let text = &src[p.end as usize..s.start as usize];
                        let last = split_lines(text).pop().unwrap_or("");
                        edits.push(Fix::replace(Span::new(p.end, s.start), format!("\n{last}")));
                    }
                    p = *s;
                }
                if edits.is_empty() {
                    continue;
                }
                ctx.report_fix(
                    next.span,
                    Some("never"),
                    "Unexpected blank line before this block.",
                    Fix::merge(src, edits),
                );
            }
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(R {
        always: opts.get(0).as_str() != Some("never"),
    }))
}
