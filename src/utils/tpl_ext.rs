//! Shared machinery of the `wrapStylisticOrCoreRule` extension rules (`vue/comma-spacing`,
//! `vue/key-spacing`, …): the @stylistic rules run over `<template>` expressions with the
//! template token store (`document.tokens` + `document.comments`) and an ESTree-shaped tree.

use oxc::span::Span;

use crate::LineIndex;
use crate::ast::TokenKind;
use crate::rule::{Ctx, Fix};
use crate::utils::indent::store::{NONE, Store, Tok};
use crate::utils::indent::tree::{Builder, F, Tree, Ty};

pub struct Ext<'a> {
    pub src: &'a str,
    pub store: Store<'a>,
    /// ESLint token `value` per store id.
    vals: Vec<&'a str>,
    pub tree: Tree,
    /// Template body `VElement` (`NONE` without one).
    pub root: u32,
    /// Traversal roots: the template body, then `<style>` `v-bind()` expressions (document
    /// visitor rules only).
    roots: Vec<u32>,
    pub lines: &'a LineIndex,
}

pub struct Report {
    pub span: (u32, u32),
    pub message: String,
    pub fix: Option<Fix>,
}

fn is_js_kind(k: TokenKind) -> bool {
    use TokenKind::*;
    matches!(
        k,
        Punctuator
            | Identifier
            | Keyword
            | String
            | Numeric
            | Template
            | RegularExpression
            | Boolean
            | Null
            | PrivateIdentifier
            | JSXText
            | JSXIdentifier
            | LineComment
            | BlockComment
    )
}

/// A `v-bind()` of a top-level `<style>`: `(span, opening paren, expression range, quoted)`.
fn style_vbind_parts(src: &str, span: Span) -> Option<(u32, (u32, u32), bool)> {
    let text = src.get(span.start as usize..span.end as usize)?;
    let open = span.start + 6 + text[6..].find('(')? as u32;
    let close = span.end - 1;
    let inner = src.get(open as usize + 1..close as usize)?;
    // Skip whitespace and `/* */` comments at both ends.
    let skip = |s: &str, from_end: bool| -> usize {
        let mut s = s;
        let mut n = 0;
        loop {
            let t = if from_end {
                s.trim_end()
            } else {
                s.trim_start()
            };
            n += s.len() - t.len();
            s = t;
            if !from_end && s.starts_with("/*") {
                let Some(e) = s.find("*/") else { break };
                n += e + 2;
                s = &s[e + 2..];
            } else if from_end && s.ends_with("*/") {
                let Some(b) = s.rfind("/*") else { break };
                n += s.len() - b;
                s = &s[..b];
            } else {
                break;
            }
        }
        n
    };
    let lead = skip(inner, false);
    let trail = skip(inner, true);
    let start = open + 1 + lead as u32;
    if lead + trail < inner.len() {
        let core = &inner[lead..inner.len() - trail];
        let q = core.as_bytes()[0];
        if (q == b'"' || q == b'\'') && core.len() >= 2 && core.as_bytes()[core.len() - 1] == q {
            // One quoted token only (no unescaped quote inside).
            let body = &core[1..core.len() - 1];
            let mut esc = false;
            let single = body.bytes().all(|c| {
                let ok = esc || c != q;
                esc = !esc && c == b'\\';
                ok
            }) && !esc;
            if single {
                return Some((open, (start + 1, start + core.len() as u32 - 1), true));
            }
        }
    }
    Some((open, (start, close), false))
}

