//! ESLint `SourceCode` token navigation over the script tokens of a [`LintFile`].
//!
//! Tokens are returned as spans (absolute byte offsets). "With comments" variants also consider
//! script comments (spans include the `//` / `/* */` delimiters), like ESLint's `includeComments`.

use oxc::span::Span;

use crate::rule::LintFile;

fn comments<'a>(file: &LintFile<'a>) -> &'a [oxc::ast::Comment] {
    file.program.map_or(&[], |p| &p.comments[..])
}

/// Comment spans of the script, in source order.
pub fn comment_spans<'a>(file: &LintFile<'a>) -> impl DoubleEndedIterator<Item = Span> + 'a {
    comments(file).iter().map(|c| c.span)
}

fn idx_first_start_ge(file: &LintFile, pos: u32) -> usize {
    file.script_tokens.partition_point(|t| t.start() < pos)
}

/// `getTokenBefore(pos)`: last token ending at or before `pos`.
pub fn token_before(file: &LintFile, pos: u32) -> Option<Span> {
    let i = idx_first_start_ge(file, pos);
    file.script_tokens[..i]
        .iter()
        .rev()
        .map(|t| Span::new(t.start(), t.end()))
        .find(|s| s.end <= pos && s.start < s.end)
}

/// `getTokenAfter(pos)`: first token starting at or after `pos`.
pub fn token_after(file: &LintFile, pos: u32) -> Option<Span> {
    let i = idx_first_start_ge(file, pos);
    file.script_tokens[i..]
        .iter()
        .map(|t| Span::new(t.start(), t.end()))
        .find(|s| s.start < s.end)
}

/// `getTokenBefore(pos, { includeComments: true })`.
pub fn token_or_comment_before(file: &LintFile, pos: u32) -> Option<Span> {
    let t = token_before(file, pos);
    let c = comments(file)
        .iter()
        .rev()
        .map(|c| c.span)
        .find(|c| c.end <= pos);
    match (t, c) {
        (Some(t), Some(c)) => Some(if c.end > t.end { c } else { t }),
        (a, b) => a.or(b),
    }
}

/// `getTokenAfter(pos, { includeComments: true })`.
pub fn token_or_comment_after(file: &LintFile, pos: u32) -> Option<Span> {
    let t = token_after(file, pos);
    let c = comments(file)
        .iter()
        .map(|c| c.span)
        .find(|c| c.start >= pos);
    match (t, c) {
        (Some(t), Some(c)) => Some(if c.start < t.start { c } else { t }),
        (a, b) => a.or(b),
    }
}

/// First token after `pos` matching `pred` on its text (`getTokenAfter(node, filter)`).
pub fn token_after_where(file: &LintFile, pos: u32, pred: impl Fn(&str) -> bool) -> Option<Span> {
    let i = idx_first_start_ge(file, pos);
    file.script_tokens[i..]
        .iter()
        .map(|t| Span::new(t.start(), t.end()))
        .find(|s| s.start < s.end && pred(file.slice(*s)))
}

/// `getTokensBetween(a, b, { includeComments })`: tokens fully inside `start..end`, in order.
/// Binary-searched: tokens and comments are sorted by start, so this is O(log n + k), not O(n) per call.
pub fn tokens_between(file: &LintFile, start: u32, end: u32, include_comments: bool) -> Vec<Span> {
    let toks = file.script_tokens;
    let lo = toks.partition_point(|t| t.start() < start);
    let hi = toks.partition_point(|t| t.start() < end);
    let mut v: Vec<Span> = toks[lo..hi.max(lo)]
        .iter()
        .map(|t| Span::new(t.start(), t.end()))
        .filter(|s| s.start < s.end && s.end <= end)
        .collect();
    if include_comments {
        let cs = comments(file);
        let lo = cs.partition_point(|c| c.span.start < start);
        let hi = cs.partition_point(|c| c.span.start < end);
        v.extend(
            cs[lo..hi.max(lo)]
                .iter()
                .map(|c| c.span)
                .filter(|s| s.end <= end),
        );
        v.sort_by_key(|s| s.start);
    }
    v
}

/// `getFirstToken(node)`.
pub fn first_token(file: &LintFile, span: Span) -> Option<Span> {
    token_after(file, span.start).filter(|t| t.end <= span.end)
}

/// `getLastToken(node)`.
pub fn last_token(file: &LintFile, span: Span) -> Option<Span> {
    token_before(file, span.end).filter(|t| t.start >= span.start)
}

/// `getCommentsBefore(node)`: comments between the previous token and `pos`.
pub fn comments_before<'a>(file: &LintFile<'a>, pos: u32) -> Vec<&'a oxc::ast::Comment> {
    let prev_end = token_before(file, pos).map_or(0, |t| t.end);
    comments(file)
        .iter()
        .filter(|c| c.span.start >= prev_end && c.span.end <= pos)
        .collect()
}

/// Whether `span` is exactly a script comment.
pub fn is_comment(file: &LintFile, span: Span) -> bool {
    comments(file).iter().any(|c| c.span == span)
}

/// 0-based line of an offset.
pub fn line(file: &LintFile, pos: u32) -> usize {
    file.lines.line_of(pos)
}

/// Byte offset of the start of the line containing `pos`.
pub fn line_start(file: &LintFile, pos: u32) -> u32 {
    file.lines.line_start(file.lines.line_of(pos)).unwrap_or(0)
}

/// Whether `span` starts and ends on the same line.
pub fn is_single_line(file: &LintFile, span: Span) -> bool {
    line(file, span.start) == line(file, span.end)
}
