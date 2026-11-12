//! vue/no-implicit-coercion (core `no-implicit-coercion` in `<template>`)

use oxc::ast::ast::*;
use oxc::ast_visit::{Visit, walk};
use oxc::span::{GetSpan, Span};

use crate::ast::Token;
use crate::rule::{Ctx, Fix, Rule, RuleOptions, Suggestion};
use crate::utils::js::{member_name, skip_parens};
use crate::utils::tpl_core::{DOCUMENT, for_each_root, token_before, visit_root};

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let o = opts.get(0);
    let flag = |k: &str, d: bool| o.get(k).and_then(|v| v.as_bool()).unwrap_or(d);
    let allow = o
        .get("allow")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    Some(Box::new(NoImplicitCoercion {
        boolean: flag("boolean", true),
        number: flag("number", true),
        string: flag("string", true),
        template: flag("disallowTemplateShorthand", false),
        allow,
    }))
}

struct NoImplicitCoercion {
    boolean: bool,
    number: bool,
    string: bool,
    template: bool,
    allow: Vec<String>,
}

impl NoImplicitCoercion {
    fn allowed(&self, op: &str) -> bool {
        self.allow.iter().any(|a| a == op)
    }
}

fn is_number_lit(e: &Expression, v: Option<f64>) -> bool {
    matches!(skip_parens(e), Expression::NumericLiteral(n) if v.is_none_or(|v| n.value == v))
}

fn is_numeric(e: &Expression) -> bool {
    match skip_parens(e) {
        Expression::NumericLiteral(_) => true,
        Expression::CallExpression(c) => matches!(
            skip_parens(&c.callee),
            Expression::Identifier(id) if matches!(id.name.as_str(), "Number" | "parseInt" | "parseFloat")
        ),
        _ => false,
    }
}

fn is_binary(e: &Expression) -> bool {
    matches!(skip_parens(e), Expression::BinaryExpression(_))
}

fn is_string_lit(e: &Expression) -> bool {
    matches!(
        skip_parens(e),
        Expression::StringLiteral(_) | Expression::TemplateLiteral(_)
    )
}

fn is_string_type(e: &Expression) -> bool {
    is_string_lit(e)
        || matches!(skip_parens(e), Expression::CallExpression(c)
            if matches!(&c.callee, Expression::Identifier(id) if id.name == "String"))
}

