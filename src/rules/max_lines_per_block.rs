//! vue/max-lines-per-block

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::tpl_style::has_invalid_eof;

struct R {
    template: Option<u64>,
    script: Option<u64>,
    style: Option<u64>,
    skip_blank_lines: bool,
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        if has_invalid_eof(doc) {
            return;
        }
        let lines = ctx.file.lines;
        for b in doc.top_level() {
            let limit = match b.name.as_str() {
                "template" => self.template,
                "script" => self.script,
                "style" => self.style,
                _ => None,
            };
            let Some(limit) = limit.filter(|l| *l > 0) else {
                continue;
            };
            let mut count =
                lines.line_of(b.span.end) as i64 - lines.line_of(b.span.start) as i64 - 1;
            if self.skip_blank_lines {
                count -= doc
                    .slice(b.span)
                    .split(['\n', '\r', '\u{2028}', '\u{2029}'])
                    .filter(|l| l.trim().is_empty())
                    .count() as i64;
            }
            if count > limit as i64 {
                ctx.report_id(
                    b.span,
                    "tooManyLines",
                    format!("Block has too many lines ({count}). Maximum allowed is {limit}."),
                );
            }
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let o = opts.get(0);
    Some(Box::new(R {
        template: o["template"].as_u64(),
        script: o["script"].as_u64(),
        style: o["style"].as_u64(),
        skip_blank_lines: o["skipBlankLines"].as_bool() == Some(true),
    }))
}
