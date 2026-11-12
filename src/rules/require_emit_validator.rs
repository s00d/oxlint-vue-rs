//! vue/require-emit-validator

use oxc::ast::ast::Expression;
use oxc::span::GetSpan;

use crate::rule::{Ctx, Fix, Rule, RuleOptions, Suggestion, fmt_msg};
use crate::utils::vue_component::{self as vc, ComponentEmit, MacroKind};

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(RequireEmitValidator))
}

struct RequireEmitValidator;

fn check(ctx: &mut Ctx, emits: &[ComponentEmit]) {
    for emit in emits {
        let (value, name, node) = match emit {
            ComponentEmit::Object { value, prop, .. } => {
                if matches!(
                    value,
                    Expression::ArrowFunctionExpression(_)
                        | Expression::FunctionExpression(_)
                        | Expression::Identifier(_)
                ) {
                    continue;
                }
                (Some(*value), emit.name(), prop.span)
            }
            ComponentEmit::Array { node, .. } => {
                let ident = match node.as_expression() {
                    Some(Expression::Identifier(i)) => Some(i.name.to_string()),
                    _ => None,
                };
                (
                    None,
                    emit.name().filter(|n| !n.is_empty()).or(ident),
                    node.span(),
                )
            }
            _ => continue,
        };
        let name = name
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "Unknown emit".into());
        if let Some(v @ Expression::NullLiteral(_)) = value {
            let sugg = Suggestion {
                message_id: Some("emptyValidation"),
                desc: "Replace with a validator function with no parameters.".into(),
                fix: Fix::replace(v.span(), "() => true"),
            };
            let msg = fmt_msg(
                "Emit \"{{name}}\" should not skip validation, or you may define a validator function with no parameters.",
                &[("name", &name)],
            );
            ctx.push(node, Some("skipped"), msg, None, vec![sugg]);
            continue;
        }
        ctx.report_id(
            node,
            "missing",
            fmt_msg(
                "Emit \"{{name}}\" should define at least its validator function.",
                &[("name", &name)],
            ),
        );
    }
}

impl Rule for RequireEmitValidator {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let file = ctx.file;
        for o in vc::vue_objects(file) {
            check(ctx, &vc::emits_from_options(o.obj));
        }
        for m in vc::macros_of(file, MacroKind::DefineEmits) {
            check(ctx, &vc::emits_from_define_emits(file, m.call));
        }
    }
}
