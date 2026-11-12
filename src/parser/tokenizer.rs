//! Port of vue-eslint-parser `src/html/tokenizer.ts` (HTML spec tokenizer with Vue extensions).
//!
//! Differences from the JS original: offsets are UTF-8 byte offsets (not UTF-16 units), gaps are
//! byte offsets too, and line/column tracking is dropped (consumers compute it from offsets).

use super::entities::{ENTITIES, LENGTHS};
use crate::ast::{Namespace, TokenKind};

pub(super) const EOF: i32 = -1;
const NULL: i32 = 0x00;
const TAB: i32 = 0x09;
const LF: i32 = 0x0a;
const FF: i32 = 0x0c;
const CR: i32 = 0x0d;
const SPACE: i32 = 0x20;
const NULL_REPLACEMENT: i32 = 0xfffd;

/// A raw HTML token (`range` + processed `value`).
#[derive(Debug, Clone)]
pub(super) struct HToken {
    pub kind: TokenKind,
    pub start: u32,
    pub end: u32,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum State {
    Data,
    TagOpen,
    EndTagOpen,
    TagName,
    Rcdata,
    RcdataLessThanSign,
    RcdataEndTagOpen,
    RcdataEndTagName,
    Rawtext,
    RawtextLessThanSign,
    RawtextEndTagOpen,
    RawtextEndTagName,
    BeforeAttributeName,
    AttributeName,
    AfterAttributeName,
    BeforeAttributeValue,
    AttributeValueDoubleQuoted,
    AttributeValueSingleQuoted,
    AttributeValueUnquoted,
    AfterAttributeValueQuoted,
    SelfClosingStartTag,
    BogusComment,
    MarkupDeclarationOpen,
    CommentStart,
    CommentStartDash,
    Comment,
    CommentLessThanSign,
    CommentLessThanSignBang,
    CommentLessThanSignBangDash,
    CommentLessThanSignBangDashDash,
    CommentEndDash,
    CommentEnd,
    CommentEndBang,
    CdataSection,
    CdataSectionBracket,
    CdataSectionEnd,
    CharacterReference,
    NamedCharacterReference,
    AmbiguousAmpersand,
    NumericCharacterReference,
    HexCharacterReferenceStart,
    DecimalCharacterReferenceStart,
    HexCharacterReference,
    DecimalCharacterReference,
    NumericCharacterReferenceEnd,
    CharacterReferenceEnd,
    VExpressionStart,
    VExpressionData,
    VExpressionEnd,
}

impl State {
    fn is_provisional(self) -> bool {
        matches!(
            self,
            State::RcdataLessThanSign
                | State::RcdataEndTagOpen
                | State::RcdataEndTagName
                | State::RawtextLessThanSign
                | State::RawtextEndTagOpen
                | State::RawtextEndTagName
        )
    }
    /// `returnState.startsWith("ATTR")`.
    fn is_attr(self) -> bool {
        matches!(
            self,
            State::AttributeName
                | State::AttributeValueDoubleQuoted
                | State::AttributeValueSingleQuoted
                | State::AttributeValueUnquoted
        )
    }
}

fn is_ws(cp: i32) -> bool {
    matches!(cp, TAB | LF | FF | CR | SPACE)
}
fn is_upper(cp: i32) -> bool {
    (0x41..=0x5a).contains(&cp)
}
fn is_lower(cp: i32) -> bool {
    (0x61..=0x7a).contains(&cp)
}
fn is_letter(cp: i32) -> bool {
    is_upper(cp) || is_lower(cp)
}
fn is_digit(cp: i32) -> bool {
    (0x30..=0x39).contains(&cp)
}
fn is_upper_hex(cp: i32) -> bool {
    (0x41..=0x46).contains(&cp)
}
fn is_lower_hex(cp: i32) -> bool {
    (0x61..=0x66).contains(&cp)
}
fn is_hex(cp: i32) -> bool {
    is_digit(cp) || is_upper_hex(cp) || is_lower_hex(cp)
}
fn is_control(cp: i32) -> bool {
    (0..=0x1f).contains(&cp) || (0x7f..=0x9f).contains(&cp)
}
fn is_surrogate(cp: i64) -> bool {
    (0xd800..=0xdfff).contains(&cp)
}
fn is_non_character(cp: i64) -> bool {
    (0xfdd0..=0xfdef).contains(&cp) || ((cp & 0xfffe) == 0xfffe && cp <= 0x10ffff)
}
fn lower(cp: i32) -> i32 {
    if is_upper(cp) { cp + 0x20 } else { cp }
}

fn alternative_cr(code: i64) -> Option<i64> {
    Some(match code {
        128 => 8364,
        130 => 8218,
        131 => 402,
        132 => 8222,
        133 => 8230,
        134 => 8224,
        135 => 8225,
        136 => 710,
        137 => 8240,
        138 => 352,
        139 => 8249,
        140 => 338,
        142 => 381,
        145 => 8216,
        146 => 8217,
        147 => 8220,
        148 => 8221,
        149 => 8226,
        150 => 8211,
        151 => 8212,
        152 => 732,
        153 => 8482,
        154 => 353,
        155 => 8250,
        156 => 339,
        158 => 382,
        159 => 376,
        _ => return None,
    })
}

fn push_cp(s: &mut String, cp: i32) {
    // Code points here are always valid scalar values (surrogates were replaced upstream).
    s.push(char::from_u32(cp as u32).unwrap_or('\u{fffd}'));
}

pub(super) struct Tokenizer<'a> {
    text: &'a str,
    pub gaps: Vec<u32>,
    last_cp: i32,
    last_cp_raw: i32,
    offset: usize,
    /// Byte length of the code point at `offset` (0 before the first one / at EOF).
    cur_len: usize,
    return_state: State,
    v_expr_script_state: Option<State>,
    reconsuming: bool,
    buffer: Vec<i32>,
    cr_start_offset: usize,
    cr_code: i64,
    committed: Option<HToken>,
    provisional: Option<HToken>,
    current: Option<HToken>,
    last_tag_open_value: Option<String>,
    token_start: Option<usize>,
    pub state: State,
    /// (code, byte offset).
    pub errors: Vec<(&'static str, u32)>,
    pub namespace: Namespace,
    pub expression_enabled: bool,
    /// Cache for the `}}` lookahead: (searched from, found at).
    close_cache: Option<(usize, Option<usize>)>,
}

impl<'a> Tokenizer<'a> {
    pub fn new(text: &'a str) -> Self {
        Self {
            text,
            gaps: Vec::new(),
            last_cp: NULL,
            last_cp_raw: NULL,
            offset: 0,
            cur_len: 0,
            return_state: State::Data,
            v_expr_script_state: None,
            reconsuming: false,
            buffer: Vec::new(),
            cr_start_offset: 0,
            cr_code: 0,
            committed: None,
            provisional: None,
            current: None,
            last_tag_open_value: None,
            token_start: None,
            state: State::Data,
            errors: Vec::new(),
            namespace: Namespace::Html,
            expression_enabled: false,
            close_cache: None,
        }
    }

