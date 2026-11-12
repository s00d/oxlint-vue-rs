//! vue/prefer-template (core `prefer-template` in `<template>` and `<style>` `v-bind()`)

use oxc::ast::ast::{BinaryExpression, BinaryOperator, Expression};
use oxc::ast_visit::{Visit, walk};
use oxc::span::{GetSpan, Span};

use crate::ast::Token;
use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::js::skip_parens;
use crate::utils::tpl_core::{DOCUMENT, first_token_between, for_each_root, token_index, visit_root};

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(PreferTemplate))
}

struct PreferTemplate;

fn concat<'x, 'b>(e: &'x Expression<'b>) -> Option<&'x BinaryExpression<'b>> {
    match skip_parens(e) {
        Expression::BinaryExpression(b) if b.operator == BinaryOperator::Addition => Some(b),
        _ => None,
    }
}

fn is_string_lit(e: &Expression) -> bool {
    matches!(
        skip_parens(e),
        Expression::StringLiteral(_) | Expression::TemplateLiteral(_)
    )
}

fn has_string(e: &Expression) -> bool {
    match concat(e) {
        Some(b) => has_string(&b.right) || has_string(&b.left),
        None => is_string_lit(e),
    }
}

fn has_non_string(e: &Expression) -> bool {
    match concat(e) {
        Some(b) => has_non_string(&b.right) || has_non_string(&b.left),
        None => !is_string_lit(e),
    }
}

/// `startsWithTemplateCurly` (espree `TemplateElement` ranges include delimiters, so a template
/// literal never starts/ends with an empty quasi).
fn curly_edge(e: &Expression, start: bool) -> bool {
    match skip_parens(e) {
        Expression::BinaryExpression(b) => curly_edge(if start { &b.left } else { &b.right }, true),
        Expression::TemplateLiteral(_) | Expression::StringLiteral(_) => false,
        _ => true,
    }
}

/// `/^(?:[^\\]|\\.)*\\(?:[1-9]|0[0-9])/su` on a raw string literal.
fn has_octal_escape(raw: &str) -> bool {
    let b = raw.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' {
            match b.get(i + 1) {
                Some(b'1'..=b'9') => return true,
                Some(b'0') if b.get(i + 2).is_some_and(u8::is_ascii_digit) => return true,
                _ => i += 2,
            }
        } else {
            i += 1;
        }
    }
    false
}

fn has_octal(src: &str, e: &Expression) -> bool {
    match concat(e) {
        Some(b) => has_octal(src, &b.left) || has_octal(src, &b.right),
        None => match skip_parens(e) {
            Expression::StringLiteral(s) => has_octal_escape(&src[s.span.start as usize..s.span.end as usize]),
            _ => false,
        },
    }
}

/// String literal raw → template literal text.
fn literal_to_template(raw: &str) -> String {
    let quote = &raw[..1];
    let body = &raw[1..raw.len() - 1];
    let mut out = String::new();
    let b = body.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let start = i;
        while i < b.len() && b[i] == b'\\' {
            i += 1;
        }
        let n = i - start;
        let rest = &body[i..];
        if rest.starts_with("${") || rest.starts_with('`') {
            if n % 2 == 0 {
                out.push('\\');
            }
            out.push_str(&body[start..i]);
            let len = if rest.starts_with('`') { 1 } else { 2 };
            out.push_str(&rest[..len]);
            i += len;
        } else if n > 0 {
            out.push_str(&body[start..i]);
        } else {
            let c = rest.chars().next().unwrap();
            out.push(c);
            i += c.len_utf8();
        }
    }
    format!("`{}`", out.replace(&format!("\\{quote}"), quote))
}

struct V<'r, 'c, 'f, 'a> {
    ctx: &'c mut Ctx<'f, 'a>,
    tokens: &'r [Token],
    non_top: Vec<Span>,
}

