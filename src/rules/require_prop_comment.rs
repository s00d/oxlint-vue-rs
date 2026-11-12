//! vue/require-prop-comment

use oxc::ast::CommentKind;

use crate::rule::{Ctx, Rule, RuleOptions, fmt_msg};
use crate::utils::tokens;
use crate::utils::vue_component::{self as vc, ComponentProp, MacroKind};

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let ty = opts.get(0)["type"].as_str().unwrap_or("JSDoc").to_string();
    Some(Box::new(RequirePropComment { ty }))
}

struct RequirePropComment {
    ty: String,
}

impl RequirePropComment {
    fn verify(&self, ctx: &mut Ctx, props: &[ComponentProp]) {
        for p in props {
            let Some(name) = p.name().filter(|n| !n.is_empty()) else {
                continue;
            };
            let Some(node) = p.node_span() else { continue };
            let last = tokens::comments_before(ctx.file, node.start)
                .last()
                .copied();
            let block = last
                .filter(|c| c.kind != CommentKind::Line)
                .map(|c| ctx.slice(c.span)[2..].to_string());
            let jsdoc = block
                .as_deref()
                .is_some_and(|v| v.starts_with('*') && !v[1..].starts_with('*'));
            let (ok, id, msg) = match self.ty.as_str() {
                "block" => (
                    block.is_some() && !jsdoc,
                    "requireBlockComment",
                    "The \"{{name}}\" property should have a block comment.",
                ),
                "line" => (
                    last.is_some_and(|c| c.kind == CommentKind::Line),
                    "requireLineComment",
                    "The \"{{name}}\" property should have a line comment.",
                ),
                "any" => (
                    last.is_some(),
                    "requireAnyComment",
                    "The \"{{name}}\" property should have a comment.",
                ),
                _ => (
                    jsdoc,
                    "requireJSDocComment",
                    "The \"{{name}}\" property should have a JSDoc comment.",
                ),
            };
            if !ok {
                ctx.report_id(node, id, fmt_msg(msg, &[("name", &name)]));
            }
        }
    }
}

impl Rule for RequirePropComment {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let file = ctx.file;
        for m in vc::macros_of(file, MacroKind::DefineProps) {
            self.verify(ctx, &vc::props_from_define_props(file, m.call));
        }
        for o in vc::vue_objects(file) {
            self.verify(ctx, &vc::props_from_options(o.obj));
        }
    }
}
