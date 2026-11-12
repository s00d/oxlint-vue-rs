//! vue/no-boolean-default

use oxc::ast::ast::{Expression, ObjectExpression};
use oxc::span::GetSpan;

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::js;
use crate::utils::vue_component::{self as vc, ComponentProp, MacroKind};

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(NoBooleanDefault {
        default_false: opts.get(0).as_str() == Some("default-false"),
    }))
}

struct NoBooleanDefault {
    default_false: bool,
}

fn is_boolean_ident(e: Option<&Expression>) -> bool {
    matches!(e.map(js::skip_parens), Some(Expression::Identifier(i)) if i.name == "Boolean")
}

impl NoBooleanDefault {
    fn verify(&self, ctx: &mut Ctx, e: &Expression) {
        if !self.default_false {
            ctx.report_id(
                e.span(),
                "noBooleanDefault",
                "Boolean prop should not set a default (Vue defaults it to false).",
            );
        } else if !matches!(js::skip_parens(e), Expression::BooleanLiteral(b) if !b.value) {
            ctx.report_id(
                e.span(),
                "defaultFalse",
                "Boolean prop should only be defaulted to false.",
            );
        }
    }

    fn process<'a>(
        &self,
        ctx: &mut Ctx,
        props: &[ComponentProp<'a>],
        other: &dyn Fn(&str) -> Vec<&'a Expression<'a>>,
    ) {
        for prop in props {
            match prop {
                ComponentProp::Object { value, .. } => {
                    let obj: Option<&ObjectExpression> = match value {
                        Expression::ObjectExpression(o) => Some(o),
                        _ => None,
                    };
                    let is_bool = is_boolean_ident(Some(value))
                        || obj.is_some_and(|o| {
                            is_boolean_ident(js::find_property(o, "type").map(|p| &p.value))
                        });
                    if !is_bool {
                        continue;
                    }
                    if let Some(d) = obj.and_then(|o| js::find_property(o, "default")) {
                        self.verify(ctx, &d.value);
                    }
                    if let Some(name) = prop.name() {
                        for d in other(&name) {
                            self.verify(ctx, d);
                        }
                    }
                }
                ComponentProp::Type(t) => {
                    if t.types.len() != 1 || t.types[0] != "Boolean" {
                        continue;
                    }
                    for d in other(&t.name) {
                        self.verify(ctx, d);
                    }
                }
                _ => {}
            }
        }
    }
}

impl Rule for NoBooleanDefault {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let file = ctx.file;
        for o in vc::vue_objects(file)
            .into_iter()
            .filter(|o| o.ty.is_component())
        {
            self.process(ctx, &vc::props_from_options(o.obj), &|_| Vec::new());
        }
        for m in vc::macros_of(file, MacroKind::DefineProps) {
            let props = vc::props_from_define_props(file, m.call);
            let (wd, _) = vc::with_defaults_prop_expressions(file, &m);
            let ds = vc::destructure_default_expressions(&m);
            self.process(ctx, &props, &|name| {
                wd.get(name)
                    .copied()
                    .into_iter()
                    .chain(ds.get(name).map(|d| d.1))
                    .collect()
            });
        }
    }
}
