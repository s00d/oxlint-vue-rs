//! vue/first-attribute-linebreak

use oxc::span::Span;

use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::tpl_style::token_before;
use crate::utils::tpl_valid::body_elements;

#[derive(Clone, Copy, PartialEq)]
enum Loc {
    Below,
    Beside,
    Ignore,
}

fn loc(v: &serde_json::Value, default: Loc) -> Loc {
    match v.as_str() {
        Some("below") => Loc::Below,
        Some("beside") => Loc::Beside,
        Some("ignore") => Loc::Ignore,
        _ => default,
    }
}

struct R {
    singleline: Loc,
    multiline: Loc,
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        let lines = ctx.file.lines;
        for e in body_elements(doc) {
            let attrs = e.attributes();
            let (Some(first), Some(last)) = (attrs.first(), attrs.last()) else {
                continue;
            };
            let location = if lines.line_of(first.span.start) == lines.line_of(last.span.end) {
                self.singleline
            } else {
                self.multiline
            };
            let tag_line = lines.line_of(e.start_tag.span.start);
            let first_line = lines.line_of(first.span.start);
            let ok = match location {
                Loc::Ignore => true,
                Loc::Beside => tag_line == first_line,
                Loc::Below => tag_line < first_line,
            };
            if ok {
                continue;
            }
            let fix = token_before(doc, first.span.start, true).map(|p| {
                Fix::replace(
                    Span::new(p.span.end, first.span.start),
                    if location == Loc::Beside { " " } else { "\n" },
                )
            });
            let (id, msg) = if location == Loc::Beside {
                ("unexpected", "Expected no linebreak before this attribute.")
            } else {
                ("expected", "Expected a linebreak before this attribute.")
            };
            ctx.report_fix(first.span, Some(id), msg, fix);
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let o = opts.get(0);
    Some(Box::new(R {
        singleline: loc(&o["singleline"], Loc::Ignore),
        multiline: loc(&o["multiline"], Loc::Below),
    }))
}