    pub fn text(&self) -> &'a str {
        self.text
    }

    pub fn next_token(&mut self) -> Option<HToken> {
        let mut cp = self.last_cp;
        while self.committed.is_none() && (cp != EOF || self.reconsuming) {
            if self.provisional.is_some() && !self.state.is_provisional() {
                self.commit_provisional_token();
                if self.committed.is_some() {
                    break;
                }
            }
            if self.reconsuming {
                self.reconsuming = false;
                cp = self.last_cp;
            } else {
                cp = self.consume_next_code_point();
            }
            let s = self.state;
            self.state = self.dispatch(s, cp);
        }
        if let Some(t) = self.committed.take() {
            return Some(t);
        }
        if self.current.is_some() {
            self.end_token();
            if let Some(t) = self.committed.take() {
                return Some(t);
            }
        }
        // A provisional token can be left behind at EOF; flush it so no text is lost.
        if let Some(t) = self.provisional.take()
            && t.start < t.end
        {
            return Some(t);
        }
        None
    }

    fn dispatch(&mut self, s: State, cp: i32) -> State {
        match s {
            State::Data => self.data(cp),
            State::TagOpen => self.tag_open(cp),
            State::EndTagOpen => self.end_tag_open(cp),
            State::TagName => self.tag_name(cp),
            State::Rcdata => self.text_state(cp, State::Rcdata),
            State::RcdataLessThanSign => self.lt_sign(cp, State::RcdataEndTagOpen, State::Rcdata),
            State::RcdataEndTagOpen => {
                self.end_tag_open_in(cp, State::RcdataEndTagName, State::Rcdata)
            }
            State::RcdataEndTagName => self.rcdata_end_tag_name(cp),
            State::Rawtext => self.text_state(cp, State::Rawtext),
            State::RawtextLessThanSign => {
                self.lt_sign(cp, State::RawtextEndTagOpen, State::Rawtext)
            }
            State::RawtextEndTagOpen => {
                self.end_tag_open_in(cp, State::RawtextEndTagName, State::Rawtext)
            }
            State::RawtextEndTagName => self.rawtext_end_tag_name(cp),
            State::BeforeAttributeName => self.before_attribute_name(cp),
            State::AttributeName => self.attribute_name(cp),
            State::AfterAttributeName => self.after_attribute_name(cp),
            State::BeforeAttributeValue => self.before_attribute_value(cp),
            State::AttributeValueDoubleQuoted => {
                self.attribute_value_quoted(cp, 0x22, State::AttributeValueDoubleQuoted)
            }
            State::AttributeValueSingleQuoted => {
                self.attribute_value_quoted(cp, 0x27, State::AttributeValueSingleQuoted)
            }
            State::AttributeValueUnquoted => self.attribute_value_unquoted(cp),
            State::AfterAttributeValueQuoted => self.after_attribute_value_quoted(cp),
            State::SelfClosingStartTag => self.self_closing_start_tag(cp),
            State::BogusComment => self.bogus_comment(cp),
            State::MarkupDeclarationOpen => self.markup_declaration_open(cp),
            State::CommentStart => self.comment_start(cp),
            State::CommentStartDash => self.comment_start_dash(cp),
            State::Comment => self.comment(cp),
            State::CommentLessThanSign => self.comment_less_than_sign(cp),
            State::CommentLessThanSignBang => {
                if cp == 0x2d {
                    State::CommentLessThanSignBangDash
                } else {
                    self.reconsume_as(State::Comment)
                }
            }
            State::CommentLessThanSignBangDash => {
                if cp == 0x2d {
                    State::CommentLessThanSignBangDashDash
                } else {
                    self.reconsume_as(State::CommentEndDash)
                }
            }
            State::CommentLessThanSignBangDashDash => {
                if cp != 0x3e && cp != EOF {
                    self.report("nested-comment");
                }
                self.reconsume_as(State::CommentEnd)
            }
            State::CommentEndDash => self.comment_end_dash(cp),
            State::CommentEnd => self.comment_end(cp),
            State::CommentEndBang => self.comment_end_bang(cp),
            State::CdataSection => self.cdata_section(cp),
            State::CdataSectionBracket => {
                if cp == 0x5d {
                    State::CdataSectionEnd
                } else {
                    self.append(0x5d);
                    self.reconsume_as(State::CdataSection)
                }
            }
            State::CdataSectionEnd => self.cdata_section_end(cp),
            State::CharacterReference => self.character_reference(cp),
            State::NamedCharacterReference => self.named_character_reference(cp),
            State::AmbiguousAmpersand => self.ambiguous_ampersand(cp),
            State::NumericCharacterReference => {
                self.cr_code = 0;
                if cp == 0x78 || cp == 0x58 {
                    self.buffer.push(cp);
                    State::HexCharacterReferenceStart
                } else {
                    self.reconsume_as(State::DecimalCharacterReferenceStart)
                }
            }
            State::HexCharacterReferenceStart => {
                if is_hex(cp) {
                    self.reconsume_as(State::HexCharacterReference)
                } else {
                    self.report("absence-of-digits-in-numeric-character-reference");
                    self.reconsume_as(State::CharacterReferenceEnd)
                }
            }
            State::DecimalCharacterReferenceStart => {
                if is_digit(cp) {
                    self.reconsume_as(State::DecimalCharacterReference)
                } else {
                    self.report("absence-of-digits-in-numeric-character-reference");
                    self.reconsume_as(State::CharacterReferenceEnd)
                }
            }
            State::HexCharacterReference => self.numeric_reference(cp, true),
            State::DecimalCharacterReference => self.numeric_reference(cp, false),
            State::NumericCharacterReferenceEnd => self.numeric_character_reference_end(),
            State::CharacterReferenceEnd => self.character_reference_end(),
            State::VExpressionStart => self.v_expression_start(cp),
            State::VExpressionData => self.v_expression_data(cp),
            State::VExpressionEnd => self.v_expression_end(cp),
        }
    }

