//! Rule API: rule trait, per-file context, diagnostics and fixes.

use std::path::Path;

use oxc::ast::ast::Program;
use oxc::semantic::Semantic;
use oxc::span::Span;
use serde_json::Value;

use crate::ast::Document;

/// A parsed `<script>` block of an SFC.
#[derive(Debug, Clone)]
pub struct ScriptBlock {
    /// The whole `<script …>…</script>` element span.
    pub span: Span,
    /// Content span (between the tags).
    pub content: Span,
    pub setup: bool,
    /// `lang` attribute value (`ts`, `tsx`, `jsx`, …), `None` = JS.
    pub lang: Option<String>,
}

/// Everything a rule can see about one file. Built once per file by [`crate::lint`].
pub struct LintFile<'a> {
    pub source: &'a str,
    pub path: &'a Path,
    /// `true` for `.vue` files parsed as SFC.
    pub is_sfc: bool,
    /// Whole-file HTML/template document (SFC only).
    pub doc: Option<Document<'a>>,
    pub scripts: Vec<ScriptBlock>,
    /// The script program. For an SFC, `<script>` and `<script setup>` contents are merged in one
    /// program, the rest of the file is masked with spaces, so spans are absolute file offsets.
    /// For a `.js/.ts` file it is the whole file.
    pub program: Option<&'a Program<'a>>,
    pub semantic: Option<&'a Semantic<'a>>,
    /// Script tokens (absolute spans), empty when there is no program.
    pub script_tokens: &'a [oxc::parser::Token],
    /// Script parsed as TypeScript (also governs template expressions).
    pub ts: bool,
    /// ESLint `settings` object (shared settings).
    pub settings: &'a Value,
    /// Declared globals (ESLint `languageOptions.globals` / oxlint `globals`): name → setting.
    pub globals: &'a Value,
    /// Precomputed line starts for location conversion.
    pub lines: &'a crate::LineIndex,
}

impl<'a> LintFile<'a> {
    pub fn slice(&self, span: Span) -> &'a str {
        &self.source[span.start as usize..span.end as usize]
    }
    pub fn script_setup(&self) -> Option<&ScriptBlock> {
        self.scripts.iter().find(|s| s.setup)
    }
    /// Whether `pos` lies inside the given script block kind.
    pub fn in_script_setup(&self, pos: u32) -> bool {
        self.script_setup()
            .is_some_and(|s| s.content.start <= pos && pos <= s.content.end)
    }
}

/// A source edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fix {
    pub span: Span,
    pub text: String,
}

