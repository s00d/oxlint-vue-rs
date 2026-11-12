//! Port of vue-eslint-parser `src/template/index.ts`: directive key parsing, directive values,
//! mustaches, dynamic arguments, v-bind same-name shorthand.

use oxc::span::Span;

use super::html::{is_script_setup, is_ts_lang};
use super::script::{ExprError, ExprParser, ExprResult, Mapper};
use crate::ast::*;

/// Output of the post-processing phase.
pub(super) struct Post<'a> {
    pub exprs: Vec<ExprContainer<'a>>,
    /// Token replacements: (range, new tokens). Ranges are disjoint.
    pub replacements: Vec<(Span, Vec<Token>)>,
    pub comments: Vec<Token>,
    pub errors: Vec<ParseError>,
}

impl<'a> Post<'a> {
    fn insert_error(&mut self, message: String, offset: u32) {
        let i = self.errors.partition_point(|e| e.offset < offset);
        self.errors.insert(
            i,
            ParseError {
                code: None,
                message,
                offset,
            },
        );
    }
    fn insert_expr_error(&mut self, e: ExprError) {
        self.insert_error(e.message, e.offset);
    }
}

fn ident(
    text: &str,
    raw: &str,
    offset: u32,
    start: usize,
    end: usize,
    name: Option<&str>,
) -> VIdentifier {
    VIdentifier {
        span: Span::new(offset + start as u32, offset + end as u32),
        name: name.map_or_else(
            || text.get(start..end).unwrap_or("").to_string(),
            str::to_string,
        ),
        raw_name: raw.get(start..end).unwrap_or("").to_string(),
    }
}

fn char_at(s: &str, i: usize) -> String {
    s.get(i..)
        .and_then(|r| r.chars().next())
        .map_or_else(|| "undefined".to_string(), |c| c.to_string())
}

struct StaticKey {
    span: Span,
    name: VIdentifier,
    argument: Option<VIdentifier>,
    modifiers: Vec<VIdentifier>,
}

/// `parseDirectiveKeyStatically`.
fn parse_key_statically(key: &VIdentifier, post: &mut Post) -> StaticKey {
    let text = key.name.as_str();
    let raw = key.raw_name.as_str();
    let offset = key.span.start;
    let mut i = 0usize;
    let mut name = None;
    let mut argument = None;
    if let Some(sign) = text
        .chars()
        .next()
        .filter(|c| matches!(c, '.' | ':' | '@' | '#'))
    {
        let n = match sign {
            ':' | '.' => "bind",
            '@' => "on",
            _ => "slot",
        };
        name = Some(ident(text, raw, offset, 0, 1, Some(n)));
        i = 1;
    } else if let Some(colon) = text.find(':') {
        name = Some(ident(text, raw, offset, 0, colon, None));
        i = colon + 1;
    }
    if name.is_some()
        && text.as_bytes().get(i) == Some(&b'[')
        && let Some(len) = text[i..].rfind(']')
    {
        argument = Some(ident(text, raw, offset, i, i + len + 1, None));
        i = i + len + 1 + usize::from(text.as_bytes().get(i + len + 1) == Some(&b'.'));
    }
    let rest = text.get(i..).unwrap_or("");
    let mut modifiers = Vec::new();
    for m in rest.split('.') {
        let id = ident(text, raw, offset, i, i + m.len(), None);
        if m.is_empty() && i < text.len() {
            post.insert_error(
                format!("Unexpected token '{}'", char_at(text, i)),
                offset + i as u32,
            );
        }
        i += m.len() + 1;
        modifiers.push(id);
    }
    let name = match name {
        Some(n) => {
            if argument.is_none() && modifiers.first().is_some_and(|m| !m.name.is_empty()) {
                argument = Some(modifiers.remove(0));
            }
            n
        }
        None => modifiers.remove(0),
    };
    modifiers.retain(|m| !m.name.is_empty());
    if name.name == "v-" {
        let at = (name.span.end - offset) as usize;
        post.insert_error(
            format!("Unexpected token '{}'", char_at(text, at)),
            name.span.end,
        );
    }
    // v-bind.prop shorthand
    if name.raw_name == "." && !modifiers.iter().any(|m| m.name == "prop") {
        let pos = (argument.as_ref().unwrap_or(&name).span.end - offset) as usize;
        modifiers.insert(0, ident(text, raw, offset, pos, pos, Some("prop")));
    }
    StaticKey {
        span: key.span,
        name,
        argument,
        modifiers,
    }
}