impl<'a> Ext<'a> {
    /// `apply_document`: the rule uses `defineDocumentVisitor` (`applyDocument: true`), so it
    /// also sees `<style>` `v-bind()` expressions and runs without a template body.
    pub fn new(ctx: &Ctx<'_, 'a>, apply_document: bool) -> Option<Self> {
        let doc = ctx.file.doc.as_ref()?;
        if doc.template_body.is_none() && !apply_document {
            return None;
        }
        let src = ctx.source();
        let vbinds: Vec<(Span, u32, (u32, u32), bool)> = crate::utils::tpl_misc::style_v_binds(doc)
            .into_iter()
            .filter_map(|s| style_vbind_parts(src, s).map(|(o, r, q)| (s, o, r, q)))
            .collect();
        let mut items: Vec<(Tok, &'a str)> =
            Vec::with_capacity(doc.tokens.len() + doc.comments.len());
        let push =
            |items: &mut Vec<(Tok, &'a str)>, s: u32, e: u32, kind: TokenKind, comment: bool| {
                let v: &'a str = if is_js_kind(kind) {
                    src.get(s as usize..e as usize).unwrap_or("")
                } else {
                    // ponytail: HTML token values only matter for regex tests on neighbours.
                    ""
                };
                items.push((
                    Tok {
                        start: s,
                        end: e,
                        kind,
                        comment,
                    },
                    v,
                ));
            };
        let mut b = Builder::new(src);
        let mut roots = Vec::new();
        let root = doc.template_body.map_or(NONE, |r| b.element(doc, r));
        if root != NONE {
            roots.push(root);
        }
        // `<style>` `v-bind()`s: new tokens replace (and split) the document tokens they cover
        // (vue-eslint-parser `replaceAndSplitTokens`).
        let mut cuts: Vec<(u32, u32)> = Vec::new();
        for v in &vbinds {
            let (span, open, (es, ee), quoted) = *v;
            let before_end = if quoted { es } else { open + 1 };
            let after_start = if quoted { ee } else { span.end - 1 };
            let alloc = oxc::allocator::Allocator::default();
            let mut masked = vec![b' '; src.len()];
            masked[es as usize..ee as usize]
                .copy_from_slice(&src.as_bytes()[es as usize..ee as usize]);
            masked[es as usize - 1] = b'(';
            masked[ee as usize] = b')';
            let Ok(code) = String::from_utf8(masked) else {
                continue;
            };
            let st = oxc::span::SourceType::default()
                .with_module(true)
                .with_typescript(ctx.file.ts);
            let ret = oxc::parser::Parser::new(&alloc, &code, st)
                .with_config(oxc::parser::config::TokensParserConfig)
                .parse();
            let stmt = match ret.program.body.as_slice() {
                [oxc::ast::ast::Statement::ExpressionStatement(stmt)]
                    if !ret.fatal_error && ret.diagnostics.is_empty() =>
                {
                    Some(stmt)
                }
                _ => None,
            };
            // An unparsable unquoted expression keeps its raw text: `(` / `)` tokens around it made
            // spacing rules add a space on every pass (the raw text then starts with it).
            if stmt.is_none() && !quoted {
                continue;
            }
            push(
                &mut items,
                span.start,
                span.start + 6,
                TokenKind::HTMLRawText,
                false,
            );
            push(&mut items, open, open + 1, TokenKind::Punctuator, false);
            if quoted {
                push(&mut items, es - 1, es, TokenKind::Punctuator, false);
                push(&mut items, ee, ee + 1, TokenKind::Punctuator, false);
            }
            push(
                &mut items,
                span.end - 1,
                span.end,
                TokenKind::Punctuator,
                false,
            );
            cuts.push((span.start, before_end));
            cuts.push((after_start, span.end));
            let Some(stmt) = stmt else { continue };
            cuts.push((before_end, after_start));
            for t in ret.tokens.iter() {
                if es <= t.start() && t.end() <= ee && t.start() < t.end() {
                    let kind = crate::parser::token_kind(t.kind());
                    push(&mut items, t.start(), t.end(), kind, false);
                }
            }
            for c in ret.program.comments.iter() {
                let kind = if c.is_line() {
                    TokenKind::LineComment
                } else {
                    TokenKind::BlockComment
                };
                push(&mut items, c.span.start, c.span.end, kind, true);
            }
            if apply_document {
                roots.push(b.expr(&stmt.expression));
            }
        }
        for (list, comment) in [(&doc.tokens, false), (&doc.comments, true)] {
            for t in list.iter() {
                let (mut s, e) = (t.span.start, t.span.end);
                if comment || !cuts.iter().any(|c| c.0 < e && s < c.1) {
                    push(&mut items, s, e, t.kind, comment);
                    continue;
                }
                let mut cs: Vec<&(u32, u32)> = cuts.iter().filter(|c| c.0 < e && s < c.1).collect();
                cs.sort();
                for c in cs {
                    if s < c.0 {
                        push(&mut items, s, c.0, t.kind, false);
                    }
                    s = s.max(c.1);
                }
                if s < e {
                    push(&mut items, s, e, t.kind, false);
                }
            }
        }
        // Same (stable) order as `Store::new`, so ids line up with `vals`.
        items.sort_by_key(|(t, _)| (t.start, t.end));
        let vals = items.iter().map(|(_, v)| *v).collect();
        let store = Store::new(src, items.into_iter().map(|(t, _)| t).collect());
        Some(Ext {
            src,
            store,
            vals,
            tree: b.t,
            root,
            roots,
            lines: ctx.file.lines,
        })
    }

