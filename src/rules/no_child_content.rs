//! vue/no-child-content

use oxc::span::Span;

use crate::ast::{Child, TokenKind};
use crate::rule::{Ctx, Fix, Rule, RuleOptions, Suggestion, fmt_msg};
use crate::utils::tpl_misc::body_attributes;
use crate::utils::tpl_valid::child_span;

struct R {
    directives: Vec<String>,
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (e, a) in body_attributes(doc) {
            let Some(k) = crate::utils::template::directive_key(a) else { continue };
            let Some(end) = e.end_tag else { continue };
            if !self.directives.contains(&k.name.name) {
                continue;
            }
            let (lo, hi) = (e.start_tag.span.end, end.start);
            let mut nodes: Vec<(Span, bool)> = e
                .children
                .iter()
                .map(|c| {
                    let empty = matches!(c, Child::Text(t) if doc.text(*t).value.trim().is_empty());
                    (child_span(doc, *c), empty)
                })
                .collect();
            nodes.extend(
                doc.comments
                    .iter()
                    .filter(|c| c.kind == TokenKind::HTMLComment && lo <= c.span.start && c.span.end <= hi)
                    .map(|c| (c.span, false)),
            );
            if !nodes.iter().any(|(_, empty)| !empty) {
                continue;
            }
            let start = nodes.iter().map(|n| n.0.start).min().unwrap();
            let stop = nodes.iter().map(|n| n.0.end).max().unwrap();
            let suggestions = Fix::merge(doc.source, nodes.iter().map(|n| Fix::remove(n.0)).collect())
                .map(|fix| Suggestion {
                    message_id: Some("removeChildContent"),
                    desc: "Remove child content.".into(),
                    fix,
                })
                .into_iter()
                .collect();
            ctx.push(
                Span::new(start, stop),
                Some("disallowedChildContent"),
                fmt_msg(
                    "Child content is disallowed because it will be overwritten by the v-{{ directiveName }} directive.",
                    &[("directiveName", &k.name.name)],
                ),
                None,
                suggestions,
            );
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let mut directives = vec!["html".to_string(), "text".to_string()];
    if let Some(a) = opts.get(0)["additionalDirectives"].as_array() {
        directives.extend(a.iter().filter_map(|v| v.as_str().map(str::to_string)));
    }
    Some(Box::new(R { directives }))
}
