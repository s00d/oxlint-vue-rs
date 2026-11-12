//! vue/define-macros-order

use oxc::ast::ast::*;
use oxc::span::{GetSpan, Span};
use serde_json::Value;

use crate::rule::{Ctx, Fix, LintFile, Rule, RuleOptions, Suggestion, fmt_msg};
use crate::utils::js;
use crate::utils::tokens;
use crate::utils::vue_component::{self as vc, MacroKind};

const KNOWN: [&str; 6] = [
    "defineEmits",
    "defineProps",
    "defineOptions",
    "defineSlots",
    "defineModel",
    "defineExpose",
];

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let o = opts.get(0);
    let order = match o.get("order").and_then(Value::as_array) {
        Some(a) => a
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        None => vec!["defineProps".to_string(), "defineEmits".to_string()],
    };
    Some(Box::new(DefineMacrosOrder {
        order,
        expose_last: o.get("defineExposeLast").and_then(Value::as_bool) == Some(true),
    }))
}

struct DefineMacrosOrder {
    order: Vec<String>,
    expose_last: bool,
}

fn is_skipped(s: &Statement) -> bool {
    match s {
        Statement::ImportDeclaration(_)
        | Statement::TSEnumDeclaration(_)
        | Statement::TSExternalModuleDeclaration(_)
        | Statement::TSNamespaceDeclaration(_)
        | Statement::TSGlobalDeclaration(_)
        | Statement::TSInterfaceDeclaration(_)
        | Statement::TSTypeAliasDeclaration(_)
        | Statement::DebuggerStatement(_)
        | Statement::EmptyStatement(_)
        | Statement::ExportNamedDeclaration(_)
        | Statement::ExportDeclaration(_)
        | Statement::ExportFromDeclaration(_) => true,
        Statement::VariableDeclaration(d) => d.declare,
        Statement::FunctionDeclaration(f) => f.declare,
        Statement::ClassDeclaration(c) => c.declare,
        Statement::ExpressionStatement(e) => {
            matches!(js::skip_parens(&e.expression), Expression::StringLiteral(l) if l.value == "use strict")
        }
        _ => false,
    }
}

/// `getLineStartIndex(token, prevToken)`.
fn line_start_index(file: &LintFile, token: Span, prev: Span) -> u32 {
    if tokens::line(file, token.start) == tokens::line(file, prev.end) {
        prev.end
    } else {
        tokens::line_start(file, token.start)
    }
}

/// The script block containing `pos` (vue-eslint-parser has the `<script>` tag tokens around it).
fn block(file: &LintFile, pos: u32) -> Option<Span> {
    file.scripts
        .iter()
        .map(|s| s.content)
        .find(|c| c.start <= pos && pos <= c.end)
}

/// `getTokenBefore(pos)`; at the start of a script block, the start tag.
fn before_tok(file: &LintFile, pos: u32) -> Span {
    let b = block(file, pos);
    match tokens::token_before(file, pos) {
        Some(t) if b.is_none_or(|b| t.start >= b.start) => t,
        _ => b.map_or(Span::new(0, 0), |b| Span::new(b.start, b.start)),
    }
}

/// `getTokenAfter(pos, { includeComments: true })`; at the end of a script block, the end tag.
fn after_tok_c(file: &LintFile, pos: u32) -> Span {
    let b = block(file, pos);
    let eof = file.source.len() as u32;
    match tokens::token_or_comment_after(file, pos) {
        Some(t) if b.is_none_or(|b| t.end <= b.end) => t,
        _ => b.map_or(Span::new(eof, eof), |b| Span::new(b.end, b.end)),
    }
}

/// (cutStart, cutEnd, beforeNodeToken, nodeComment) of a node to move.
fn cut(file: &LintFile, node: Span) -> (u32, u32, Span, Span) {
    let before = before_tok(file, node.start);
    let node_comment = after_tok_c(file, before.end);
    let next = after_tok_c(file, node.end);
    (
        line_start_index(file, node_comment, before),
        line_start_index(file, next, node),
        before,
        node_comment,
    )
}

fn move_before(file: &LintFile, node: Span, target: Span) -> Vec<Fix> {
    let (cut_start, cut_end, _, node_comment) = cut(file, node);
    let target_comment = after_tok_c(file, before_tok(file, target.start).end);
    let text = file.slice(Span::new(node_comment.start, node.end));
    let after_target = after_tok_c(file, target.end);
    let after_text = file.slice(Span::new(target.end, after_target.start));
    let semi = if !text.ends_with(';') && !after_text.contains('\n') {
        ";"
    } else {
        ""
    };
    vec![
        Fix::insert(target_comment.start, format!("{text}{after_text}{semi}")),
        Fix::remove(Span::new(cut_start, cut_end)),
    ]
}

