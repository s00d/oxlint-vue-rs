//! vue/no-multi-spaces

use oxc::span::Span;

use crate::ast::{Token, TokenKind};
use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::tpl_style::{token_after, tokens_with_comments};

struct R {
    ignore_properties: bool,
    ignore_eol_comments: bool,
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        let lines = ctx.file.lines;
        let line = |o: u32| lines.line_of(o);
        let is_prop = |t: &Token| t.kind == TokenKind::Punctuator && doc.slice(t.span) == ":";
        let mut lists: Vec<Vec<&Token>> = Vec::new();
        for e in doc.top_level() {
            if Some(e.id) == doc.template_body {
                lists.push(tokens_with_comments(doc, e.span));
            } else {
                lists.push(doc.tokens_in(e.start_tag.span).iter().collect());
                if let Some(t) = e.end_tag {
                    lists.push(doc.tokens_in(t).iter().collect());
                }
            }
        }
        for list in lists {
            let Some((&first, rest)) = list.split_first() else {
                continue;
            };
            let mut prev = first;
            for &t in rest {
                let mut ignore = self.ignore_properties && (is_prop(t) || is_prop(prev));
                if !ignore
                    && self.ignore_eol_comments
                    && matches!(t.kind, TokenKind::LineComment | TokenKind::BlockComment)
                {
                    let next = token_after(doc, t.span.end, true);
                    if next.is_none_or(|n| line(n.span.start) > line(t.span.end)) {
                        ignore = true;
                    }
                }
                if !ignore
                    && t.span.start > prev.span.end + 1
                    && line(t.span.start) == line(prev.span.start)
                {
                    let r = Span::new(prev.span.end, t.span.start);
                    ctx.report_fix(
                        r,
                        Some("multipleSpaces"),
                        format!("Multiple spaces found before '{}'.", doc.slice(t.span)),
                        Some(Fix::replace(r, " ")),
                    );
                }
                prev = t;
            }
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let o = opts.get(0);
    Some(Box::new(R {
        ignore_properties: o["ignoreProperties"].as_bool() == Some(true),
        ignore_eol_comments: o["ignoreEOLComments"].as_bool() == Some(true),
    }))
}
