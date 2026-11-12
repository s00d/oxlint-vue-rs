//! Port of vue-eslint-parser `src/html/intermediate-tokenizer.ts`: groups raw HTML tokens into
//! start tags, end tags, texts and mustaches; keeps the flat token / comment lists.

use super::tokenizer::{HToken, State, Tokenizer};
use crate::ast::{Namespace, TokenKind};

#[derive(Debug)]
pub(super) struct RawAttr {
    pub start: u32,
    pub end: u32,
    /// Lowercased name.
    pub name: String,
    pub key_start: u32,
    pub key_end: u32,
    /// `(start, end, decoded value)`.
    pub value: Option<(u32, u32, String)>,
}

#[derive(Debug)]
pub(super) struct StartTag {
    pub start: u32,
    pub end: u32,
    pub name: String,
    pub raw_name: String,
    pub self_closing: bool,
    pub attributes: Vec<RawAttr>,
}

#[derive(Debug)]
pub(super) enum IToken {
    StartTag(StartTag),
    EndTag {
        start: u32,
        end: u32,
        name: String,
    },
    Text {
        start: u32,
        end: u32,
        value: String,
    },
    /// `start_tok_end` = end of `{{`, `end_tok_start` = start of `}}`.
    Mustache {
        start: u32,
        end: u32,
        value: String,
        start_tok_end: u32,
        end_tok_start: u32,
    },
}

impl IToken {
    fn end_mut(&mut self) -> &mut u32 {
        match self {
            IToken::StartTag(t) => &mut t.end,
            IToken::EndTag { end, .. }
            | IToken::Text { end, .. }
            | IToken::Mustache { end, .. } => end,
        }
    }
    fn end(&self) -> u32 {
        match self {
            IToken::StartTag(t) => t.end,
            IToken::EndTag { end, .. }
            | IToken::Text { end, .. }
            | IToken::Mustache { end, .. } => *end,
        }
    }
}

pub(super) struct Intermediate<'a> {
    pub tokenizer: Tokenizer<'a>,
    current: Option<IToken>,
    /// Whether the last attribute of the current start tag is "open" (`this.attribute != null`).
    attribute: bool,
    attribute_names: Vec<String>,
    /// Index into `tokens` of the pending `{{`.
    expression_start: Option<usize>,
    /// Indices into `tokens` of the deferred tokens after `{{`.
    expression_tokens: Vec<usize>,
    pub tokens: Vec<HToken>,
    pub comments: Vec<HToken>,
}

impl<'a> Intermediate<'a> {
    pub fn new(tokenizer: Tokenizer<'a>) -> Self {
        Self {
            tokenizer,
            current: None,
            attribute: false,
            attribute_names: Vec::new(),
            expression_start: None,
            expression_tokens: Vec::new(),
            tokens: Vec::new(),
            comments: Vec::new(),
        }
    }

    pub fn set_state(&mut self, s: State) {
        self.tokenizer.state = s;
    }
    pub fn namespace(&self) -> Namespace {
        self.tokenizer.namespace
    }
    pub fn set_namespace(&mut self, ns: Namespace) {
        self.tokenizer.namespace = ns;
    }
    pub fn expression_enabled(&self) -> bool {
        self.tokenizer.expression_enabled
    }
    pub fn set_expression_enabled(&mut self, v: bool) {
        self.tokenizer.expression_enabled = v;
    }

    pub fn next_token(&mut self) -> Option<IToken> {
        let mut result = None;
        let mut saw_none = false;
        while result.is_none() {
            let Some(token) = self.tokenizer.next_token() else {
                saw_none = true;
                break;
            };
            result = self.process(token);
        }
        if result.is_none()
            && saw_none
            && (self.current.is_some() || self.expression_start.is_some())
        {
            result = self.commit();
        }
        result
    }

    fn report(&mut self, offset: u32, code: &'static str) {
        // Interleave with tokenizer errors in emission order (JS shares one array).
        self.tokenizer.errors.push((code, offset));
    }

