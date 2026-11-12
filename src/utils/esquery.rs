//! An ESLint selector (esquery) engine over the ESTree-shaped [`Tree`] of `indent/tree.rs`.
//!
//! Mirrors esquery 1.7: type / `*`, `[path]`, `[path op value]` (`=`, `!=` with string, number,
//! bare name, `/re/flags` or `type(t)` values; `<`, `<=`, `>`, `>=` with JS coercion), dotted
//! paths through child fields, list indices and `length`, `.field`, combinators ` `, `>`, `~`,
//! `+`, selector lists, `:not()`, `:matches()` / `:is()`, `:has()`, `:first-child`,
//! `:last-child`, `:nth-child(n)`, `:nth-last-child(n)`, `:statement`, `:expression`,
//! `:declaration`, `:function`, `:pattern`.
//! ponytail: `:scope` / `:exit` never match; leaf properties are the ones [`leaf`] knows.

use std::collections::HashMap;

use oxc::span::Span;

use crate::ast::{AttributeKey, AttributeValue, DirectiveArgument, Document};
use crate::utils::indent::tree::{F, NONE, Tree, Ty, flag};
use crate::utils::tpl_misc::js_to_number;

/// A property value (`getPath` result).
#[derive(Debug, Clone, PartialEq)]
pub enum Val {
    Str(String),
    Num(f64),
    Bool(bool),
    Null,
    Undefined,
    Node(u32),
    List(Vec<u32>),
    /// A BigInt (decimal digits).
    BigInt(String),
    /// A `RegExp` object (its `/source/flags` text).
    RegExp(String),
    /// The ESTree `Literal.regex` object: `{ pattern, flags }`.
    RegexInfo(String, String),
}

impl Val {
    /// JS template-string conversion (`${value}`).
    fn js_string(&self) -> String {
        match self {
            Val::Str(s) | Val::BigInt(s) | Val::RegExp(s) => s.clone(),
            Val::Num(n) => crate::utils::js::js_number(*n),
            Val::Bool(b) => b.to_string(),
            Val::Null => "null".into(),
            Val::Undefined => "undefined".into(),
            Val::Node(_) | Val::RegexInfo(..) => "[object Object]".into(),
            Val::List(l) => l
                .iter()
                .map(|&x| if x == NONE { "" } else { "[object Object]" })
                .collect::<Vec<_>>()
                .join(","),
        }
    }
    fn present(&self) -> bool {
        !matches!(self, Val::Null | Val::Undefined)
    }
    /// `typeof`.
    fn type_of(&self) -> &'static str {
        match self {
            Val::Str(_) => "string",
            Val::Num(_) => "number",
            Val::Bool(_) => "boolean",
            Val::Undefined => "undefined",
            Val::BigInt(_) => "bigint",
            _ => "object",
        }
    }
}

/// JS `a < b` (`IsLessThan`); `None` = undefined (a NaN operand).
fn js_less(a: &Val, b: &Val) -> Option<bool> {
    // ToPrimitive: objects become strings.
    let prim = |v: &Val| match v {
        Val::Node(_) | Val::List(_) | Val::RegExp(_) | Val::RegexInfo(..) => {
            Val::Str(v.js_string())
        }
        o => o.clone(),
    };
    let (a, b) = (prim(a), prim(b));
    if let (Val::Str(x), Val::Str(y)) = (&a, &b) {
        return Some(x.encode_utf16().lt(y.encode_utf16()));
    }
    // ToNumeric; a BigInt is compared through f64.
    // ponytail: BigInts beyond 2^53 lose precision.
    let num = |v: &Val| match v {
        Val::Num(n) => *n,
        Val::Bool(b) => f64::from(u8::from(*b)),
        Val::Null => 0.0,
        Val::Str(s) => js_to_number(s).unwrap_or(f64::NAN),
        Val::BigInt(d) => d.parse().unwrap_or(f64::NAN),
        _ => f64::NAN,
    };
    let big_str = |s: &Val, other: &Val| matches!((s, other), (Val::Str(_), Val::BigInt(_)));
    // StringToBigInt of a non-integer string is undefined.
    if (big_str(&a, &b) || big_str(&b, &a))
        && [&a, &b]
            .iter()
            .any(|v| matches!(v, Val::Str(s) if js_to_number(s).is_none_or(|n| n.fract() != 0.0)))
    {
        return None;
    }
    let (x, y) = (num(&a), num(&b));
    (!x.is_nan() && !y.is_nan()).then_some(x < y)
}

