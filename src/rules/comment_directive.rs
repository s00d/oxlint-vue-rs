//! vue/comment-directive
//!
//! Upstream reports marker messages that its processor turns into template `<!-- eslint-disable -->`
//! support; that suppression lives in `crate::directives`. The only observable reports are the
//! `reportUnusedDisableDirectives` ones: this rule emits one per template disable directive (or per
//! listed rule), and `directives::filter` drops those whose directive silenced a problem.

use oxc::span::Span;

use crate::ast::{Document, TokenKind};
use crate::rule::{Ctx, Rule, RuleOptions};

/// `stripDirectiveComment` + `COMMENT_DIRECTIVE_B` / `COMMENT_DIRECTIVE_L`: the directive type
/// (`eslint-disable`, `eslint-disable-line`, `eslint-disable-next-line`; `None` for enable / other)
/// and the rules as (byte index in the comment value, name).
fn parse(value: &str) -> Option<(&'static str, Vec<(usize, &str)>)> {
    let text = strip(value);
    let t = text.trim_start();
    let body = t.strip_prefix("eslint-")?;
    let (ty, rest): (&'static str, &str) = if let Some(r) = body.strip_prefix("disable-next-line") {
        ("eslint-disable-next-line", r)
    } else if let Some(r) = body.strip_prefix("disable-line") {
        ("eslint-disable-line", r)
    } else {
        ("eslint-disable", body.strip_prefix("disable")?)
    };
    let ws = rest.len() - rest.trim_start().len();
    if ws == 0 && !rest.is_empty() {
        return None;
    }
    // `/([^\s,]+)[\s,]*/g` from the end of the directive match; each index = previous match end.
    let mut start = text.len() - rest.len() + ws;
    let mut rules = Vec::new();
    let mut pos = start;
    let sep = |c: char| c.is_whitespace() || c == ',';
    while let Some(off) = text[pos..].find(|c: char| !sep(c)) {
        let s = pos + off;
        let e = text[s..].find(sep).map_or(text.len(), |i| s + i);
        let after = text[e..]
            .find(|c: char| !sep(c))
            .map_or(text.len(), |i| e + i);
        rules.push((start, &text[s..e]));
        start = after;
        pos = after;
    }
    Some((ty, rules))
}

/// `value.split(/\s-{2,}\s/u, 1)[0]`.
pub(crate) fn strip(value: &str) -> &str {
    let b: Vec<(usize, char)> = value.char_indices().collect();
    for (k, &(i, c)) in b.iter().enumerate() {
        if !c.is_whitespace() {
            continue;
        }
        let mut j = k + 1;
        while j < b.len() && b[j].1 == '-' {
            j += 1;
        }
        if j - k > 2 && j < b.len() && b[j].1.is_whitespace() {
            return &value[..i];
        }
    }
    value
}

/// HTML comments of the template body and top-level comments outside every SFC block.
fn directive_comments<'d>(doc: &'d Document) -> impl Iterator<Item = &'d crate::ast::Token> {
    let body = doc.template_body.map(|id| doc.element(id).span);
    doc.comments.iter().filter(move |c| {
        c.kind == TokenKind::HTMLComment
            && (body.is_some_and(|b| b.start <= c.span.start && c.span.end <= b.end)
                || doc
                    .top_level()
                    .all(|e| c.span.end <= e.span.start || e.span.end <= c.span.start))
    })
}

struct CommentDirective {
    report_unused: bool,
}

impl Rule for CommentDirective {
    fn run(&self, ctx: &mut Ctx) {
        if !self.report_unused {
            return;
        }
        let Some(doc) = ctx.doc() else { return };
        for c in directive_comments(doc) {
            let Some((ty, rules)) = parse(&c.value) else {
                continue;
            };
            if ty != "eslint-disable"
                && ctx.file.lines.line_of(c.span.start) != ctx.file.lines.line_of(c.span.end)
            {
                continue;
            }
            if rules.is_empty() {
                ctx.report_id(
                    c.span,
                    "unused",
                    format!("Unused {ty} directive (no problems were reported)."),
                );
            }
            let value_start = c.span.start + 4; // `<!--`
            for (i, name) in rules {
                let s = value_start + i as u32;
                let span = Span::new(s, s + name.len() as u32);
                ctx.report_id(
                    span,
                    "unusedRule",
                    format!("Unused {ty} directive (no problems were reported from '{name}')."),
                );
                // `sourceCode.getLocFromIndex`
                let loc = ctx.mixed_loc(span, true, true);
                ctx.set_last_loc(loc);
            }
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(CommentDirective {
        report_unused: opts.get(0)["reportUnusedDisableDirectives"].as_bool() == Some(true),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_like_upstream() {
        assert_eq!(
            parse(" eslint-disable ").unwrap(),
            ("eslint-disable", vec![])
        );
        assert_eq!(
            parse(" eslint-disable vue/a, vue/b -- why").unwrap(),
            ("eslint-disable", vec![(16, "vue/a"), (23, "vue/b")])
        );
        assert_eq!(
            parse("eslint-disable-line vue/x").unwrap(),
            ("eslint-disable-line", vec![(20, "vue/x")])
        );
        assert!(parse("eslint-enable").is_none());
        assert!(parse("eslint-disabled").is_none());
    }

    #[test]
    fn unused_reports_follow_suppression() {
        use crate::{FileOptions, Severity, configure, lint_source};
        let rules = [
            configure(
                "comment-directive",
                Severity::Error,
                &[serde_json::json!({"reportUnusedDisableDirectives": true})],
                &serde_json::Value::Null,
            )
            .unwrap()
            .unwrap(),
            configure(
                "no-duplicate-attributes",
                Severity::Error,
                &[],
                &serde_json::Value::Null,
            )
            .unwrap()
            .unwrap(),
        ];
        let opts = FileOptions {
            template_directives: true,
            ..FileOptions::default()
        };
        let run = |src: &str| -> Vec<String> {
            lint_source(src, std::path::Path::new("t.vue"), &opts, &rules)
                .into_iter()
                .map(|d| d.message)
                .collect()
        };
        assert_eq!(
            run("<template>\n<!-- eslint-disable -->\n<div id id=\"a\"></div>\n</template>"),
            Vec::<String>::new()
        );
        assert_eq!(
            run("<template>\n<!-- eslint-disable -->\n<div id=\"a\"></div>\n</template>"),
            ["Unused eslint-disable directive (no problems were reported)."]
        );
        assert_eq!(
            run(
                "<template>\n<!-- eslint-disable vue/no-duplicate-attributes, vue/other -->\n<div id id></div>\n</template>"
            ),
            ["Unused eslint-disable directive (no problems were reported from 'vue/other')."]
        );
        assert_eq!(
            run(
                "<template>\n<div id id></div><!-- eslint-disable-line vue/no-duplicate-attributes -->\n</template>"
            ),
            Vec::<String>::new()
        );
    }
}
