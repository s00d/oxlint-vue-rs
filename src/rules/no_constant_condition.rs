//! vue/no-constant-condition (core `no-constant-condition` in `<template>`, plus `v-if` /
//! `v-else-if` / `v-show` values as conditions)

use oxc::ast::ast::*;
use oxc::ast_visit::{Visit, walk};
use oxc::span::GetSpan;

use crate::ast::{AttributeValue, VExpression};
use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::js::skip_parens;
use crate::utils::template::directive_key;
use crate::utils::tpl_core::{BODY, for_each_root, visit_root};
use crate::utils::tpl_valid::body_elements;

#[derive(Clone, Copy, PartialEq)]
enum Loops {
    All,
    AllExceptWhileTrue,
    None,
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let loops = match opts.get(0).get("checkLoops") {
        Some(serde_json::Value::Bool(true)) => Loops::All,
        Some(serde_json::Value::Bool(false)) => Loops::None,
        Some(serde_json::Value::String(s)) if s == "all" => Loops::All,
        Some(serde_json::Value::String(s)) if s == "none" => Loops::None,
        _ => Loops::AllExceptWhileTrue,
    };
    Some(Box::new(NoConstantCondition { loops }))
}

struct NoConstantCondition {
    loops: Loops,
}

/// `getBooleanValue` of a literal.
fn literal_bool(e: &Expression) -> Option<bool> {
    Some(match e {
        Expression::BooleanLiteral(b) => b.value,
        Expression::NullLiteral(_) => false,
        Expression::NumericLiteral(n) => n.value != 0.0 && !n.value.is_nan(),
        Expression::StringLiteral(s) => !s.value.is_empty(),
        Expression::BigIntLiteral(b) => !b.value.trim_start_matches('0').is_empty(),
        Expression::RegExpLiteral(_) => true,
        _ => return None,
    })
}

fn is_literal(e: &Expression) -> bool {
    literal_bool(e).is_some()
}

fn logical_identity(e: &Expression, op: &str) -> bool {
    match skip_parens(e) {
        e if is_literal(e) => {
            let v = literal_bool(e);
            (op == "||" && v == Some(true)) || (op == "&&" && v == Some(false))
        }
        Expression::UnaryExpression(u) => op == "&&" && u.operator == UnaryOperator::Void,
        Expression::LogicalExpression(l) => {
            op == l.operator.as_str()
                && (logical_identity(&l.left, op) || logical_identity(&l.right, op))
        }
        Expression::AssignmentExpression(a) => {
            matches!(
                a.operator,
                AssignmentOperator::LogicalOr | AssignmentOperator::LogicalAnd
            ) && op == &a.operator.as_str()[..2]
                && logical_identity(&a.right, op)
        }
        _ => false,
    }
}

