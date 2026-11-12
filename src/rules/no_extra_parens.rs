//! vue/no-extra-parens (`@stylistic/no-extra-parens` in `<template>` and `<style>` `v-bind()`,
//! plus the Vue check of parenthesized mustache / `v-bind` values; dynamic arguments skipped)

use std::collections::HashSet;

use oxc::ast::ast::*;
use oxc::ast_visit::{Visit, walk};
use oxc::semantic::ScopeFlags;
use oxc::span::{GetSpan, Span};
use crate::utils::regexp::JsRegex as Regex;

use crate::ast::{ExprParent, Token, TokenKind, VExpression};
use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::js::skip_parens;
use crate::utils::tpl_core::{Root, Scope, for_each_root, visit_root};

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let all = opts.get(0).as_str() != Some("functions");
    let o = opts.get(1);
    let off = |k: &str| all && o.get(k).and_then(|v| v.as_bool()) == Some(false);
    let spread = o
        .get("allowNodesInSpreadElement")
        .and_then(|v| v.as_object())
        .map(|m| {
            m.iter()
                .filter(|(_, v)| v.as_bool() == Some(true))
                .map(|(k, _)| k.clone())
                .collect()
        })
        .unwrap_or_default();
    Some(Box::new(NoExtraParens {
        all,
        except_cond_assign: off("conditionalAssign"),
        except_cond_ternary: off("ternaryOperandBinaryExpressions"),
        ignore_nested_binary: off("nestedBinaryExpressions"),
        except_return_assign: off("returnAssign"),
        ignore_arrow_conditionals: off("enforceForArrowConditionals"),
        ignore_sequence: off("enforceForSequenceExpressions"),
        ignore_new_in_member: off("enforceForNewInMemberExpressions"),
        ignore_fn_proto: off("enforceForFunctionPrototypeMethods"),
        allow_nested_ternary: off("nestedConditionalExpressions"),
        comment_pattern: o
            .get("allowParensAfterCommentPattern")
            .and_then(|v| v.as_str())
            .filter(|_| all)
            .and_then(|s| Regex::with_flags(s, "u").ok()),
        allow_in_spread: if all { spread } else { Vec::new() },
    }))
}

// ponytail: `ignoredNodes` selectors, `ignoreJSX`, the `for (… in …;;)` init buffering and
// parens around TS type nodes are not ported (none occur in template expressions).
struct NoExtraParens {
    all: bool,
    except_cond_assign: bool,
    except_cond_ternary: bool,
    ignore_nested_binary: bool,
    except_return_assign: bool,
    ignore_arrow_conditionals: bool,
    ignore_sequence: bool,
    ignore_new_in_member: bool,
    ignore_fn_proto: bool,
    allow_nested_ternary: bool,
    comment_pattern: Option<Regex>,
    allow_in_spread: Vec<String>,
}

/// Inner expression and the number of `ParenthesizedExpression` layers around it.
fn unparen<'x, 'b>(mut e: &'x Expression<'b>) -> (&'x Expression<'b>, usize) {
    let mut n = 0;
    while let Expression::ParenthesizedExpression(p) = e {
        e = &p.expression;
        n += 1;
    }
    (e, n)
}

fn layers(e: &Expression) -> usize {
    unparen(e).1
}

fn is_type_assertion(e: &Expression) -> bool {
    matches!(
        unparen(e).0,
        Expression::TSAsExpression(_)
            | Expression::TSNonNullExpression(_)
            | Expression::TSSatisfiesExpression(_)
            | Expression::TSTypeAssertion(_)
    )
}

/// `getPrecedence` (ESTree node types; TS expression nodes are not in eslint-visitor-keys).
fn precedence(e: &Expression) -> i32 {
    match unparen(e).0 {
        Expression::SequenceExpression(_) => 0,
        Expression::AssignmentExpression(_)
        | Expression::ArrowFunctionExpression(_)
        | Expression::YieldExpression(_) => 1,
        Expression::ConditionalExpression(_) => 3,
        Expression::LogicalExpression(l) => match l.operator {
            LogicalOperator::Or | LogicalOperator::Coalesce => 4,
            LogicalOperator::And => 5,
        },
        Expression::BinaryExpression(b) => binary_precedence(b.operator),
        Expression::PrivateInExpression(_) => 10,
        Expression::UnaryExpression(_) | Expression::AwaitExpression(_) => 16,
        Expression::UpdateExpression(_) => 17,
        Expression::CallExpression(_)
        | Expression::ChainExpression(_)
        | Expression::ImportExpression(_) => 18,
        Expression::NewExpression(_) => 19,
        Expression::TSAsExpression(_)
        | Expression::TSSatisfiesExpression(_)
        | Expression::TSNonNullExpression(_)
        | Expression::TSTypeAssertion(_)
        | Expression::TSInstantiationExpression(_) => -1,
        _ => 20,
    }
}

fn binary_precedence(op: BinaryOperator) -> i32 {
    use BinaryOperator::*;
    match op {
        BitwiseOR => 6,
        BitwiseXOR => 7,
        BitwiseAnd => 8,
        Equality | Inequality | StrictEquality | StrictInequality => 9,
        LessThan | LessEqualThan | GreaterThan | GreaterEqualThan | In | Instanceof => 10,
        ShiftLeft | ShiftRight | ShiftRightZeroFill => 11,
        Addition | Subtraction => 12,
        Multiplication | Division | Remainder => 13,
        Exponential => 15,
    }
}

fn skip_chain<'x, 'b>(e: &'x Expression<'b>) -> Option<&'x CallExpression<'b>> {
    match unparen(e).0 {
        Expression::CallExpression(c) => Some(c),
        Expression::ChainExpression(ch) => match &ch.expression {
            ChainElement::CallExpression(c) => Some(c),
            _ => None,
        },
        _ => None,
    }
}