    // ---- reading -------------------------------------------------------------------------

    fn consume_next_code_point(&mut self) -> i32 {
        loop {
            if self.offset >= self.text.len() {
                self.last_cp = EOF;
                self.last_cp_raw = EOF;
                return EOF;
            }
            self.offset += self.cur_len;
            if self.offset >= self.text.len() {
                self.offset = self.text.len();
                self.cur_len = 0;
                self.last_cp = EOF;
                self.last_cp_raw = EOF;
                return EOF;
            }
            let c = self.text[self.offset..]
                .chars()
                .next()
                .unwrap_or('\u{fffd}');
            self.cur_len = c.len_utf8();
            let cp = c as i32;
            if is_non_character(cp as i64) {
                self.report("noncharacter-in-input-stream");
            }
            if is_control(cp) && !is_ws(cp) && cp != NULL {
                self.report("control-character-in-input-stream");
            }
            // Skip LF to convert CRLF → LF.
            if self.last_cp_raw == CR && cp == LF {
                self.last_cp = LF;
                self.last_cp_raw = LF;
                self.gaps.push(self.offset as u32);
                continue;
            }
            self.last_cp = cp;
            self.last_cp_raw = cp;
            if cp == CR {
                self.last_cp = LF;
                return LF;
            }
            return cp;
        }
    }

    fn reconsume_as(&mut self, state: State) -> State {
        self.reconsuming = true;
        state
    }

    fn report(&mut self, code: &'static str) {
        self.errors.push((code, self.offset as u32));
    }

    fn set_start_token_mark(&mut self) {
        self.token_start = Some(self.offset);
    }
    fn clear_start_token_mark(&mut self) {
        self.token_start = None;
    }

    fn start_token(&mut self, kind: TokenKind) {
        if self.token_start.is_none() {
            self.set_start_token_mark();
        }
        let offset = self.token_start.unwrap_or(self.offset);
        if self.current.is_some() {
            self.end_token();
        }
        self.token_start = None;
        self.current = Some(HToken {
            kind,
            start: offset as u32,
            end: offset as u32,
            value: String::new(),
        });
    }

    fn end_token(&mut self) {
        let Some(mut token) = self.current.take() else {
            return;
        };
        if self.token_start.is_none() {
            self.set_start_token_mark();
        }
        let offset = self.token_start.unwrap_or(self.offset);
        let provisional = self.state.is_provisional();
        self.token_start = None;
        token.end = offset as u32;
        if token.start == token.end && !provisional {
            return;
        }
        if provisional {
            if self.provisional.is_some() {
                self.commit_provisional_token();
            }
            self.provisional = Some(token);
        } else {
            self.commit_token(token);
        }
    }