fn is_empty_string(e: &Expression) -> bool {
    match skip_parens(e) {
        Expression::StringLiteral(s) => s.value.is_empty(),
        Expression::TemplateLiteral(t) => {
            t.quasis.len() == 1 && t.quasis[0].value.cooked.as_ref().is_some_and(|c| c.is_empty())
        }
        _ => false,
    }
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

struct V<'r, 'c, 'f, 'a> {
    rule: &'r NoImplicitCoercion,
    ctx: &'c mut Ctx<'f, 'a>,
    tokens: &'r [Token],
    /// `x * 1` directly on the left of `/` (`isMultiplyByFractionOfOne`).
    fraction: Vec<Span>,
    tagged: Vec<Span>,
}

impl V<'_, '_, '_, '_> {
    fn text(&self, e: &Expression) -> String {
        let e = skip_parens(e);
        let t = self.ctx.slice(e.span());
        if matches!(e, Expression::SequenceExpression(_)) {
            format!("({t})")
        } else {
            t.to_string()
        }
    }
    fn report(&mut self, span: Span, rec: String, suggest: bool, fix: bool) {
        // ponytail: `canTokensBeAdjacent` approximated by identifier-character adjacency.
        let mut text = rec.clone();
        if let Some(prev) = token_before(self.tokens, span.start)
            && prev.span.end == span.start
            && prev.value.chars().last().is_some_and(is_ident_char)
            && rec.chars().next().is_some_and(is_ident_char)
        {
            text = format!(" {rec}");
        }
        let edit = Fix::replace(span, text);
        let msg = format!("Unexpected implicit coercion encountered. Use `{rec}` instead.");
        let sugg = if !fix && suggest {
            vec![Suggestion {
                message_id: Some("useRecommendation"),
                desc: format!("Use `{rec}` instead."),
                fix: edit.clone(),
            }]
        } else {
            Vec::new()
        };
        self.ctx.push(
            span,
            Some("implicitCoercion"),
            msg,
            fix.then_some(edit),
            sugg,
        );
    }
}

impl<'b> Visit<'b> for V<'_, '_, '_, '_> {
    crate::skip_ts_types!('b);
    fn visit_unary_expression(&mut self, u: &UnaryExpression<'b>) {
        walk::walk_unary_expression(self, u);
        let r = self.rule;
        let arg = skip_parens(&u.argument);
        let inner_unary = match arg {
            Expression::UnaryExpression(i) => Some(i),
            _ => None,
        };
        if !r.allowed("!!")
            && r.boolean
            && u.operator == UnaryOperator::LogicalNot
            && let Some(i) = inner_unary.filter(|i| i.operator == UnaryOperator::LogicalNot)
        {
            let rec = format!("Boolean({})", self.text(&i.argument));
            self.report(u.span, rec, true, true);
        }
        if !r.allowed("~") && r.boolean && u.operator == UnaryOperator::BitwiseNot {
            let (call, chain) = match arg {
                Expression::CallExpression(c) => (Some(&**c), false),
                Expression::ChainExpression(ch) => match &ch.expression {
                    ChainElement::CallExpression(c) => (Some(&**c), true),
                    _ => (None, false),
                },
                _ => (None, false),
            };
            let index_of = call.is_some_and(|c| {
                skip_parens(&c.callee)
                    .as_member_expression()
                    .and_then(member_name)
                    .is_some_and(|n| n == "indexOf" || n == "lastIndexOf")
            });
            if index_of {
                let cmp = if chain { ">= 0" } else { "!== -1" };
                let rec = format!("{} {cmp}", self.ctx.slice(arg.span()));
                self.report(u.span, rec, false, false);
            }
        }
        if !r.allowed("+") && r.number && u.operator == UnaryOperator::UnaryPlus && !is_numeric(arg)
        {
            let rec = format!("Number({})", self.text(arg));
            self.report(u.span, rec, true, false);
        }
        if !r.allowed("- -")
            && r.number
            && u.operator == UnaryOperator::UnaryNegation
            && let Some(i) = inner_unary.filter(|i| i.operator == UnaryOperator::UnaryNegation)
            && !is_numeric(&i.argument)
        {
            let rec = format!("Number({})", self.text(&i.argument));
            self.report(u.span, rec, true, false);
        }
    }

    fn visit_binary_expression(&mut self, b: &BinaryExpression<'b>) {
        if b.operator == BinaryOperator::Division
            && let Expression::BinaryExpression(l) = &b.left
            && l.operator == BinaryOperator::Multiplication
        {
            self.fraction.push(l.span);
        }
        walk::walk_binary_expression(self, b);
        let r = self.rule;
        if !r.allowed("*")
            && r.number
            && b.operator == BinaryOperator::Multiplication
            && (is_number_lit(&b.left, Some(1.0)) || is_number_lit(&b.right, Some(1.0)))
            && !(is_number_lit(&b.right, Some(1.0)) && self.fraction.contains(&b.span))
        {
            let operand = if !is_binary(&b.right) && !is_numeric(&b.right) {
                Some(&b.right)
            } else if !is_binary(&b.left) && !is_numeric(&b.left) {
                Some(&b.left)
            } else {
                None
            };
            if let Some(o) = operand {
                let rec = format!("Number({})", self.text(o));
                self.report(b.span, rec, true, false);
            }
        }
        if !r.allowed("-")
            && r.number
            && b.operator == BinaryOperator::Subtraction
            && is_number_lit(&b.right, Some(0.0))
            && !is_numeric(&b.left)
        {
            let rec = format!("Number({})", self.text(&b.left));
            self.report(b.span, rec, true, false);
        }
        if !r.allowed("+")
            && r.string
            && b.operator == BinaryOperator::Addition
            && ((is_empty_string(&b.left) && !is_string_type(&b.right))
                || (is_empty_string(&b.right) && !is_string_type(&b.left)))
        {
            let o = if is_empty_string(&b.left) { &b.right } else { &b.left };
            let rec = format!("String({})", self.text(o));
            self.report(b.span, rec, true, false);
        }
    }

    fn visit_assignment_expression(&mut self, a: &AssignmentExpression<'b>) {
        walk::walk_assignment_expression(self, a);
        let r = self.rule;
        if !r.allowed("+")
            && r.string
            && a.operator == AssignmentOperator::Addition
            && is_empty_string(&a.right)
        {
            let code = self.ctx.slice(a.left.span());
            let rec = format!("{code} = String({code})");
            self.report(a.span, rec, true, false);
        }
    }

    fn visit_tagged_template_expression(&mut self, t: &TaggedTemplateExpression<'b>) {
        self.tagged.push(t.quasi.span);
        walk::walk_tagged_template_expression(self, t);
    }

    fn visit_template_literal(&mut self, t: &TemplateLiteral<'b>) {
        walk::walk_template_literal(self, t);
        if !self.rule.template || self.tagged.contains(&t.span) || t.expressions.len() != 1 {
            return;
        }
        let empty = |i: usize| t.quasis[i].value.cooked.as_ref().is_some_and(|c| c.is_empty());
        if !empty(0) || !empty(1) || is_string_type(&t.expressions[0]) {
            return;
        }
        let rec = format!("String({})", self.text(&t.expressions[0]));
        self.report(t.span, rec, true, false);
    }
}

impl Rule for NoImplicitCoercion {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        for_each_root(ctx, DOCUMENT, |ctx, root| {
            let mut v = V {
                rule: self,
                ctx,
                tokens: root.tokens,
                fraction: Vec::new(),
                tagged: Vec::new(),
            };
            visit_root(&mut v, root.expr);
        });
    }
}
