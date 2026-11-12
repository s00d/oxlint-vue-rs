//! vue/no-props-shadow

use std::collections::HashSet;

use oxc::semantic::SymbolFlags;
use oxc::span::Span;

use crate::rule::{Ctx, Rule, RuleOptions, fmt_msg};
use crate::utils::js;
use crate::utils::tpl_analysis::module_bindings;
use crate::utils::vue_component::{self as vc, MacroKind};

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(NoPropsShadow))
}

struct NoPropsShadow;

impl Rule for NoPropsShadow {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let file = ctx.file;
        let Some(sem) = file.semantic else { return };
        let mut props = HashSet::new();
        let mut decls: Vec<Span> = Vec::new();
        for m in vc::macros_of(file, MacroKind::DefineProps) {
            props.extend(vc::props_from_define_props(file, m.call).iter().filter_map(|p| p.name()));
            if let Some(d) = m.declarator {
                decls.push(d.span);
            }
        }
        if props.is_empty() {
            return;
        }
        let sc = sem.scoping();
        for (name, sym) in module_bindings(file) {
            if !props.contains(name) {
                continue;
            }
            let f = sc.symbol_flags(sym);
            let type_only = (f.is_type() && !f.is_value() && !f.is_import())
                || (f.contains(SymbolFlags::TypeImport) && !f.contains(SymbolFlags::Import));
            let span = sc.symbol_span(sym);
            if type_only || decls.iter().any(|d| js::in_range(*d, span)) {
                continue;
            }
            ctx.report_id(
                span,
                "shadow",
                fmt_msg("Declaration of '{{name}}' shadows the prop with the same name.", &[("name", name)]),
            );
        }
    }
}