#[derive(Debug, Clone)]
enum Value {
    Str(String),
    Num(f64),
    Regex(crate::utils::regexp::JsRegex),
    Type(String),
    Unsupported,
}

/// esquery `strUnescape`.
fn str_unescape(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('b') => out.push('\u{8}'),
            Some('f') => out.push('\u{c}'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('v') => out.push('\u{b}'),
            Some(o) => out.push(o),
            None => {}
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Op {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug, Clone)]
enum Atom {
    Any,
    Type(String),
    Attr(Vec<String>, Option<(Op, Value)>),
    Field(String),
    Not(Vec<Selector>),
    Is(Vec<Selector>),
    Has(Vec<Selector>),
    NthChild(i64),
    NthLastChild(i64),
    Class(String),
    Never,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Comb {
    Descendant,
    Child,
    Sibling,
    Adjacent,
}

/// One complex selector: compounds left to right, each with the combinator before it.
#[derive(Debug, Clone)]
pub struct Selector(Vec<(Comb, Vec<Atom>)>);

struct P<'s> {
    s: &'s [u8],
    src: &'s str,
    i: usize,
}

impl P<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }
    fn ident(&mut self) -> String {
        let st = self.i;
        while self.peek().is_some_and(|c| {
            c.is_ascii_alphanumeric() || matches!(c, b'_' | b'$' | b'-') || c >= 0x80
        }) {
            self.i += 1;
        }
        self.src[st..self.i].to_string()
    }
    /// Text up to the matching `)` (nesting-aware), consuming it.
    fn group(&mut self) -> String {
        let st = self.i;
        let mut depth = 0;
        let mut quote = None;
        while let Some(c) = self.peek() {
            match (quote, c) {
                (Some(q), c) if c == q => quote = None,
                (Some(_), b'\\') => self.i += 1,
                (Some(_), _) => {}
                (None, b'"' | b'\'') => quote = Some(c),
                (None, b'(') => depth += 1,
                (None, b')') if depth == 0 => break,
                (None, b')') => depth -= 1,
                _ => {}
            }
            self.i += 1;
        }
        let out = self.src[st..self.i.min(self.s.len())].to_string();
        self.i += 1;
        out
    }

    fn list(&mut self) -> Vec<Selector> {
        let mut out = vec![self.selector()];
        loop {
            self.ws();
            if self.peek() == Some(b',') {
                self.i += 1;
                out.push(self.selector());
            } else {
                return out;
            }
        }
    }

    fn selector(&mut self) -> Selector {
        let mut parts = Vec::new();
        let mut comb = Comb::Descendant;
        loop {
            self.ws();
            let compound = self.compound();
            if compound.is_empty() {
                break;
            }
            parts.push((comb, compound));
            let before = self.i;
            self.ws();
            comb = match self.peek() {
                Some(b'>') => Comb::Child,
                Some(b'~') => Comb::Sibling,
                Some(b'+') => Comb::Adjacent,
                Some(b',' | b')') | None => break,
                _ if self.i > before => {
                    comb = Comb::Descendant;
                    continue;
                }
                _ => break,
            };
            self.i += 1;
        }
        Selector(parts)
    }

    fn compound(&mut self) -> Vec<Atom> {
        let mut out = Vec::new();
        loop {
            match self.peek() {
                Some(b'*') => {
                    self.i += 1;
                    out.push(Atom::Any);
                }
                Some(c) if c.is_ascii_alphabetic() || c == b'_' => {
                    out.push(Atom::Type(self.ident()))
                }
                Some(b'.') => {
                    self.i += 1;
                    out.push(Atom::Field(self.ident()));
                }
                Some(b'[') => {
                    self.i += 1;
                    out.push(self.attr());
                }
                Some(b':') => {
                    self.i += 1;
                    out.push(self.pseudo());
                }
                _ => return out,
            }
        }
    }

    fn attr(&mut self) -> Atom {
        self.ws();
        let mut path = vec![self.ident()];
        while self.peek() == Some(b'.') {
            self.i += 1;
            path.push(self.ident());
        }
        self.ws();
        let op = match (self.peek(), self.s.get(self.i + 1).copied()) {
            (Some(b'!'), Some(b'=')) => Some((Op::Ne, 2)),
            (Some(b'<'), Some(b'=')) => Some((Op::Le, 2)),
            (Some(b'>'), Some(b'=')) => Some((Op::Ge, 2)),
            (Some(b'='), _) => Some((Op::Eq, 1)),
            (Some(b'<'), _) => Some((Op::Lt, 1)),
            (Some(b'>'), _) => Some((Op::Gt, 1)),
            _ => None,
        };
        let Some((op, len)) = op else {
            while self.peek().is_some_and(|c| c != b']') {
                self.i += 1;
            }
            self.i += 1;
            return Atom::Attr(path, None);
        };
        self.i += len;
        self.ws();
        let rest = &self.src[self.i..];
        let eq = matches!(op, Op::Eq | Op::Ne);
        let value = match self.peek() {
            Some(b't') if eq && rest.starts_with("type(") => {
                self.i += 5;
                let st = self.i;
                while self.peek().is_some_and(|c| c != b')' && c != b' ') {
                    self.i += 1;
                }
                let t = self.src[st..self.i].to_string();
                if self.peek() == Some(b')') {
                    self.i += 1;
                    Value::Type(t)
                } else {
                    Value::Unsupported
                }
            }
            Some(b'/') if eq => {
                self.i += 1;
                let st = self.i;
                let mut class = false;
                while let Some(c) = self.peek() {
                    match c {
                        b'\\' => self.i += 1,
                        b'[' => class = true,
                        b']' => class = false,
                        b'/' if !class => break,
                        _ => {}
                    }
                    self.i += 1;
                }
                let pat = self.src[st..self.i.min(self.s.len())].to_string();
                self.i += 1;
                let fs = self.i;
                while self
                    .peek()
                    .is_some_and(|c| matches!(c, b'i' | b'm' | b's' | b'u'))
                {
                    self.i += 1;
                }
                let flags = self.src[fs.min(self.s.len())..self.i.min(self.s.len())].to_string();
                crate::utils::regexp::JsRegex::with_flags(&pat, &flags)
                    .map_or(Value::Unsupported, Value::Regex)
            }
            Some(q @ (b'"' | b'\'')) => {
                self.i += 1;
                let st = self.i;
                while let Some(c) = self.peek() {
                    if c == q {
                        break;
                    }
                    if c == b'\\' {
                        self.i += 1;
                    }
                    self.i += 1;
                }
                let raw = &self.src[st..self.i.min(self.s.len())];
                self.i += 1;
                Value::Str(str_unescape(raw))
            }
            Some(c) if c.is_ascii_digit() || c == b'.' => {
                // `([0-9]* ".")? [0-9]+`
                let st = self.i;
                while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    self.i += 1;
                }
                if self.peek() == Some(b'.') {
                    self.i += 1;
                    while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                        self.i += 1;
                    }
                }
                self.src[st..self.i]
                    .parse::<f64>()
                    .map_or(Value::Unsupported, Value::Num)
            }
            _ => {
                // `identifierName`: `[^ [\],():#!=><~+.]+`
                let st = self.i;
                while self
                    .peek()
                    .is_some_and(|c| !b" [],():#!=><~+.".contains(&c))
                {
                    self.i += 1;
                }
                if self.i == st {
                    Value::Unsupported
                } else {
                    Value::Str(self.src[st..self.i].to_string())
                }
            }
        };
        self.ws();
        if self.peek() == Some(b']') {
            self.i += 1;
        }
        Atom::Attr(path, Some((op, value)))
    }

    fn pseudo(&mut self) -> Atom {
        let name = self.ident();
        let arg = if self.peek() == Some(b'(') {
            self.i += 1;
            Some(self.group())
        } else {
            None
        };
        let sub = |s: &str| parse(s);
        match (name.as_str(), arg) {
            ("not", Some(a)) => Atom::Not(sub(&a)),
            ("matches" | "is", Some(a)) => Atom::Is(sub(&a)),
            ("has", Some(a)) => Atom::Has(sub(a.trim_start().trim_start_matches('>'))),
            ("first-child", None) => Atom::NthChild(1),
            ("last-child", None) => Atom::NthLastChild(1),
            ("nth-child", Some(a)) => a.trim().parse().map_or(Atom::Never, Atom::NthChild),
            ("nth-last-child", Some(a)) => a.trim().parse().map_or(Atom::Never, Atom::NthLastChild),
            (c @ ("statement" | "expression" | "declaration" | "function" | "pattern"), None) => {
                Atom::Class(c.to_string())
            }
            _ => Atom::Never,
        }
    }
}

