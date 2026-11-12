//! vue/prefer-use-template-ref

use std::collections::HashMap;

use oxc::ast::ast::{BindingPattern, CallExpression, Expression, Statement};

use crate::ast::{AttributeKey, AttributeValue};
use crate::rule::{Ctx, Rule, RuleOptions, fmt_msg};
use crate::utils::js;
use crate::utils::tpl_misc::body_attributes;
use crate::utils::vue_component as vc;

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(PreferUseTemplateRef))
}

struct PreferUseTemplateRef;

fn script_refs<'a>(body: &'a [Statement<'a>], out: &mut HashMap<String, &'a CallExpression<'a>>) {
    for s in body {
        let Statement::VariableDeclaration(d) = s else { continue };
        let Some(decl) = d.declarations.first() else { continue };
        if let Some(Expression::CallExpression(c)) = decl.init.as_ref().map(js::skip_parens)
            && let Expression::Identifier(callee) = &c.callee
            && let BindingPattern::BindingIdentifier(id) = &decl.id
            && matches!(callee.name.as_str(), "ref" | "shallowRef")
        {
            out.insert(id.name.to_string(), c);
        }
    }
}

impl Rule for PreferUseTemplateRef {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let file = ctx.file;
        let Some(doc) = ctx.doc() else { return };
        let mut refs = HashMap::new();
        // Script-setup `Program` first, then setup functions (traversal order).
        if vc::script_setup_block(file).is_some()
            && let Some(p) = file.program
        {
            script_refs(&p.body, &mut refs);
        }
        for o in vc::vue_objects(file) {
            for f in vc::setup_functions(o.obj) {
                if let Some(b) = f.body() {
                    script_refs(&b.statements, &mut refs);
                }
            }
        }
        let mut seen = Vec::new();
        for (_, a) in body_attributes(doc) {
            if let (AttributeKey::Plain(k), Some(AttributeValue::Literal(l))) = (&a.key, &a.value)
                && k.name == "ref"
                && !l.value.is_empty()
                && !seen.contains(&l.value)
            {
                seen.push(l.value.clone());
                if let Some(c) = refs.get(&l.value) {
                    let name = match &c.callee {
                        Expression::Identifier(i) => i.name.as_str(),
                        _ => "",
                    };
                    ctx.report_id(
                        c.span,
                        "preferUseTemplateRef",
                        fmt_msg("Replace '{{name}}' with 'useTemplateRef'.", &[("name", name)]),
                    );
                }
            }
        }
    }
}
