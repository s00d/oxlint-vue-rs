//! Indentation engine shared by `vue/html-indent` and `vue/script-indent` — a port of
//! eslint-plugin-vue `lib/utils/indent-common.ts`, `indent-ts.ts` and `indent-utils.ts`.
//!
//! - [`store`]: ESLint `TokenStore` semantics over template or script tokens.
//! - [`tree`]: ESTree-shaped view of the oxc AST / template document.
//! - [`engine`]: offsets, node handlers (in ESLint selector order) and line validation.

mod engine;
pub mod store;
pub mod tree;

use oxc::span::Span;
use serde_json::Value;

use crate::ast::TokenKind;
use crate::rule::{Ctx, Fix, RuleOptions};

use store::{Store, Tok};
use tree::{Builder, F};

pub struct Opts {
    pub indent_char: char,
    pub indent_size: i32,
    pub base_indent: i32,
    pub attribute: i32,
    pub close_start: i32,
    pub close_end: i32,
    pub close_self: i32,
    pub switch_case: i32,
    pub align_attrs: bool,
    pub ignores: Vec<Selector>,
}

fn int(v: &Value) -> Option<i32> {
    v.as_i64().map(|n| n as i32)
}

impl Opts {
    /// `parseOptions(context.options[0], context.options[1] || {}, defaultOptions)`.
    pub fn parse(opts: &RuleOptions, default_base_indent: i32) -> Self {
        let mut o = Opts {
            indent_char: ' ',
            indent_size: 2,
            base_indent: default_base_indent,
            attribute: 1,
            close_start: 0,
            close_end: 0,
            close_self: 0,
            switch_case: 0,
            align_attrs: true,
            ignores: Vec::new(),
        };
        match opts.get(0) {
            Value::Number(n) => {
                if let Some(n) = n.as_i64() {
                    o.indent_size = n as i32;
                }
            }
            Value::String(s) if s == "tab" => {
                o.indent_char = '\t';
                o.indent_size = 1;
            }
            _ => {}
        }
        let u = opts.get(1);
        if let Some(n) = int(&u["baseIndent"]) {
            o.base_indent = n;
        }
        if let Some(n) = int(&u["attribute"]) {
            o.attribute = n;
        }
        match &u["closeBracket"] {
            Value::Number(n) => {
                let n = n.as_i64().unwrap_or(0) as i32;
                (o.close_start, o.close_end, o.close_self) = (n, n, n);
            }
            Value::Object(m) => {
                let g = |k: &str| m.get(k).and_then(int).unwrap_or(0);
                (o.close_start, o.close_end, o.close_self) =
                    (g("startTag"), g("endTag"), g("selfClosingTag"));
            }
            _ => {}
        }
        if let Some(n) = int(&u["switchCase"]) {
            o.switch_case = n;
        }
        if let Some(b) = u["alignAttributesVertically"].as_bool() {
            o.align_attrs = b;
        }
        if let Some(a) = u["ignores"].as_array() {
            o.ignores = a
                .iter()
                .filter_map(Value::as_str)
                .map(Selector::parse)
                .collect();
        }
        o
    }
}

/// A problem found by the engine.
pub struct Report {
    pub start: u32,
    pub end: u32,
    pub loc: [u32; 4],
    pub message: String,
    pub fix: (u32, u32, String),
}

#[derive(Clone, Copy, Debug)]
pub enum Comb {
    Descendant,
    Child,
}

#[derive(Debug)]
pub enum Simple {
    Any,
    Type(String),
    Attr(String, Option<String>),
    Field(F),
    /// Unsupported selector syntax: never matches.
    Never,
}

/// A small esquery subset for the `ignores` option: `Type`, `*`, `[attr]` / `[attr=value]`,
/// `.field`, descendant and `>` child combinators.
// ponytail: no :not/:matches/sibling selectors; add when a real config needs them.
#[derive(Debug)]
pub struct Selector {
    /// Compounds left to right, each with the combinator that precedes it.
    parts: Vec<(Comb, Vec<Simple>)>,
}

impl Selector {
    pub fn parse(src: &str) -> Selector {
        let mut parts: Vec<(Comb, Vec<Simple>)> = Vec::new();
        let mut comb = Comb::Descendant;
        let s = src.trim();
        let b = s.as_bytes();
        let mut i = 0;
        let mut cur: Vec<Simple> = Vec::new();
        let ident = |i: &mut usize| {
            let st = *i;
            while *i < b.len() && (b[*i].is_ascii_alphanumeric() || b[*i] == b'_' || b[*i] == b'$')
            {
                *i += 1;
            }
            s[st..*i].to_string()
        };
        while i < b.len() {
            match b[i] {
                b' ' | b'>' => {
                    let mut child = false;
                    while i < b.len() && (b[i] == b' ' || b[i] == b'>') {
                        child |= b[i] == b'>';
                        i += 1;
                    }
                    if !cur.is_empty() {
                        parts.push((comb, std::mem::take(&mut cur)));
                    }
                    comb = if child { Comb::Child } else { Comb::Descendant };
                }
                b'*' => {
                    cur.push(Simple::Any);
                    i += 1;
                }
                b'.' => {
                    i += 1;
                    let name = ident(&mut i);
                    cur.push(F::from_name(&name).map_or(Simple::Never, Simple::Field));
                }
                b'[' => {
                    let end = s[i..].find(']').map_or(s.len(), |e| i + e);
                    let inner = &s[i + 1..end];
                    let simple = match inner.split_once('=') {
                        Some((k, v)) => Simple::Attr(
                            k.trim().to_string(),
                            Some(v.trim().trim_matches(['"', '\'']).to_string()),
                        ),
                        None => Simple::Attr(inner.trim().to_string(), None),
                    };
                    cur.push(simple);
                    i = end + 1;
                }
                c if c.is_ascii_alphabetic() => {
                    let name = ident(&mut i);
                    cur.push(Simple::Type(name));
                }
                _ => {
                    cur.push(Simple::Never);
                    i += 1;
                }
            }
        }
        if !cur.is_empty() {
            parts.push((comb, cur));
        }
        Selector { parts }
    }
}

