//! vue/no-restricted-block

use crate::utils::regexp::JsRegex as Regex;

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::regexp::to_regexp;
use crate::utils::tpl_style::has_invalid_eof;

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let options = opts
        .options
        .iter()
        .filter_map(|o| {
            let (pat, msg) = match o.as_str() {
                Some(s) => (s, None),
                None => (
                    o.get("element")?.as_str()?,
                    o.get("message").and_then(|m| m.as_str()).map(String::from),
                ),
            };
            Some((to_regexp(pat)?, msg))
        })
        .collect();
    Some(Box::new(NoRestrictedBlock { options }))
}

struct NoRestrictedBlock {
    options: Vec<(Regex, Option<String>)>,
}

impl Rule for NoRestrictedBlock {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let Some(doc) = ctx.doc() else { return };
        if has_invalid_eof(doc) {
            return;
        }
        for block in doc.top_level() {
            let Some((_, msg)) = self.options.iter().find(|(r, _)| r.is_match(&block.raw_name))
            else {
                continue;
            };
            let message = msg
                .clone()
                .unwrap_or_else(|| format!("Using `<{}>` is not allowed.", block.raw_name));
            ctx.report_id(block.start_tag.span, "restrictedBlock", message);
        }
    }
}
