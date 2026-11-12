//! vue/no-irregular-whitespace

use oxc::ast::ast::{RegExpLiteral, StringLiteral, TemplateLiteral};
use oxc::ast_visit::{Visit, walk};
use oxc::span::Span;

use crate::ast::{AttributeKey, AttributeValue, Child};
use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::tpl_core::{BODY, for_each_root, visit_root};
use crate::utils::tpl_misc::quasi_span;
use crate::utils::tpl_valid::body_elements;

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let o = opts.get(0);
    let flag = |k: &str, d: bool| o.get(k).and_then(|v| v.as_bool()).unwrap_or(d);
    Some(Box::new(NoIrregularWhitespace {
        comments: flag("skipComments", false),
        strings: flag("skipStrings", true),
        templates: flag("skipTemplates", false),
        regexps: flag("skipRegExps", false),
        attr_values: flag("skipHTMLAttributeValues", false),
        text: flag("skipHTMLTextContents", false),
    }))
}

struct NoIrregularWhitespace {
    comments: bool,
    strings: bool,
    templates: bool,
    regexps: bool,
    attr_values: bool,
    text: bool,
}

/// `IRREGULAR_WHITESPACE` (without the line terminators).
fn is_irregular_ws(c: char) -> bool {
    matches!(
        c,
        '\u{c}' | '\u{b}' | '\u{85}' | '\u{feff}' | '\u{a0}' | '\u{1680}' | '\u{180e}'
            | '\u{2000}'..='\u{200b}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{3000}'
    )
}

/// `ALL_IRREGULARS`.
fn has_irregular(s: &str) -> bool {
    s.chars()
        .any(|c| is_irregular_ws(c) || c == '\u{2028}' || c == '\u{2029}')
}

/// Removal ranges collected from literals / template elements.
struct Skips<'r> {
    rule: &'r NoIrregularWhitespace,
    src: &'r str,
    out: Vec<Span>,
    /// Template expressions: TS type nodes are not visited (ESTree visitor keys).
    template: bool,
}

impl Skips<'_> {
    fn text(&self, s: Span) -> &str {
        &self.src[s.start as usize..s.end as usize]
    }
}

impl<'b> Visit<'b> for Skips<'_> {
    fn visit_ts_type_annotation(&mut self, it: &oxc::ast::ast::TSTypeAnnotation<'b>) {
        if !self.template {
            walk::walk_ts_type_annotation(self, it);
        }
    }
    fn visit_string_literal(&mut self, it: &StringLiteral<'b>) {
        if self.rule.strings && has_irregular(self.text(it.span)) {
            self.out.push(it.span);
        }
    }
    fn visit_reg_exp_literal(&mut self, it: &RegExpLiteral<'b>) {
        if self.rule.regexps && has_irregular(self.text(it.span)) {
            self.out.push(it.span);
        }
    }
    fn visit_template_literal(&mut self, it: &TemplateLiteral<'b>) {
        if self.rule.templates {
            for (i, q) in it.quasis.iter().enumerate() {
                if has_irregular(q.value.raw.as_str()) {
                    self.out.push(quasi_span(it, i));
                }
            }
        }
        walk::walk_template_literal(self, it);
    }
}

/// ESTree `Program` range: the whole text for a script file; in an SFC the `<script>` content,
/// with `<script>` + `<script setup>` first statement .. last statement (checked against ESLint).
fn program_range(ctx: &Ctx) -> Option<Span> {
    let program = ctx.file.program?;
    if !ctx.file.is_sfc {
        return Some(Span::new(0, ctx.source().len() as u32));
    }
    match ctx.file.scripts.as_slice() {
        [s] => Some(s.content),
        _ => {
            let spans = program
                .body
                .iter()
                .map(oxc::span::GetSpan::span)
                .chain(program.directives.iter().map(|d| d.span));
            let start = spans.clone().map(|s| s.start).min()?;
            let end = spans.map(|s| s.end).max()?;
            Some(Span::new(start, end))
        }
    }
}

impl Rule for NoIrregularWhitespace {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let src = ctx.source();
        let mut errors: Vec<Span> = Vec::new();
        let mut run: Option<usize> = None;
        for (i, c) in src.char_indices() {
            if is_irregular_ws(c) {
                run.get_or_insert(i);
                continue;
            }
            if let Some(s) = run.take() {
                errors.push(Span::new(s as u32, i as u32));
            }
            if c == '\u{2028}' || c == '\u{2029}' {
                errors.push(Span::new(i as u32, (i + c.len_utf8()) as u32));
            }
        }
        if let Some(s) = run {
            errors.push(Span::new(s as u32, src.len() as u32));
        }
        if errors.is_empty() {
            return;
        }
        let mut skips = Skips {
            rule: self,
            src,
            out: Vec::new(),
            template: false,
        };
        if let Some(p) = ctx.file.program {
            skips.visit_program(p);
        }
        let mut skip = std::mem::take(&mut skips.out);
        for_each_root(ctx, BODY, |_, root| {
            let mut s = Skips {
                rule: self,
                src,
                out: Vec::new(),
                template: true,
            };
            visit_root(&mut s, root.expr);
            skip.extend(s.out);
        });
        let doc = ctx.doc();
        if let Some(doc) = doc {
            for e in body_elements(doc) {
                if self.attr_values {
                    for a in e.attributes() {
                        if let (AttributeKey::Plain(_), Some(AttributeValue::Literal(l))) =
                            (&a.key, &a.value)
                            && has_irregular(doc.slice(l.span))
                        {
                            skip.push(l.span);
                        }
                    }
                }
                if self.text {
                    for c in &e.children {
                        if let Child::Text(t) = c {
                            let span = doc.text(*t).span;
                            if has_irregular(doc.slice(span)) {
                                skip.push(span);
                            }
                        }
                    }
                }
            }
        }
        if self.comments {
            if let Some(p) = ctx.file.program {
                for c in &p.comments {
                    if has_irregular(c.content_span().source_text(src)) {
                        skip.push(c.span);
                    }
                }
            }
            if let Some(doc) = doc
                && doc.template_body.is_some()
            {
                for c in &doc.comments {
                    if has_irregular(&c.value) {
                        skip.push(c.span);
                    }
                }
            }
        }
        for s in &skip {
            errors.retain(|e| e.start < s.start || s.end <= e.start);
        }
        let script = program_range(ctx).unwrap_or(Span::new(0, 0));
        let template = doc
            .and_then(|d| d.template_body)
            .map(|id| ctx.doc().unwrap().element(id).span)
            .unwrap_or(Span::new(0, 0));
        let inside = |r: Span, p: u32| r.start <= p && p < r.end;
        for e in errors {
            if inside(script, e.start) || inside(template, e.start) {
                ctx.report_id(e, "disallow", "Irregular whitespace not allowed.");
            }
        }
    }
}