fn push_reports(ctx: &mut Ctx, reports: Vec<Report>) {
    for r in reports {
        let fix = Fix::replace(Span::new(r.fix.0, r.fix.1), r.fix.2);
        ctx.report_fix(Span::new(r.start, r.end), None, r.message, Some(fix));
        if let Some(d) = ctx.out.last_mut() {
            d.loc = Some(r.loc);
        }
    }
}

fn is_vue(ctx: &Ctx) -> bool {
    ctx.file.path.to_string_lossy().ends_with(".vue")
}

/// `vue/html-indent`.
pub fn run_html(ctx: &mut Ctx, opts: &Opts) {
    if !is_vue(ctx) {
        return;
    }
    let Some(doc) = ctx.doc() else { return };
    let Some(root) = doc.template_body else {
        return;
    };
    let src = ctx.source();
    let root_span = doc.element(root).span;
    let mut all: Vec<Tok> = doc
        .tokens_in(root_span)
        .iter()
        .map(|t| Tok {
            start: t.span.start,
            end: t.span.end,
            kind: t.kind,
            comment: false,
        })
        .collect();
    all.extend(doc.comments.iter().map(|c| Tok {
        start: c.span.start,
        end: c.span.end,
        kind: c.kind,
        comment: true,
    }));
    let store = Store::new(src, all);
    let mut b = Builder::new(src);
    let root_id = b.element(doc, root);
    let tree = b.t;
    let mut e = engine::Engine::new(&tree, &store, opts, ctx.file.lines, root_id);
    e.run();
    let reports = std::mem::take(&mut e.out);
    push_reports(ctx, reports);
}

/// `vue/script-indent`.
pub fn run_script(ctx: &mut Ctx, opts: &Opts) {
    if !is_vue(ctx) || !ctx.file.is_sfc {
        return;
    }
    let Some(program) = ctx.file.program else {
        return;
    };
    let src = ctx.source();
    let scripts = &ctx.file.scripts;
    let mut all: Vec<Tok> = ctx
        .file
        .script_tokens
        .iter()
        .filter(|t| t.start() < t.end())
        .map(|t| Tok {
            start: t.start(),
            end: t.end(),
            kind: crate::parser::token_kind(t.kind()),
            comment: false,
        })
        .collect();
    let (Some(first_code), Some(last_code)) = (all.first().copied(), all.last().copied()) else {
        return;
    };
    all.extend(program.comments.iter().map(|c| Tok {
        start: c.span.start,
        end: c.span.end,
        kind: if c.is_line() {
            TokenKind::LineComment
        } else {
            TokenKind::BlockComment
        },
        comment: true,
    }));
    // vue-eslint-parser adds the `<script>` / `</script>` tags as Punctuator tokens.
    if let Some(doc) = ctx.doc() {
        for s in scripts {
            let Some(el) = doc.top_level().find(|e| e.span == s.span) else {
                continue;
            };
            let st = el.start_tag.span;
            all.push(Tok {
                start: st.start,
                end: st.end,
                kind: TokenKind::Punctuator,
                comment: false,
            });
            if let Some(et) = el.end_tag {
                all.push(Tok {
                    start: et.start,
                    end: et.end,
                    kind: TokenKind::Punctuator,
                    comment: false,
                });
            }
        }
    }
    // ESTree `Program` range: ESLint 9 parsers start it at the beginning of the script text (so
    // leading comments are inside) and end it at the last token; with `<script>` + `<script setup>`
    // vue-eslint-parser spans min(start of the `<script>` text, first statement) to the last
    // statement.
    let stmts = || {
        program
            .body
            .iter()
            .map(oxc::span::GetSpan::span)
            .chain(program.directives.iter().map(|d| d.span))
    };
    let range = if scripts.len() > 1 {
        let text = scripts.iter().find(|s| !s.setup).map(|s| s.content.start);
        let start = stmts().map(|s| s.start).chain(text).min();
        let end = stmts().map(|s| s.end).max();
        (
            start.unwrap_or(first_code.start),
            end.unwrap_or(last_code.end),
        )
    } else {
        let text = scripts
            .first()
            .map_or(first_code.start, |s| s.content.start);
        (text.min(first_code.start), last_code.end)
    };
    let store = Store::new(src, all);
    let mut b = Builder::new(src);
    let root_id = b.program(program, range);
    let tree = b.t;
    let mut e = engine::Engine::new(&tree, &store, opts, ctx.file.lines, root_id);
    e.run();
    let reports = std::mem::take(&mut e.out);
    push_reports(ctx, reports);
}
