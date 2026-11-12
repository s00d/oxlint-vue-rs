//! vue/dot-notation (core `dot-notation` in `<template>` and `<style>` `v-bind()`)

use oxc::ast::ast::{Expression, MemberExpression};
use oxc::ast_visit::{Visit, walk};
use oxc::span::{GetSpan, Span};
use crate::utils::regexp::JsRegex as Regex;

use crate::ast::{Token, TokenKind};
use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::js::skip_parens;
use crate::utils::tpl_core::{
    DOCUMENT, first_token_between, for_each_root, has_comment_in, token_after, token_before,
    visit_root,
};

/// `eslint/lib/rules/utils/keywords.js`.
const KEYWORDS: &[&str] = &[
    "abstract", "boolean", "break", "byte", "case", "catch", "char", "class", "const", "continue",
    "debugger", "default", "delete", "do", "double", "else", "enum", "export", "extends", "false",
    "final", "finally", "float", "for", "function", "goto", "if", "implements", "import", "in",
    "instanceof", "int", "interface", "long", "native", "new", "null", "package", "private",
    "protected", "public", "return", "short", "static", "super", "switch", "synchronized", "this",
    "throw", "throws", "transient", "true", "try", "typeof", "var", "void", "volatile", "while",
    "with",
];

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let o = opts.get(0);
    let allow_pattern = o
        .get("allowPattern")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .and_then(|s| Regex::with_flags(s, "u").ok());
    Some(Box::new(DotNotation {
        allow_keywords: o.get("allowKeywords").and_then(|v| v.as_bool()).unwrap_or(true),
        allow_pattern,
    }))
}

struct DotNotation {
    allow_keywords: bool,
    allow_pattern: Option<Regex>,
}

fn valid_identifier(s: &str) -> bool {
    let mut c = s.chars();
    c.next()
        .is_some_and(|f| f.is_ascii_alphabetic() || f == '_' || f == '$')
        && c.all(|x| x.is_ascii_alphanumeric() || x == '_' || x == '$')
}

fn is_word_token(t: &Token) -> bool {
    matches!(
        t.kind,
        TokenKind::Identifier
            | TokenKind::Keyword
            | TokenKind::Numeric
            | TokenKind::Boolean
            | TokenKind::Null
    )
}

struct V<'r, 'c, 'f, 'a> {
    rule: &'r DotNotation,
    ctx: &'c mut Ctx<'f, 'a>,
    tokens: &'r [Token],
    comments: &'r [Token],
}

impl V<'_, '_, '_, '_> {
    fn computed(&mut self, span: Span, object: &Expression, prop: Span, value: &str, literal: bool, optional: bool) {
        if !valid_identifier(value)
            || (!self.rule.allow_keywords && KEYWORDS.contains(&value))
            || self.rule.allow_pattern.as_ref().is_some_and(|r| r.is_match(value))
        {
            return;
        }
        let key = if literal {
            crate::utils::js::json_string(value)
        } else {
            format!("`{value}`")
        };
        let object = skip_parens(object);
        let fix = (|| {
            let lb = first_token_between(self.tokens, object.span().end, span.end, |t| {
                t.kind == TokenKind::Punctuator && t.value == "["
            })?;
            let rb = token_before(self.tokens, span.end)?;
            if has_comment_in(self.comments, Span::new(lb.span.start, rb.span.end)) {
                return None;
            }
            let mut edits = Vec::new();
            if !optional {
                let decimal = matches!(object, Expression::NumericLiteral(_))
                    && {
                        let raw = self.ctx.slice(object.span());
                        raw == "0" || (!raw.starts_with('0') && raw.bytes().all(|b| b.is_ascii_digit()))
                    };
                edits.push(Fix::insert(lb.span.start, if decimal { " ." } else { "." }));
            }
            edits.push(Fix::replace(Span::new(lb.span.start, rb.span.end), value));
            if let Some(next) = token_after(self.tokens, span.end)
                && next.span.start == rb.span.end
                && is_word_token(next)
            {
                edits.push(Fix::insert(span.end, " "));
            }
            Fix::merge(self.ctx.source(), edits)
        })();
        self.ctx.report_fix(
            prop,
            Some("useDot"),
            format!("[{key}] is better written in dot notation."),
            fix,
        );
    }
}

impl<'b> Visit<'b> for V<'_, '_, '_, '_> {
    crate::skip_ts_types!('b);
    fn visit_member_expression(&mut self, m: &MemberExpression<'b>) {
        walk::walk_member_expression(self, m);
        match m {
            MemberExpression::ComputedMemberExpression(c) => {
                let prop = skip_parens(&c.expression);
                let (value, literal) = match prop {
                    Expression::StringLiteral(s) => (s.value.display().to_string(), true),
                    Expression::BooleanLiteral(b) => (b.value.to_string(), true),
                    Expression::NullLiteral(_) => ("null".to_string(), true),
                    Expression::TemplateLiteral(t) if t.expressions.is_empty() => {
                        match t.quasis.first().and_then(|q| q.value.cooked.as_ref()) {
                            Some(c) => (c.display().to_string(), false),
                            None => return,
                        }
                    }
                    _ => return,
                };
                self.computed(c.span, &c.object, prop.span(), &value, literal, c.optional);
            }
            MemberExpression::StaticMemberExpression(s) => {
                let name = s.property.name.as_str();
                if self.rule.allow_keywords || !KEYWORDS.contains(&name) {
                    return;
                }
                let fix = (|| {
                    let dot = token_before(self.tokens, s.property.span.start)?;
                    if !s.optional
                        && matches!(skip_parens(&s.object), Expression::Identifier(id) if id.name == "let")
                    {
                        return None;
                    }
                    if has_comment_in(self.comments, Span::new(dot.span.start, s.property.span.end)) {
                        return None;
                    }
                    let mut edits = Vec::new();
                    if !s.optional {
                        edits.push(Fix::remove(dot.span));
                    }
                    edits.push(Fix::replace(s.property.span, format!("[\"{name}\"]")));
                    Fix::merge(self.ctx.source(), edits)
                })();
                self.ctx.report_fix(
                    s.property.span,
                    Some("useBrackets"),
                    format!(".{name} is a syntax error."),
                    fix,
                );
            }
            MemberExpression::PrivateFieldExpression(_) => {}
        }
    }
}

impl Rule for DotNotation {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        for_each_root(ctx, DOCUMENT, |ctx, root| {
            let mut v = V {
                rule: self,
                ctx,
                tokens: root.tokens,
                comments: root.comments,
            };
            visit_root(&mut v, root.expr);
        });
    }
}
