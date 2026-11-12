//! vue/no-undef-directives

use std::collections::HashSet;

use oxc::ast::ast::Expression;

use crate::ast::AttributeKey;
use crate::rule::{Ctx, Rule, RuleOptions, fmt_msg};
use crate::utils::casing::{camel_case, capitalize, kebab_case};
use crate::utils::js;
use crate::utils::regexp::GroupMatcher;
use crate::utils::template::is_builtin_directive_name;
use crate::utils::tpl_analysis::{global_names, module_bindings};
use crate::utils::tpl_misc::body_attributes;
use crate::utils::vue_component as vc;

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let ignore = opts.get(0)["ignore"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
        .unwrap_or_default();
    Some(Box::new(NoUndefDirectives {
        ignore: GroupMatcher::new(ignore),
    }))
}

struct NoUndefDirectives {
    ignore: GroupMatcher,
}

fn defined_in_options(raw: &str, names: &HashSet<String>) -> bool {
    if names.contains(raw) {
        return true;
    }
    let camel = camel_case(raw).to_lowercase();
    names.iter().any(|n| {
        let l = n.to_lowercase();
        *n != l && l == camel
    })
}

impl Rule for NoUndefDirectives {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let file = ctx.file;
        let Some(doc) = ctx.doc() else { return };
        let mut options = HashSet::new();
        for o in vc::vue_objects(file) {
            let Some(p) = js::properties(o.obj).find(|p| {
                js::prop_name(p).as_deref() == Some("directives")
                    && matches!(js::skip_parens(&p.value), Expression::ObjectExpression(_))
            }) else {
                continue;
            };
            if let Expression::ObjectExpression(d) = js::skip_parens(&p.value) {
                options.extend(js::properties(d).filter_map(js::prop_name).filter(|n| !n.is_empty()));
            }
        }
        let setup: Option<HashSet<String>> = vc::is_script_setup(file).then(|| {
            module_bindings(file)
                .into_iter()
                .map(|(n, _)| n.to_string())
                .chain(global_names(file))
                .collect()
        });
        for (_, a) in body_attributes(doc) {
            let AttributeKey::Directive(k) = &a.key else {
                continue;
            };
            if is_builtin_directive_name(&k.name.name) {
                continue;
            }
            let raw = if k.name.raw_name.is_empty() { &k.name.name } else { &k.name.raw_name };
            if is_builtin_directive_name(raw) || self.ignore.is_match(raw) || self.ignore.is_match(&kebab_case(raw)) {
                continue;
            }
            let in_setup = setup
                .as_ref()
                .is_some_and(|s| s.contains(&format!("v{}", capitalize(&camel_case(raw)))));
            if in_setup || defined_in_options(raw, &options) {
                continue;
            }
            ctx.report_id(
                k.span,
                "undef",
                fmt_msg("The 'v-{{name}}' directive has been used, but not defined.", &[("name", raw)]),
            );
        }
    }
}