fn is_iife(e: &Expression) -> bool {
    skip_chain(e).is_some_and(|c| {
        matches!(skip_parens(&c.callee), Expression::FunctionExpression(_))
    })
}

/// `isImmediateFunctionPrototypeMethodCall`.
fn is_fn_proto_call(e: &Expression) -> bool {
    let Some(c) = skip_chain(e) else { return false };
    let callee = match skip_parens(&c.callee) {
        Expression::ChainExpression(ch) => ch.expression.as_member_expression(),
        e => e.as_member_expression(),
    };
    callee.is_some_and(|m| {
        matches!(skip_parens(m.object()), Expression::FunctionExpression(_))
            && crate::utils::js::member_name(m).is_some_and(|n| n == "call" || n == "apply")
    })
}

/// `doesMemberExpressionContainCallExpression` (ESTree objects skip parentheses).
fn member_contains_call(object: &Expression) -> bool {
    let mut cur = skip_parens(object);
    loop {
        match cur.as_member_expression() {
            Some(m) => cur = skip_parens(m.object()),
            None => return matches!(cur, Expression::CallExpression(_)),
        }
    }
}

fn is_logical(e: &Expression) -> bool {
    matches!(unparen(e).0, Expression::LogicalExpression(l) if l.operator != LogicalOperator::Coalesce)
}
fn is_coalesce(e: &Expression) -> bool {
    matches!(unparen(e).0, Expression::LogicalExpression(l) if l.operator == LogicalOperator::Coalesce)
}
fn is_assign(e: &Expression) -> bool {
    matches!(skip_parens(e), Expression::AssignmentExpression(_))
}

fn contains_assignment(e: &Expression) -> bool {
    match skip_parens(e) {
        Expression::AssignmentExpression(_) => true,
        Expression::ConditionalExpression(c) => is_assign(&c.consequent) || is_assign(&c.alternate),
        Expression::BinaryExpression(b) => is_assign(&b.left) || is_assign(&b.right),
        Expression::LogicalExpression(b) => is_assign(&b.left) || is_assign(&b.right),
        _ => false,
    }
}

/// ESLint `canTokensBeAdjacent` for two tokens.
fn can_be_adjacent(l: &Token, r: &Token) -> bool {
    use TokenKind::*;
    if l.kind == Punctuator || r.kind == Punctuator {
        if l.kind == Punctuator && r.kind == Punctuator {
            let plus = |v: &str| v == "+" || v == "++";
            let minus = |v: &str| v == "-" || v == "--";
            return !((plus(&l.value) && plus(&r.value)) || (minus(&l.value) && minus(&r.value)));
        }
        if l.kind == Punctuator
            && l.value == "/"
            && matches!(r.kind, BlockComment | LineComment | RegularExpression)
        {
            return false;
        }
        return true;
    }
    if matches!(l.kind, String | Template) || matches!(r.kind, String | Template) {
        return true;
    }
    if l.kind != Numeric && r.kind == Numeric && r.value.starts_with('.') {
        return true;
    }
    if l.kind == BlockComment || r.kind == BlockComment || r.kind == LineComment {
        return true;
    }
    r.kind == PrivateIdentifier
}

/// `/@type\s*\{[^}]+\}/`.
fn is_type_tag(s: &str) -> bool {
    let mut rest = s;
    while let Some(i) = rest.find("@type") {
        rest = &rest[i + 5..];
        if let Some(body) = rest.trim_start().strip_prefix('{')
            && body.find('}').is_some_and(|e| e > 0)
        {
            return true;
        }
    }
    false
}

const MSG: &str = "Unnecessary parentheses around expression.";

struct V<'r, 'c, 'f, 'a> {
    rule: &'r NoExtraParens,
    ctx: &'c mut Ctx<'f, 'a>,
    /// Code tokens of the container (no HTML whitespace).
    tokens: Vec<&'r Token>,
    comments: &'r [Token],
    ignore_tokens: HashSet<u32>,
    /// Member expressions in a `new` callee chain (`isMemberExpInNewCallee`).
    in_new_callee: HashSet<Span>,
    /// Member callees of `function(){}.call()` (`isImmediateFunctionPrototypeMethodCall(parent)`).
    proto_callees: HashSet<Span>,
    /// Inside a return statement / expression-bodied arrow (`isInReturnStatement`).
    in_return: usize,
    /// Expressions of expression statements directly in a function body.
    top_level_stmts: HashSet<Span>,
    /// Unparenthesized return argument and the line of `return`.
    return_arg: Option<(Span, usize)>,
}

impl V<'_, '_, '_, '_> {
    fn idx(&self, pos: u32) -> usize {
        self.tokens.partition_point(|t| t.span.start < pos)
    }
    fn before(&self, pos: u32) -> Option<&Token> {
        let i = self.tokens.partition_point(|t| t.span.end <= pos);
        i.checked_sub(1).map(|i| self.tokens[i])
    }
    fn after(&self, pos: u32) -> Option<&Token> {
        self.tokens.get(self.idx(pos)).copied()
    }
    fn src(&self) -> &str {
        self.ctx.source()
    }
    fn line(&self, pos: u32) -> usize {
        self.ctx.file.lines.line_of(pos)
    }

