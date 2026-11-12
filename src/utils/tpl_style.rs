//! Helpers for the HTML / SFC stylistic rules (`html-*`, `*-style`, block rules, `max-len`, …).

use oxc::span::Span;

use crate::ast::*;

/// `utils.hasInvalidEOF(program)`: the template body has an `eof-*` parse error.
pub fn has_invalid_eof(doc: &Document) -> bool {
    doc.template_body.is_some()
        && doc
            .errors
            .iter()
            .any(|e| e.code.as_deref().is_some_and(|c| c.starts_with("eof-")))
}

/// Span of an attribute key node (`node.key`).
pub fn key_span(a: &Attribute) -> Span {
    match &a.key {
        AttributeKey::Plain(k) => k.span,
        AttributeKey::Directive(k) => k.span,
    }
}

/// Template-body tokens (the root `<template>` start tag .. end tag), what
/// `getTemplateBodyTokenStore()` sees.
pub fn body_tokens<'d>(doc: &'d Document) -> &'d [Token] {
    match doc.template_body {
        Some(root) => doc.tokens_in(doc.element(root).span),
        None => &[],
    }
}

/// Every tag (`VStartTag` / `VEndTag` span) of the whole document (`defineDocumentVisitor`),
/// in document order: `(element, is_end_tag, span)`.
pub fn document_tags<'d>(doc: &'d Document) -> Vec<(&'d Element, bool, Span)> {
    let mut out: Vec<(&Element, bool, Span)> = Vec::new();
    for e in &doc.elements {
        out.push((e, false, e.start_tag.span));
        if let Some(s) = e.end_tag {
            out.push((e, true, s));
        }
    }
    out.sort_by_key(|t| t.2.start);
    out
}

/// Token store (`document.tokens` + `document.comments`) navigation, ESLint `TokenStore` style.
/// `getFirstToken(node)`: first token inside `span`.
pub fn first_token<'d>(doc: &'d Document, span: Span) -> Option<&'d Token> {
    doc.tokens_in(span).first()
}

/// `getLastToken(node, skip)`.
pub fn last_token<'d>(doc: &'d Document, span: Span, skip: usize) -> Option<&'d Token> {
    doc.tokens_in(span).iter().rev().nth(skip)
}

fn comments_in<'d>(doc: &'d Document, span: Span) -> &'d [Token] {
    let lo = doc.comments.partition_point(|t| t.span.start < span.start);
    let hi = doc.comments.partition_point(|t| t.span.end <= span.end);
    if hi <= lo { &[] } else { &doc.comments[lo..hi] }
}

/// `getTokenBefore(pos)`: last token ending at or before `pos` (optionally comments too).
pub fn token_before<'d>(doc: &'d Document, pos: u32, comments: bool) -> Option<&'d Token> {
    let t = doc.tokens_in(Span::new(0, pos)).last();
    if !comments {
        return t;
    }
    let c = comments_in(doc, Span::new(0, pos)).last();
    match (t, c) {
        (Some(t), Some(c)) => Some(if c.span.start > t.span.start { c } else { t }),
        (t, c) => t.or(c),
    }
}

/// `getTokenAfter(pos)`: first token starting at or after `pos` (optionally comments too).
pub fn token_after<'d>(doc: &'d Document, pos: u32, comments: bool) -> Option<&'d Token> {
    let end = doc.source.len() as u32;
    let t = doc.tokens_in(Span::new(pos, end)).first();
    if !comments {
        return t;
    }
    let c = comments_in(doc, Span::new(pos, end)).first();
    match (t, c) {
        (Some(t), Some(c)) => Some(if c.span.start < t.span.start { c } else { t }),
        (t, c) => t.or(c),
    }
}

