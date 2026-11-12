//! vue/no-unused-components

use oxc::ast::ast::Expression;

use crate::ast::{AttributeKey, AttributeValue, Namespace};
use crate::rule::{Ctx, Rule, RuleOptions, fmt_msg};
use crate::utils::casing::{camel_case, is_camel_case, is_pascal_case, pascal_case};
use crate::utils::js;
use crate::utils::template::{
    directive_value, is_html_well_known_element_name, is_math_well_known_element_name,
    is_svg_well_known_element_name,
};
use crate::utils::tpl_analysis::registered_components;
use crate::utils::tpl_valid::body_elements;
use crate::utils::vue_component as vc;

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(NoUnusedComponents {
        ignore_binding: opts.get(0)["ignoreWhenBindingPresent"]
            .as_bool()
            .unwrap_or(true),
    }))
}

struct NoUnusedComponents {
    ignore_binding: bool,
}

impl Rule for NoUnusedComponents {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let file = ctx.file;
        let Some(doc) = ctx.doc() else { return };
        let Some(root) = doc.template_body else { return };
        let Some(last) = vc::vue_objects(file).last().copied() else {
            return;
        };
        let registered = registered_components(last.obj);
        if registered.is_empty() || doc.element(root).attr("src").is_some() {
            return;
        }
        let mut used: Vec<String> = Vec::new();
        for e in body_elements(doc) {
            let known = is_html_well_known_element_name(&e.raw_name)
                || is_svg_well_known_element_name(&e.raw_name)
                || is_math_well_known_element_name(&e.raw_name);
            let ns_ok = matches!(e.namespace, Namespace::Html | Namespace::Svg | Namespace::MathMl);
            if ns_ok && !known {
                used.push(e.raw_name.clone());
            }
            for a in e.attributes() {
                match &a.key {
                    AttributeKey::Directive(k)
                        if k.name.name == "is"
                            || (k.name.name == "bind" && k.static_argument() == Some("is")) =>
                    {
                        let Some(expr) = directive_value(doc, a).and_then(|c| c.expression.as_ref())
                        else {
                            continue;
                        };
                        match expr {
                            crate::ast::VExpression::Expression(e) => match js::skip_parens(e) {
                                Expression::StringLiteral(s) => used.push(s.value.display().to_string()),
                                Expression::NumericLiteral(_)
                                | Expression::BooleanLiteral(_)
                                | Expression::NullLiteral(_)
                                | Expression::RegExpLiteral(_)
                                | Expression::BigIntLiteral(_) => {}
                                _ if self.ignore_binding => return,
                                _ => {}
                            },
                            _ if self.ignore_binding => return,
                            _ => {}
                        }
                    }
                    AttributeKey::Plain(k) if k.name == "is" => {
                        if let Some(AttributeValue::Literal(l)) = &a.value {
                            used.push(l.value.strip_prefix("vue:").unwrap_or(&l.value).to_string());
                        }
                    }
                    _ => {}
                }
            }
        }
        for (p, name) in registered {
            let ok = if is_pascal_case(&name) || is_camel_case(&name) {
                used.iter()
                    .any(|n| !n.contains('_') && (name == pascal_case(n) || name == camel_case(n)))
            } else {
                used.contains(&name)
            };
            if !ok {
                ctx.report_id(
                    p.span,
                    "unused",
                    fmt_msg(
                        "The \"{{name}}\" component has been registered but not used.",
                        &[("name", &name)],
                    ),
                );
            }
        }
    }
}