    fn applies(&self, e: &Expression) -> bool {
        let n = unparen(e).0;
        if matches!(n, Expression::SequenceExpression(_)) && self.rule.ignore_sequence {
            return false;
        }
        if self.rule.ignore_fn_proto && is_fn_proto_call(n) {
            return false;
        }
        self.rule.all
            || matches!(
                n,
                Expression::FunctionExpression(_) | Expression::ArrowFunctionExpression(_)
            )
    }
    fn excess(&self, e: &Expression) -> bool {
        layers(e) >= 1 && self.applies(e)
    }
    fn double(&self, e: &Expression) -> bool {
        layers(e) >= 2 && self.applies(e)
    }
    fn excess_prec(&self, e: &Expression, limit: i32) -> bool {
        self.excess_prec_as(e, precedence(e), limit)
    }
    /// `hasExcessParensWithPrecedence` with the precedence of a replaced node type.
    fn excess_prec_as(&self, e: &Expression, prec: i32, limit: i32) -> bool {
        self.excess(e) && (prec >= limit || layers(e) >= 2)
    }
    /// Type assertions are checked as `SequenceExpression`s (precedence 0).
    fn excess_prec_ta(&self, e: &Expression, limit: i32) -> bool {
        let prec = if is_type_assertion(e) { 0 } else { precedence(e) };
        self.excess_prec_as(e, prec, limit)
    }

    fn report(&mut self, e: &Expression) {
        let (n, k) = unparen(e);
        let span = n.span();
        if k < 2 {
            if self
                .after(span.start)
                .is_some_and(|t| self.ignore_tokens.contains(&t.span.start))
            {
                return;
            }
            if is_iife(n) && !skip_chain(n).is_some_and(|c| layers(&c.callee) >= 1) {
                return;
            }
        }
        self.report_span(span, k, matches!(n, Expression::StringLiteral(_)));
    }

    fn report_span(&mut self, span: Span, k: usize, string: bool) {
        let (Some(lp), Some(rp)) = (self.before(span.start), self.after(span.end)) else {
            return;
        };
        let (mut lp, mut rp) = (lp.span, rp.span);
        let mut need = 2;
        while self.allowed_by_comment(lp) {
            if k < need {
                return;
            }
            let (Some(l), Some(r)) = (self.before(lp.start), self.after(rp.end)) else {
                return;
            };
            (lp, rp) = (l.span, r.span);
            need += 1;
        }
        let fixable = !string || k >= 2 || !self.top_level_stmts.contains(&span);
        let fix = fixable
            .then(|| {
                let lead = if self.requires_leading_space(lp) { " " } else { "" };
                let trail = if self.requires_trailing_space(span, rp) { " " } else { "" };
                Fix::merge(
                    self.src(),
                    vec![Fix::replace(lp, lead), Fix::replace(rp, trail)],
                )
            })
            .flatten();
        self.ctx.report_fix(lp, Some("unexpected"), MSG, fix);
    }

    fn comment_or_token_before(&self, pos: u32) -> Option<Token> {
        let t = self.before(pos).cloned();
        let ci = self.comments.partition_point(|c| c.span.end <= pos);
        let c = ci.checked_sub(1).map(|i| self.comments[i].clone());
        match (t, c) {
            (Some(t), Some(c)) => Some(if c.span.end > t.span.end { c } else { t }),
            (t, c) => t.or(c),
        }
    }
    fn comment_or_token_after(&self, pos: u32) -> Option<Token> {
        let t = self.after(pos).cloned();
        let ci = self.comments.partition_point(|c| c.span.start < pos);
        let c = self.comments.get(ci).cloned();
        match (t, c) {
            (Some(t), Some(c)) => Some(if c.span.start < t.span.start { c } else { t }),
            (t, c) => t.or(c),
        }
    }
    fn requires_leading_space(&self, lp: Span) -> bool {
        let (Some(b), Some(a)) = (
            self.comment_or_token_before(lp.start),
            self.comment_or_token_after(lp.end),
        ) else {
            return false;
        };
        b.span.end == lp.start && lp.end == a.span.start && !can_be_adjacent(&b, &a)
    }
    fn requires_trailing_space(&self, node: Span, rp: Span) -> bool {
        let (Some(next), Some(last)) = (self.after(rp.end), self.before(node.end)) else {
            return false;
        };
        let space = self.src()[rp.end as usize..next.span.start as usize]
            .chars()
            .any(char::is_whitespace);
        !space && !can_be_adjacent(last, next)
    }
    fn allowed_by_comment(&self, lp: Span) -> bool {
        let prev = self.before(lp.start).map_or(0, |t| t.span.end);
        let ci = self.comments.partition_point(|c| c.span.end <= lp.start);
        let Some(last) = ci.checked_sub(1).map(|i| &self.comments[i]) else {
            return false;
        };
        if last.span.start < prev {
            return false;
        }
        let jsdoc = last.kind == TokenKind::BlockComment && last.value.starts_with('*');
        (jsdoc && is_type_tag(&last.value))
            || self
                .rule
                .comment_pattern
                .as_ref()
                .is_some_and(|r| r.is_match(&last.value))
    }

    /// Paren layers around a span by tokens (assignment targets keep no parens in oxc).
    fn token_layers(&self, span: Span) -> usize {
        let mut n = 0;
        let (mut s, mut e) = (span.start, span.end);
        while let (Some(l), Some(r)) = (self.before(s), self.after(e)) {
            if l.value != "(" || r.value != ")" {
                break;
            }
            n += 1;
            s = l.span.start;
            e = r.span.end;
        }
        n
    }
    fn simple_target_span(t: &SimpleAssignmentTarget) -> Option<Span> {
        match t {
            SimpleAssignmentTarget::AssignmentTargetIdentifier(_)
            | SimpleAssignmentTarget::StaticMemberExpression(_)
            | SimpleAssignmentTarget::ComputedMemberExpression(_)
            | SimpleAssignmentTarget::PrivateFieldExpression(_) => Some(t.span()),
            _ => None,
        }
    }
    fn check_target(&mut self, t: &AssignmentTarget) {
        if let Some(span) = t.as_simple_assignment_target().and_then(Self::simple_target_span)
            && self.rule.all
        {
            let k = self.token_layers(span);
            if k >= 1 {
                self.report_span(span, k, false);
            }
        }
    }
    fn check_maybe_default(&mut self, t: &AssignmentTargetMaybeDefault) {
        if let Some(t) = t.as_assignment_target() {
            self.check_target(t);
        }
    }