/// Parse a selector list.
pub fn parse(src: &str) -> Vec<Selector> {
    let mut p = P {
        s: src.as_bytes(),
        src,
        i: 0,
    };
    let list = p.list();
    p.ws();
    if p.i < src.len() {
        // Trailing garbage: treat as unsupported.
        return vec![Selector(vec![(Comb::Descendant, vec![Atom::Never])])];
    }
    list
}

/// Template node properties read from the document, by range.
#[derive(Default)]
struct VProps {
    /// VElement / VIdentifier: (name, rawName).
    names: HashMap<(u32, u32), (String, String)>,
    /// VLiteral / VText: value.
    values: HashMap<(u32, u32), String>,
}

impl VProps {
    fn new(doc: &Document) -> Self {
        let mut p = VProps::default();
        let r = |s: Span| (s.start, s.end);
        for e in &doc.elements {
            p.names
                .insert(r(e.span), (e.name.clone(), e.raw_name.clone()));
            for a in e.attributes() {
                let mut ids = Vec::new();
                match &a.key {
                    AttributeKey::Plain(k) => ids.push(k),
                    AttributeKey::Directive(k) => {
                        ids.push(&k.name);
                        if let Some(DirectiveArgument::Static(i)) = &k.argument {
                            ids.push(i);
                        }
                        ids.extend(k.modifiers.iter().filter(|m| !m.raw_name.is_empty()));
                    }
                }
                for i in ids {
                    p.names
                        .insert(r(i.span), (i.name.clone(), i.raw_name.clone()));
                }
                if let Some(AttributeValue::Literal(l)) = &a.value {
                    p.values.insert(r(l.span), l.value.clone());
                }
            }
        }
        for t in &doc.texts {
            p.values.insert(r(t.span), t.value.clone());
        }
        p
    }
}

