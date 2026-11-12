//! vue/require-macro-variable-name

use oxc::ast::AstKind;
use oxc::ast::ast::{BindingIdentifier, BindingPattern, Expression};
use oxc::semantic::Semantic;
use oxc::span::{GetSpan, Span};

use crate::rule::{Ctx, Fix, LintFile, Rule, RuleOptions, Suggestion, fmt_msg};
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
        // Two declarations renamed to one name (`--fix-suggestions` applies both) would clash.
        let mut taken: Vec<&str> = Vec::new();
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
            let sugg = if taken.contains(&want.as_str()) {
                None
            } else {
                taken.push(want);
                rename(file, sem, id, d.span, span, want)
            };
            let sugg = sugg.map(|fix| Suggestion {
                message_id: Some("changeName"),
                desc: fmt_msg(
                    "Change the variable name to \"{{variableName}}\".",
                    &[("variableName", want)],
                ),
                fix,
            });
            let msg = fmt_msg(
                "The variable name of \"{{macroName}}\" must be \"{{variableName}}\".",
                &[("macroName", macro_name), ("variableName", want)],
            );
            ctx.push(span, Some("requireName"), msg, None, sugg.into_iter().collect());
        }
    }
}

/// Rename the variable with its uses in the script and the template. eslint-plugin-vue renames only
/// the declaration (uses then point nowhere, or the name is declared twice). No suggestion when the
/// new name is taken or a use is a shorthand property (`{ emits }` would change its key).
fn rename(
    file: &LintFile,
    sem: &Semantic,
    id: &BindingIdentifier,
    decl: Span,
    span: Span,
    want: &str,
) -> Option<Fix> {
    let scoping = sem.scoping();
    if scoping.symbol_names().any(|n| n == want) {
        return None;
    }
    let mut edits = vec![Fix::replace(span, want)];
    if let Some(sym) = id.symbol_id.get() {
        for &rid in scoping.get_resolved_reference_ids(sym) {
            let node = scoping.get_reference(rid).node_id();
            let at = sem.nodes().kind(node).span();
            // `const useSlots = useSlots()`: the call means the import, not the TDZ binding.
            if decl.start <= at.start && at.end <= decl.end {
                continue;
            }
            if let AstKind::ObjectProperty(p) = sem.nodes().parent_kind(node)
                && p.shorthand
            {
                return None;
            }
            edits.push(Fix::replace(at, want));
        }
    }
    if let Some(doc) = &file.doc {
        for r in doc.exprs.iter().flat_map(|c| &c.references) {
            if r.variable.is_some() {
                continue;
            }
            if r.name == want {
                return None;
            }
            if r.name == id.name.as_str() {
                edits.push(Fix::replace(r.span, want));
            }
        }
    }
    Fix::merge(file.source, edits)
}