    fn cond_assign(&self, test: &Expression) -> bool {
        self.rule.except_cond_assign && is_assign(test)
    }

    fn check_argument(&mut self, node_prec: i32, arg: &Expression) {
        if self.excess_prec_ta(arg, node_prec) {
            self.report(arg);
        }
    }

    fn check_binary(
        &mut self,
        node: Span,
        prec: i32,
        exp: bool,
        coalesce: Option<bool>,
        left: &Expression,
        right: &Expression,
    ) {
        let (la, ra) = (is_type_assertion(left), is_type_assertion(right));
        if la && ra {
            return;
        }
        let lp = if la { 0 } else { precedence(left) };
        let rp = if ra { 0 } else { precedence(right) };
        let nested = |x: &Expression| {
            matches!(
                unparen(x).0,
                Expression::BinaryExpression(_) | Expression::LogicalExpression(_)
            )
        };
        let skip_l = self.rule.ignore_nested_binary && !la && nested(left);
        let skip_r = self.rule.ignore_nested_binary && !ra && nested(right);
        // `isMixedLogicalAndCoalesceExpressions(child, n)`.
        let mixed = |x: &Expression, ta: bool| {
            !ta && match coalesce {
                Some(true) => is_logical(x),
                Some(false) => is_coalesce(x),
                None => false,
            }
        };
        let return_split = self.return_arg.is_some_and(|(s, line)| {
            s == node && line != self.line(unparen(left).0.span().start)
        });
        if !skip_l && self.excess(left) {
            let unary = !la
                && exp
                && matches!(
                    unparen(left).0,
                    Expression::AwaitExpression(_) | Expression::UnaryExpression(_)
                );
            if (!unary && !mixed(left, la) && !return_split && (lp > prec || (lp == prec && !exp)))
                || layers(left) >= 2
            {
                self.report(left);
            }
        }
        if !skip_r
            && self.excess(right)
            && ((!mixed(right, ra) && (rp > prec || (rp == prec && exp))) || layers(right) >= 2)
        {
            self.report(right);
        }
    }

    fn check_call_new(
        &mut self,
        is_new: bool,
        iife: bool,
        node_parens: bool,
        callee: &Expression,
        args: &[Argument],
        optional: bool,
    ) {
        let node_prec = if is_new { 19 } else { 18 };
        if self.excess_prec_ta(callee, node_prec) {
            let c = unparen(callee).0;
            let ta = is_type_assertion(callee);
            let exempt = iife
                || (!ta
                    && matches!(c, Expression::NewExpression(_))
                    && !self.new_with_parens(c)
                    && !(is_new && !node_parens))
                || (!ta && is_new && c.is_member_expression() && member_contains_call_m(c))
                || (!ta && !optional && matches!(c, Expression::ChainExpression(_)));
            if self.double(callee) || !exempt {
                self.report(callee);
            }
        }
        for a in args {
            if let Some(e) = a.as_expression()
                && self.excess_prec(e, 1)
            {
                self.report(e);
            }
        }
    }

    /// `isNewExpressionWithParens`.
    fn new_with_parens(&self, e: &Expression) -> bool {
        let Expression::NewExpression(n) = e else {
            return false;
        };
        self.new_parens(n)
    }
    fn new_parens(&self, n: &NewExpression) -> bool {
        if !n.arguments.is_empty() {
            return true;
        }
        let last = self.before(n.span.end);
        let pen = last.and_then(|l| self.before(l.span.start));
        last.is_some_and(|l| l.value == ")")
            && pen.is_some_and(|p| p.value == "(")
            && skip_parens(&n.callee).span().end < n.span.end
    }

    fn check_member(&mut self, m: &MemberExpression) {
        let span = m.span();
        let object = m.object();
        let o = unparen(object).0;
        let ta = is_type_assertion(object);
        let computed = matches!(m, MemberExpression::ComputedMemberExpression(_));
        let obj_excess = if self.in_new_callee.contains(&span) && member_contains_call(object) {
            self.double(object)
        } else {
            self.excess(object) && !(self.rule.ignore_fn_proto && self.proto_callees.contains(&span))
        };
        if obj_excess {
            let oprec = if ta { 0 } else { precedence(object) };
            let decimal_or_regex = match o {
                Expression::NumericLiteral(n) => {
                    let raw = &self.src()[n.span.start as usize..n.span.end as usize];
                    raw == "0"
                        || (!raw.starts_with('0') && raw.bytes().all(|b| b.is_ascii_digit()))
                }
                Expression::RegExpLiteral(_) => true,
                _ => false,
            };
            let hit = (oprec >= 20 && (computed || !decimal_or_regex))
                || (!ta && matches!(o, Expression::CallExpression(_)))
                || (!ta
                    && !self.rule.ignore_new_in_member
                    && matches!(o, Expression::NewExpression(_))
                    && self.new_with_parens(o))
                || (!ta && m.optional() && matches!(o, Expression::ChainExpression(_)));
            if hit {
                self.report(object);
            }
        }
        if let MemberExpression::ComputedMemberExpression(c) = m
            && (is_type_assertion(&c.expression) && layers(&c.expression) >= 1
                || self.excess(&c.expression))
        {
            self.report(&c.expression);
        }
    }