/// JS string literal value (common escapes).
fn unquote(raw: &str) -> String {
    let body = &raw[1..raw.len().saturating_sub(1).max(1)];
    let mut out = String::new();
    let mut it = body.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('b') => out.push('\u{8}'),
            Some('f') => out.push('\u{c}'),
            Some('v') => out.push('\u{b}'),
            Some('0') => out.push('\0'),
            Some('u') => {
                let hex: String = it.by_ref().take(4).collect();
                if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    out.push(ch);
                }
            }
            Some('\n') | None => {}
            Some(o) => out.push(o),
        }
    }
    out
}

/// BigInt literal text (without `n`, no separators) → decimal digits.
fn bigint_digits(t: &str) -> String {
    let lower = t.to_ascii_lowercase();
    let radix = [("0x", 16), ("0o", 8), ("0b", 2)]
        .into_iter()
        .find_map(|(p, r)| lower.strip_prefix(p).map(|d| (d, r)));
    match radix {
        Some((d, r)) => {
            u128::from_str_radix(d, r).map_or_else(|_| t.to_string(), |v| v.to_string())
        }
        None => t.to_string(),
    }
}

/// ESTree leaf properties (`Val::Undefined` when unknown).
fn leaf(tree: &Tree, src: &str, vp: &VProps, n: u32, name: &str) -> Val {
    let node = tree.node(n);
    let text = &src[node.start as usize..node.end as usize];
    let key = (node.start, node.end);
    let ident = |t: &str| {
        let t = t.trim_start_matches('#');
        t.chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
            .collect::<String>()
    };
    let regex = text.starts_with('/');
    let bigint = !regex && text.ends_with('n') && !text.starts_with(['"', '\'']);
    match (node.ty, name) {
        (Ty::Identifier | Ty::PrivateIdentifier, "name") => Val::Str(ident(text)),
        (Ty::Literal, "raw") => Val::Str(text.to_string()),
        (Ty::Literal, "value") => match text {
            "true" => Val::Bool(true),
            "false" => Val::Bool(false),
            "null" => Val::Null,
            t if t.starts_with(['"', '\'']) => Val::Str(unquote(t)),
            t if regex => Val::RegExp(t.to_string()),
            t if bigint => Val::BigInt(bigint_digits(&t[..t.len() - 1].replace('_', ""))),
            t => js_to_number(&t.replace('_', "")).map_or(Val::Undefined, Val::Num),
        },
        (Ty::Literal, "regex") if regex => {
            let end = text.rfind('/').unwrap_or(0);
            Val::RegexInfo(text[1..end.max(1)].to_string(), text[end + 1..].to_string())
        }
        (Ty::Literal, "bigint") if bigint => {
            Val::Str(bigint_digits(&text[..text.len() - 1].replace('_', "")))
        }
        (
            Ty::BinaryExpression
            | Ty::LogicalExpression
            | Ty::AssignmentExpression
            | Ty::UnaryExpression
            | Ty::UpdateExpression,
            "operator",
        ) => tree
            .ops
            .get(&n)
            .map_or(Val::Undefined, |o| Val::Str((*o).to_string())),
        (Ty::UnaryExpression, "prefix") => Val::Bool(true),
        (Ty::UpdateExpression, "prefix") => Val::Bool(tree.has(n, flag::PREFIX)),
        (Ty::Property, "shorthand") => Val::Bool(tree.has(n, flag::SHORTHAND)),
        (Ty::Property, "method") => Val::Bool(tree.has(n, flag::METHOD)),
        (Ty::VariableDeclaration, "kind") => Val::Str(ident(text)),
        (_, "computed") => Val::Bool(tree.has(n, flag::COMPUTED)),
        (_, "optional") => Val::Bool(tree.has(n, flag::OPTIONAL)),
        (_, "async") => Val::Bool(tree.has(n, flag::ASYNC)),
        (Ty::VAttribute, "directive") => {
            let k = tree.get(n, F::Key);
            Val::Bool(k != NONE && tree.ty(k) == Ty::VDirectiveKey)
        }
        (Ty::VElement | Ty::VIdentifier, "name") => vp
            .names
            .get(&key)
            .map_or(Val::Undefined, |v| Val::Str(v.0.clone())),
        (Ty::VElement | Ty::VIdentifier, "rawName") => vp
            .names
            .get(&key)
            .map_or(Val::Undefined, |v| Val::Str(v.1.clone())),
        (Ty::VLiteral | Ty::VText, "value") => vp
            .values
            .get(&key)
            .map_or(Val::Undefined, |v| Val::Str(v.clone())),
        _ => Val::Undefined,
    }
}