    fn commit_token(&mut self, token: HToken) {
        if token.kind == TokenKind::HTMLTagOpen {
            self.last_tag_open_value = Some(token.value.clone());
        }
        // ponytail: JS asserts here; if a token is already committed keep the first one.
        if self.committed.is_none() {
            self.committed = Some(token);
        }
    }

    fn commit_provisional_token(&mut self) {
        if let Some(token) = self.provisional.take()
            && token.start < token.end
        {
            self.commit_token(token);
        }
    }

    fn rollback_provisional_token(&mut self) {
        if let Some(p) = self.provisional.take() {
            self.current = Some(p);
        }
    }

    fn append(&mut self, cp: i32) {
        if let Some(t) = self.current.as_mut() {
            push_cp(&mut t.value, cp);
        }
    }

    fn current_kind(&self) -> Option<TokenKind> {
        self.current.as_ref().map(|t| t.kind)
    }

    fn is_appropriate_end_tag_open(&self) -> bool {
        match (&self.current, &self.last_tag_open_value) {
            (Some(t), Some(v)) => t.kind == TokenKind::HTMLEndTagOpen && &t.value == v,
            _ => false,
        }
    }

    // ---- states --------------------------------------------------------------------------

    fn data(&mut self, mut cp: i32) -> State {
        self.clear_start_token_mark();
        loop {
            let kind = if is_ws(cp) {
                TokenKind::HTMLWhitespace
            } else {
                TokenKind::HTMLText
            };
            if let Some(k) = self.current_kind()
                && k != kind
            {
                self.end_token();
                return self.reconsume_as(self.state);
            }
            if self.current.is_none() {
                self.start_token(kind);
            }
            if cp == 0x26 {
                self.return_state = State::Data;
                return State::CharacterReference;
            }
            if cp == 0x3c {
                self.set_start_token_mark();
                return State::TagOpen;
            }
            if cp == 0x7b && self.expression_enabled {
                self.set_start_token_mark();
                self.return_state = State::Data;
                return State::VExpressionStart;
            }
            if cp == 0x7d && self.expression_enabled {
                self.set_start_token_mark();
                self.return_state = State::Data;
                return State::VExpressionEnd;
            }
            if cp == EOF {
                return State::Data;
            }
            if cp == NULL {
                self.report("unexpected-null-character");
            }
            self.append(cp);
            cp = self.consume_next_code_point();
        }
    }

    /// RCDATA / RAWTEXT.
    fn text_state(&mut self, mut cp: i32, me: State) -> State {
        self.clear_start_token_mark();
        let text_kind = if me == State::Rcdata {
            TokenKind::HTMLRCDataText
        } else {
            TokenKind::HTMLRawText
        };
        loop {
            let kind = if is_ws(cp) {
                TokenKind::HTMLWhitespace
            } else {
                text_kind
            };
            if let Some(k) = self.current_kind()
                && k != kind
            {
                self.end_token();
                return self.reconsume_as(self.state);
            }
            if self.current.is_none() {
                self.start_token(kind);
            }
            if cp == 0x26 && me == State::Rcdata {
                self.return_state = State::Rcdata;
                return State::CharacterReference;
            }
            if cp == 0x3c {
                self.set_start_token_mark();
                return if me == State::Rcdata {
                    State::RcdataLessThanSign
                } else {
                    State::RawtextLessThanSign
                };
            }
            if cp == 0x7b && self.expression_enabled {
                self.set_start_token_mark();
                self.return_state = me;
                return State::VExpressionStart;
            }
            if cp == 0x7d && self.expression_enabled {
                self.set_start_token_mark();
                self.return_state = me;
                return State::VExpressionEnd;
            }
            if cp == EOF {
                return State::Data;
            }
            if cp == NULL {
                self.report("unexpected-null-character");
                cp = NULL_REPLACEMENT;
            }
            self.append(cp);
            cp = self.consume_next_code_point();
        }
    }

    fn tag_open(&mut self, cp: i32) -> State {
        if cp == 0x21 {
            return State::MarkupDeclarationOpen;
        }
        if cp == 0x2f {
            return State::EndTagOpen;
        }
        if is_letter(cp) {
            self.start_token(TokenKind::HTMLTagOpen);
            return self.reconsume_as(State::TagName);
        }
        if cp == 0x3f {
            self.report("unexpected-question-mark-instead-of-tag-name");
            self.start_token(TokenKind::HTMLBogusComment);
            return self.reconsume_as(State::BogusComment);
        }
        if cp == EOF {
            self.clear_start_token_mark();
            self.report("eof-before-tag-name");
            self.append(0x3c);
            return State::Data;
        }
        self.report("invalid-first-character-of-tag-name");
        self.append(0x3c);
        self.reconsume_as(State::Data)
    }

    fn end_tag_open(&mut self, cp: i32) -> State {
        if is_letter(cp) {
            self.start_token(TokenKind::HTMLEndTagOpen);
            return self.reconsume_as(State::TagName);
        }
        if cp == 0x3e {
            self.end_token();
            self.report("missing-end-tag-name");
            return State::Data;
        }
        if cp == EOF {
            self.clear_start_token_mark();
            self.report("eof-before-tag-name");
            self.append(0x3c);
            self.append(0x2f);
            return State::Data;
        }
        self.report("invalid-first-character-of-tag-name");
        self.start_token(TokenKind::HTMLBogusComment);
        self.reconsume_as(State::BogusComment)
    }