    /// `checkExpressionOrExportStatement`.
    fn check_statement_expression(&mut self, e: &Expression) {
        let (n, k) = unparen(e);
        let span = n.span();
        let first = if k >= 1 {
            self.before(span.start)
        } else {
            self.after(span.start)
        };
        if let Some(first) = first.cloned()
            && first.value == "("
        {
            let i = self.idx(first.span.end);
            if let Some(second) = self.tokens[i..].iter().find(|t| t.value != "(").copied() {
                let j = self.idx(second.span.end);
                let third = self.tokens.get(j).copied();
                let after_closing = self.tokens[j..].iter().find(|t| t.value != ")").copied();
                let kw = second.kind == TokenKind::Keyword;
                let ignore = second.value == "{"
                    || (kw
                        && (second.value == "function"
                            || second.value == "class"
                            || (second.value == "let"
                                && after_closing.is_some_and(|t| {
                                    t.value == "[" || t.kind == TokenKind::Identifier
                                }))))
                    || (second.kind == TokenKind::Identifier
                        && second.value == "async"
                        && third.is_some_and(|t| {
                            t.kind == TokenKind::Keyword && t.value == "function"
                        }));
                if ignore {
                    self.ignore_tokens.insert(second.span.start);
                }
            }
        }
        if self.excess(e) {
            self.report(e);
        }
    }

    fn no_line_terminator(&mut self, token_end: u32, arg: &Expression) {
        let same = self.line(token_end) == self.line(unparen(arg).0.span().start);
        if if same { self.excess(arg) } else { self.double(arg) } {
            self.report(arg);
        }
    }

    fn mark_new_callee(&mut self, callee: &Expression) {
        let mut cur = skip_parens(callee);
        while let Some(m) = cur.as_member_expression() {
            self.in_new_callee.insert(m.span());
            cur = skip_parens(m.object());
        }
    }

    fn mark_function_body(&mut self, body: &FunctionBody) {
        for s in &body.statements {
            if let Statement::ExpressionStatement(e) = s {
                self.top_level_stmts
                    .insert(unparen(&e.expression).0.span());
            }
        }
    }
}

fn member_contains_call_m(c: &Expression) -> bool {
    c.as_member_expression()
        .is_some_and(|m| member_contains_call(m.object()))
}