    // ---- tree ----

    pub fn ty(&self, n: u32) -> Ty {
        self.tree.ty(n)
    }
    pub fn range(&self, n: u32) -> (u32, u32) {
        self.tree.range(n)
    }
    pub fn get(&self, n: u32, f: F) -> Option<u32> {
        let c = self.tree.get(n, f);
        (c != NONE).then_some(c)
    }
    pub fn list(&self, n: u32, f: F) -> &[u32] {
        self.tree.list(n, f)
    }
    pub fn parent(&self, n: u32) -> Option<u32> {
        let p = self.tree.parent(n);
        (p != NONE).then_some(p)
    }

    /// Template body nodes in traversal (pre-) order. With `skip_dyn`, nodes inside dynamic
    /// directive arguments are left out (`skipDynamicArguments`).
    pub fn nodes(&self, skip_dyn: bool) -> Vec<u32> {
        let mut out = Vec::new();
        let mut stack: Vec<u32> = self.roots.iter().rev().copied().collect();
        let mut kids = Vec::new();
        while let Some(n) = stack.pop() {
            if skip_dyn
                && self.ty(n) == Ty::VExpressionContainer
                && self
                    .parent(n)
                    .is_some_and(|p| self.ty(p) == Ty::VDirectiveKey)
            {
                continue;
            }
            // vue-eslint-parser traverses templates with ESTree visitor keys: TS type
            // annotations / arguments / parameters are never visited (only TS expression
            // wrappers like `as` / `satisfies` / `!`, via their `expression`).
            let name = self.ty(n).name();
            if name.starts_with("TS") && !name.ends_with("Expression") {
                continue;
            }
            out.push(n);
            kids.clear();
            self.tree.children(n, &mut kids);
            stack.extend(kids.iter().rev());
        }
        out
    }

    /// Ranges of dynamic directive arguments (`VDirectiveKey > VExpressionContainer`).
    pub fn dyn_arg_ranges(&self) -> Vec<(u32, u32)> {
        (0..self.tree.nodes.len() as u32)
            .filter(|&n| {
                self.ty(n) == Ty::VExpressionContainer
                    && self
                        .parent(n)
                        .is_some_and(|p| self.ty(p) == Ty::VDirectiveKey)
            })
            .map(|n| self.range(n))
            .collect()
    }

