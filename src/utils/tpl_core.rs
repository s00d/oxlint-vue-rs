//! Helpers for the "extension" rules that apply an ESLint core rule to `<template>` expressions
//! (eslint-plugin-vue `wrapCoreRule`): which expressions are visited and token navigation that
//! works for both template expressions and `<style>` `v-bind()` expressions.

use oxc::allocator::Allocator;
use oxc::ast_visit::Visit;
use oxc::span::Span;

use crate::ast::{ExprParent, Token, TokenKind, VExpression};
use crate::parser::{ParserOptions, parse_expression_at};
use crate::rule::Ctx;
use crate::utils::template::template_expr_containers;
use crate::utils::tpl_misc::style_v_binds;

/// One expression container visited by a wrapped core rule.
pub struct Root<'r, 'a> {
    pub expr: &'r VExpression<'a>,
    /// Tokens around/inside the expression (template: the whole document's tokens).
    pub tokens: &'r [Token],
    /// Comments (template: the whole document's comments).
    pub comments: &'r [Token],
    /// Container span (`{{ }}` / attribute value incl. quotes / `v-bind( … )`).
    pub container: Span,
    /// The dynamic argument of a directive key (`v-bind:[expr]`).
    pub dynamic_arg: bool,
    /// A `<style>` `v-bind()` expression.
    pub style: bool,
    /// Where the container sits (`None` for `<style>` `v-bind()`).
    pub parent: Option<ExprParent>,
}

/// Which containers a wrapped rule visits.
#[derive(Clone, Copy)]
pub struct Scope {
    /// `applyDocument`: the whole document (template + `<style>` `v-bind()`), otherwise the
    /// template body only.
    pub document: bool,
    /// `skipDynamicArguments`.
    pub skip_dynamic_args: bool,
}

pub const BODY: Scope = Scope {
    document: false,
    skip_dynamic_args: false,
};
pub const DOCUMENT: Scope = Scope {
    document: true,
    skip_dynamic_args: false,
};

/// Call `f` for every expression container in `scope`, in document order (style last).
pub fn for_each_root<'f, 'a, F>(ctx: &mut Ctx<'f, 'a>, scope: Scope, mut f: F)
where
    F: for<'r, 'b> FnMut(&mut Ctx<'f, 'a>, Root<'r, 'b>),
{
    let Some(doc) = ctx.doc() else { return };
    let containers: Vec<_> = if scope.document {
        doc.exprs.iter().collect()
    } else {
        template_expr_containers(doc).collect()
    };
    for c in containers {
        let Some(expr) = &c.expression else { continue };
        let dynamic_arg = matches!(c.parent, ExprParent::DirectiveArgument(..));
        if dynamic_arg && scope.skip_dynamic_args {
            continue;
        }
        f(
            ctx,
            Root {
                expr,
                tokens: &doc.tokens,
                comments: &doc.comments,
                container: c.span,
                dynamic_arg,
                style: false,
                parent: Some(c.parent),
            },
        );
    }
    if !scope.document {
        return;
    }
    let opts = ParserOptions {
        ts: ctx.file.ts,
        jsx: false,
    };
    let alloc = Allocator::default();
    for container in style_v_binds(doc) {
        let Some((expr, tokens, comments)) =
            parse_style_v_bind(&alloc, doc.source, container, &opts)
        else {
            continue;
        };
        f(
            ctx,
            Root {
                expr: &expr,
                tokens: &tokens,
                comments: &comments,
                container,
                dynamic_arg: false,
                style: true,
                parent: None,
            },
        );
    }
}