fn move_to_last(file: &LintFile, node: Span, target: Span) -> Vec<Fix> {
    let (cut_start, cut_end, before, _) = cut(file, node);
    vec![
        Fix::insert(target.end, file.slice(Span::new(before.end, node.end))),
        Fix::remove(Span::new(cut_start, cut_end)),
    ]
}

impl Rule for DefineMacrosOrder {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let file = ctx.file;
        let (Some(setup), Some(program)) = (vc::script_setup_block(file), file.program) else {
            return;
        };
        // name -> top-level statement spans (last write wins, models accumulate).
        let mut macros: Vec<(String, Vec<Span>)> = Vec::new();
        let mut set =
            |name: &str, s: Span, push: bool| match macros.iter_mut().find(|(n, _)| n == name) {
                Some((_, v)) if push => v.push(s),
                Some((_, v)) => *v = vec![s],
                None => macros.push((name.to_string(), vec![s])),
            };
        let mut expose: Option<Span> = None;
        let found = vc::script_setup_macros(file);
        for (i, stmt) in program.body.iter().enumerate() {
            let s = stmt.span();
            for m in found.iter().filter(|m| m.statement_index == i) {
                match m.kind {
                    MacroKind::DefineExpose => expose = Some(s),
                    MacroKind::DefineModel => set("defineModel", s, true),
                    k => set(k.name(), s, false),
                }
            }
            if !js::in_range(setup.content, s) {
                continue;
            }
            let calls: Vec<&Expression> = match stmt {
                Statement::ExpressionStatement(e) => vec![js::skip_parens(&e.expression)],
                Statement::VariableDeclaration(d) => d
                    .declarations
                    .iter()
                    .filter_map(|d| d.init.as_ref().map(js::skip_parens))
                    .collect(),
                _ => Vec::new(),
            };
            for c in calls {
                if let Expression::CallExpression(call) = c
                    && let Expression::Identifier(id) = &call.callee
                    && self.order.iter().any(|o| o == id.name.as_str())
                    && !KNOWN.contains(&id.name.as_str())
                {
                    set(&id.name, s, false);
                }
            }
        }
        let get = |name: &str| {
            macros
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };

        let first = program
            .body
            .iter()
            .position(|s| js::in_range(setup.span, s.span()) && !is_skipped(s));
        let ordered: Vec<(String, Span)> = self
            .order
            .iter()
            .flat_map(|n| get(n).into_iter().map(move |s| (n.clone(), s)))
            .collect();

        if self.expose_last
            && let (Some(e), Some(last)) = (expose, program.body.last())
            && last.span() != e
        {
            let fix = Fix::merge(file.source, move_to_last(file, e, last.span()));
            ctx.push(
                e,
                Some("defineExposeNotTheLast"),
                "`defineExpose` should be the last statement in `<script setup>`.".into(),
                None,
                fix.into_iter()
                    .map(|fix| Suggestion {
                        message_id: Some("putExposeAtTheLast"),
                        desc: "Put `defineExpose` as the last statement in `<script setup>`."
                            .into(),
                        fix,
                    })
                    .collect(),
            );
        }

        let Some(first) = first else { return };
        for (index, (name, node)) in ordered.iter().enumerate() {
            let Some(target) = program.body.get(first + index).map(GetSpan::span) else {
                return;
            };
            if *node == target {
                continue;
            }
            let mut moving: Vec<Span> = ordered[index..].iter().map(|(_, s)| *s).collect();
            if let Some(p) = moving.iter().position(|s| *s == target) {
                moving.truncate(p);
            }
            let before = self.order.iter().find(|n| get(n).contains(&target));
            let (id, msg) = match before {
                Some(b) => (
                    "macrosUnordered",
                    fmt_msg(
                        "{{macro}} should be above {{before}}.",
                        &[("macro", name), ("before", b)],
                    ),
                ),
                None => (
                    "macrosNotAtTop",
                    fmt_msg(
                        "{{macro}} should be placed at the top of `<script setup>` (after any potential import statements or type definitions).",
                        &[("macro", name)],
                    ),
                ),
            };
            let edits: Vec<Fix> = moving
                .iter()
                .flat_map(|n| move_before(file, *n, target))
                .collect();
            ctx.report_fix(moving[0], Some(id), msg, Fix::merge(file.source, edits));
            return;
        }
    }
}
