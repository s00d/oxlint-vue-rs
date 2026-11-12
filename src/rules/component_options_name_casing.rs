//! vue/component-options-name-casing

use oxc::ast::ast::PropertyKey;
use oxc::span::GetSpan;

use crate::rule::{Ctx, Fix, Rule, RuleOptions, Suggestion, fmt_msg};
use crate::utils::vue_component as vc;
use crate::utils::{casing, js};

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let case = opts.get(0).as_str().unwrap_or("PascalCase").to_string();
    Some(Box::new(ComponentOptionsNameCasing { case }))
}

struct ComponentOptionsNameCasing {
    case: String,
}

/// `getOptionsComponentName`: identifier key (computed too) or string literal key.
fn options_component_name(key: &PropertyKey) -> Option<String> {
    match key {
        PropertyKey::StaticIdentifier(i) => Some(i.name.to_string()),
        PropertyKey::Identifier(i) => Some(i.name.to_string()),
        PropertyKey::StringLiteral(s) => Some(s.value.display().to_string()),
        _ => None,
    }
}

impl Rule for ComponentOptionsNameCasing {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let file = ctx.file;
        let case = self.case.as_str();
        let auto_fix = case == "PascalCase";
        for o in vc::vue_objects(file) {
            let Some(comps) = js::find_property(o.obj, "components") else {
                continue;
            };
            let oxc::ast::ast::Expression::ObjectExpression(comps) = &comps.value else {
                continue;
            };
            for p in js::properties(comps) {
                let Some(name) = options_component_name(&p.key) else {
                    continue;
                };
                if casing::checker(case)(&name) {
                    continue;
                }
                let converted = casing::converter(case)(&name);
                let key = p.key.span();
                let msg = fmt_msg(
                    "Component name \"{{component}}\" is not {{caseType}}.",
                    &[("component", &name), ("caseType", case)],
                );
                let fix = if p.shorthand {
                    let lhs = if !auto_fix && case == "kebab-case" {
                        format!("'{converted}'")
                    } else {
                        converted.clone()
                    };
                    Fix::replace(p.span, format!("{lhs}: {name}"))
                } else if !auto_fix && case == "kebab-case" {
                    Fix::replace(key, format!("'{converted}'"))
                } else {
                    Fix::replace(key, converted.clone())
                };
                if auto_fix {
                    ctx.push(key, Some("caseNotMatched"), msg, Some(fix), Vec::new());
                } else {
                    let desc = fmt_msg(
                        "Rename component name to be in {{caseType}}.",
                        &[("caseType", case)],
                    );
                    ctx.push(
                        key,
                        Some("caseNotMatched"),
                        msg,
                        None,
                        vec![Suggestion {
                            message_id: Some("possibleRenaming"),
                            desc,
                            fix,
                        }],
                    );
                }
            }
        }
    }
}
