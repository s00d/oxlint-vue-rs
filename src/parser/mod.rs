//! Port of vue-eslint-parser (v10.4): HTML tokenizer, intermediate tokenizer, template tree
//! builder, directive / expression parsing (oxc), references and variables.
//!
//! The whole `.vue` file is parsed as one HTML document fragment (SFC mode: tag names are
//! compared by `rawName`). Top-level `<template>` content is a Vue template; other top-level
//! blocks keep raw text.

mod entities;
mod html;
mod intermediate;
mod script;
mod template;
mod tokenizer;

pub(crate) use script::token_kind;

use oxc::allocator::Allocator;
use oxc::span::Span;

use crate::ast::*;
use html::{Post as Task, TreeBuilder};
use script::ExprParser;
use template::{Converter, Post};

#[derive(Debug, Clone, Copy, Default)]
pub struct ParserOptions {
    /// Parse template expressions as TypeScript.
    pub ts: bool,
    /// Allow JSX in expressions.
    pub jsx: bool,
}

/// Parse a whole `.vue` source into a [`Document`]. Never fails or panics: errors go to
/// `Document::errors`.
pub fn parse_document<'a>(
    alloc: &'a Allocator,
    source: &'a str,
    opts: &ParserOptions,
) -> Document<'a> {
    let mut tb = TreeBuilder::new(source);
    tb.parse();
    let TreeBuilder {
        itok,
        mut elements,
        texts,
        mustaches,
        children,
        posts,
        ..
    } = tb;
    let html_tokens = itok.tokens;
    let html_comments = itok.comments;
    let tokenizer = itok.tokenizer;
    let gaps = tokenizer.gaps;
    let errors = tokenizer
        .errors
        .into_iter()
        .map(|(code, offset)| ParseError {
            code: Some(code.to_string()),
            message: code.to_string(),
            offset,
        })
        .collect();

    let exprs = mustaches
        .into_iter()
        .map(|(id, span, parent)| ExprContainer {
            id,
            span,
            parent: ExprParent::Element(parent),
            expression: None,
            references: Vec::new(),
        })
        .collect();
    let mut conv = Converter {
        ep: ExprParser {
            alloc,
            gaps: &gaps,
            ts: opts.ts,
            jsx: opts.jsx,
        },
        source,
        post: Post {
            exprs,
            replacements: Vec::new(),
            comments: Vec::new(),
            errors,
        },
    };
    for task in posts {
        match task {
            Task::Directive { element, attr } => conv.directive(&mut elements, element, attr),
            Task::Mustache {
                expr,
                value,
                start,
                end,
            } => conv.mustache(expr, &value, start, end),
        }
    }
    let Post {
        mut exprs,
        mut replacements,
        comments: expr_comments,
        errors,
    } = conv.post;
    template::resolve_references(&elements, &mut exprs);

    // Merge token replacements (disjoint ranges) into the HTML token list.
    replacements.sort_by_key(|(r, _)| r.start);
    let mut tokens = Vec::with_capacity(
        html_tokens.len() + replacements.iter().map(|r| r.1.len()).sum::<usize>(),
    );
    let mut html = html_tokens.into_iter().map(to_token).peekable();
    for (range, new) in replacements {
        while let Some(t) = html.next_if(|t| t.span.start < range.start) {
            tokens.push(t);
        }
        while html.next_if(|t| t.span.end <= range.end).is_some() {}
        tokens.extend(new);
    }
    tokens.extend(html);

    let mut comments: Vec<Token> = html_comments.into_iter().map(to_token).collect();
    comments.extend(expr_comments);
    comments.sort_by_key(|t| t.span.start);

    let template_body = children.iter().find_map(|c| match c {
        Child::Element(id) if elements[id.0 as usize].name == "template" => Some(*id),
        _ => None,
    });
    let template_body = template_body.filter(|id| {
        let lang =
            html::plain_attr(&elements[id.0 as usize], "lang").and_then(|a| match &a.value {
                Some(AttributeValue::Literal(l)) => Some(l.value.as_str()),
                _ => None,
            });
        lang.is_none_or(|l| l.is_empty() || l == "html")
    });

    Document {
        source,
        elements,
        texts,
        exprs,
        children,
        template_body,
        tokens,
        comments,
        errors,
    }
}

fn to_token(t: tokenizer::HToken) -> Token {
    Token {
        kind: t.kind,
        span: Span::new(t.start, t.end),
        value: t.value,
    }
}

/// Parse `code` (located at byte offset `base` of the file, no entity gaps) as a template
/// expression without filters, e.g. the argument of a `<style>` `v-bind()` (vue-eslint-parser
/// `src/style`). Returns the expression with its tokens and comments; `None` on a parse error.
pub fn parse_expression_at<'a>(
    alloc: &'a Allocator,
    code: &str,
    base: u32,
    opts: &ParserOptions,
) -> Option<(VExpression<'a>, Vec<Token>, Vec<Token>)> {
    let ep = ExprParser {
        alloc,
        gaps: &[],
        ts: opts.ts,
        jsx: opts.jsx,
    };
    let r = ep
        .parse_expression(code, &script::Mapper::new(&[], base), false, false)
        .ok()?;
    Some((r.expression?, r.tokens, r.comments))
}