    fn commit(&mut self) -> Option<IToken> {
        let mut token = self.current.take();
        self.attribute = false;
        if let Some(si) = self.expression_start.take() {
            // VExpressionEnd was not found: concatenate the deferred tokens to the committed token.
            let exprs = std::mem::take(&mut self.expression_tokens);
            let start = &self.tokens[si];
            let end = exprs.last().map_or(start, |&i| &self.tokens[i]);
            let (s0, e1) = (start.start, end.end);
            let mut value = start.value.clone();
            for &i in &exprs {
                value.push_str(&self.tokens[i].value);
            }
            match &mut token {
                None => {
                    token = Some(IToken::Text {
                        start: s0,
                        end: e1,
                        value,
                    })
                }
                Some(IToken::Text { end, value: v, .. }) => {
                    *end = e1;
                    v.push_str(&value);
                }
                // ponytail: unreachable in JS (throws); keep the token as is.
                Some(_) => {}
            }
        }
        token
    }

    fn process(&mut self, token: HToken) -> Option<IToken> {
        match token.kind {
            TokenKind::HTMLAssociation => {
                let end = token.end;
                self.tokens.push(token);
                if self.attribute
                    && let Some(IToken::StartTag(t)) = &mut self.current
                {
                    if let Some(a) = t.attributes.last_mut() {
                        a.end = end;
                    }
                    t.end = end;
                }
                None
            }
            TokenKind::HTMLBogusComment | TokenKind::HTMLComment => {
                self.comments.push(token);
                if matches!(self.current, Some(IToken::Text { .. })) {
                    return self.commit();
                }
                None
            }
            TokenKind::HTMLCDataText
            | TokenKind::HTMLRCDataText
            | TokenKind::HTMLRawText
            | TokenKind::HTMLText
            | TokenKind::HTMLWhitespace => self.process_text(token),
            TokenKind::HTMLEndTagOpen => {
                let (start, end, name) = (token.start, token.end, token.value.clone());
                self.tokens.push(token);
                let mut result = None;
                if self.current.is_some() || self.expression_start.is_some() {
                    result = self.commit();
                }
                self.current = Some(IToken::EndTag { start, end, name });
                result
            }
            TokenKind::HTMLIdentifier => {
                let (start, end) = (token.start, token.end);
                let name = token.value.clone();
                self.tokens.push(token);
                match &self.current {
                    Some(IToken::EndTag { .. }) => {
                        self.report(start, "end-tag-with-attributes");
                        return None;
                    }
                    Some(IToken::StartTag(_)) => {}
                    _ => return None,
                }
                if self.attribute_names.contains(&name) {
                    self.report(start, "duplicate-attribute");
                }
                self.attribute_names.push(name.clone());
                if let Some(IToken::StartTag(t)) = &mut self.current {
                    t.attributes.push(RawAttr {
                        start,
                        end,
                        name,
                        key_start: start,
                        key_end: end,
                        value: None,
                    });
                    t.end = end;
                }
                self.attribute = true;
                None
            }
            TokenKind::HTMLLiteral => {
                let (start, end) = (token.start, token.end);
                let value = token.value.clone();
                self.tokens.push(token);
                if self.attribute
                    && let Some(IToken::StartTag(t)) = &mut self.current
                {
                    if let Some(a) = t.attributes.last_mut() {
                        a.end = end;
                        a.value = Some((start, end, value));
                    }
                    t.end = end;
                }
                None
            }
            TokenKind::HTMLSelfClosingTagClose => {
                let (start, end) = (token.start, token.end);
                self.tokens.push(token);
                match &mut self.current {
                    Some(IToken::StartTag(t)) => t.self_closing = true,
                    Some(IToken::EndTag { .. }) => {
                        self.report(start, "end-tag-with-trailing-solidus")
                    }
                    _ => return None,
                }
                if let Some(c) = &mut self.current {
                    *c.end_mut() = end;
                }
                self.commit()
            }
            TokenKind::HTMLTagClose => {
                let end = token.end;
                self.tokens.push(token);
                match &mut self.current {
                    Some(IToken::StartTag(_))
                    | Some(IToken::EndTag { .. })
                    | Some(IToken::Mustache { .. }) => {}
                    _ => return None,
                }
                if let Some(c) = &mut self.current {
                    *c.end_mut() = end;
                }
                self.commit()
            }
            TokenKind::HTMLTagOpen => {
                let (start, end) = (token.start, token.end);
                let name = token.value.clone();
                let text = self.tokenizer.text();
                let raw_name = text
                    .get(start as usize + 1..end as usize)
                    .unwrap_or("")
                    .to_string();
                self.tokens.push(token);
                let mut result = None;
                if self.current.is_some() || self.expression_start.is_some() {
                    result = self.commit();
                }
                self.current = Some(IToken::StartTag(StartTag {
                    start,
                    end,
                    name,
                    raw_name,
                    self_closing: false,
                    attributes: Vec::new(),
                }));
                self.attribute = false;
                self.attribute_names.clear();
                result
            }
            TokenKind::VExpressionStart => {
                if self.expression_start.is_some() {
                    return self.process_text(token);
                }
                let separated = self
                    .current
                    .as_ref()
                    .is_some_and(|c| c.end() != token.start);
                let result = if separated { self.commit() } else { None };
                self.tokens.push(token);
                self.expression_start = Some(self.tokens.len() - 1);
                result
            }
            TokenKind::VExpressionEnd => {
                let Some(si) = self.expression_start else {
                    return self.process_text(token);
                };
                let start_end = self.tokens[si].end;
                let end_end = self
                    .expression_tokens
                    .last()
                    .map_or(start_end, |&i| self.tokens[i].end);
                // `{{}}` is handled as a text.
                if token.start == start_end {
                    let start_tok = self.tokens.remove(si);
                    self.expression_start = None;
                    let result = self.process_text(start_tok);
                    self.process_text(token);
                    return result;
                }
                // If invalid notation `</>` exists directly before this token, separate it.
                if end_end != token.start {
                    let result = self.commit();
                    self.process_text(token);
                    return result;
                }
                let exprs = std::mem::take(&mut self.expression_tokens);
                let mut value = String::new();
                for &i in &exprs {
                    value.push_str(&self.tokens[i].value);
                }
                let start = self.tokens[si].start;
                let (start_tok_end, end_tok_start, end) = (start_end, token.start, token.end);
                self.tokens.push(token);
                self.expression_start = None;
                let result = if self.current.is_some() {
                    self.commit()
                } else {
                    None
                };
                self.current = Some(IToken::Mustache {
                    start,
                    end,
                    value,
                    start_tok_end,
                    end_tok_start,
                });
                result.or_else(|| self.commit())
            }
            _ => None,
        }
    }

    fn process_text(&mut self, token: HToken) -> Option<IToken> {
        let (start, end) = (token.start, token.end);
        let value = token.value.clone();
        self.tokens.push(token);
        let mut result = None;
        if let Some(si) = self.expression_start {
            // Defer this token until a VExpressionEnd token or a non-text token appear.
            let last_end = self
                .expression_tokens
                .last()
                .map_or(self.tokens[si].end, |&i| self.tokens[i].end);
            if last_end == start {
                self.expression_tokens.push(self.tokens.len() - 1);
                return None;
            }
            result = self.commit();
        } else if let Some(cur) = &mut self.current {
            if let IToken::Text {
                end: e, value: v, ..
            } = cur
                && *e == start
            {
                v.push_str(&value);
                *e = end;
                return None;
            }
            result = self.commit();
        }
        self.current = Some(IToken::Text { start, end, value });
        result
    }
}
