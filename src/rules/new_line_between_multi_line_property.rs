//! vue/new-line-between-multi-line-property

use oxc::ast::AstKind;
use oxc::ast::ast::*;
use oxc::ast_visit::Visit;
use oxc::span::{GetSpan, Span};
use serde_json::Value;

use crate::rule::{Ctx, Fix, LintFile, Rule, RuleOptions};
use crate::utils::{tokens, vue_component as vc};

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let min = opts
        .get(0)
        .get("minLineOfMultilineProperty")
        .and_then(Value::as_u64)
        .filter(|n| *n != 0)
        .unwrap_or(2) as usize;
    Some(Box::new(NewLine { min }))
}

struct NewLine {
    min: usize,
}

/// lastToken(pre), tokens between (with comments), firstToken(cur).
fn between_tokens(file: &LintFile, pre: Span, cur: Span) -> Vec<Span> {
    let mut v = Vec::new();
    v.extend(tokens::last_token(file, pre));
    v.extend(tokens::tokens_between(file, pre.end, cur.start, true));
    v.extend(tokens::first_token(file, cur));
    v
}

struct V<'c, 'f, 'a> {
    min: usize,
    ctx: &'c mut Ctx<'f, 'a>,
    file: &'f LintFile<'a>,
    /// One entry per entered ObjectExpression: whether it is a Vue object.
    vue: Vec<bool>,
    /// One entry per entered CallExpression: whether it was counted (inside a Vue object).
    calls: Vec<bool>,
}

impl<'a> Visit<'a> for V<'_, '_, 'a> {
    fn enter_node(&mut self, kind: AstKind<'a>) {
        match kind {
            AstKind::CallExpression(_) => {
                let in_vue = self.vue.iter().any(|v| *v);
                self.calls.push(in_vue);
            }
            AstKind::ObjectExpression(obj) => {
                self.vue.push(vc::vue_object_type(self.file, obj).is_some());
                if self.vue.iter().any(|v| *v) && !self.calls.iter().any(|c| *c) {
                    self.check(obj);
                }
            }
            _ => {}
        }
    }
    fn leave_node(&mut self, kind: AstKind<'a>) {
        match kind {
            AstKind::CallExpression(_) => {
                self.calls.pop();
            }
            AstKind::ObjectExpression(_) => {
                self.vue.pop();
            }
            _ => {}
        }
    }
}

impl<'a> V<'_, '_, 'a> {
    fn check(&mut self, obj: &ObjectExpression<'a>) {
        let file = self.file;
        for w in obj.properties.windows(2) {
            let (pre, cur) = (w[0].span(), w[1].span());
            if tokens::line(file, pre.end) - tokens::line(file, pre.start) + 1 < self.min {
                continue;
            }
            let toks = between_tokens(file, pre, cur);
            if toks
                .windows(2)
                .any(|t| tokens::line(file, t[1].start) >= tokens::line(file, t[0].end) + 2)
            {
                continue;
            }
            let fix = match toks
                .windows(2)
                .find(|t| tokens::line(file, t[0].end) < tokens::line(file, t[1].start))
            {
                Some(t) => Fix::insert(t[0].end, "\n"),
                None => {
                    let comma = tokens::token_after_where(file, pre.end, |s| s == ",");
                    Fix::insert(comma.map_or(pre.end, |c| c.end), "\n\n")
                }
            };
            self.ctx.report_fix(
                Span::new(pre.end, cur.start),
                Some("missingEmptyLine"),
                "Enforce new lines between multi-line properties in Vue components.",
                Some(fix),
            );
        }
    }
}

impl Rule for NewLine {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let file = ctx.file;
        let Some(program) = file.program else { return };
        let mut v = V {
            min: self.min,
            ctx,
            file,
            vue: Vec::new(),
            calls: Vec::new(),
        };
        v.visit_program(program);
    }
}