/// JS `\s` (WhiteSpace + LineTerminator, what `/\s/u` and `String#trim` use).
pub fn is_js_ws(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

/// JS `String#trim`.
pub fn js_trim(s: &str) -> &str {
    s.trim_matches(is_js_ws)
}

/// Number of line breaks (`\r\n`, `\r`, `\n`, U+2028, U+2029) in `s`.
pub fn linebreak_count(s: &str) -> usize {
    let b = s.as_bytes();
    let mut n = 0;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\r' => {
                n += 1;
                if b.get(i + 1) == Some(&b'\n') {
                    i += 1;
                }
            }
            b'\n' => n += 1,
            0xE2 if b.get(i + 1) == Some(&0x80) && matches!(b.get(i + 2), Some(0xA8 | 0xA9)) => {
                n += 1;
                i += 2;
            }
            _ => {}
        }
        i += 1;
    }
    n
}

/// JS `text.split(/\r\n|[\r\n  ]/u)`.
pub fn split_lines(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let (mut start, mut i) = (0, 0);
    while i < b.len() {
        let len = match b[i] {
            b'\r' if b.get(i + 1) == Some(&b'\n') => 2,
            b'\r' | b'\n' => 1,
            0xE2 if b.get(i + 1) == Some(&0x80) && matches!(b.get(i + 2), Some(0xA8 | 0xA9)) => 3,
            _ => 0,
        };
        if len > 0 {
            out.push(&s[start..i]);
            i += len;
            start = i;
        } else {
            i += 1;
        }
    }
    out.push(&s[start..]);
    out
}

/// A tiny element selector (subset of `lib/utils/selector.ts`, used by `block-order`):
/// comma lists of compound selectors made of `tag`, `*`, `[attr]`, `[attr<op>value i]`,
/// `.class`, `#id`, `:not(…)`, `:is(…)`, `:where(…)`.
/// ponytail: no combinators / `:has` / `:nth-*`; such a selector never matches. Port the rest of
/// selector.ts when an option needs it.
#[derive(Debug)]
pub enum Simple {
    Tag(String),
    Universal,
    Attr {
        key: String,
        op: Option<String>,
        value: String,
        insensitive: bool,
    },
    Class(String),
    Id(String),
    Not(Vec<Vec<Simple>>),
    Is(Vec<Vec<Simple>>),
    Never,
}

pub fn parse_selector(s: &str) -> Vec<Vec<Simple>> {
    split_top(s, ',').into_iter().map(parse_compound).collect()
}