    fn tag_name(&mut self, mut cp: i32) -> State {
        loop {
            if is_ws(cp) {
                self.end_token();
                return State::BeforeAttributeName;
            }
            if cp == 0x2f {
                self.end_token();
                self.set_start_token_mark();
                return State::SelfClosingStartTag;
            }
            if cp == 0x3e {
                self.start_token(TokenKind::HTMLTagClose);
                return State::Data;
            }
            if cp == EOF {
                self.report("eof-in-tag");
                return State::Data;
            }
            if cp == NULL {
                self.report("unexpected-null-character");
                cp = NULL_REPLACEMENT;
            }
            self.append(lower(cp));
            cp = self.consume_next_code_point();
        }
    }

    fn lt_sign(&mut self, cp: i32, end_tag_open: State, back: State) -> State {
        if cp == 0x2f {
            self.buffer.clear();
            return end_tag_open;
        }
        self.append(0x3c);
        self.reconsume_as(back)
    }

    fn end_tag_open_in(&mut self, cp: i32, name_state: State, back: State) -> State {
        if is_letter(cp) {
            self.start_token(TokenKind::HTMLEndTagOpen);
            return self.reconsume_as(name_state);
        }
        self.append(0x3c);
        self.append(0x2f);
        self.reconsume_as(back)
    }

    fn rollback_end_tag_name(&mut self, back: State) -> State {
        self.rollback_provisional_token();
        self.append(0x3c);
        self.append(0x2f);
        let buf = std::mem::take(&mut self.buffer);
        for &c in &buf {
            self.append(c);
        }
        self.buffer = buf;
        self.reconsume_as(back)
    }

    fn rcdata_end_tag_name(&mut self, mut cp: i32) -> State {
        loop {
            if is_ws(cp) && self.is_appropriate_end_tag_open() {
                self.end_token();
                return State::BeforeAttributeName;
            }
            if cp == 0x2f && self.is_appropriate_end_tag_open() {
                self.end_token();
                self.set_start_token_mark();
                return State::SelfClosingStartTag;
            }
            if cp == 0x3e && self.is_appropriate_end_tag_open() {
                self.start_token(TokenKind::HTMLTagClose);
                return State::Data;
            }
            if !is_letter(cp) {
                return self.rollback_end_tag_name(State::Rcdata);
            }
            self.append(lower(cp));
            self.buffer.push(cp);
            cp = self.consume_next_code_point();
        }
    }

    fn rawtext_end_tag_name(&mut self, mut cp: i32) -> State {
        loop {
            if cp == 0x2f && self.is_appropriate_end_tag_open() {
                self.end_token();
                self.set_start_token_mark();
                return State::SelfClosingStartTag;
            }
            if cp == 0x3e && self.is_appropriate_end_tag_open() {
                self.start_token(TokenKind::HTMLTagClose);
                return State::Data;
            }
            if is_ws(cp) && self.is_appropriate_end_tag_open() {
                self.end_token();
                return State::BeforeAttributeName;
            }
            if !is_letter(cp) && !self.maybe_valid_custom_block(cp) {
                return self.rollback_end_tag_name(State::Rawtext);
            }
            self.append(lower(cp));
            self.buffer.push(cp);
            cp = self.consume_next_code_point();
        }
    }

    /// `lastTagOpenToken.value.startsWith(currentToken.value + nextCp)` (e.g. `</i18n>`).
    fn maybe_valid_custom_block(&self, cp: i32) -> bool {
        let (Some(cur), Some(last)) = (&self.current, &self.last_tag_open_value) else {
            return false;
        };
        if cp == EOF {
            return false;
        }
        let Some(c) = char::from_u32(cp as u32) else {
            return false;
        };
        last.starts_with(cur.value.as_str()) && last[cur.value.len()..].starts_with(c)
    }

    fn before_attribute_name(&mut self, mut cp: i32) -> State {
        while is_ws(cp) {
            cp = self.consume_next_code_point();
        }
        if cp == 0x2f || cp == 0x3e || cp == EOF {
            return self.reconsume_as(State::AfterAttributeName);
        }
        if cp == 0x3d {
            self.report("unexpected-equals-sign-before-attribute-name");
            self.start_token(TokenKind::HTMLIdentifier);
            self.append(cp);
            return State::AttributeName;
        }
        self.start_token(TokenKind::HTMLIdentifier);
        self.reconsume_as(State::AttributeName)
    }

    fn attribute_name(&mut self, mut cp: i32) -> State {
        loop {
            if is_ws(cp) || cp == 0x2f || cp == 0x3e || cp == EOF {
                self.end_token();
                return self.reconsume_as(State::AfterAttributeName);
            }
            if cp == 0x3d {
                self.start_token(TokenKind::HTMLAssociation);
                return State::BeforeAttributeValue;
            }
            if cp == NULL {
                self.report("unexpected-null-character");
                cp = NULL_REPLACEMENT;
            }
            if cp == 0x22 || cp == 0x27 || cp == 0x3c {
                self.report("unexpected-character-in-attribute-name");
            }
            self.append(lower(cp));
            cp = self.consume_next_code_point();
        }
    }

