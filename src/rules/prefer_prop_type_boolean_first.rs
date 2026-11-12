//! vue/prefer-prop-type-boolean-first

use oxc::ast::ast::{ArrayExpression, Expression};
use oxc::span::GetSpan;

use crate::rule::{Ctx, Fix, Rule, RuleOptions, Suggestion};
use crate::utils::js;
use crate::utils::vue_component::{self as vc, ComponentProp, MacroKind};

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(PreferPropTypeBooleanFirst))
}

struct PreferPropTypeBooleanFirst;

fn check_array(ctx: &mut Ctx, arr: &ArrayExpression) {
    let Some(idx) = arr.elements.iter().position(
        |e| matches!(e.as_expression(), Some(Expression::Identifier(i)) if i.name == "Boolean"),
    ) else {
        return;
    };
    if idx == 0 {
        return;
    }
    let mut code: Vec<&str> = vec!["Boolean"];
    for (i, e) in arr.elements.iter().enumerate() {
        if i != idx && !e.is_elision() {
            code.push(ctx.slice(e.span()));
        }
    }
    let fix = Fix::replace(arr.span, format!("[{}]", code.join(", ")));
    let sugg = Suggestion {
        message_id: Some("moveToFirst"),
        desc: "Move `Boolean` to be first in prop types.".into(),
        fix,
    };
    ctx.push(
        arr.elements[idx].span(),
        Some("shouldBeFirst"),
        "Type `Boolean` should be at first in prop types.".into(),
        None,
        vec![sugg],
    );
}

fn check(ctx: &mut Ctx, props: &[ComponentProp]) {
    for p in props {
        let ComponentProp::Object { value, .. } = p else {
            continue;
        };
        match value {
            Expression::ArrayExpression(a) => check_array(ctx, a),
            Expression::ObjectExpression(o) => {
                if let Some(t) = js::find_property(o, "type")
                    && let Expression::ArrayExpression(a) = &t.value
                {
                    check_array(ctx, a);
                }
            }
            _ => {}
        }
    }
}

impl Rule for PreferPropTypeBooleanFirst {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let file = ctx.file;
        for m in vc::macros_of(file, MacroKind::DefineProps) {
            check(ctx, &vc::props_from_define_props(file, m.call));
        }
        for o in vc::vue_objects(file) {
            check(ctx, &vc::props_from_options(o.obj));
        }
    }
}
