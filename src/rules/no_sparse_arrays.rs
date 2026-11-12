//! vue/no-sparse-arrays (core `no-sparse-arrays` in `<template>`)

use oxc::ast::ast::{ArrayExpression, ArrayExpressionElement};
use oxc::ast_visit::{Visit, walk};
use oxc::span::GetSpan;

use crate::ast::Token;
use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::js::skip_parens;
use crate::utils::tpl_core::{BODY, for_each_root, token_index, visit_root};

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(NoSparseArrays))
}

struct NoSparseArrays;

struct V<'r, 'c, 'f, 'a> {
    ctx: &'c mut Ctx<'f, 'a>,
    tokens: &'r [Token],
}

impl<'b> Visit<'b> for V<'_, '_, '_, '_> {
    crate::skip_ts_types!('b);
    fn visit_array_expression(&mut self, a: &ArrayExpression<'b>) {
        walk::walk_array_expression(self, a);
        let hole = |e: &ArrayExpressionElement| matches!(e, ArrayExpressionElement::Elision(_));
        if !a.elements.iter().any(hole) {
            return;
        }
        let n = a.elements.len();
        // Position after which the next comma is searched (first token = `[`).
        let mut from = a.span.start + 1;
        for (i, el) in a.elements.iter().enumerate() {
            if i == n - 1 && !hole(el) {
                return;
            }
            if !hole(el) {
                from = match el.as_expression() {
                    Some(x) => skip_parens(x).span().end,
                    None => el.span().end,
                };
            }
            let start = token_index(self.tokens, from);
            let Some(comma) = self.tokens[start..]
                .iter()
                .find(|t| t.kind == crate::ast::TokenKind::Punctuator && t.value == ",")
            else {
                return;
            };
            from = comma.span.end;
            if hole(el) {
                self.ctx
                    .report_id(comma.span, "unexpectedSparseArray", "Unexpected comma in middle of array.");
            }
        }
    }
}

impl Rule for NoSparseArrays {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        for_each_root(ctx, BODY, |ctx, root| {
            visit_root(&mut V { ctx, tokens: root.tokens }, root.expr);
        });
    }
}