fn tok(kind: TokenKind, span: Span, value: &str) -> Token {
    Token {
        kind,
        span,
        value: value.to_string(),
    }
}

/// `parseDirectiveKeyTokens`. The argument token is at the returned index (if any).
fn key_tokens(k: &StaticKey) -> (Vec<Token>, Option<usize>) {
    let mut tokens = Vec::new();
    let name = &k.name;
    let shorthand = name.span.end - name.span.start == 1;
    if shorthand {
        tokens.push(tok(TokenKind::Punctuator, name.span, &name.raw_name));
    } else {
        tokens.push(tok(TokenKind::HTMLIdentifier, name.span, &name.raw_name));
        if let Some(arg) = &k.argument {
            tokens.push(tok(
                TokenKind::Punctuator,
                Span::new(name.span.end, arg.span.start),
                ":",
            ));
        }
    }
    let mut arg_index = None;
    if let Some(arg) = &k.argument {
        arg_index = Some(tokens.len());
        tokens.push(tok(TokenKind::HTMLIdentifier, arg.span, &arg.raw_name));
    }
    let mut last = k.argument.as_ref().unwrap_or(name).span.end;
    for m in &k.modifiers {
        if m.raw_name.is_empty() {
            continue;
        }
        tokens.push(tok(
            TokenKind::Punctuator,
            Span::new(last, m.span.start),
            ".",
        ));
        tokens.push(tok(TokenKind::HTMLIdentifier, m.span, &m.raw_name));
        last = m.span.end;
    }
    (tokens, arg_index)
}

/// `camelize`: `-(\w)` → uppercase.
fn camelize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '-'
            && let Some(&n) = chars.peek()
            && (n.is_ascii_alphanumeric() || n == '_')
        {
            out.push(n.to_ascii_uppercase());
            chars.next();
            continue;
        }
        out.push(c);
    }
    out
}

fn is_invalid_dynamic_argument_next_char(c: char) -> bool {
    super::script::is_js_space(c) || matches!(c, '=' | '/' | '>')
}

fn json_escape_char(c: char) -> String {
    match c {
        '\n' => "\\n".into(),
        '\r' => "\\r".into(),
        '\t' => "\\t".into(),
        '\u{8}' => "\\b".into(),
        '\u{c}' => "\\f".into(),
        '"' => "\\\"".into(),
        '\\' => "\\\\".into(),
        c if (c as u32) < 0x20 => format!("\\u{:04x}", c as u32),
        c => c.to_string(),
    }
}

pub(super) struct Converter<'a, 'g> {
    pub ep: ExprParser<'a, 'g>,
    pub source: &'a str,
    pub post: Post<'a>,
}