impl V<'_, '_, '_, '_> {
    /// `getTextBetween`: the text between consecutive tokens from `a` to `b` (whitespace and
    /// comments, no tokens).
    fn between(&self, a: u32, b: u32) -> String {
        let src = self.ctx.source();
        let mut out = String::new();
        let mut pos = a;
        for t in self.tokens[token_index(self.tokens, a)..]
            .iter()
            .take_while(|t| t.span.end <= b)
        {
            out.push_str(&src[pos as usize..t.span.start as usize]);
            pos = t.span.end;
        }
        out.push_str(&src[pos as usize..b as usize]);
        out
    }

    fn template(&self, e: &Expression, before: Option<&str>, after: Option<&str>) -> Option<String> {
        let e = skip_parens(e);
        let src = self.ctx.source();
        let text = |s: Span| &src[s.start as usize..s.end as usize];
        match e {
            Expression::StringLiteral(s) => return Some(literal_to_template(text(s.span))),
            Expression::TemplateLiteral(t) => return Some(text(t.span).to_string()),
            _ => {}
        }
        if let Some(b) = concat(e).filter(|_| has_string(e)) {
            let (l, r) = (skip_parens(&b.left), skip_parens(&b.right));
            let plus = first_token_between(self.tokens, l.span().end, r.span().start, |t| {
                t.value == "+"
            })?;
            let before_plus = self.between(l.span().end, plus.span.start);
            let after_plus = self.between(plus.span.end, r.span().start);
            if curly_edge(l, false) {
                let joined = format!("{before_plus}{after_plus}");
                let lt = self.template(l, before, Some(&joined))?;
                let rt = self.template(r, None, after)?;
                return Some(format!("{}{}", &lt[..lt.len() - 1], &rt[1..]));
            }
            if curly_edge(r, true) {
                let joined = format!("{before_plus}{after_plus}");
                let lt = self.template(l, before, None)?;
                let rt = self.template(r, Some(&joined), after)?;
                return Some(format!("{}{}", &lt[..lt.len() - 1], &rt[1..]));
            }
            let lt = self.template(l, before, None)?;
            let rt = self.template(r, after, None)?;
            return Some(format!("{lt}{before_plus}+{after_plus}{rt}"));
        }
        Some(format!(
            "`${{{}{}{}}}`",
            before.unwrap_or(""),
            text(e.span()),
            after.unwrap_or("")
        ))
    }
}

impl<'b> Visit<'b> for V<'_, '_, '_, '_> {
    crate::skip_ts_types!('b);
    fn visit_expression(&mut self, e: &Expression<'b>) {
        if let Expression::BinaryExpression(b) = e
            && b.operator == BinaryOperator::Addition
        {
            for c in [&b.left, &b.right] {
                if let Some(c) = concat(c) {
                    self.non_top.push(c.span);
                }
            }
            if !self.non_top.contains(&b.span) && has_string(e) && has_non_string(e) {
                let fix = if has_octal(self.ctx.source(), e) {
                    None
                } else {
                    self.template(e, None, None)
                        .map(|t| Fix::replace(b.span, t))
                };
                self.ctx.report_fix(
                    b.span,
                    Some("unexpectedStringConcatenation"),
                    "Unexpected string concatenation.",
                    fix,
                );
            }
        }
        walk::walk_expression(self, e);
    }
}

impl Rule for PreferTemplate {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        for_each_root(ctx, DOCUMENT, |ctx, root| {
            let mut v = V {
                ctx,
                tokens: root.tokens,
                non_top: Vec::new(),
            };
            visit_root(&mut v, root.expr);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::literal_to_template as t;
    #[test]
    fn literal() {
        assert_eq!(t("'a${b}'"), "`a\\${b}`");
        assert_eq!(t("'it\\'s'"), "`it's`");
        assert_eq!(t("'x\\`'"), "`x\\``");
        assert_eq!(t("\"#\""), "`#`");
    }
}