pub struct Matcher<'t> {
    pub tree: &'t Tree,
    src: &'t str,
    vp: VProps,
}

fn field_name(f: F) -> String {
    let d = format!("{f:?}");
    let mut c = d.chars();
    match c.next() {
        Some(first) => first.to_ascii_lowercase().to_string() + c.as_str(),
        None => d,
    }
}

impl<'t> Matcher<'t> {
    /// `doc`: the SFC document, for template node names and values.
    pub fn new(tree: &'t Tree, src: &'t str, doc: Option<&Document>) -> Self {
        Matcher {
            tree,
            src,
            vp: doc.map(VProps::new).unwrap_or_default(),
        }
    }

    fn child_field(&self, n: u32, name: &str) -> Option<Val> {
        let mut kids = Vec::new();
        self.tree.children(n, &mut kids);
        for &k in &kids {
            if let Some(f) = self.tree.field_of(k)
                && field_name(f) == name
            {
                let list = self.tree.list(n, f);
                return Some(if list.is_empty() {
                    Val::Node(k)
                } else {
                    Val::List(list.to_vec())
                });
            }
        }
        None
    }

    fn get_path(&self, n: u32, path: &[String]) -> Val {
        let mut cur = Val::Node(n);
        for seg in path {
            cur = match cur {
                Val::Node(x) => {
                    if seg == "type" {
                        Val::Str(self.tree.ty(x).name().to_string())
                    } else {
                        self.child_field(x, seg)
                            .unwrap_or_else(|| leaf(self.tree, self.src, &self.vp, x, seg))
                    }
                }
                Val::List(l) if seg == "length" => Val::Num(l.len() as f64),
                Val::List(l) => match seg.parse::<usize>().ok().and_then(|i| l.get(i)) {
                    Some(&x) if x == NONE => Val::Null,
                    Some(&x) => Val::Node(x),
                    None => Val::Undefined,
                },
                Val::Str(s) if seg == "length" => Val::Num(s.encode_utf16().count() as f64),
                Val::Str(s) => {
                    let u: Vec<u16> = s.encode_utf16().collect();
                    match seg.parse::<usize>().ok().and_then(|i| u.get(i)) {
                        Some(&c) => Val::Str(String::from_utf16_lossy(&[c])),
                        None => Val::Undefined,
                    }
                }
                Val::RegexInfo(p, _) if seg == "pattern" => Val::Str(p),
                Val::RegexInfo(_, f) if seg == "flags" => Val::Str(f),
                // `getPath` stops at null / undefined.
                v @ (Val::Null | Val::Undefined) => return v,
                _ => Val::Undefined,
            };
        }
        cur
    }