    /// `sourceCode.getNodeByRangeIndex(index)` over the template body.
    pub fn node_by_index(&self, index: u32) -> Option<u32> {
        let mut cur = *self
            .roots
            .iter()
            .find(|&&r| self.range(r).0 <= index && index < self.range(r).1)?;
        'outer: loop {
            for c in self.tree.child_iter(cur) {
                let (s, e) = self.range(c);
                if s <= index && index < e {
                    cur = c;
                    continue 'outer;
                }
            }
            return Some(cur);
        }
    }

    // ---- tokens ----

    pub fn tr(&self, t: u32) -> (u32, u32) {
        self.store.range(t)
    }
    pub fn val(&self, t: u32) -> &'a str {
        self.vals[t as usize]
    }
    pub fn kind(&self, t: u32) -> TokenKind {
        self.store.tok(t).kind
    }
    pub fn is_punct(&self, t: u32, v: &str) -> bool {
        self.kind(t) == TokenKind::Punctuator && self.val(t) == v
    }
    /// eslint-utils `isCommentToken` (`Line` / `Block`).
    pub fn is_comment(&self, t: u32) -> bool {
        matches!(
            self.kind(t),
            TokenKind::LineComment | TokenKind::BlockComment
        )
    }
    pub fn first(&self, r: (u32, u32)) -> Option<u32> {
        self.store.fwd(r.0 as i64, r.1 as i64, false).next()
    }
    pub fn first_skip(&self, r: (u32, u32), skip: usize) -> Option<u32> {
        self.store.fwd(r.0 as i64, r.1 as i64, false).nth(skip)
    }
    pub fn first_f(&self, r: (u32, u32), f: impl Fn(u32) -> bool) -> Option<u32> {
        self.store
            .fwd(r.0 as i64, r.1 as i64, false)
            .find(|&t| f(t))
    }
    pub fn last(&self, r: (u32, u32)) -> Option<u32> {
        self.store.bwd(r.0 as i64, r.1 as i64, false).next()
    }
    pub fn last_skip(&self, r: (u32, u32), skip: usize) -> Option<u32> {
        self.store.bwd(r.0 as i64, r.1 as i64, false).nth(skip)
    }
    pub fn last_f(&self, r: (u32, u32), f: impl Fn(u32) -> bool) -> Option<u32> {
        self.store
            .bwd(r.0 as i64, r.1 as i64, false)
            .find(|&t| f(t))
    }
    pub fn tokens(&self, r: (u32, u32)) -> Vec<u32> {
        self.store.fwd(r.0 as i64, r.1 as i64, false).collect()
    }
    pub fn tokens_c(&self, r: (u32, u32)) -> Vec<u32> {
        self.store.fwd(r.0 as i64, r.1 as i64, true).collect()
    }
    pub fn before(&self, r: (u32, u32)) -> Option<u32> {
        self.store.bwd(-1, r.0 as i64, false).next()
    }
    pub fn before_c(&self, r: (u32, u32)) -> Option<u32> {
        self.store.bwd(-1, r.0 as i64, true).next()
    }
    pub fn before_f(&self, r: (u32, u32), f: impl Fn(u32) -> bool) -> Option<u32> {
        self.store.bwd(-1, r.0 as i64, false).find(|&t| f(t))
    }
    pub fn after(&self, r: (u32, u32)) -> Option<u32> {
        self.store.fwd(r.1 as i64, -1, false).next()
    }
    pub fn after_c(&self, r: (u32, u32)) -> Option<u32> {
        self.store.fwd(r.1 as i64, -1, true).next()
    }
    pub fn after_f(&self, r: (u32, u32), f: impl Fn(u32) -> bool) -> Option<u32> {
        self.store.fwd(r.1 as i64, -1, false).find(|&t| f(t))
    }
    pub fn between(&self, l: (u32, u32), r: (u32, u32)) -> Vec<u32> {
        self.store.fwd(l.1 as i64, r.0 as i64, false).collect()
    }
    pub fn between_c(&self, l: (u32, u32), r: (u32, u32)) -> Vec<u32> {
        self.store.fwd(l.1 as i64, r.0 as i64, true).collect()
    }
    pub fn first_between_f(
        &self,
        l: (u32, u32),
        r: (u32, u32),
        f: impl Fn(u32) -> bool,
    ) -> Option<u32> {
        self.store
            .fwd(l.1 as i64, r.0 as i64, false)
            .find(|&t| f(t))
    }
    /// `sourceCode.commentsExistBetween(left, right)`.
    pub fn comments_between(&self, l: (u32, u32), r: (u32, u32)) -> bool {
        self.store
            .fwd(l.1 as i64, r.0 as i64, true)
            .any(|t| self.store.tok(t).comment)
    }
    /// `sourceCode.getCommentsInside(node)`.
    pub fn has_comments_inside(&self, r: (u32, u32)) -> bool {
        self.store
            .fwd(r.0 as i64, r.1 as i64, true)
            .any(|t| self.store.tok(t).comment)
    }

    /// `sourceCode.isSpaceBetween(first, second)` (ranges of nodes or tokens).
    pub fn space_between(&self, a: (u32, u32), b: (u32, u32)) -> bool {
        let overlap = (a.0 <= b.0 && a.1 >= b.0) || (b.0 <= a.0 && b.1 >= a.0);
        if overlap {
            return false;
        }
        let (s, e) = if a.1 <= b.0 { (a, b) } else { (b, a) };
        let mut cur = self.last(s).map_or(s, |t| self.tr(t));
        let fin = self.first(e).map_or(e, |t| self.tr(t));
        while cur != fin {
            let Some(next) = self.store.fwd(cur.1 as i64, -1, true).next() else {
                return false;
            };
            let n = self.tr(next);
            if cur.1 != n.0 {
                return true;
            }
            if n == cur {
                return false;
            }
            cur = n;
        }
        false
    }

    // ---- lines ----

    pub fn line(&self, pos: u32) -> usize {
        self.lines.line_of(pos)
    }
    /// `isTokenOnSameLine(a, b)`: `a.loc.end.line === b.loc.start.line`.
    pub fn same_line(&self, a: (u32, u32), b: (u32, u32)) -> bool {
        self.line(a.1) == self.line(b.0)
    }
    /// `isSingleLine(node)`.
    pub fn single_line(&self, r: (u32, u32)) -> bool {
        self.line(r.0) == self.line(r.1)
    }
    pub fn text(&self, r: (u32, u32)) -> &'a str {
        self.src.get(r.0 as usize..r.1 as usize).unwrap_or("")
    }
}

