//! vue/html-comment-indent

use oxc::span::Span;

use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::js::json_string;
use crate::utils::tpl_style::{html_comments, is_js_ws};

struct R {
    indent_char: char,
    indent_text: String,
}

fn unit(c: char) -> String {
    match c {
        '\t' => "tab".into(),
        ' ' => "space".into(),
        c => json_string(&c.to_string()),
    }
}

fn display(s: &str, unit_char: Option<char>) -> String {
    if let Some(u) = unit_char
        && s.is_empty()
    {
        return format!("0 {}s", unit(u));
    }
    let n = s.encode_utf16().count();
    match s.chars().next() {
        Some(c @ (' ' | '\t')) if s.chars().all(|x| x == c) => {
            format!("{n} {}{}", unit(c), if n == 1 { "" } else { "s" })
        }
        _ => json_string(s),
    }
}

/// (line text, line start offset) of a 0-based line.
fn line_text<'a>(ctx: &Ctx<'_, 'a>, line: usize) -> (&'a str, u32) {
    let lines = ctx.file.lines;
    let src = ctx.source();
    let start = lines.line_start(line).unwrap_or(src.len() as u32);
    let end = lines.line_start(line + 1).unwrap_or(src.len() as u32) as usize;
    let t = &src[start as usize..end];
    (
        t.trim_end_matches(['\n', '\r', '\u{2028}', '\u{2029}']),
        start,
    )
}

fn indent_of(t: &str) -> &str {
    &t[..t.len() - t.trim_start_matches(is_js_ws).len()]
}

impl R {
    fn validate(&self, ctx: &mut Ctx, line: usize, base: &str, offset: usize) {
        let (t, start) = line_text(ctx, line);
        if t.trim_matches(is_js_ws).is_empty() {
            return;
        }
        let actual = indent_of(t);
        let exp_off = self.indent_text.repeat(offset);
        let expected = format!("{base}{exp_off}");
        let fix = Fix::replace(Span::new(start, start + actual.len() as u32), expected);
        if !base.is_empty() && (actual.len() < base.len() || !actual.starts_with(base)) {
            let cut = actual
                .char_indices()
                .nth(base.chars().count())
                .map_or(actual.len(), |(i, _)| i);
            let (id, msg) = if actual.is_empty() {
                (
                    "missingBaseIndentation",
                    format!(
                        "Expected base point indentation of {}, but not found.",
                        display(base, None)
                    ),
                )
            } else {
                (
                    "unexpectedBaseIndentation",
                    format!(
                        "Expected base point indentation of {}, but found {}.",
                        display(base, None),
                        display(&actual[..cut], None)
                    ),
                )
            };
            ctx.report_fix(
                Span::new(start, start + actual.len() as u32),
                Some(id),
                msg,
                Some(fix),
            );
            return;
        }
        let off = &actual[base.len()..];
        let off_start = start + base.len() as u32;
        for (i, c) in off.char_indices() {
            if c != self.indent_char {
                let s = off_start + i as u32;
                ctx.report_fix(
                    Span::new(s, s + c.len_utf8() as u32),
                    Some("unexpectedIndentationCharacter"),
                    format!(
                        "Expected {} character, but found {} character.",
                        unit(self.indent_char),
                        unit(c)
                    ),
                    Some(fix),
                );
                return;
            }
        }
        if off.len() != exp_off.len() {
            let (id, word) = if base.is_empty() {
                ("unexpectedIndentation", "indentation")
            } else {
                ("unexpectedRelativeIndentation", "relative indentation")
            };
            ctx.report_fix(
                Span::new(off_start, start + actual.len() as u32),
                Some(id),
                format!(
                    "Expected {word} of {} but found {}.",
                    display(&exp_off, Some(self.indent_char)),
                    display(off, Some(self.indent_char))
                ),
                Some(fix),
            );
        }
    }
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        let lines = ctx.file.lines;
        for c in html_comments(doc, &[], true) {
            let open_line = lines.line_of(c.open.start);
            let base = indent_of(line_text(ctx, open_line).0).to_string();
            let end_line = if let Some(v) = c.value {
                let sl = lines.line_of(v.start);
                let el = lines.line_of(v.end);
                let check = if lines.line_of(c.open.end) == sl {
                    sl + 1
                } else {
                    sl
                };
                for l in check..=el {
                    self.validate(ctx, l, &base, 1);
                }
                el
            } else {
                lines.line_of(c.open.end)
            };
            let close_line = lines.line_of(c.close.start);
            if end_line < close_line {
                self.validate(ctx, close_line, &base, 0);
            }
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let o = opts.get(0);
    let (indent_char, size) = match (o.as_f64(), o.as_str()) {
        (Some(n), _) if n.fract() == 0.0 && n >= 0.0 => (' ', n as usize),
        (_, Some("tab")) => ('\t', 1),
        _ => (' ', 2),
    };
    Some(Box::new(R {
        indent_char,
        indent_text: indent_char.to_string().repeat(size),
    }))
}