impl<'b> Visit<'b> for V<'_, '_, '_, '_> {
    crate::skip_ts_types!('b);
    fn visit_expression(&mut self, e: &Expression<'b>) {
        match e {
            Expression::ArrayExpression(a) => {
                for el in &a.elements {
                    if let Some(x) = el.as_expression() {
                        // Type assertions are checked as `FunctionExpression`s here.
                        let prec = if is_type_assertion(x) { 20 } else { precedence(x) };
                        if self.excess_prec_as(x, prec, 1) {
                            self.report(x);
                        }
                    }
                }
            }
            Expression::ArrowFunctionExpression(a) => {
                if let Some(body) = a.get_expression() {
                    let n = unparen(body).0;
                    let skip = is_type_assertion(body)
                        || (self.rule.except_return_assign && contains_assignment(body))
                        || (matches!(n, Expression::ConditionalExpression(_))
                            && self.rule.ignore_arrow_conditionals);
                    if !skip {
                        let i = self.idx(n.span().start);
                        let first = self.tokens[i..]
                            .iter()
                            .find(|t| t.value != "(")
                            .map(|t| (t.span, t.value == "{"));
                        if let Some((fs, true)) = first
                            && self.before(fs.start).is_some_and(|t| t.value == "(")
                        {
                            self.ignore_tokens.insert(fs.start);
                        }
                        if self.excess_prec(body, 1) {
                            self.report(body);
                        }
                    }
                }
            }
            Expression::AssignmentExpression(a) => {
                if let Some(span) = a
                    .left
                    .as_simple_assignment_target()
                    .and_then(Self::simple_target_span)
                    && self.rule.all
                {
                    let ident = matches!(
                        a.left,
                        AssignmentTarget::AssignmentTargetIdentifier(_)
                    );
                    let anon = ident
                        && matches!(
                            a.operator,
                            AssignmentOperator::Assign
                                | AssignmentOperator::LogicalAnd
                                | AssignmentOperator::LogicalOr
                                | AssignmentOperator::LogicalNullish
                        )
                        && match skip_parens(&a.right) {
                            Expression::ArrowFunctionExpression(_) => true,
                            Expression::FunctionExpression(f) => f.id.is_none(),
                            Expression::ClassExpression(c) => c.id.is_none(),
                            _ => false,
                        };
                    let k = self.token_layers(span);
                    if k >= 1 && (!anon || k >= 2) {
                        self.report_span(span, k, false);
                    }
                }
                let return_assign = self.rule.except_return_assign && self.in_return > 0;
                if !return_assign && self.excess_prec(&a.right, 1) {
                    self.report(&a.right);
                }
            }
            Expression::AwaitExpression(a) => self.check_argument(16, &a.argument),
            Expression::UnaryExpression(u) => self.check_argument(16, &u.argument),
            Expression::BinaryExpression(b) => {
                let p = binary_precedence(b.operator);
                let exp = b.operator == BinaryOperator::Exponential;
                self.check_binary(b.span, p, exp, None, &b.left, &b.right);
            }
            Expression::LogicalExpression(l) => {
                let c = Some(l.operator == LogicalOperator::Coalesce);
                self.check_binary(l.span, precedence(e), false, c, &l.left, &l.right);
            }
            Expression::NewExpression(n) => {
                self.mark_new_callee(&n.callee);
                let parens = self.new_parens(n);
                self.check_call_new(true, false, parens, &n.callee, &n.arguments, false);
            }
            Expression::ClassExpression(c) => self.check_class(c),
            Expression::ConditionalExpression(c) => {
                let return_assign = self.rule.except_return_assign
                    && self.in_return > 0
                    && contains_assignment(e);
                if !return_assign {
                    let ternary = |x: &Expression| {
                        self.rule.except_cond_ternary
                            && matches!(
                                unparen(x).0,
                                Expression::BinaryExpression(_) | Expression::LogicalExpression(_)
                            )
                    };
                    let nested = |x: &Expression| {
                        self.rule.allow_nested_ternary
                            && matches!(unparen(x).0, Expression::ConditionalExpression(_))
                    };
                    // Only the first type-assertion operand is replaced by a sequence.
                    let ta = [&c.test, &c.consequent, &c.alternate]
                        .iter()
                        .position(|x| is_type_assertion(x));
                    let prec = |i: usize, x: &Expression| {
                        if ta == Some(i) { 0 } else { precedence(x) }
                    };
                    if !ternary(&c.test)
                        && !nested(&c.test)
                        && !self.cond_assign(&c.test)
                        && self.excess_prec_as(&c.test, prec(0, &c.test), 4)
                    {
                        self.report(&c.test);
                    }
                    if !ternary(&c.consequent)
                        && !nested(&c.consequent)
                        && self.excess_prec_as(&c.consequent, prec(1, &c.consequent), 1)
                    {
                        self.report(&c.consequent);
                    }
                    if !ternary(&c.alternate)
                        && !nested(&c.alternate)
                        && self.excess_prec_as(&c.alternate, prec(2, &c.alternate), 1)
                    {
                        self.report(&c.alternate);
                    }
                }
            }
            Expression::ImportExpression(i) => {
                let seq = matches!(unparen(&i.source).0, Expression::SequenceExpression(_));
                if if seq { self.double(&i.source) } else { self.excess(&i.source) } {
                    self.report(&i.source);
                }
            }
            Expression::ObjectExpression(o) => {
                for p in &o.properties {
                    if let ObjectPropertyKind::ObjectProperty(p) = p
                        && self.excess_prec(&p.value, 1)
                    {
                        self.report(&p.value);
                    }
                }
            }
            Expression::SequenceExpression(s) => {
                for x in &s.expressions {
                    if self.excess_prec(x, 0) {
                        self.report(x);
                    }
                }
            }
            Expression::TemplateLiteral(t) => {
                for x in &t.expressions {
                    if self.excess(x) {
                        self.report(x);
                    }
                }
            }
            Expression::UpdateExpression(u) => {
                if let Some(span) = Self::simple_target_span(&u.argument)
                    && self.rule.all
                {
                    let k = self.token_layers(span);
                    let same_line = self.line(span.end) == self.line(u.span.end.saturating_sub(1));
                    let hit = if u.prefix || same_line { k >= 1 } else { k >= 2 };
                    if hit {
                        self.report_span(span, k, false);
                    }
                }
            }
            Expression::YieldExpression(y) => {
                if let Some(arg) = &y.argument
                    && !is_type_assertion(arg)
                {
                    let same = self.line(y.span.start) == self.line(unparen(arg).0.span().start);
                    let nlt = if same { self.excess(arg) } else { self.double(arg) };
                    if (precedence(arg) >= 1 && nlt) || self.double(arg) {
                        self.report(arg);
                    }
                }
            }
            _ => {}
        }
        if let Expression::ArrowFunctionExpression(a) = e
            && a.get_expression().is_some()
        {
            self.in_return += 1;
            walk::walk_expression(self, e);
            self.in_return -= 1;
            return;
        }
        walk::walk_expression(self, e);
    }

    fn visit_call_expression(&mut self, c: &CallExpression<'b>) {
        if self.rule.ignore_fn_proto
            && let Some(m) = skip_parens(&c.callee).as_member_expression()
            && matches!(skip_parens(m.object()), Expression::FunctionExpression(_))
            && crate::utils::js::member_name(m).is_some_and(|n| n == "call" || n == "apply")
        {
            self.proto_callees.insert(m.span());
        }
        let iife = matches!(skip_parens(&c.callee), Expression::FunctionExpression(_));
        self.check_call_new(false, iife, true, &c.callee, &c.arguments, c.optional);
        walk::walk_call_expression(self, c);
    }

    fn visit_member_expression(&mut self, m: &MemberExpression<'b>) {
        self.check_member(m);
        walk::walk_member_expression(self, m);
    }

    fn visit_object_property(&mut self, p: &ObjectProperty<'b>) {
        if p.computed
            && let Some(k) = p.key.as_expression()
            && self.excess_prec(k, 1)
        {
            self.report(k);
        }
        walk::walk_object_property(self, p);
    }

    fn visit_spread_element(&mut self, s: &SpreadElement<'b>) {
        let n = unparen(&s.argument).0;
        let ty = match n {
            Expression::ConditionalExpression(_) => "ConditionalExpression",
            Expression::LogicalExpression(_) => "LogicalExpression",
            Expression::AwaitExpression(_) => "AwaitExpression",
            _ => "",
        };
        if !is_type_assertion(&s.argument)
            && !self.rule.allow_in_spread.iter().any(|t| t == ty)
            && self.excess_prec(&s.argument, 1)
        {
            self.report(&s.argument);
        }
        walk::walk_spread_element(self, s);
    }

    fn visit_function(&mut self, f: &Function<'b>, flags: ScopeFlags) {
        if let Some(body) = &f.body {
            self.mark_function_body(body);
        }
        walk::walk_function(self, f, flags);
    }

    fn visit_function_body(&mut self, b: &FunctionBody<'b>) {
        self.mark_function_body(b);
        walk::walk_function_body(self, b);
    }

    fn visit_class(&mut self, c: &Class<'b>) {
        if c.r#type == ClassType::ClassDeclaration {
            self.check_class(c);
        }
        walk::walk_class(self, c);
    }

