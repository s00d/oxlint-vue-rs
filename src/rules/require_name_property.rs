//! vue/require-name-property

use oxc::span::{GetSpan, Span};

use crate::rule::{Ctx, Fix, LintFile, Rule, RuleOptions, Suggestion};
use crate::utils::vue_component::{self as vc, DefinitionType, VueObjectType};
use crate::utils::{js, tokens};

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(RequireNameProperty))
}

struct RequireNameProperty;

/// `getLineEmptyIndent`: leading whitespace of the line holding `pos`.
fn line_indent(file: &LintFile, pos: u32) -> String {
    let src = file.source;
    let start = src[..pos as usize]
        .rfind(['\n', '\r', '\u{2028}', '\u{2029}'])
        .map_or(0, |i| i + src[i..].chars().next().unwrap().len_utf8());
    src[start..]
        .chars()
        .take_while(|c| c.is_whitespace())
        .collect()
}

impl Rule for RequireNameProperty {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let file = ctx.file;
        if vc::is_script_setup(file) {
            return;
        }
        for o in vc::vue_objects(file) {
            if o.ty == VueObjectType::Definition
                && vc::definition_type(file, o.obj) == Some(DefinitionType::Mixin)
            {
                continue;
            }
            if js::properties(o.obj)
                .any(|p| !p.computed && js::prop_name(p).as_deref() == Some("name"))
            {
                continue;
            }
            let stem = file.path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let fix = match o.obj.properties.first() {
                Some(first) => {
                    let start = first.span().start;
                    let ft = tokens::first_token(file, first.span()).map_or(start, |t| t.start);
                    Some(Fix::insert(
                        start,
                        format!("name: '{stem}',\n{}", line_indent(file, ft)),
                    ))
                }
                None => {
                    let (Some(l), Some(r)) = (
                        tokens::first_token(file, o.obj.span),
                        tokens::last_token(file, o.obj.span),
                    ) else {
                        continue;
                    };
                    (file.slice(l) == "{" && file.slice(r) == "}").then(|| {
                        let ind = line_indent(file, l.start);
                        let inner = Span::new(l.end, r.start);
                        if file.slice(inner).trim().is_empty() {
                            Fix::replace(inner, format!("\n{ind}  name: '{stem}'\n{ind}"))
                        } else {
                            // Only comments inside: keep them (eslint-plugin-vue replaces them).
                            Fix::insert(l.end, format!("\n{ind}  name: '{stem}',"))
                        }
                    })
                }
            };
            let sugg = fix.map(|fix| Suggestion {
                message_id: Some("addName"),
                desc: "Add name property to component.".into(),
                fix,
            });
            ctx.push(
                o.obj.span,
                Some("missingName"),
                "Required name property is not set.".into(),
                None,
                sugg.into_iter().collect(),
            );
        }
    }
}
