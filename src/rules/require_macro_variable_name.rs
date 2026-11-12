//! vue/require-macro-variable-name

use oxc::ast::AstKind;
use oxc::ast::ast::{BindingPattern, Expression};

use crate::rule::{Ctx, Fix, Rule, RuleOptions, Suggestion, fmt_msg};
use crate::utils::js;
use crate::utils::vue_component as vc;

const MACROS: [(&str, &str); 5] = [
    ("defineProps", "props"),
    ("defineEmits", "emit"),
    ("defineSlots", "slots"),
    ("useSlots", "slots"),
    ("useAttrs", "attrs"),
];

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let o = opts.get(0);
    let names = MACROS
        .iter()
        .map(|(m, d)| (*m, o[*m].as_str().unwrap_or(d).to_string()))
        .collect();
    Some(Box::new(RequireMacroVariableName { names }))
}

struct RequireMacroVariableName {
    names: Vec<(&'static str, String)>,
}

impl Rule for RequireMacroVariableName {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let file = ctx.file;
        let Some(sem) = file.semantic else { return };
        for n in sem.nodes().iter() {
            let AstKind::VariableDeclarator(d) = n.kind() else {
                continue;
            };
            if !vc::in_script_setup(file, d.span) {
                continue;
            }
            let Some(Expression::CallExpression(call)) = d.init.as_ref().map(js::skip_parens)
            else {
                continue;
            };
            let Expression::Identifier(callee) = &call.callee else {
                continue;
            };
            let macro_name = if callee.name == "withDefaults" {
                "defineProps"
            } else {
                callee.name.as_str()
            };
            let Some((_, want)) = self.names.iter().find(|(m, _)| *m == macro_name) else {
                continue;
            };
            let BindingPattern::BindingIdentifier(id) = &d.id else {
                continue;
            };
            if id.name == want.as_str() {
                continue;
            }
            let span = js::declarator_id_span(d);
            let sugg = Suggestion {
                message_id: Some("changeName"),
                desc: fmt_msg(
                    "Change the variable name to \"{{variableName}}\".",
                    &[("variableName", want)],
                ),
                fix: Fix::replace(span, want.clone()),
            };
            let msg = fmt_msg(
                "The variable name of \"{{macroName}}\" must be \"{{variableName}}\".",
                &[("macroName", macro_name), ("variableName", want)],
            );
            ctx.push(span, Some("requireName"), msg, None, vec![sugg]);
        }
    }
}
