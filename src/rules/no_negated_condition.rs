//! vue/no-negated-condition (core `no-negated-condition` in `<template>`)

use oxc::ast::ast::{BinaryOperator, ConditionalExpression, Expression, IfStatement, Statement, UnaryOperator};
use oxc::ast_visit::{Visit, walk};

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::js::skip_parens;
use crate::utils::tpl_core::{DOCUMENT, for_each_root, visit_root};

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(NoNegatedCondition))
}

struct NoNegatedCondition;

fn negated(test: &Expression) -> bool {
    match skip_parens(test) {
        Expression::UnaryExpression(u) => u.operator == UnaryOperator::LogicalNot,
        Expression::BinaryExpression(b) => matches!(
            b.operator,
            BinaryOperator::Inequality | BinaryOperator::StrictInequality
        ),
        _ => false,
    }
}

struct V<'c, 'f, 'a>(&'c mut Ctx<'f, 'a>);

const MSG: &str = "Unexpected negated condition.";

impl<'b> Visit<'b> for V<'_, '_, '_> {
    crate::skip_ts_types!('b);
    fn visit_if_statement(&mut self, it: &IfStatement<'b>) {
        walk::walk_if_statement(self, it);
        let else_plain = it
            .alternate
            .as_ref()
            .is_some_and(|a| !matches!(a, Statement::IfStatement(_)));
        if else_plain && negated(&it.test) {
            self.0.report_id(it.span, "unexpectedNegated", MSG);
        }
    }
    fn visit_conditional_expression(&mut self, it: &ConditionalExpression<'b>) {
        walk::walk_conditional_expression(self, it);
        if negated(&it.test) {
            self.0.report_id(it.span, "unexpectedNegated", MSG);
        }
    }
}

impl Rule for NoNegatedCondition {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        for_each_root(ctx, DOCUMENT, |ctx, root| visit_root(&mut V(ctx), root.expr));
    }
}