impl<'a, 'g> Converter<'a, 'g> {
    fn mapper(&self, base: u32) -> Mapper<'g> {
        Mapper::new(self.ep.gaps, base)
    }

    fn push_expr(&mut self, span: Span, parent: ExprParent, r: Option<ExprResult<'a>>) -> ExprId {
        let id = ExprId(self.post.exprs.len() as u32);
        let (expression, references) = match r {
            Some(r) => (r.expression, r.references),
            None => (None, Vec::new()),
        };
        self.post.exprs.push(ExprContainer {
            id,
            span,
            parent,
            expression,
            references,
        });
        id
    }

    /// `processMustache`.
    pub fn mustache(&mut self, expr: ExprId, value: &str, start: u32, end: u32) {
        let m = self.mapper(start);
        match self.ep.parse_expression(value, &m, true, true) {
            Ok(r) => {
                self.post
                    .replacements
                    .push((Span::new(start, end), r.tokens));
                self.post.comments.extend(r.comments);
                let c = &mut self.post.exprs[expr.0 as usize];
                c.expression = r.expression;
                c.references = r.references;
            }
            Err(e) => self.post.insert_expr_error(e),
        }
    }

    /// `convertToDirective`.
    pub fn directive(&mut self, elements: &mut [Element], el: ElementId, idx: u32) {
        let element = &elements[el.0 as usize];
        let attr = &element.start_tag.attributes[idx as usize];
        let AttributeKey::Plain(key) = &attr.key else {
            return;
        };
        let key = key.clone();
        let literal = match &attr.value {
            Some(AttributeValue::Literal(l)) => Some(l.clone()),
            _ => None,
        };
        let el_raw_name = element.raw_name.clone();
        let is_generic_host = element.parent.is_none()
            && el_raw_name == "script"
            && is_script_setup(element)
            && is_ts_lang(element);

        // createDirectiveKey
        let mut sk = parse_key_statically(&key, &mut self.post);
        let (mut tokens, arg_index) = key_tokens(&sk);
        if sk.name.name.starts_with("v-") {
            sk.name.name.drain(..2);
        }
        if sk.name.raw_name.starts_with("v-") {
            sk.name.raw_name.drain(..2);
        }
        let mut argument = sk.argument.take().map(DirectiveArgument::Static);
        // convertDynamicArgument
        if let Some(DirectiveArgument::Static(a)) = &argument
            && a.name.starts_with('[')
            && a.name.ends_with(']')
            && a.raw_name.len() >= 2
        {
            let span = a.span;
            let code = &a.raw_name[1..a.raw_name.len() - 1];
            let m = self.mapper(span.start + 1);
            match self.ep.parse_expression(code, &m, false, false) {
                Ok(r) => {
                    let mut arg_tokens = Vec::with_capacity(r.tokens.len() + 2);
                    arg_tokens.push(tok(
                        TokenKind::Punctuator,
                        Span::new(span.start, span.start + 1),
                        "[",
                    ));
                    arg_tokens.extend(r.tokens.iter().cloned());
                    arg_tokens.push(tok(
                        TokenKind::Punctuator,
                        Span::new(span.end - 1, span.end),
                        "]",
                    ));
                    if let Some(ai) = arg_index {
                        tokens.splice(ai..ai + 1, arg_tokens);
                    }
                    self.post.comments.extend(r.comments.iter().cloned());
                    let id = self.push_expr(span, ExprParent::DirectiveArgument(el, idx), Some(r));
                    argument = Some(DirectiveArgument::Dynamic(id));
                }
                Err(e) => {
                    self.post.insert_expr_error(e);
                    let id = self.push_expr(span, ExprParent::DirectiveArgument(el, idx), None);
                    argument = Some(DirectiveArgument::Dynamic(id));
                }
            }
        }
        self.post.replacements.push((key.span, tokens));

        if let Some(DirectiveArgument::Static(a)) = &argument
            && a.name.starts_with('[')
        {
            let next = self
                .source
                .get(a.span.end as usize..)
                .and_then(|s| s.chars().next());
            if next.is_none_or(is_invalid_dynamic_argument_next_char) {
                let ch = next.map_or_else(|| "EOF".to_string(), json_escape_char);
                self.post.insert_error(
                    format!("Dynamic argument cannot contain the '{ch}' character."),
                    a.span.end,
                );
            }
        }

        let dkey = DirectiveKey {
            span: sk.span,
            name: sk.name,
            argument,
            modifiers: sk.modifiers,
        };
        let name = dkey.name.name.clone();
        let mut value = None;

        match literal {
            None => {
                if name == "bind"
                    && let Some(DirectiveArgument::Static(a)) = &dkey.argument
                    && let Some((expr, reference)) = self
                        .ep
                        .parse_same_name_identifier(&camelize(&a.raw_name), a.span)
                {
                    let id = ExprId(self.post.exprs.len() as u32);
                    self.post.exprs.push(ExprContainer {
                        id,
                        span: a.span,
                        parent: ExprParent::DirectiveValue(el, idx),
                        expression: Some(VExpression::Expression(expr)),
                        references: vec![reference],
                    });
                    value = Some(AttributeValue::Expr(id));
                }
            }
            Some(lit) => {
                let first = self.source.as_bytes().get(lit.span.start as usize).copied();
                let quoted = matches!(first, Some(b'"' | b'\''));
                let m = self.mapper(lit.span.start + u32::from(quoted));
                let has_arg = dkey.argument.is_some();
                let kind = if name == "for" {
                    "for"
                } else if name == "on" {
                    "on"
                } else if name == "slot"
                    || name == "slot-scope"
                    || (name == "scope" && el_raw_name == "template")
                {
                    "slot"
                } else if name == "bind" {
                    "bind"
                } else if name == "generic" && is_generic_host {
                    "generic"
                } else {
                    ""
                };
                let code = lit.value.as_str();
                let result = if quoted && code.is_empty() {
                    Ok(ExprResult::empty())
                } else {
                    match kind {
                        "for" => self.ep.parse_v_for(code, &m),
                        "on" if has_arg => self.ep.parse_v_on(code, &m),
                        "slot" => self.ep.parse_slot_scope(code, &m),
                        "bind" => self.ep.parse_expression(code, &m, false, true),
                        "generic" => self.ep.parse_generic(code, &m),
                        _ => self.ep.parse_expression(code, &m, false, false),
                    }
                };
                match result {
                    Ok(mut r) => {
                        let mut toks = std::mem::take(&mut r.tokens);
                        if quoted {
                            let q = (first.unwrap_or(b'"') as char).to_string();
                            toks.insert(
                                0,
                                tok(
                                    TokenKind::Punctuator,
                                    Span::new(lit.span.start, lit.span.start + 1),
                                    &q,
                                ),
                            );
                            toks.push(tok(
                                TokenKind::Punctuator,
                                Span::new(lit.span.end - 1, lit.span.end),
                                &q,
                            ));
                        }
                        self.post.replacements.push((lit.span, toks));
                        self.post.comments.append(&mut r.comments);
                        elements[el.0 as usize].variables.append(&mut r.variables);
                        let id =
                            self.push_expr(lit.span, ExprParent::DirectiveValue(el, idx), Some(r));
                        value = Some(AttributeValue::Expr(id));
                    }
                    Err(e) => {
                        self.post.insert_expr_error(e);
                        let id =
                            self.push_expr(lit.span, ExprParent::DirectiveValue(el, idx), None);
                        value = Some(AttributeValue::Expr(id));
                    }
                }
            }
        }
        let attr = &mut elements[el.0 as usize].start_tag.attributes[idx as usize];
        attr.key = AttributeKey::Directive(dkey);
        attr.value = value;
    }
}

/// Resolve container references to element variables (`resolveReferences`).
pub(super) fn resolve_references(elements: &[Element], exprs: &mut [ExprContainer]) {
    for c in exprs.iter_mut() {
        let start = match c.parent {
            ExprParent::Element(e) => e,
            ExprParent::DirectiveValue(e, _) | ExprParent::DirectiveArgument(e, _) => Some(e),
        };
        for r in &mut c.references {
            let mut cur = start;
            'outer: while let Some(e) = cur {
                let el = &elements[e.0 as usize];
                for (i, v) in el.variables.iter().enumerate() {
                    if v.name == r.name {
                        r.variable = Some((e, i as u32));
                        break 'outer;
                    }
                }
                cur = el.parent;
            }
        }
    }
}