    fn after_attribute_name(&mut self, mut cp: i32) -> State {
        while is_ws(cp) {
            cp = self.consume_next_code_point();
        }
        if cp == 0x2f {
            self.set_start_token_mark();
            return State::SelfClosingStartTag;
        }
        if cp == 0x3d {
            self.start_token(TokenKind::HTMLAssociation);
            return State::BeforeAttributeValue;
        }
        if cp == 0x3e {
            self.start_token(TokenKind::HTMLTagClose);
            return State::Data;
        }
        if cp == EOF {
            self.report("eof-in-tag");
            return State::Data;
        }
        self.start_token(TokenKind::HTMLIdentifier);
        self.reconsume_as(State::AttributeName)
    }

    fn before_attribute_value(&mut self, mut cp: i32) -> State {
        self.end_token();
        while is_ws(cp) {
            cp = self.consume_next_code_point();
        }
        if cp == 0x3e {
            self.report("missing-attribute-value");
            self.start_token(TokenKind::HTMLTagClose);
            return State::Data;
        }
        self.start_token(TokenKind::HTMLLiteral);
        if cp == 0x22 {
            return State::AttributeValueDoubleQuoted;
        }
        if cp == 0x27 {
            return State::AttributeValueSingleQuoted;
        }
        self.reconsume_as(State::AttributeValueUnquoted)
    }

    fn attribute_value_quoted(&mut self, mut cp: i32, quote: i32, me: State) -> State {
        loop {
            if cp == quote {
                return State::AfterAttributeValueQuoted;
            }
            if cp == 0x26 {
                self.return_state = me;
                return State::CharacterReference;
            }
            if cp == NULL {
                self.report("unexpected-null-character");
                cp = NULL_REPLACEMENT;
            }
            if cp == EOF {
                self.report("eof-in-tag");
                return State::Data;
            }
            self.append(cp);
            cp = self.consume_next_code_point();
        }
    }

    fn attribute_value_unquoted(&mut self, mut cp: i32) -> State {
        loop {
            if is_ws(cp) {
                self.end_token();
                return State::BeforeAttributeName;
            }
            if cp == 0x26 {
                self.return_state = State::AttributeValueUnquoted;
                return State::CharacterReference;
            }
            if cp == 0x3e {
                self.start_token(TokenKind::HTMLTagClose);
                return State::Data;
            }
            if cp == NULL {
                self.report("unexpected-null-character");
                cp = NULL_REPLACEMENT;
            }
            if matches!(cp, 0x22 | 0x27 | 0x3c | 0x3d | 0x60) {
                self.report("unexpected-character-in-unquoted-attribute-value");
            }
            if cp == EOF {
                self.report("eof-in-tag");
                return State::Data;
            }
            self.append(cp);
            cp = self.consume_next_code_point();
        }
    }

    fn after_attribute_value_quoted(&mut self, cp: i32) -> State {
        self.end_token();
        if is_ws(cp) {
            return State::BeforeAttributeName;
        }
        if cp == 0x2f {
            self.set_start_token_mark();
            return State::SelfClosingStartTag;
        }
        if cp == 0x3e {
            self.start_token(TokenKind::HTMLTagClose);
            return State::Data;
        }
        if cp == EOF {
            self.report("eof-in-tag");
            return State::Data;
        }
        self.report("missing-whitespace-between-attributes");
        self.reconsume_as(State::BeforeAttributeName)
    }

    fn self_closing_start_tag(&mut self, cp: i32) -> State {
        if cp == 0x3e {
            self.start_token(TokenKind::HTMLSelfClosingTagClose);
            // Vue.js supports self-closing elements, so never switch to RCDATA/RAWTEXT here.
            return State::Data;
        }
        if cp == EOF {
            self.report("eof-in-tag");
            return State::Data;
        }
        self.report("unexpected-solidus-in-tag");
        self.clear_start_token_mark();
        self.reconsume_as(State::BeforeAttributeName)
    }

    fn bogus_comment(&mut self, mut cp: i32) -> State {
        loop {
            if cp == 0x3e || cp == EOF {
                return State::Data;
            }
            if cp == NULL {
                cp = NULL_REPLACEMENT;
            }
            self.append(cp);
            cp = self.consume_next_code_point();
        }
    }

    fn markup_declaration_open(&mut self, cp: i32) -> State {
        let b = self.text.as_bytes();
        if cp == 0x2d && b.get(self.offset + 1) == Some(&b'-') {
            self.offset += 1;
            self.start_token(TokenKind::HTMLComment);
            return State::CommentStart;
        }
        if cp == 0x44 && b.get(self.offset + 1..self.offset + 7) == Some(b"OCTYPE") {
            self.start_token(TokenKind::HTMLBogusComment);
            self.append(cp);
            return State::BogusComment;
        }
        if cp == 0x5b && b.get(self.offset + 1..self.offset + 7) == Some(b"CDATA[") {
            self.offset += 6;
            if self.namespace == Namespace::Html {
                self.report("cdata-in-html-content");
                self.start_token(TokenKind::HTMLBogusComment);
                if let Some(t) = self.current.as_mut() {
                    t.value.push_str("[CDATA[");
                }
                return State::BogusComment;
            }
            self.start_token(TokenKind::HTMLCDataText);
            return State::CdataSection;
        }
        self.report("incorrectly-opened-comment");
        self.start_token(TokenKind::HTMLBogusComment);
        self.reconsume_as(State::BogusComment)
    }