/// `astUtils.isConstant`. Template identifiers never resolve to global variables in
/// eslint-plugin-vue's scope, so `undefined` / `Boolean(…)` are not constant.
fn is_constant(e: &Expression, bool_pos: bool) -> bool {
    let e = skip_parens(e);
    if is_literal(e) {
        return true;
    }
    match e {
        Expression::ArrowFunctionExpression(_)
        | Expression::FunctionExpression(_)
        | Expression::ClassExpression(_)
        | Expression::ObjectExpression(_) => true,
        Expression::TemplateLiteral(t) => {
            (bool_pos
                && t.quasis
                    .iter()
                    .any(|q| q.value.cooked.as_ref().is_some_and(|c| !c.is_empty())))
                || t.expressions.iter().all(|x| is_constant(x, false))
        }
        Expression::ArrayExpression(a) => {
            bool_pos
                || a.elements.iter().all(|el| match el {
                    ArrayExpressionElement::Elision(_) => true,
                    ArrayExpressionElement::SpreadElement(s) => is_constant(&s.argument, false),
                    el => el.as_expression().is_some_and(|x| is_constant(x, false)),
                })
        }
        Expression::UnaryExpression(u) => match u.operator {
            UnaryOperator::Void => true,
            UnaryOperator::Typeof if bool_pos => true,
            UnaryOperator::LogicalNot => is_constant(&u.argument, true),
            _ => is_constant(&u.argument, false),
        },
        Expression::BinaryExpression(b) => {
            is_constant(&b.left, false)
                && is_constant(&b.right, false)
                && b.operator != BinaryOperator::In
        }
        Expression::LogicalExpression(l) => {
            let op = l.operator.as_str();
            let lc = is_constant(&l.left, bool_pos);
            let rc = is_constant(&l.right, bool_pos);
            (lc && rc)
                || (lc && logical_identity(&l.left, op))
                || (bool_pos && rc && logical_identity(&l.right, op))
        }
        Expression::NewExpression(_) => bool_pos,
        Expression::AssignmentExpression(a) => match a.operator {
            AssignmentOperator::Assign => is_constant(&a.right, bool_pos),
            AssignmentOperator::LogicalOr | AssignmentOperator::LogicalAnd if bool_pos => {
                logical_identity(&a.right, &a.operator.as_str()[..2])
            }
            _ => false,
        },
        Expression::SequenceExpression(s) => s
            .expressions
            .last()
            .is_some_and(|x| is_constant(x, bool_pos)),
        _ => false,
    }
}

const MSG: &str = "Unexpected constant condition.";

struct V<'r, 'c, 'f, 'a> {
    rule: &'r NoConstantCondition,
    ctx: &'c mut Ctx<'f, 'a>,
}

impl V<'_, '_, '_, '_> {
    fn check(&mut self, test: &Expression) {
        if is_constant(test, true) {
            self.ctx
                .report_id(skip_parens(test).span(), "unexpected", MSG);
        }
    }
    fn check_loop(&mut self, test: Option<&Expression>) {
        if self.rule.loops != Loops::None
            && let Some(t) = test
        {
            self.check(t);
        }
    }
}

// ponytail: loop conditions are reported without the core's `yield` exemption (generators in
// template handlers are not a real case).
impl<'b> Visit<'b> for V<'_, '_, '_, '_> {
    crate::skip_ts_types!('b);
    fn visit_conditional_expression(&mut self, it: &ConditionalExpression<'b>) {
        self.check(&it.test);
        walk::walk_conditional_expression(self, it);
    }
    fn visit_if_statement(&mut self, it: &IfStatement<'b>) {
        self.check(&it.test);
        walk::walk_if_statement(self, it);
    }
    fn visit_while_statement(&mut self, it: &WhileStatement<'b>) {
        let while_true = matches!(&it.test, Expression::BooleanLiteral(b) if b.value);
        if !(while_true && self.rule.loops == Loops::AllExceptWhileTrue) {
            self.check_loop(Some(&it.test));
        }
        walk::walk_while_statement(self, it);
    }
    fn visit_do_while_statement(&mut self, it: &DoWhileStatement<'b>) {
        self.check_loop(Some(&it.test));
        walk::walk_do_while_statement(self, it);
    }
    fn visit_for_statement(&mut self, it: &ForStatement<'b>) {
        self.check_loop(it.test.as_ref());
        walk::walk_for_statement(self, it);
    }
}

impl Rule for NoConstantCondition {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        for_each_root(ctx, BODY, |ctx, root| {
            visit_root(&mut V { rule: self, ctx }, root.expr);
        });
        let Some(doc) = ctx.doc() else { return };
        for e in body_elements(doc) {
            for a in e.attributes() {
                let Some(k) = directive_key(a) else { continue };
                if !matches!(k.name.name.as_str(), "if" | "else-if" | "show") {
                    continue;
                }
                let Some(AttributeValue::Expr(id)) = &a.value else {
                    continue;
                };
                if let Some(VExpression::Expression(x)) = &doc.expr(*id).expression {
                    V { rule: self, ctx }.check(x);
                }
            }
        }
    }
}