fn split_top(s: &str, sep: char) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut start, mut quote) = (0i32, 0, None::<char>);
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '(' | '[') => depth += 1,
            (None, ')' | ']') => depth -= 1,
            (None, c) if c == sep && depth == 0 => {
                out.push(&s[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

fn ident_end(s: &str, from: usize) -> usize {
    s[from..]
        .char_indices()
        .find(|(_, c)| !(c.is_alphanumeric() || matches!(c, '-' | '_') || !c.is_ascii()))
        .map_or(s.len(), |(i, _)| from + i)
}

fn parse_compound(s: &str) -> Vec<Simple> {
    let s = s.trim();
    let mut out = Vec::new();
    let mut i = 0;
    let b = s.as_bytes();
    while i < b.len() {
        match b[i] {
            b'*' => {
                out.push(Simple::Universal);
                i += 1;
            }
            b'.' | b'#' => {
                let e = ident_end(s, i + 1);
                let v = s[i + 1..e].to_string();
                out.push(if b[i] == b'.' {
                    Simple::Class(v)
                } else {
                    Simple::Id(v)
                });
                i = e.max(i + 1);
            }
            b'[' => {
                let Some(close) = s[i..].find(']').map(|c| c + i) else {
                    return vec![Simple::Never];
                };
                out.push(parse_attr(&s[i + 1..close]));
                i = close + 1;
            }
            b':' => {
                let e = ident_end(s, i + 1);
                let name = &s[i + 1..e];
                let args = if s[e..].starts_with('(') {
                    let mut depth = 0;
                    let mut close = None;
                    for (j, c) in s[e..].char_indices() {
                        match c {
                            '(' => depth += 1,
                            ')' => {
                                depth -= 1;
                                if depth == 0 {
                                    close = Some(e + j);
                                    break;
                                }
                            }
                            _ => {}
                        }
                    }
                    let Some(close) = close else {
                        return vec![Simple::Never];
                    };
                    i = close + 1;
                    Some(&s[e + 1..close])
                } else {
                    i = e;
                    None
                };
                out.push(match (name, args) {
                    ("not", Some(a)) => Simple::Not(parse_selector(a)),
                    ("is" | "where", Some(a)) => Simple::Is(parse_selector(a)),
                    _ => Simple::Never,
                });
            }
            c if c.is_ascii_alphanumeric() || c == b'-' || c == b'_' || !c.is_ascii() => {
                let e = ident_end(s, i);
                out.push(Simple::Tag(s[i..e].to_string()));
                i = e;
            }
            _ => return vec![Simple::Never],
        }
    }
    if out.is_empty() {
        out.push(Simple::Never);
    }
    out
}

fn parse_attr(s: &str) -> Simple {
    let s = s.trim();
    let Some(op_at) = s.find(['=', '~', '|', '^', '$', '*']) else {
        return Simple::Attr {
            key: s.to_string(),
            op: None,
            value: String::new(),
            insensitive: false,
        };
    };
    let key = s[..op_at].trim().to_string();
    let rest = &s[op_at..];
    let op_len = if rest.starts_with('=') { 1 } else { 2 };
    let op = rest[..op_len.min(rest.len())].to_string();
    let mut v = rest[op_len.min(rest.len())..].trim();
    let mut insensitive = false;
    if let Some(x) = v.strip_suffix(" i").or_else(|| v.strip_suffix(" I")) {
        v = x.trim_end();
        insensitive = true;
    }
    let value =
        if v.len() >= 2 && (v.starts_with('"') || v.starts_with('\'')) && v.ends_with(&v[..1]) {
            v[1..v.len() - 1].to_string()
        } else {
            v.to_string()
        };
    Simple::Attr {
        key,
        op: Some(op),
        value,
        insensitive,
    }
}

/// Whether `e` matches a parsed selector list.
pub fn selector_matches(sel: &[Vec<Simple>], e: &Element) -> bool {
    sel.iter().any(|c| c.iter().all(|s| simple_matches(s, e)))
}

fn simple_matches(s: &Simple, e: &Element) -> bool {
    let attr = |k: &str| -> Option<String> {
        e.attr(&k.to_ascii_lowercase()).map(|a| match &a.value {
            Some(AttributeValue::Literal(l)) => l.value.clone(),
            _ => String::new(),
        })
    };
    match s {
        Simple::Tag(t) => &e.raw_name == t,
        Simple::Universal => true,
        Simple::Attr {
            key,
            op,
            value,
            insensitive,
        } => {
            let Some(v) = attr(key) else { return false };
            let Some(op) = op else { return true };
            let (v, val) = if *insensitive {
                (v.to_lowercase(), value.to_lowercase())
            } else {
                (v, value.clone())
            };
            match op.as_str() {
                "=" => v == val,
                "~=" => v.split(is_js_ws).any(|x| x == val),
                "|=" => v == val || v.starts_with(&format!("{val}-")),
                "^=" => v.starts_with(&val),
                "$=" => v.ends_with(&val),
                "*=" => v.contains(&val),
                _ => false,
            }
        }
        Simple::Class(c) => attr("class").is_some_and(|v| v.split(is_js_ws).any(|x| x == c)),
        Simple::Id(i) => attr("id").as_deref() == Some(i),
        Simple::Not(l) => !selector_matches(l, e),
        Simple::Is(l) => selector_matches(l, e),
        Simple::Never => false,
    }
}

/// `ParsedHTMLComment` of `lib/utils/html-comments.ts` (spans instead of tokens).
pub struct HtmlComment {
    pub open: Span,
    pub open_decoration: Option<Span>,
    pub value: Option<Span>,
    pub close_decoration: Option<Span>,
    pub close: Span,
}

/// `html-comments.ts` `defineVisitor`: parsed `<!-- -->` comments of `templateBody.comments`
/// (vue-eslint-parser puts every document comment there; only when a template exists). Skips IE
/// conditional comments and, unless `include_directives`, `eslint-disable/enable` ones.
pub fn html_comments(
    doc: &Document,
    exceptions: &[String],
    include_directives: bool,
) -> Vec<HtmlComment> {
    if doc.template_body.is_none() || has_invalid_eof(doc) {
        return Vec::new();
    }
    let exceptions: Vec<&str> = exceptions
        .iter()
        .map(String::as_str)
        .filter(|e| !e.is_empty())
        .collect();
    let mut out = Vec::new();
    for c in &doc.comments {
        if c.kind != TokenKind::HTMLComment {
            continue;
        }
        let v = c.value.as_str();
        let directive = {
            let t = v.trim_start_matches(is_js_ws);
            t.starts_with("eslint-enable") || t.starts_with("eslint-disable")
        };
        let ie = v.starts_with("[if") && v[3..].starts_with(is_js_ws) || v.ends_with("[endif]");
        if (!include_directives && directive) || ie {
            continue;
        }
        let text = doc.slice(c.span);
        if !text.starts_with("<!--") || !text.ends_with("-->") || text.len() < 7 {
            continue;
        }
        let mut value = &text[4..text.len() - 3];
        let mut open_dec = "";
        for e in &exceptions {
            let mut i = 0;
            while value[i..].starts_with(e) {
                i += e.len();
            }
            if open_dec.len() < i {
                open_dec = &value[..i];
            }
        }
        value = &value[open_dec.len()..];
        let before = value.len() - value.trim_start_matches(is_js_ws).len();
        value = &value[before..];
        let mut close_dec = "";
        for e in &exceptions {
            let mut i = value.len();
            while value[..i].ends_with(e) {
                i -= e.len();
            }
            if close_dec.len() < value.len() - i {
                close_dec = &value[i..];
            }
        }
        value = &value[..value.len() - close_dec.len()];
        let trimmed = value.trim_end_matches(is_js_ws);
        let after = if trimmed.is_empty() {
            value.len()
        } else {
            value.len() - trimmed.len()
        };
        value = &value[..value.len() - after];
        let mut pos = c.span.start;
        let mut take = |len: usize| {
            let s = Span::new(pos, pos + len as u32);
            pos = s.end;
            s
        };
        let open = take(4);
        let open_decoration = (!open_dec.is_empty()).then(|| take(open_dec.len()));
        take(before);
        let value_span = (!value.is_empty()).then(|| take(value.len()));
        take(after);
        let close_decoration = (!close_dec.is_empty()).then(|| take(close_dec.len()));
        let close = take(3);
        out.push(HtmlComment {
            open,
            open_decoration,
            value: value_span,
            close_decoration,
            close,
        });
    }
    out
}

/// Tokens and comments inside `span`, merged in source order (`getTokens(node, {includeComments})`).
pub fn tokens_with_comments<'d>(doc: &'d Document, span: Span) -> Vec<&'d Token> {
    let mut v: Vec<&Token> = doc
        .tokens_in(span)
        .iter()
        .chain(comments_in(doc, span))
        .collect();
    v.sort_by_key(|t| t.span.start);
    v
}

/// Content-newline helpers: first / last non-whitespace token or comment after `pos` / before `pos`.
pub fn non_ws_after<'d>(doc: &'d Document, pos: u32) -> Option<&'d Token> {
    let end = doc.source.len() as u32;
    let t = doc
        .tokens_in(Span::new(pos, end))
        .iter()
        .find(|t| t.kind != TokenKind::HTMLWhitespace);
    let c = comments_in(doc, Span::new(pos, end)).first();
    match (t, c) {
        (Some(t), Some(c)) => Some(if c.span.start < t.span.start { c } else { t }),
        (t, c) => t.or(c),
    }
}

pub fn non_ws_before<'d>(doc: &'d Document, pos: u32) -> Option<&'d Token> {
    let t = doc
        .tokens_in(Span::new(0, pos))
        .iter()
        .rev()
        .find(|t| t.kind != TokenKind::HTMLWhitespace);
    let c = comments_in(doc, Span::new(0, pos)).last();
    match (t, c) {
        (Some(t), Some(c)) => Some(if c.span.start > t.span.start { c } else { t }),
        (t, c) => t.or(c),
    }
}