    fn atom(&self, n: u32, a: &Atom) -> bool {
        let t = self.tree;
        match a {
            Atom::Any => true,
            Atom::Type(name) => t.ty(n).name().eq_ignore_ascii_case(name),
            Atom::Attr(path, None) => self.get_path(n, path).present(),
            Atom::Attr(path, Some((op, value))) => {
                let v = self.get_path(n, path);
                let lit = |l: &Value| match l {
                    Value::Str(s) => Some(Val::Str(s.clone())),
                    Value::Num(x) => Some(Val::Num(*x)),
                    _ => None,
                };
                match (op, value) {
                    (_, Value::Unsupported) => false,
                    (Op::Eq, Value::Regex(re)) => matches!(&v, Val::Str(s) if re.is_match(s)),
                    // `!re.test(value)` coerces the value to a string.
                    (Op::Ne, Value::Regex(re)) => !re.is_match(&v.js_string()),
                    (Op::Eq | Op::Ne, Value::Type(t)) => (v.type_of() == t) == (*op == Op::Eq),
                    (Op::Eq | Op::Ne, l) => {
                        let l = lit(l).map(|l| l.js_string()).unwrap_or_default();
                        (v.js_string() == l) == (*op == Op::Eq)
                    }
                    (_, l) => {
                        let Some(l) = lit(l) else { return false };
                        match op {
                            Op::Lt => js_less(&v, &l) == Some(true),
                            Op::Gt => js_less(&l, &v) == Some(true),
                            Op::Le => js_less(&l, &v) == Some(false),
                            _ => js_less(&v, &l) == Some(false),
                        }
                    }
                }
            }
            Atom::Field(f) => t.field_of(n).is_some_and(|x| field_name(x) == *f),
            Atom::Not(list) => !list.iter().any(|s| self.matches(n, s)),
            Atom::Is(list) => list.iter().any(|s| self.matches(n, s)),
            Atom::Has(list) => {
                let mut stack = Vec::new();
                t.children(n, &mut stack);
                while let Some(c) = stack.pop() {
                    if list.iter().any(|s| self.matches(c, s)) {
                        return true;
                    }
                    t.children(c, &mut stack);
                }
                false
            }
            Atom::NthChild(i) | Atom::NthLastChild(i) => {
                let Some((list, idx)) = self.siblings(n) else {
                    return false;
                };
                let pos = if matches!(a, Atom::NthChild(_)) {
                    idx as i64 + 1
                } else {
                    (list.len() - idx) as i64
                };
                pos == *i
            }
            Atom::Class(c) => {
                let ty = t.ty(n);
                let name = ty.name();
                match c.as_str() {
                    "statement" => ty.is_statement_class(),
                    "declaration" => name.ends_with("Declaration"),
                    "function" => matches!(
                        ty,
                        Ty::FunctionDeclaration
                            | Ty::FunctionExpression
                            | Ty::ArrowFunctionExpression
                    ),
                    "pattern" => name.ends_with("Pattern") || self.is_expression(n),
                    _ => self.is_expression(n),
                }
            }
            Atom::Never => false,
        }
    }