/// Pushes the reports; with `skip_dyn_report` those overlapping a dynamic directive argument
/// are dropped (`skipDynamicArgumentsReport`).
pub fn emit(ctx: &mut Ctx, e: &Ext, reports: Vec<Report>, skip_dyn_report: bool) {
    let dyn_ranges = if skip_dyn_report {
        e.dyn_arg_ranges()
    } else {
        Vec::new()
    };
    for r in reports {
        if dyn_ranges.iter().any(|d| r.span.0 < d.1 && d.0 < r.span.1) {
            continue;
        }
        ctx.report_fix(Span::new(r.span.0, r.span.1), None, r.message, r.fix);
    }
}

pub fn rep(span: (u32, u32), message: impl Into<String>, fix: Option<Fix>) -> Report {
    Report {
        span,
        message: message.into(),
        fix,
    }
}

pub fn ins(at: u32, text: &str) -> Option<Fix> {
    Some(Fix::insert(at, text))
}
pub fn del(a: u32, b: u32) -> Option<Fix> {
    Some(Fix::remove(Span::new(a, b)))
}
pub fn repl(a: u32, b: u32, text: impl Into<String>) -> Option<Fix> {
    Some(Fix::replace(Span::new(a, b), text))
}

/// String option at `i` (or `default`).
pub fn opt_str<'o>(opts: &'o crate::rule::RuleOptions, i: usize, default: &'o str) -> &'o str {
    opts.get(i).as_str().unwrap_or(default)
}

/// Boolean field of the object option at `i`.
pub fn opt_bool(opts: &crate::rule::RuleOptions, i: usize, key: &str) -> Option<bool> {
    opts.get(i).get(key).and_then(|v| v.as_bool())
}

/// `LINEBREAK_MATCHER` (`/\r\n|[\r\n  ]/`).
pub fn has_linebreak(s: &str) -> bool {
    s.contains(['\r', '\n', '\u{2028}', '\u{2029}'])
}

// ---- ESTree node facts the tree does not store ----

impl Ext<'_> {
    /// `Property.shorthand`.
    pub fn prop_shorthand(&self, p: u32) -> bool {
        match (self.get(p, F::Key), self.get(p, F::Value)) {
            (Some(k), Some(v)) => self.range(k).0 == self.range(v).0,
            _ => false,
        }
    }
    /// `Property.method || kind !== "init"` (method, getter or setter).
    pub fn prop_method_or_accessor(&self, p: u32) -> bool {
        let (Some(k), Some(v)) = (self.get(p, F::Key), self.get(p, F::Value)) else {
            return false;
        };
        self.ty(v) == Ty::FunctionExpression
            && self
                .first_between_f(self.range(k), self.range(v), |t| self.is_punct(t, ":"))
                .is_none()
    }
    /// `Property.kind`: `"get"`, `"set"` or `"init"`.
    pub fn prop_kind(&self, p: u32) -> &'static str {
        let Some(k) = self.get(p, F::Key) else {
            return "init";
        };
        if !self.prop_method_or_accessor(p) {
            return "init";
        }
        match self.first(self.range(p)) {
            Some(t) if self.tr(t).0 < self.range(k).0 => match self.val(t) {
                "get" => "get",
                "set" => "set",
                _ => "init",
            },
            _ => "init",
        }
    }
    /// `Property.method` (plain / async / generator method).
    pub fn prop_method(&self, p: u32) -> bool {
        self.prop_method_or_accessor(p) && self.prop_kind(p) == "init"
    }
    /// The operator token of a binary-like node (`left op right`): the first token between the
    /// operands that is not a closing paren.
    pub fn operator_token(&self, n: u32) -> Option<u32> {
        let l = self.get(n, F::Left)?;
        let r = self.get(n, F::Right)?;
        self.first_between_f(self.range(l), self.range(r), |t| !self.is_punct(t, ")"))
    }
}

