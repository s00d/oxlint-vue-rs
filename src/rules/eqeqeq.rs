//! vue/eqeqeq (core `eqeqeq` in `<template>` and `<style>` `v-bind()`)

use oxc::ast::ast::{BinaryExpression, BinaryOperator, Expression, UnaryOperator};
use oxc::ast_visit::{Visit, walk};
use oxc::span::GetSpan;

use crate::rule::{Ctx, Fix, Rule, RuleOptions, Suggestion};
use crate::utils::js::skip_parens;
use crate::utils::tpl_core::{DOCUMENT, first_token_between, for_each_root, visit_root};

#[derive(Clone, Copy, PartialEq)]
enum Null {
    Always,
    Never,
    Ignore,
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let config = opts.get(0).as_str().unwrap_or("always");
    let null = if config == "always" {
        match opts.get(1).get("null").and_then(|v| v.as_str()) {
            Some("never") => Null::Never,
            Some("ignore") => Null::Ignore,
            _ => Null::Always,
        }
    } else {
        Null::Ignore
    };
    Some(Box::new(Eqeqeq {
        smart: config == "smart",
        null,
    }))
}

struct Eqeqeq {
    smart: bool,
    null: Null,
}

fn literal_type(e: &Expression) -> Option<&'static str> {
    Some(match skip_parens(e) {
        Expression::StringLiteral(_) => "string",
        Expression::NumericLiteral(_) => "number",
        Expression::BooleanLiteral(_) => "boolean",
        Expression::BigIntLiteral(_) => "bigint",
        Expression::NullLiteral(_) | Expression::RegExpLiteral(_) => "object",
        Expression::TemplateLiteral(t) if t.expressions.is_empty() => "string",
        _ => return None,
    })
}

fn is_typeof(e: &Expression) -> bool {
    matches!(skip_parens(e), Expression::UnaryExpression(u) if u.operator == UnaryOperator::Typeof)
}

struct V<'r, 'c, 'f, 'a> {
    rule: &'r Eqeqeq,
    ctx: &'c mut Ctx<'f, 'a>,
    tokens: &'r [crate::ast::Token],
}

impl V<'_, '_, '_, '_> {
    fn report(&mut self, b: &BinaryExpression, expected: &str) {
        let actual = b.operator.as_str();
        let from = skip_parens(&b.left).span().end;
        let to = skip_parens(&b.right).span().start;
        let Some(op) = first_token_between(self.tokens, from, to, |t| t.value == actual) else {
            return;
        };
        let msg = format!("Expected '{expected}' and instead saw '{actual}'.");
        let fix = Fix::replace(op.span, expected);
        let fixable = is_typeof(&b.left)
            || is_typeof(&b.right)
            || literal_type(&b.left).is_some_and(|t| Some(t) == literal_type(&b.right));
        if fixable {
            self.ctx.report_fix(op.span, Some("unexpected"), msg, Some(fix));
        } else {
            let s = Suggestion {
                message_id: Some("replaceOperator"),
                desc: format!("Use '{expected}' instead of '{actual}'."),
                fix,
            };
            self.ctx.push(op.span, Some("unexpected"), msg, None, vec![s]);
        }
    }
}

impl<'b> Visit<'b> for V<'_, '_, '_, '_> {
    crate::skip_ts_types!('b);
    fn visit_binary_expression(&mut self, b: &BinaryExpression<'b>) {
        walk::walk_binary_expression(self, b);
        let is_null = matches!(skip_parens(&b.left), Expression::NullLiteral(_))
            || matches!(skip_parens(&b.right), Expression::NullLiteral(_));
        match b.operator {
            BinaryOperator::Equality | BinaryOperator::Inequality => {}
            BinaryOperator::StrictEquality | BinaryOperator::StrictInequality => {
                if self.rule.null == Null::Never && is_null {
                    let op = b.operator.as_str();
                    self.report(b, &op[..op.len() - 1]);
                }
                return;
            }
            _ => return,
        }
        if self.rule.smart
            && (is_typeof(&b.left)
                || is_typeof(&b.right)
                || literal_type(&b.left).is_some_and(|t| Some(t) == literal_type(&b.right))
                || is_null)
        {
            return;
        }
        if self.rule.null != Null::Always && is_null {
            return;
        }
        let expected = format!("{}=", b.operator.as_str());
        self.report(b, &expected);
    }
}

impl Rule for Eqeqeq {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        for_each_root(ctx, DOCUMENT, |ctx, root| {
            let mut v = V {
                rule: self,
                ctx,
                tokens: root.tokens,
            };
            visit_root(&mut v, root.expr);
        });
    }
}
