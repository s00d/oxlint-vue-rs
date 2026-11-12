//! vue/no-template-shadow

use std::collections::HashSet;

use crate::ast::{Child, Document, ElementId};
use crate::rule::{Ctx, Rule, RuleOptions, fmt_msg};
use crate::utils::vue_component as vc;

struct R {
    allow: Vec<String>,
}

const GROUPS: [&str; 6] = ["props", "computed", "data", "asyncData", "methods", "setup"];

fn walk(
    ctx: &mut Ctx,
    doc: &Document,
    id: ElementId,
    js: &HashSet<String>,
    allow: &[String],
    mut scope: Vec<String>,
) {
    let e = doc.element(id);
    for v in &e.variables {
        if allow.contains(&v.name) {
            continue;
        }
        if scope.contains(&v.name) || js.contains(&v.name) {
            ctx.report(
                v.span,
                fmt_msg("Variable '{{name}}' is already declared in the upper scope.", &[("name", &v.name)]),
            );
        } else {
            scope.push(v.name.clone());
        }
    }
    for c in &e.children {
        if let Child::Element(c) = c {
            walk(ctx, doc, *c, js, allow, scope.clone());
        }
    }
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let file = ctx.file;
        let Some(doc) = file.doc.as_ref() else { return };
        let Some(root) = doc.template_body else { return };
        let mut js: HashSet<String> = HashSet::new();
        if vc::is_script_setup(file)
            && let Some(sem) = file.semantic
        {
            let s = sem.scoping();
            for (name, _) in s.get_bindings(s.root_scope_id()).iter() {
                js.insert(name.to_string());
            }
        }
        for m in vc::macros_of(file, vc::MacroKind::DefineProps) {
            for p in vc::props_from_define_props(file, m.call) {
                if let Some(n) = p.name() {
                    js.insert(n);
                }
            }
        }
        for o in vc::vue_objects(file) {
            for p in vc::iterate_properties(o.obj, &GROUPS) {
                js.insert(p.name);
            }
        }
        walk(ctx, doc, root, &js, &self.allow, Vec::new());
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let allow = opts.get(0)["allow"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    Some(Box::new(R { allow }))
}