/// A `Literal` node's value kind.
pub enum Lit {
    Str(String),
    Num(f64),
    Other,
}

impl Ext<'_> {
    pub fn literal(&self, n: u32) -> Lit {
        if self.ty(n) != Ty::Literal {
            return Lit::Other;
        }
        let raw = self.text(self.range(n));
        match raw.as_bytes().first() {
            Some(b'"' | b'\'') if raw.len() >= 2 => Lit::Str(js_unescape(&raw[1..raw.len() - 1])),
            Some(b'0'..=b'9' | b'.') if !raw.ends_with('n') => {
                crate::utils::tpl_misc::js_to_number(raw).map_or(Lit::Other, Lit::Num)
            }
            _ => Lit::Other,
        }
    }
    /// `getStaticPropertyName(property)` for a non-computed key (`None` = `null`).
    pub fn static_key_name(&self, p: u32) -> Option<String> {
        let k = self.get(p, F::Key)?;
        match self.ty(k) {
            Ty::Identifier => Some(self.text(self.range(k)).to_string()),
            Ty::Literal => match self.literal(k) {
                Lit::Str(s) => Some(s),
                Lit::Num(n) => Some(crate::utils::js::js_number(n)),
                Lit::Other => Some(self.text(self.range(k)).to_string()),
            },
            Ty::TemplateLiteral if self.list(k, F::Expressions).is_empty() => {
                let r = self.range(k);
                Some(js_unescape(self.text((r.0 + 1, r.1.saturating_sub(1)))))
            }
            _ => None,
        }
    }
}

/// Cooked value of a JS string literal body.
pub fn js_unescape(s: &str) -> String {
    if !s.contains('\\') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let Some(e) = it.next() else { break };
        match e {
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'r' => out.push('\r'),
            'b' => out.push('\u{8}'),
            'f' => out.push('\u{c}'),
            'v' => out.push('\u{b}'),
            '0' if !it.peek().is_some_and(|c| c.is_ascii_digit()) => out.push('\0'),
            'x' => {
                let h: String = it.by_ref().take(2).collect();
                if let Some(c) = u32::from_str_radix(&h, 16).ok().and_then(char::from_u32) {
                    out.push(c);
                }
            }
            'u' => {
                let code = if it.peek() == Some(&'{') {
                    it.next();
                    let h: String = it.by_ref().take_while(|&c| c != '}').collect();
                    u32::from_str_radix(&h, 16).ok()
                } else {
                    let h: String = it.by_ref().take(4).collect();
                    u32::from_str_radix(&h, 16).ok()
                };
                if let Some(c) = code.and_then(char::from_u32) {
                    out.push(c);
                }
            }
            '\r' => {
                if it.peek() == Some(&'\n') {
                    it.next();
                }
            }
            '\n' | '\u{2028}' | '\u{2029}' => {}
            other => out.push(other),
        }
    }
    out
}

/// `getNextLocation(sourceCode, loc)` as an offset: the next column, or the next line start.
pub fn next_location(src: &str, pos: u32) -> u32 {
    let rest = &src[(pos as usize).min(src.len())..];
    match rest.chars().next() {
        None => pos,
        Some('\r') if rest.starts_with("\r\n") => pos + 2,
        Some(c) => pos + c.len_utf8() as u32,
    }
}