    fn comment_start(&mut self, cp: i32) -> State {
        if cp == 0x2d {
            return State::CommentStartDash;
        }
        if cp == 0x3e {
            self.report("abrupt-closing-of-empty-comment");
            return State::Data;
        }
        self.reconsume_as(State::Comment)
    }

    fn comment_start_dash(&mut self, cp: i32) -> State {
        if cp == 0x2d {
            return State::CommentEnd;
        }
        if cp == 0x3e {
            self.report("abrupt-closing-of-empty-comment");
            return State::Data;
        }
        if cp == EOF {
            self.report("eof-in-comment");
            return State::Data;
        }
        self.append(0x2d);
        self.reconsume_as(State::Comment)
    }

    fn comment(&mut self, mut cp: i32) -> State {
        loop {
            if cp == 0x3c {
                self.append(0x3c);
                return State::CommentLessThanSign;
            }
            if cp == 0x2d {
                return State::CommentEndDash;
            }
            if cp == NULL {
                self.report("unexpected-null-character");
                cp = NULL_REPLACEMENT;
            }
            if cp == EOF {
                self.report("eof-in-comment");
                return State::Data;
            }
            self.append(cp);
            cp = self.consume_next_code_point();
        }
    }

    fn comment_less_than_sign(&mut self, mut cp: i32) -> State {
        loop {
            if cp == 0x21 {
                self.append(cp);
                return State::CommentLessThanSignBang;
            }
            if cp != 0x3c {
                return self.reconsume_as(State::Comment);
            }
            self.append(cp);
            cp = self.consume_next_code_point();
        }
    }

    fn comment_end_dash(&mut self, cp: i32) -> State {
        if cp == 0x2d {
            return State::CommentEnd;
        }
        if cp == EOF {
            self.report("eof-in-comment");
            return State::Data;
        }
        self.append(0x2d);
        self.reconsume_as(State::Comment)
    }

    fn comment_end(&mut self, mut cp: i32) -> State {
        loop {
            if cp == 0x3e {
                return State::Data;
            }
            if cp == 0x21 {
                return State::CommentEndBang;
            }
            if cp == EOF {
                self.report("eof-in-comment");
                return State::Data;
            }
            self.append(0x2d);
            if cp != 0x2d {
                self.append(0x2d);
                return self.reconsume_as(State::Comment);
            }
            cp = self.consume_next_code_point();
        }
    }

    fn comment_end_bang(&mut self, cp: i32) -> State {
        if cp == 0x2d {
            self.append(0x2d);
            self.append(0x21);
            return State::CommentEndDash;
        }
        if cp == 0x3e {
            self.report("incorrectly-closed-comment");
            return State::Data;
        }
        if cp == EOF {
            self.report("eof-in-comment");
            return State::Data;
        }
        self.append(0x2d);
        self.append(0x21);
        self.reconsume_as(State::Comment)
    }

    fn cdata_section(&mut self, mut cp: i32) -> State {
        loop {
            if cp == 0x5d {
                return State::CdataSectionBracket;
            }
            if cp == EOF {
                self.report("eof-in-cdata");
                return State::Data;
            }
            self.append(cp);
            cp = self.consume_next_code_point();
        }
    }

    fn cdata_section_end(&mut self, mut cp: i32) -> State {
        loop {
            if cp == 0x3e {
                return State::Data;
            }
            if cp != 0x5d {
                self.append(0x5d);
                self.append(0x5d);
                return self.reconsume_as(State::CdataSection);
            }
            self.append(0x5d);
            cp = self.consume_next_code_point();
        }
    }

    fn character_reference(&mut self, cp: i32) -> State {
        self.cr_start_offset = self.offset.saturating_sub(1);
        self.buffer.clear();
        self.buffer.push(0x26);
        if is_digit(cp) || is_letter(cp) {
            return self.reconsume_as(State::NamedCharacterReference);
        }
        if cp == 0x23 {
            self.buffer.push(cp);
            return State::NumericCharacterReference;
        }
        self.reconsume_as(State::CharacterReferenceEnd)
    }

    fn named_character_reference(&mut self, cp: i32) -> State {
        let b = self.text.as_bytes();
        for &len in LENGTHS {
            let Some(text) = b.get(self.offset..self.offset + len) else {
                continue;
            };
            let Ok(idx) = ENTITIES.binary_search_by(|(name, _)| name.as_bytes().cmp(text)) else {
                continue;
            };
            let codepoints = ENTITIES[idx].1;
            let semi = text.last() == Some(&b';');
            // vue-eslint-parser reads the code point right after the FIRST char of the match.
            let next = self
                .text
                .get(self.offset + 1..)
                .and_then(|s| s.chars().next())
                .map(|c| c as i32);
            self.offset += len - 1;
            if self.return_state.is_attr()
                && !semi
                && next.is_some_and(|n| n == 0x3d || is_letter(n) || is_digit(n))
            {
                for &c in text {
                    self.buffer.push(c as i32);
                }
            } else {
                if !semi {
                    self.report("missing-semicolon-after-character-reference");
                }
                self.buffer.clear();
                self.buffer.extend(codepoints.iter().map(|&c| c as i32));
            }
            return State::CharacterReferenceEnd;
        }
        let buf = std::mem::take(&mut self.buffer);
        for &c in &buf {
            self.append(c);
        }
        self.buffer = buf;
        self.append(cp);
        State::AmbiguousAmpersand
    }