    fn is_expression(&self, n: u32) -> bool {
        let ty = self.tree.ty(n);
        let name = ty.name();
        let p = self.tree.parent(n);
        name.ends_with("Expression")
            || name.ends_with("Literal")
            || (ty == Ty::Identifier && (p == NONE || self.tree.ty(p) != Ty::MetaProperty))
            || ty == Ty::MetaProperty
    }

    /// The list field of the parent that holds `n`, and the index.
    fn siblings(&self, n: u32) -> Option<(&[u32], usize)> {
        let p = self.tree.parent(n);
        if p == NONE {
            return None;
        }
        let f = self.tree.field_of(n)?;
        let list = self.tree.list(p, f);
        list.iter().position(|&x| x == n).map(|i| (list, i))
    }

    fn compound(&self, n: u32, atoms: &[Atom]) -> bool {
        atoms.iter().all(|a| self.atom(n, a))
    }

    fn matches_from(&self, n: u32, parts: &[(Comb, Vec<Atom>)]) -> bool {
        let Some(((comb, last), rest)) = parts.split_last() else {
            return true;
        };
        if !self.compound(n, last) {
            return false;
        }
        if rest.is_empty() {
            return true;
        }
        let t = self.tree;
        match comb {
            Comb::Child => {
                let p = t.parent(n);
                p != NONE && self.matches_from(p, rest)
            }
            Comb::Descendant => {
                let mut p = t.parent(n);
                while p != NONE {
                    if self.matches_from(p, rest) {
                        return true;
                    }
                    p = t.parent(p);
                }
                false
            }
            Comb::Sibling | Comb::Adjacent => {
                let Some((list, idx)) = self.siblings(n) else {
                    return false;
                };
                let before = list[..idx].iter().copied().filter(|&x| x != NONE);
                if *comb == Comb::Adjacent {
                    list[..idx]
                        .last()
                        .is_some_and(|&x| x != NONE && self.matches_from(x, rest))
                } else {
                    before.into_iter().any(|x| self.matches_from(x, rest))
                }
            }
        }
    }

    pub fn matches(&self, n: u32, s: &Selector) -> bool {
        self.matches_from(n, &s.0)
    }

    pub fn matches_any(&self, n: u32, list: &[Selector]) -> bool {
        list.iter().any(|s| self.matches(n, s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses() {
        assert_eq!(parse("CallExpression").len(), 1);
        assert_eq!(parse("A, B > C").len(), 2);
        let s = &parse("CallExpression > :nth-child(3)[value!=true]")[0];
        assert_eq!(s.0.len(), 2);
        let s = &parse(
            "CallExpression[callee.type='Identifier'][callee.name='$gettext'] TemplateLiteral",
        )[0];
        assert_eq!(s.0.len(), 2);
        assert_eq!(s.0[0].1.len(), 3);
        let s = &parse("VElement > VExpressionContainer CallExpression")[0];
        assert_eq!(s.0.len(), 3);
        assert_eq!(s.0[2].0, Comb::Descendant);
        assert!(
            matches!(parse(":not(Identifier, Literal)")[0].0[0].1[0], Atom::Not(ref l) if l.len() == 2)
        );
    }
}