    fn visit_method_definition(&mut self, m: &MethodDefinition<'b>) {
        if m.computed
            && let Some(k) = m.key.as_expression()
            && self.excess_prec(k, 1)
        {
            self.report(k);
        }
        walk::walk_method_definition(self, m);
    }

    fn visit_property_definition(&mut self, p: &PropertyDefinition<'b>) {
        if p.computed
            && let Some(k) = p.key.as_expression()
            && self.excess_prec(k, 1)
        {
            self.report(k);
        }
        if let Some(v) = &p.value
            && self.excess_prec(v, 1)
        {
            self.report(v);
        }
        walk::walk_property_definition(self, p);
    }

    fn visit_formal_parameter(&mut self, p: &FormalParameter<'b>) {
        if let Some(init) = &p.initializer
            && self.excess_prec(init, 1)
        {
            self.report(init);
        }
        walk::walk_formal_parameter(self, p);
    }

    fn visit_assignment_pattern(&mut self, p: &AssignmentPattern<'b>) {
        if self.excess_prec(&p.right, 1) {
            self.report(&p.right);
        }
        walk::walk_assignment_pattern(self, p);
    }

    fn visit_assignment_target_with_default(&mut self, it: &AssignmentTargetWithDefault<'b>) {
        self.check_target(&it.binding);
        if self.excess_prec(&it.init, 1) {
            self.report(&it.init);
        }
        walk::walk_assignment_target_with_default(self, it);
    }

    fn visit_assignment_target_property_identifier(
        &mut self,
        it: &AssignmentTargetPropertyIdentifier<'b>,
    ) {
        if let Some(init) = &it.init
            && self.excess_prec(init, 1)
        {
            self.report(init);
        }
        walk::walk_assignment_target_property_identifier(self, it);
    }

    fn visit_assignment_target_property_property(
        &mut self,
        it: &AssignmentTargetPropertyProperty<'b>,
    ) {
        if it.computed
            && let Some(k) = it.name.as_expression()
            && self.excess_prec(k, 1)
        {
            self.report(k);
        }
        self.check_maybe_default(&it.binding);
        walk::walk_assignment_target_property_property(self, it);
    }

    fn visit_array_assignment_target(&mut self, it: &ArrayAssignmentTarget<'b>) {
        for el in it.elements.iter().flatten() {
            self.check_maybe_default(el);
        }
        walk::walk_array_assignment_target(self, it);
    }

    fn visit_assignment_target_rest(&mut self, it: &AssignmentTargetRest<'b>) {
        self.check_target(&it.target);
        walk::walk_assignment_target_rest(self, it);
    }

    fn visit_expression_statement(&mut self, s: &ExpressionStatement<'b>) {
        self.check_statement_expression(&s.expression);
        walk::walk_expression_statement(self, s);
    }

    fn visit_if_statement(&mut self, s: &IfStatement<'b>) {
        if self.excess(&s.test) && !self.cond_assign(&s.test) {
            self.report(&s.test);
        }
        walk::walk_if_statement(self, s);
    }

    fn visit_while_statement(&mut self, s: &WhileStatement<'b>) {
        if self.excess(&s.test) && !self.cond_assign(&s.test) {
            self.report(&s.test);
        }
        walk::walk_while_statement(self, s);
    }

    fn visit_do_while_statement(&mut self, s: &DoWhileStatement<'b>) {
        if self.excess(&s.test) && !self.cond_assign(&s.test) {
            self.report(&s.test);
        }
        walk::walk_do_while_statement(self, s);
    }

    fn visit_for_statement(&mut self, s: &ForStatement<'b>) {
        if let Some(t) = &s.test
            && self.excess(t)
            && !self.cond_assign(t)
            && !is_type_assertion(t)
        {
            self.report(t);
        }
        if let Some(u) = &s.update
            && self.excess(u)
            && !is_type_assertion(u)
        {
            self.report(u);
        }
        if let Some(init) = s.init.as_ref().and_then(|i| i.as_expression())
            && !is_type_assertion(init)
            && self.excess(init)
        {
            self.report(init);
        }
        walk::walk_for_statement(self, s);
    }

    fn visit_for_in_statement(&mut self, s: &ForInStatement<'b>) {
        if !is_type_assertion(&s.right) {
            if let Some(t) = s.left.as_assignment_target() {
                self.check_target(t);
            }
            if self.excess(&s.right) {
                self.report(&s.right);
            }
        }
        walk::walk_for_in_statement(self, s);
    }

    fn visit_for_of_statement(&mut self, s: &ForOfStatement<'b>) {
        if let Some(t) = s.left.as_assignment_target() {
            self.check_target(t);
        }
        if !is_type_assertion(&s.right) && self.excess_prec(&s.right, 1) {
            self.report(&s.right);
        }
        walk::walk_for_of_statement(self, s);
    }

    fn visit_return_statement(&mut self, s: &ReturnStatement<'b>) {
        let saved = self.return_arg.take();
        if let Some(arg) = &s.argument
            && layers(arg) == 0
        {
            self.return_arg = Some((arg.span(), self.line(s.span.start)));
        }
        if let Some(arg) = &s.argument {
            let exception = self.rule.except_return_assign && contains_assignment(arg);
            if !exception && !matches!(unparen(arg).0, Expression::RegExpLiteral(_)) {
                self.no_line_terminator(s.span.start + 6, arg);
            }
        }
        self.in_return += 1;
        walk::walk_return_statement(self, s);
        self.in_return -= 1;
        self.return_arg = saved;
    }

