//! An ESLint selector (esquery) engine over the ESTree-shaped [`Tree`] of `indent/tree.rs`.
//!
//! ponytail: supported subset — type / `*`, `[path]`, `[path op value]` (`=`, `!=`, `<`, `<=`,
//! `>`, `>=`; string, number, regexp `/re/flags`, bare literal values; dotted paths through child
//! fields, `length` of lists), `.field`, combinators ` `, `>`, `~`, `+`, selector lists,
//! `:not()`, `:matches()` / `:is()`, `:has()`, `:first-child`, `:last-child`, `:nth-child(n)`,
//! `:nth-last-child(n)`, `:statement`, `:expression`, `:declaration`, `:function`, `:pattern`.
//! Not supported (never match): `type()` values, `:nth-child(an+b)`, `:scope`, `:exit`.
//! Leaf properties come from the caller (`leaf`), so only the ones it knows can be matched.

use crate::utils::indent::tree::{F, NONE, Tree, Ty};

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
}

impl Val {
    /// JS template-string conversion (`${value}`).
    fn js_string(&self) -> String {
        match self {
            Val::Str(s) => s.clone(),
            Val::Num(n) => crate::utils::js::js_number(*n),
            Val::Bool(b) => b.to_string(),
            Val::Null => "null".into(),
            Val::Undefined => "undefined".into(),
            Val::Node(_) => "[object Object]".into(),
            Val::List(_) => String::new(),
        }
    }
    fn present(&self) -> bool {
        !matches!(self, Val::Null | Val::Undefined)
    }
}

#[derive(Debug, Clone)]
enum Value {
    Literal(String),
    Regex(crate::utils::regexp::JsRegex),
    Unsupported,
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
        let value = match self.peek() {
            Some(q @ (b'"' | b'\'')) => {
                self.i += 1;
                let mut v = String::new();
                while let Some(c) = self.peek() {
                    self.i += 1;
                    if c == q {
                        break;
                    }
                    if c == b'\\' {
                        if let Some(n) = self.peek() {
                            v.push(n as char);
                            self.i += 1;
                        }
                        continue;
                    }
                    let ch = self.src[self.i - 1..].chars().next().unwrap_or(' ');
                    v.push(ch);
                    self.i += ch.len_utf8() - 1;
                }
                Value::Literal(v)
            }
            Some(b'/') => {
                self.i += 1;
                let st = self.i;
                while self.peek().is_some_and(|c| c != b'/') {
                    if self.peek() == Some(b'\\') {
                        self.i += 1;
                    }
                    self.i += 1;
                }
                let pat = self.src[st..self.i.min(self.s.len())].to_string();
                self.i += 1;
                let flags = self.ident();
                crate::utils::regexp::JsRegex::with_flags(&pat, &flags)
                    .map_or(Value::Unsupported, Value::Regex)
            }
            _ => {
                let st = self.i;
                while self
                    .peek()
                    .is_some_and(|c| c != b']' && !c.is_ascii_whitespace())
                {
                    self.i += 1;
                }
                let v = self.src[st..self.i].to_string();
                if v.starts_with("type(") {
                    Value::Unsupported
                } else {
                    Value::Literal(v)
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

/// Leaf property getter: `(node, name) -> value` (`Val::Undefined` when unknown).
pub type Leaf<'l> = &'l dyn Fn(u32, &str) -> Val;

pub struct Matcher<'t> {
    pub tree: &'t Tree,
    pub leaf: Leaf<'t>,
}

fn field_name(f: F) -> String {
    let d = format!("{f:?}");
    let mut c = d.chars();
    match c.next() {
        Some(first) => first.to_ascii_lowercase().to_string() + c.as_str(),
        None => d,
    }
}

impl Matcher<'_> {
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
                            .unwrap_or_else(|| (self.leaf)(x, seg))
                    }
                }
                Val::List(l) if seg == "length" => Val::Num(l.len() as f64),
                Val::Str(s) if seg == "length" => Val::Num(s.encode_utf16().count() as f64),
                _ => return Val::Undefined,
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
                match (op, value) {
                    (_, Value::Unsupported) => false,
                    (Op::Eq | Op::Ne, Value::Regex(re)) => {
                        let m = matches!(&v, Val::Str(s) if re.is_match(s));
                        m == (*op == Op::Eq)
                    }
                    (Op::Eq | Op::Ne, Value::Literal(l)) => {
                        (v.js_string() == *l) == (*op == Op::Eq)
                    }
                    (_, Value::Literal(l)) => {
                        let (Val::Num(x), Ok(y)) = (&v, l.parse::<f64>()) else {
                            return false;
                        };
                        match op {
                            Op::Lt => *x < y,
                            Op::Le => *x <= y,
                            Op::Gt => *x > y,
                            _ => *x >= y,
                        }
                    }
                    _ => false,
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
