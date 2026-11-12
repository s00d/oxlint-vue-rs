//! vue/no-useless-concat (core `no-useless-concat` in `<template>`)

use oxc::ast::ast::{BinaryExpression, BinaryOperator, Expression};
use oxc::ast_visit::{Visit, walk};
use oxc::span::GetSpan;

use crate::ast::{Token, TokenKind};
use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::js::skip_parens;
use crate::utils::tpl_core::{DOCUMENT, first_token_between, for_each_root, visit_root};

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(NoUselessConcat))
}

struct NoUselessConcat;

fn concat<'x, 'b>(e: &'x Expression<'b>) -> Option<&'x BinaryExpression<'b>> {
    match skip_parens(e) {
        Expression::BinaryExpression(b) if b.operator == BinaryOperator::Addition => Some(b),
        _ => None,
    }
}

fn is_string(e: &Expression) -> bool {
    matches!(
        e,
        Expression::StringLiteral(_) | Expression::TemplateLiteral(_)
    )
}

struct V<'r, 'c, 'f, 'a> {
    ctx: &'c mut Ctx<'f, 'a>,
    tokens: &'r [Token],
}

impl<'b> Visit<'b> for V<'_, '_, '_, '_> {
    crate::skip_ts_types!('b);
    fn visit_binary_expression(&mut self, b: &BinaryExpression<'b>) {
        walk::walk_binary_expression(self, b);
        if b.operator != BinaryOperator::Addition {
            return;
        }
        let mut left = skip_parens(&b.left);
        while let Some(c) = concat(left) {
            left = skip_parens(&c.right);
        }
        let mut right = skip_parens(&b.right);
        while let Some(c) = concat(right) {
            right = skip_parens(&c.left);
        }
        if !is_string(left) || !is_string(right) {
            return;
        }
        let lines = self.ctx.file.lines;
        if lines.line_of(left.span().end) != lines.line_of(right.span().start) {
            return;
        }
        let op = first_token_between(self.tokens, left.span().end, right.span().start, |t| {
            t.kind == TokenKind::Punctuator && t.value == "+"
        });
        if let Some(op) = op {
            self.ctx.report_id(
                op.span,
                "unexpectedConcat",
                "Unexpected string concatenation of literals.",
            );
        }
    }
}

impl Rule for NoUselessConcat {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        for_each_root(ctx, DOCUMENT, |ctx, root| {
            visit_root(&mut V { ctx, tokens: root.tokens }, root.expr);
        });
    }
}