    fn ambiguous_ampersand(&mut self, mut cp: i32) -> State {
        while is_digit(cp) || is_letter(cp) {
            self.append(cp);
            cp = self.consume_next_code_point();
        }
        if cp == 0x3b {
            self.report("unknown-named-character-reference");
        }
        self.reconsume_as(self.return_state)
    }

    fn numeric_reference(&mut self, mut cp: i32, hex: bool) -> State {
        loop {
            let digit = if is_digit(cp) {
                Some(cp - 0x30)
            } else if hex && is_upper_hex(cp) {
                Some(cp - 0x37)
            } else if hex && is_lower_hex(cp) {
                Some(cp - 0x57)
            } else {
                None
            };
            match digit {
                Some(d) => {
                    // Saturate: anything above 0x10FFFF is reported the same way.
                    self.cr_code =
                        (self.cr_code * if hex { 16 } else { 10 } + d as i64).min(0x0011_0000);
                }
                None => {
                    if cp == 0x3b {
                        return State::NumericCharacterReferenceEnd;
                    }
                    self.report("missing-semicolon-after-character-reference");
                    return self.reconsume_as(State::NumericCharacterReferenceEnd);
                }
            }
            cp = self.consume_next_code_point();
        }
    }

    fn numeric_character_reference_end(&mut self) -> State {
        let mut code = self.cr_code;
        if code == 0 {
            self.report("null-character-reference");
            code = NULL_REPLACEMENT as i64;
        } else if code > 0x10ffff {
            self.report("character-reference-outside-unicode-range");
            code = NULL_REPLACEMENT as i64;
        } else if is_surrogate(code) {
            self.report("surrogate-character-reference");
            code = NULL_REPLACEMENT as i64;
        } else if is_non_character(code) {
            self.report("noncharacter-character-reference");
        } else if code == 0x0d || (is_control(code as i32) && !is_ws(code as i32)) {
            self.report("control-character-reference");
            code = alternative_cr(code).unwrap_or(code);
        }
        self.buffer.clear();
        self.buffer.push(code as i32);
        self.reconsume_as(State::CharacterReferenceEnd)
    }

    fn character_reference_end(&mut self) -> State {
        let len0 = self.current.as_ref().map_or(0, |t| t.value.len());
        let buf = std::mem::take(&mut self.buffer);
        for &c in &buf {
            self.append(c);
        }
        self.buffer = buf;
        let new_len = self.current.as_ref().map_or(0, |t| t.value.len()) - len0;
        for i in self.cr_start_offset + new_len..self.offset {
            self.gaps.push(i as u32);
        }
        self.reconsume_as(self.return_state)
    }

    fn v_expression_start(&mut self, cp: i32) -> State {
        if cp == 0x7b {
            self.start_token(TokenKind::VExpressionStart);
            self.append(0x7b);
            self.append(0x7b);
            // `vueFeatures.interpolationAsNonHTML` defaults to true.
            let from = (self.offset + 1).min(self.text.len());
            let found = match self.close_cache {
                Some((searched, hit)) if searched <= from && hit.is_none_or(|h| h >= from) => hit,
                _ => {
                    let hit = self.text[from..].find("}}").map(|i| i + from);
                    self.close_cache = Some((from, hit));
                    hit
                }
            };
            if found.is_none() {
                self.report("x-missing-interpolation-end");
                return self.return_state;
            }
            self.v_expr_script_state = Some(self.return_state);
            return State::VExpressionData;
        }
        self.append(0x7b);
        self.reconsume_as(self.return_state)
    }

    fn v_expression_data(&mut self, mut cp: i32) -> State {
        self.clear_start_token_mark();
        let state = self.v_expr_script_state.unwrap_or(State::Data);
        loop {
            let kind = if is_ws(cp) {
                TokenKind::HTMLWhitespace
            } else if state == State::Rcdata {
                TokenKind::HTMLRCDataText
            } else if state == State::Rawtext {
                TokenKind::HTMLRawText
            } else {
                TokenKind::HTMLText
            };
            if let Some(k) = self.current_kind()
                && k != kind
            {
                self.end_token();
                return self.reconsume_as(self.state);
            }
            if self.current.is_none() {
                self.start_token(kind);
            }
            if cp == 0x26 && state != State::Rawtext {
                self.return_state = State::VExpressionData;
                return State::CharacterReference;
            }
            if cp == 0x7d {
                self.set_start_token_mark();
                self.return_state = State::VExpressionData;
                return State::VExpressionEnd;
            }
            if cp == EOF {
                self.report("x-missing-interpolation-end");
                return State::Data;
            }
            if cp == NULL {
                self.report("unexpected-null-character");
            }
            self.append(cp);
            cp = self.consume_next_code_point();
        }
    }

    fn v_expression_end(&mut self, cp: i32) -> State {
        if cp == 0x7d {
            self.start_token(TokenKind::VExpressionEnd);
            self.append(0x7d);
            self.append(0x7d);
            if let Some(state) = self.v_expr_script_state.take() {
                return state;
            }
            return self.return_state;
        }
        self.append(0x7d);
        self.reconsume_as(self.return_state)
    }
}
