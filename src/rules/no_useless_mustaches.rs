//! vue/no-useless-mustaches

use std::sync::LazyLock;

use crate::utils::regexp::JsRegex as Regex;

use crate::ast::{Child, Document, ExprContainer, ExprParent, VExpression};
use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::js::{is_string_literal, string_literal_value};
use crate::utils::tpl_misc::{body_exprs, has_js_comment, has_useful_escape, string_raw};
use crate::utils::tpl_valid::expr_span;

struct R {
    ignore_comment: bool,
    ignore_escape: bool,
}

static QUOTED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^(?:&(?:quot|apos|#\d+|#x[\da-f]+);|["'`])([\s\S]*)(?:&(?:quot|apos|#\d+|#x[\da-f]+);|["'`])$"#)
        .unwrap()
});
static CHAR_REF: LazyLock<Regex> = LazyLock::new(|| Regex::with_flags(r"&(?:[a-z][\da-z]*|#\d+|#x[\da-f]+);", "i").unwrap());
static BACKSLASH_REF: LazyLock<Regex> = LazyLock::new(|| Regex::with_flags(r"&(?:bsol|#0*92|#x0*5c);", "i").unwrap());

fn strip_quotes(text: &str) -> Option<String> {
    let f = text.chars().next()?;
    if matches!(f, '"' | '\'' | '`') && text.len() >= 2 && text.ends_with(f) {
        return Some(text[1..text.len() - 1].to_string());
    }
    if matches!(f, '"' | '\'' | '`') && text.len() == 1 {
        // `text[0] === text.at(-1)` with a one-char text.
        return Some(String::new());
    }
    QUOTED.capture(text, 1)
}

fn has_unsafe_char_ref(text: &str) -> bool {
    if !text.contains('&') {
        return false;
    }
    let incomplete = CHAR_REF.replace_all(text, "").contains('&');
    text.contains('\\') || BACKSLASH_REF.is_match(text) || incomplete
}

fn after_char_ref_start(src: &str, offset: usize) -> bool {
    let b = src.as_bytes();
    let mut i = offset as isize - 1;
    while i >= 0 && (b[i as usize].is_ascii_alphanumeric() || b[i as usize] == b'_' || b[i as usize] == b'#') {
        i -= 1;
    }
    i >= 0 && b[i as usize] == b'&'
}

fn char_at(src: &str, pos: Option<usize>) -> String {
    pos.and_then(|p| src.get(p..)).and_then(|s| s.chars().next()).map(String::from).unwrap_or_default()
}

/// Vue drops a whitespace-only text node at the start / end of an element (`condense`), but keeps it
/// as a space once it is part of a text node. `<b>\n  {{ ':' }}\n</b>` renders ":", `<b>\n  :\n</b>`
/// renders " : " (eslint-plugin-vue fixes it anyway).
fn merges_dropped_whitespace(doc: &Document, c: &ExprContainer) -> bool {
    let ExprParent::Element(Some(p)) = c.parent else {
        return false;
    };
    let parent = doc.element(p);
    if parent.name == "pre" {
        return false;
    }
    let kids = &parent.children;
    let Some(i) = kids.iter().position(|k| *k == Child::Expr(c.id)) else {
        return false;
    };
    let ws_text = |k: Option<&Child>| match k {
        Some(Child::Text(t)) => doc
            .text(*t)
            .value
            .chars()
            .all(|c| matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0c')),
        _ => false,
    };
    (i == 1 && ws_text(kids.first())) || (i + 2 == kids.len() && ws_text(kids.last()))
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for c in body_exprs(doc) {
            if !matches!(c.parent, ExprParent::Element(Some(_))) {
                continue;
            }
            let Some(VExpression::Expression(x)) = &c.expression else { continue };
            if !is_string_literal(x) {
                continue;
            }
            let Some(value) = string_literal_value(x) else { continue };
            let raw = string_raw(doc, x).unwrap_or("");
            let has_comment = has_js_comment(doc, c.span);
            if self.ignore_comment && has_comment {
                continue;
            }
            let has_escape = has_useful_escape(raw, &value);
            if self.ignore_escape && has_escape {
                continue;
            }
            let fix = (!has_comment && !has_escape && !merges_dropped_whitespace(doc, c))
                .then(|| strip_quotes(doc.slice(expr_span(x))))
                .flatten()
                .filter(|t| {
                    !t.contains('\n')
                        && !t.starts_with(char::is_whitespace)
                        && !t.ends_with(char::is_whitespace)
                })
                .and_then(|text| {
                    let mut rep = String::new();
                    let mut it = text.replace('<', "&lt;").chars().collect::<Vec<_>>().into_iter();
                    while let Some(ch) = it.next() {
                        if ch == '\\' {
                            match it.next() {
                                Some(n) => rep.push(n),
                                None => rep.push('\\'),
                            }
                        } else {
                            rep.push(ch);
                        }
                    }
                    let src = doc.source;
                    let before = (c.span.start as usize).checked_sub(1).and_then(|p| {
                        (0..=p).rev().find(|i| src.is_char_boundary(*i))
                    });
                    let cb = char_at(src, before);
                    let ca = char_at(src, Some(c.span.end as usize));
                    let starts = format!("{cb}{rep}{ca}").contains("{{");
                    let joins = after_char_ref_start(src, c.span.start as usize)
                        && format!("{rep}{ca}")
                            .chars()
                            .next()
                            .is_some_and(|f| f.is_ascii_alphanumeric() || matches!(f, '_' | '#' | ';'));
                    (!(starts || joins || has_unsafe_char_ref(&text))).then(|| Fix::replace(c.span, rep))
                });
            ctx.report_fix(c.span, None, "Unexpected mustache interpolation with a string literal value.", fix);
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let o = opts.get(0);
    Some(Box::new(R {
        ignore_comment: o["ignoreIncludesComment"].as_bool() == Some(true),
        ignore_escape: o["ignoreStringEscape"].as_bool() == Some(true),
    }))
}