    fn visit_throw_statement(&mut self, s: &ThrowStatement<'b>) {
        if !is_type_assertion(&s.argument) {
            self.no_line_terminator(s.span.start + 5, &s.argument);
        }
        walk::walk_throw_statement(self, s);
    }

    fn visit_switch_statement(&mut self, s: &SwitchStatement<'b>) {
        if self.excess(&s.discriminant) {
            self.report(&s.discriminant);
        }
        walk::walk_switch_statement(self, s);
    }

    fn visit_switch_case(&mut self, s: &SwitchCase<'b>) {
        if let Some(t) = &s.test
            && !is_type_assertion(t)
            && self.excess(t)
        {
            self.report(t);
        }
        walk::walk_switch_case(self, s);
    }

    fn visit_with_statement(&mut self, s: &WithStatement<'b>) {
        if self.excess(&s.object) {
            self.report(&s.object);
        }
        walk::walk_with_statement(self, s);
    }

    fn visit_variable_declarator(&mut self, d: &VariableDeclarator<'b>) {
        if let Some(init) = &d.init {
            let prec = if is_type_assertion(init) { 20 } else { precedence(init) };
            if self.excess_prec_as(init, prec, 1)
                && !matches!(unparen(init).0, Expression::RegExpLiteral(_))
            {
                self.report(init);
            }
        }
        walk::walk_variable_declarator(self, d);
    }
}

impl V<'_, '_, '_, '_> {
    fn check_class(&mut self, c: &Class) {
        let Some(sup) = c.heritage.as_ref().map(|h| &h.expression) else {
            return;
        };
        let prec = if matches!(unparen(sup).0, Expression::TSAsExpression(_)) {
            0
        } else {
            precedence(sup)
        };
        if if prec > 17 { self.excess(sup) } else { self.double(sup) } {
            self.report(sup);
        }
    }

    /// The Vue part: a parenthesized mustache / `v-bind` / `<style>` `v-bind()` value.
    fn verify_container(&mut self, root: &Root) {
        let expr = match root.expr {
            VExpression::Expression(x) => x,
            VExpression::FilterSequence { expression, .. } => expression,
            _ => return,
        };
        let n = unparen(expr).0;
        let span = n.span();
        let k = self.token_layers(span);
        if k == 0 {
            return;
        }
        // A template expression cannot be a bare sequence (`{{ (a, b) }}`): Vue compiles it into an
        // argument / property position, where `a, b` means something else.
        let ok = k >= 2
            || (!matches!(n, Expression::SequenceExpression(_))
                && (!is_iife(n) || skip_chain(n).is_some_and(|c| layers(&c.callee) >= 1))
                && !self.unwrap_changes_to_filter(span)
                && !(root.style && self.style_without_quote(root.container, span)));
        if !ok {
            return;
        }
        let (Some(lp), Some(rp)) = (self.before(span.start), self.after(span.end)) else {
            return;
        };
        let (lp, rp) = (lp.span, rp.span);
        let inner = self.src()[lp.end as usize..rp.start as usize].to_string();
        self.ctx.report_fix(
            lp,
            Some("unexpected"),
            MSG,
            Some(Fix::replace(Span::new(lp.start, rp.end), inner)),
        );
    }

    fn unwrap_changes_to_filter(&self, span: Span) -> bool {
        let mut stack: Vec<&str> = Vec::new();
        for t in &self.tokens[self.idx(span.start)..] {
            if t.span.end > span.end {
                break;
            }
            if let Some(top) = stack.last() {
                if t.value == *top {
                    stack.pop();
                    continue;
                }
            } else if t.value == "|" {
                return true;
            }
            match t.value.as_str() {
                "(" => stack.push(")"),
                "[" => stack.push("]"),
                "{" => stack.push("}"),
                _ => {}
            }
        }
        false
    }

    fn style_without_quote(&self, container: Span, span: Span) -> bool {
        let i = self.idx(container.start) + 1;
        let j = self.idx(span.start);
        i <= j && self.tokens[i..j].iter().all(|t| t.value == "(")
    }
}

impl Rule for NoExtraParens {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let scope = Scope {
            document: true,
            skip_dynamic_args: true,
        };
        for_each_root(ctx, scope, |ctx, root| {
            let lo = root.tokens.partition_point(|t| t.span.start < root.container.start);
            let hi = root.tokens.partition_point(|t| t.span.end <= root.container.end);
            let tokens = root.tokens[lo..hi.max(lo)]
                .iter()
                .filter(|t| t.kind != TokenKind::HTMLWhitespace)
                .collect();
            let mut v = V {
                rule: self,
                ctx,
                tokens,
                comments: root.comments,
                ignore_tokens: HashSet::new(),
                in_new_callee: HashSet::new(),
                proto_callees: HashSet::new(),
                in_return: 0,
                top_level_stmts: HashSet::new(),
                return_arg: None,
            };
            let vue = root.style
                || match root.parent {
                    Some(ExprParent::Element(_)) => true,
                    Some(ExprParent::DirectiveValue(e, i)) => {
                        let doc = v.ctx.doc().unwrap();
                        doc.element(e)
                            .attributes()
                            .get(i as usize)
                            .and_then(crate::utils::template::directive_key)
                            .is_some_and(|k| k.name.name == "bind")
                    }
                    _ => false,
                };
            if vue {
                v.verify_container(&root);
            }
            visit_root(&mut v, root.expr);
        });
    }
}
