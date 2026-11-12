//! vue/no-duplicate-attr-inheritance

use oxc::ast::ast::{Expression, ObjectExpression};

use crate::ast::{AttributeKey, AttributeValue, Child};
use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::js;
use crate::utils::tpl_misc::body_attributes;
use crate::utils::vue_component::{self as vc, MacroKind};

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(NoDuplicateAttrInheritance {
        multi_root: opts.get(0)["checkMultiRootNodes"].as_bool() == Some(true),
    }))
}

struct NoDuplicateAttrInheritance {
    multi_root: bool,
}

/// `inheritAttrs: <literal>` → its truthiness.
fn inherit_attrs(o: &ObjectExpression) -> Option<bool> {
    let p = js::find_property(o, "inheritAttrs")?;
    Some(match js::skip_parens(&p.value) {
        Expression::BooleanLiteral(b) => b.value,
        Expression::NullLiteral(_) => false,
        Expression::NumericLiteral(n) => n.value != 0.0 && !n.value.is_nan(),
        Expression::StringLiteral(s) => !s.value.is_empty(),
        Expression::BigIntLiteral(b) => !b.value.trim_start_matches(['0', 'n']).is_empty(),
        Expression::RegExpLiteral(_) => true,
        _ => return None,
    })
}

impl Rule for NoDuplicateAttrInheritance {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let file = ctx.file;
        let Some(doc) = ctx.doc() else { return };
        let Some(root) = doc.template_body else { return };
        let mut objs: Vec<&ObjectExpression> = vc::vue_objects(file).iter().map(|o| o.obj).collect();
        for m in vc::macros_of(file, MacroKind::DefineOptions) {
            if let Some(Expression::ObjectExpression(o)) = m.arg(0) {
                objs.push(o);
            }
        }
        objs.sort_by_key(|o| o.span.start);
        let mut inherit = true;
        for o in objs {
            if let Some(v) = inherit_attrs(o) {
                inherit = v;
            }
        }
        if !inherit {
            return;
        }
        let roots: Vec<_> = doc
            .element(root)
            .children
            .iter()
            .filter_map(|c| match c {
                Child::Element(e) => Some(doc.element(*e)),
                _ => None,
            })
            .collect();
        let conditional = roots.len() >= 2
            && roots[0].has_directive("if")
            && roots.last().is_some_and(|l| l.has_directive("else-if") || l.has_directive("else"))
            && roots[1..roots.len() - 1].iter().all(|e| e.has_directive("else-if"));
        if !self.multi_root && roots.len() > 1 && !conditional {
            return;
        }
        for (_, a) in body_attributes(doc) {
            let (AttributeKey::Directive(k), Some(AttributeValue::Expr(id))) = (&a.key, &a.value) else {
                continue;
            };
            if k.name.name != "bind" || k.argument.is_some() {
                continue;
            }
            if let Some(r) = doc
                .expr(*id)
                .references
                .iter()
                .find(|r| r.variable.is_none() && r.name == "$attrs")
            {
                ctx.report_id(r.span, "noDuplicateAttrInheritance", "Set \"inheritAttrs\" to false.");
            }
        }
    }
}
