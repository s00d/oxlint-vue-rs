//! vue/require-typed-object-prop

use oxc::ast::ast::{ArrayExpression, Expression, IdentifierReference, PropertyKey};

use crate::rule::{Ctx, Fix, Rule, RuleOptions, Suggestion};
use crate::utils::js;
use crate::utils::vue_component::{self as vc, ComponentProp, MacroKind};

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(RequireTypedObjectProp))
}

struct RequireTypedObjectProp;

fn check_ident(ctx: &mut Ctx, id: &IdentifierReference) {
    if id.name != "Object" && id.name != "Array" {
        return;
    }
    let arr = if id.name == "Array" { "[]" } else { "" };
    let sugg = |base: &str| {
        let ty = format!("{base}{arr}");
        Suggestion {
            message_id: Some("addTypeAnnotation"),
            desc: format!("Add `{ty}` type annotation."),
            fix: Fix::insert(id.span.end, format!(" as PropType<{ty}>")),
        }
    };
    ctx.push(
        id.span,
        Some("expectedTypeAnnotation"),
        "Expected type annotation on object prop.".into(),
        None,
        vec![sugg("any"), sugg("unknown")],
    );
}

fn check_array(ctx: &mut Ctx, a: &ArrayExpression) {
    for e in &a.elements {
        if let Some(Expression::Identifier(id)) = e.as_expression() {
            check_ident(ctx, id);
        }
    }
}

fn check(ctx: &mut Ctx, props: &[ComponentProp]) {
    for p in props {
        let ComponentProp::Object { prop, .. } = p else {
            continue;
        };
        match js::skip_parens(&prop.value) {
            Expression::Identifier(id) => check_ident(ctx, id),
            Expression::ArrayExpression(a) => check_array(ctx, a),
            Expression::ObjectExpression(o) => {
                let ty = js::properties(o).find(|p| {
                    matches!(&p.key, PropertyKey::StaticIdentifier(k) if k.name == "type")
                        || matches!(&p.key, PropertyKey::Identifier(k) if k.name == "type")
                });
                match ty.map(|t| js::skip_parens(&t.value)) {
                    Some(Expression::Identifier(id)) => check_ident(ctx, id),
                    Some(Expression::ArrayExpression(a)) => check_array(ctx, a),
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

impl Rule for RequireTypedObjectProp {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let file = ctx.file;
        if vc::is_vue_file(file) {
            if file.doc.is_none() || !file.scripts.iter().any(|s| s.lang.as_deref() == Some("ts")) {
                return;
            }
        } else if !vc::is_ts_file(file) {
            return;
        }
        for m in vc::macros_of(file, MacroKind::DefineProps) {
            check(ctx, &vc::props_from_define_props(file, m.call));
        }
        for o in vc::vue_objects(file) {
            check(ctx, &vc::props_from_options(o.obj));
        }
    }
}