/// Skip CSS whitespace and `/* */` comments from `i`.
fn skip_css_trivia(b: &[u8], mut i: usize, end: usize) -> usize {
    loop {
        while i < end && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i + 1 < end && b[i] == b'/' && b[i + 1] == b'*' {
            i += 2;
            while i + 1 < end && !(b[i] == b'*' && b[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(end);
        } else {
            return i;
        }
    }
}

/// Parse the argument of `v-bind( … )` (`container` = `v-bind` .. `)`), with the
/// `v-bind`/`(`/quote/`)` tokens vue-eslint-parser adds around it.
fn parse_style_v_bind<'a>(
    alloc: &'a Allocator,
    src: &str,
    container: Span,
    opts: &ParserOptions,
) -> Option<(VExpression<'a>, Vec<Token>, Vec<Token>)> {
    let b = src.as_bytes();
    let (s, e) = (container.start as usize, container.end as usize);
    let close = e - 1;
    let open = skip_css_trivia(b, s + 6, close);
    if b.get(open) != Some(&b'(') {
        return None;
    }
    // A single quoted token (comments around it allowed) = quoted expression.
    let first = skip_css_trivia(b, open + 1, close);
    let mut quote = None;
    if first < close && matches!(b[first], b'"' | b'\'') {
        let q = b[first];
        let mut i = first + 1;
        while i < close && b[i] != q {
            i += if b[i] == b'\\' { 2 } else { 1 };
        }
        if i < close && skip_css_trivia(b, i + 1, close) == close {
            quote = Some((q, first + 1, i));
        }
    }
    let (es, ee) = match quote {
        Some((_, qs, qe)) => (qs, qe),
        None => (first, close),
    };
    let code = src.get(es..ee)?;
    let (expr, inner, comments) = parse_expression_at(alloc, code, es as u32, opts)?;
    let tok = |kind, a: usize, z: usize| Token {
        kind,
        span: Span::new(a as u32, z as u32),
        value: src[a..z].to_string(),
    };
    let mut tokens = vec![
        tok(TokenKind::HTMLRawText, s, s + 6),
        tok(TokenKind::Punctuator, open, open + 1),
    ];
    if let Some((_, qs, _)) = quote {
        tokens.push(tok(TokenKind::Punctuator, qs - 1, qs));
    }
    tokens.extend(inner);
    if let Some((_, _, qe)) = quote {
        tokens.push(tok(TokenKind::Punctuator, qe, qe + 1));
    }
    tokens.push(tok(TokenKind::Punctuator, close, close + 1));
    Some((expr, tokens, comments))
}

/// Visit every oxc node of a root with an oxc visitor.
pub fn visit_root<'a, V: Visit<'a>>(v: &mut V, e: &VExpression<'a>) {
    match e {
        VExpression::Expression(x) => v.visit_expression(x),
        VExpression::FilterSequence {
            expression,
            filters,
            ..
        } => {
            v.visit_expression(expression);
            for f in filters {
                v.visit_arguments(&f.arguments);
            }
        }
        VExpression::For {
            left, rest, right, ..
        } => {
            for p in left.iter().flatten() {
                v.visit_binding_pattern(p);
            }
            if let Some(r) = rest {
                v.visit_binding_rest_element(r);
            }
            v.visit_expression(right);
        }
        VExpression::On { body, .. } => v.visit_statements(body),
        VExpression::SlotScope { params, .. } => v.visit_formal_parameters(params),
        VExpression::Generic { params, .. } => v.visit_ts_type_parameter_declaration(params),
    }
}

/// Index of the first token starting at or after `pos`.
pub fn token_index(tokens: &[Token], pos: u32) -> usize {
    tokens.partition_point(|t| t.span.start < pos)
}

/// Last token ending at or before `pos`.
pub fn token_before(tokens: &[Token], pos: u32) -> Option<&Token> {
    let i = tokens.partition_point(|t| t.span.end <= pos);
    i.checked_sub(1).map(|i| &tokens[i])
}

/// First token starting at or after `pos`.
pub fn token_after(tokens: &[Token], pos: u32) -> Option<&Token> {
    tokens.get(token_index(tokens, pos))
}

/// `sourceCode.getFirstTokenBetween(a, b, pred)` with `a.end` / `b.start` positions.
pub fn first_token_between(
    tokens: &[Token],
    from: u32,
    to: u32,
    pred: impl Fn(&Token) -> bool,
) -> Option<&Token> {
    tokens[token_index(tokens, from)..]
        .iter()
        .take_while(|t| t.span.end <= to)
        .find(|t| pred(t))
}

/// Whether a comment lies inside `span`.
pub fn has_comment_in(comments: &[Token], span: Span) -> bool {
    let i = comments.partition_point(|c| c.span.start < span.start);
    comments.get(i).is_some_and(|c| c.span.end <= span.end)
}

/// vue-eslint-parser walks templates with ESTree visitor keys: the TS-only fields of ESTree
/// nodes (`typeAnnotation`, `typeParameters`, `typeArguments`, `returnType`) are not visited,
/// while TS nodes themselves fall back to all their keys (`x as 1` visits the literal type).
/// Expands to the matching visit overrides; use inside `impl<'b> Visit<'b> for …`.
// ponytail: type arguments nested inside a TS type (`x as A<B>`) are skipped too.
#[macro_export]
macro_rules! skip_ts_types {
    ($l:lifetime) => {
        fn visit_ts_type_annotation(&mut self, _: &oxc::ast::ast::TSTypeAnnotation<$l>) {}
        fn visit_ts_type_parameter_instantiation(
            &mut self,
            _: &oxc::ast::ast::TSTypeParameterInstantiation<$l>,
        ) {
        }
        fn visit_ts_type_parameter_declaration(
            &mut self,
            _: &oxc::ast::ast::TSTypeParameterDeclaration<$l>,
        ) {
        }
        fn visit_ts_instantiation_expression(
            &mut self,
            it: &oxc::ast::ast::TSInstantiationExpression<$l>,
        ) {
            self.visit_expression(&it.expression);
            for t in &it.type_arguments.params {
                self.visit_ts_type(t);
            }
        }
    };
}
