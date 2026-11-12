//! vue/component-name-in-template-casing

use std::collections::HashSet;

use oxc::ast::ast::Expression;
use oxc::semantic::SymbolFlags;

use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::casing::{checker, exact_convert, pascal_case};
use crate::utils::js;
use crate::utils::regexp::{GroupMatcher, is_regexp};
use crate::utils::template::{
    is_html_well_known_element_name, is_math_well_known_element_name,
    is_svg_well_known_element_name, is_vue_builtin_element_name,
};
use crate::utils::tpl_style::{first_token, has_invalid_eof};
use crate::utils::tpl_valid::body_elements;
use crate::utils::vue_component as vc;

struct R {
    case: &'static str,
    ignores: GroupMatcher,
    global_strings: Vec<String>,
    global_patterns: GroupMatcher,
    registered_only: bool,
}

fn component_name(raw: &str) -> &str {
    raw.split('.').next().unwrap_or(raw)
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let file = ctx.file;
        let Some(doc) = ctx.doc() else { return };
        if has_invalid_eof(doc) {
            return;
        }
        let mut registered: HashSet<String> =
            self.global_strings.iter().map(|s| pascal_case(s)).collect();
        if self.registered_only {
            if vc::is_script_setup(file)
                && let Some(sem) = file.semantic
            {
                let s = sem.scoping();
                for (name, sym) in s.get_bindings(s.root_scope_id()).iter() {
                    if !s.symbol_flags(*sym).contains(SymbolFlags::TypeImport) {
                        registered.insert(name.to_string());
                    }
                }
            }
            for o in vc::vue_objects(file) {
                let comps = js::properties(o.obj).find(|p| {
                    js::prop_name(p).as_deref() == Some("components")
                        && matches!(p.value, Expression::ObjectExpression(_))
                });
                if let Some(p) = comps
                    && let Expression::ObjectExpression(c) = &p.value
                {
                    for q in js::properties(c) {
                        if let Some(n) = js::prop_name(q) {
                            registered.insert(n);
                        }
                    }
                }
            }
        }
        let src = ctx.source();
        for e in body_elements(doc) {
            let raw = e.raw_name.as_str();
            if self.ignores.is_match(raw)
                || is_html_well_known_element_name(raw)
                || is_svg_well_known_element_name(raw)
                || is_math_well_known_element_name(raw)
                || is_vue_builtin_element_name(raw)
            {
                continue;
            }
            if self.registered_only
                && !registered.contains(&pascal_case(component_name(raw)))
                && !self.global_patterns.is_match(raw)
            {
                continue;
            }
            let name = component_name(raw);
            if checker(self.case)(name) {
                continue;
            }
            let Some(open) = first_token(doc, e.start_tag.span) else {
                continue;
            };
            let casing = format!("{}{}", exact_convert(self.case, name), &raw[name.len()..]);
            let mut edits = vec![Fix::replace(open.span, format!("<{casing}"))];
            if let Some(end) = e.end_tag
                && let Some(t) = first_token(doc, end)
            {
                edits.push(Fix::replace(t.span, format!("</{casing}")));
            }
            // A name the conversion cannot change (`foo--bar`) would be "fixed" on every pass.
            let fix = (casing != raw).then(|| Fix::merge(src, edits)).flatten();
            ctx.report_fix(
                open.span,
                Some("incorrectCase"),
                format!("Component name \"{raw}\" is not {}.", self.case),
                fix,
            );
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let o = opts.get(1);
    let strs = |v: &serde_json::Value| -> Vec<String> {
        v.as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str())
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default()
    };
    let ignores = strs(&o["ignores"]);
    let globals = strs(&o["globals"]);
    let (pat, plain): (Vec<&String>, Vec<&String>) = globals.iter().partition(|g| is_regexp(g));
    Some(Box::new(R {
        case: if opts.get(0).as_str() == Some("kebab-case") {
            "kebab-case"
        } else {
            "PascalCase"
        },
        ignores: GroupMatcher::new(ignores.iter().map(String::as_str)),
        global_strings: plain.into_iter().cloned().collect(),
        global_patterns: GroupMatcher::new(pat.into_iter().map(String::as_str)),
        registered_only: o["registeredComponentsOnly"].as_bool() != Some(false),
    }))
}
