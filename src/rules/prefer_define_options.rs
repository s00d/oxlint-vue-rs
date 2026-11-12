//! vue/prefer-define-options

use oxc::ast::ast::{ExportDefaultDeclarationKind, Statement};
use oxc::span::{GetSpan, Span};

use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::vue_component::{self as vc, MacroKind};
use crate::utils::{js, tokens};

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(PreferDefineOptions))
}

struct PreferDefineOptions;

impl Rule for PreferDefineOptions {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let file = ctx.file;
        let (Some(setup), Some(program)) = (vc::script_setup_block(file), file.program) else {
            return;
        };
        let Some(ed) = program.body.iter().rev().find_map(|s| match s {
            Statement::ExportDefaultDeclaration(e) => Some(e),
            _ => None,
        }) else {
            return;
        };
        let has_define_options = !vc::macros_of(file, MacroKind::DefineOptions).is_empty();
        let fix = (!has_define_options).then(|| {
            let mut remove = ed.span;
            if let Some(script) = file.scripts.iter().find(|s| !s.setup) {
                let c = script.content;
                if tokens::tokens_between(file, c.start, c.end, true)
                    .iter()
                    .all(|t| js::in_range(remove, *t))
                {
                    remove = script.span;
                }
            }
            let line = file.lines.line_of(remove.start);
            let ls = file.lines.line_start(line).unwrap_or(0);
            if file.source[ls as usize..remove.start as usize]
                .trim()
                .is_empty()
            {
                remove.start = if line == 0 {
                    0
                } else {
                    // End of the previous line (before its one line terminator; blank lines above stay).
                    let prev = &file.source[..ls as usize];
                    let prev = prev.strip_suffix('\n').unwrap_or(prev);
                    prev.strip_suffix('\r').unwrap_or(prev).len() as u32
                };
            }
            let last_import = program
                .body
                .iter()
                .rev()
                .find(|s| {
                    matches!(s, Statement::ImportDeclaration(_))
                        && js::in_range(setup.span, s.span())
                })
                .map(|s| s.span().end);
            let insert_at = last_import.unwrap_or(setup.content.start);
            let decl = match &ed.declaration {
                ExportDefaultDeclarationKind::FunctionDeclaration(f) => f.span,
                ExportDefaultDeclarationKind::ClassDeclaration(c) => c.span,
                ExportDefaultDeclarationKind::TSInterfaceDeclaration(i) => i.span,
                d => d.as_expression().map_or(Span::empty(0), GetSpan::span),
            };
            Fix::merge(
                file.source,
                vec![
                    Fix::remove(remove),
                    Fix::insert(
                        insert_at,
                        format!("\ndefineOptions({})\n", file.slice(decl)),
                    ),
                ],
            )
        });
        ctx.report_fix(
            ed.span,
            Some("preferDefineOptions"),
            "Use `defineOptions` instead of default export.",
            fix.flatten(),
        );
    }
}