impl Fix {
    pub fn replace(span: Span, text: impl Into<String>) -> Self {
        Self {
            span,
            text: text.into(),
        }
    }
    pub fn insert(at: u32, text: impl Into<String>) -> Self {
        Self {
            span: Span::new(at, at),
            text: text.into(),
        }
    }
    pub fn remove(span: Span) -> Self {
        Self {
            span,
            text: String::new(),
        }
    }
    /// Merge several edits into one (ESLint semantics for a fixer returning an array):
    /// the result replaces min(start)..max(end) with the edited text.
    /// Returns `None` when edits overlap (ESLint throws there; rules must not do it).
    pub fn merge(source: &str, mut edits: Vec<Fix>) -> Option<Fix> {
        if edits.is_empty() {
            return None;
        }
        if edits.len() == 1 {
            return edits.pop();
        }
        edits.sort_by_key(|f| (f.span.start, f.span.end));
        let start = edits[0].span.start;
        let end = edits.iter().map(|f| f.span.end).max().unwrap();
        let mut text = String::new();
        let mut pos = start;
        for f in &edits {
            if f.span.start < pos {
                return None;
            }
            text.push_str(&source[pos as usize..f.span.start as usize]);
            text.push_str(&f.text);
            pos = f.span.end;
        }
        text.push_str(&source[pos as usize..end as usize]);
        Some(Fix {
            span: Span::new(start, end),
            text,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Suggestion {
    pub message_id: Option<&'static str>,
    pub desc: String,
    pub fix: Fix,
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    /// Rule name without the `vue/` prefix.
    pub rule: &'static str,
    pub span: Span,
    pub message: String,
    pub message_id: Option<&'static str>,
    pub fix: Option<Fix>,
    pub suggestions: Vec<Suggestion>,
    pub severity: Severity,
    /// Explicit `[line, column, endLine, endColumn]` (1-based, ESLint columns) for reports whose
    /// ESLint `loc` is not derived from source offsets (e.g. `max-len` tab-expanded columns).
    pub loc: Option<[u32; 4]>,
}

impl Diagnostic {
    /// `[line, column, endLine, endColumn]` (1-based, UTF-16 columns) as ESLint reports them.
    pub fn location(&self, lines: &crate::LineIndex, src: &str) -> [u32; 4] {
        self.loc.unwrap_or_else(|| {
            let (l, c) = lines.line_col(src, self.span.start);
            let (el, ec) = lines.line_col(src, self.span.end);
            [l, c, el, ec]
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Warn,
    Error,
}

/// Per-rule reporting context.
pub struct Ctx<'f, 'a> {
    pub file: &'f LintFile<'a>,
    pub rule: &'static str,
    pub severity: Severity,
    pub(crate) out: &'f mut Vec<Diagnostic>,
}

impl<'f, 'a> Ctx<'f, 'a> {
    pub fn source(&self) -> &'a str {
        self.file.source
    }
    pub fn doc(&self) -> Option<&'f Document<'a>> {
        self.file.doc.as_ref()
    }
    pub fn slice(&self, span: Span) -> &'a str {
        self.file.slice(span)
    }
    /// Report with a plain message.
    pub fn report(&mut self, span: Span, message: impl Into<String>) {
        self.push(span, None, message.into(), None, Vec::new());
    }
    /// Report with message id + message (message ids are kept for diagnostics/tests).
    pub fn report_id(&mut self, span: Span, id: &'static str, message: impl Into<String>) {
        self.push(span, Some(id), message.into(), None, Vec::new());
    }
    pub fn report_fix(
        &mut self,
        span: Span,
        id: Option<&'static str>,
        message: impl Into<String>,
        fix: Option<Fix>,
    ) {
        self.push(span, id, message.into(), fix, Vec::new());
    }
    /// Full form.
    pub fn push(
        &mut self,
        span: Span,
        message_id: Option<&'static str>,
        message: String,
        fix: Option<Fix>,
        suggestions: Vec<Suggestion>,
    ) {
        self.out.push(Diagnostic {
            rule: self.rule,
            span,
            message,
            message_id,
            fix,
            suggestions,
            severity: self.severity,
            loc: None,
        });
    }
    /// Report with an explicit ESLint location (see [`Diagnostic::loc`]); `span` is still used
    /// for inline-directive matching.
    pub fn report_at(&mut self, span: Span, id: &'static str, message: String, loc: [u32; 4]) {
        self.push(span, Some(id), message, None, Vec::new());
        if let Some(d) = self.out.last_mut() {
            d.loc = Some(loc);
        }
    }
}

/// Options passed to a rule factory: the ESLint options array (`["error", opt1, opt2]` minus severity)
/// and the shared settings.
pub struct RuleOptions<'o> {
    pub options: &'o [Value],
    pub settings: &'o Value,
}

impl<'o> RuleOptions<'o> {
    /// `options[i]` or `Value::Null`.
    pub fn get(&self, i: usize) -> &'o Value {
        static NULL: Value = Value::Null;
        self.options.get(i).unwrap_or(&NULL)
    }
}

pub trait Rule: Send + Sync {
    fn run(&self, ctx: &mut Ctx<'_, '_>);
}

/// Static rule description.
pub struct RuleDef {
    /// Name without `vue/`.
    pub name: &'static str,
    /// `None` = not implemented yet.
    pub create: fn(&RuleOptions) -> Option<Box<dyn Rule>>,
}

/// Interpolate `{{name}}` placeholders like ESLint's message data.
pub fn fmt_msg(template: &str, data: &[(&str, &str)]) -> String {
    let mut s = template.to_string();
    for (k, v) in data {
        s = s
            .replace(&format!("{{{{{k}}}}}"), v)
            .replace(&format!("{{{{ {k} }}}}"), v);
    }
    s
}
