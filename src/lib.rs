//! oxlint-vue-rs: eslint-plugin-vue rules for the oxc toolchain.

pub mod ast;
pub mod config;
pub mod directives;
pub mod parser;
pub mod rule;
pub mod rules;
pub mod utils;

use std::path::Path;

use oxc::allocator::Allocator;
use oxc::parser::{ParseOptions, Parser, config::TokensParserConfig};
use oxc::semantic::SemanticBuilder;
use oxc::span::{SourceType, Span};
use serde_json::Value;

pub use rule::{Ctx, Diagnostic, Fix, LintFile, Rule, RuleDef, RuleOptions, ScriptBlock, Severity};

/// Line starts + UTF-16 aware column conversion (ESLint reports UTF-16 columns).
pub struct LineIndex {
    starts: Vec<u32>,
    /// Per line: all ASCII, so a column is a byte difference (no UTF-16 re-encoding).
    ascii: Vec<bool>,
}

impl LineIndex {
    /// Line table for `src`: vue-eslint-parser's (`\n` only) for an SFC, ESLint's otherwise.
    pub fn for_source(src: &str, sfc: bool) -> Self {
        if sfc { Self::sfc(src) } else { Self::new(src) }
    }
    /// vue-eslint-parser `LinesAndColumns`: an SFC breaks lines only on `\n` (CR, U+2028/9 don't).
    pub fn sfc(src: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(
            src.bytes()
                .enumerate()
                .filter(|&(_, b)| b == b'\n')
                .map(|(i, _)| i as u32 + 1),
        );
        Self::with_ascii(src, starts)
    }

    fn with_ascii(src: &str, starts: Vec<u32>) -> Self {
        let b = src.as_bytes();
        let ascii = (0..starts.len())
            .map(|i| {
                let end = starts.get(i + 1).map_or(b.len(), |&e| e as usize);
                b[starts[i] as usize..end].is_ascii()
            })
            .collect();
        Self { starts, ascii }
    }
    /// ESLint line breaks (`\n`, `\r\n`, `\r`, U+2028, U+2029) for plain scripts.
    pub fn new(src: &str) -> Self {
        let mut starts = vec![0];
        let b = src.as_bytes();
        let mut i = 0;
        while i < b.len() {
            match b[i] {
                b'\n' => starts.push(i as u32 + 1),
                b'\r' => {
                    if b.get(i + 1) == Some(&b'\n') {
                        i += 1;
                    }
                    starts.push(i as u32 + 1);
                }
                // U+2028 / U+2029 are line terminators for ESLint too.
                0xE2 if b.get(i + 1) == Some(&0x80)
                    && matches!(b.get(i + 2), Some(0xA8 | 0xA9)) =>
                {
                    i += 2;
                    starts.push(i as u32 + 1);
                }
                _ => {}
            }
            i += 1;
        }
        Self::with_ascii(src, starts)
    }
    /// 1-based line, 1-based UTF-16 column (ESLint/RuleTester convention).
    pub fn line_col(&self, src: &str, offset: u32) -> (u32, u32) {
        let line = self.starts.partition_point(|&s| s <= offset) - 1;
        let start = self.starts[line] as usize;
        let end = (offset as usize).min(src.len());
        if self.ascii.get(line) == Some(&true) {
            return (line as u32 + 1, end.saturating_sub(start) as u32 + 1);
        }
        // ponytail: a long non-ASCII line re-encodes its prefix per lookup; cache if that shows up.
        let col: usize = src
            .get(start..end)
            .map_or(end.saturating_sub(start), |s| s.encode_utf16().count());
        (line as u32 + 1, col as u32 + 1)
    }
    /// Byte offset of a 0-based line start.
    pub fn line_start(&self, line0: usize) -> Option<u32> {
        self.starts.get(line0).copied()
    }
    /// 0-based line of a byte offset.
    pub fn line_of(&self, offset: u32) -> usize {
        self.starts.partition_point(|&s| s <= offset) - 1
    }
    pub fn line_count(&self) -> usize {
        self.starts.len()
    }
}

/// How to treat the input.
#[derive(Debug, Clone, Default)]
pub struct FileOptions {
    /// Force SFC (`Some(true)`) or plain script (`Some(false)`); `None` = by extension (`.vue`).
    pub sfc: Option<bool>,
    /// Force TypeScript for scripts / template expressions (e.g. typescript-eslint parser in tests).
    pub ts: bool,
    /// Enable JSX in scripts.
    pub jsx: bool,
    /// ESLint shared settings.
    pub settings: Value,
    /// Declared globals (`{ "ref": "readonly" }`), as in ESLint/oxlint config.
    pub globals: Value,
    /// Honor `<!-- eslint-disable … -->` comments in the template (CLI: yes; plain ESLint: no).
    pub template_directives: bool,
    /// Extra rule prefixes accepted in directives besides `vue/`.
    pub prefixes: Vec<String>,
    /// Ignore every inline disable directive (ESLint `noInlineConfig`).
    pub no_inline_config: bool,
}

/// A rule ready to run.
pub struct ConfiguredRule {
    pub name: &'static str,
    pub severity: Severity,
    pub rule: Box<dyn Rule>,
}

/// Instantiate a rule by name (without `vue/`). `Ok(None)` = known but not implemented.
pub fn configure(
    name: &str,
    severity: Severity,
    options: &[Value],
    settings: &Value,
) -> Result<Option<ConfiguredRule>, String> {
    let def = rules::ALL
        .iter()
        .find(|d| d.name == name)
        .ok_or_else(|| format!("unknown rule vue/{name}"))?;
    Ok(
        (def.create)(&RuleOptions { options, settings }).map(|rule| ConfiguredRule {
            name: def.name,
            severity,
            rule,
        }),
    )
}

fn mask_scripts(source: &str, blocks: &[ScriptBlock]) -> String {
    let mut out: Vec<u8> = source
        .bytes()
        .map(|b| if b == b'\n' || b == b'\r' { b } else { b' ' })
        .collect();
    for b in blocks {
        let (s, e) = (b.content.start as usize, b.content.end as usize);
        out[s..e].copy_from_slice(&source.as_bytes()[s..e]);
    }
    // Masked bytes all become ASCII; script regions are copied whole at char boundaries.
    String::from_utf8(out).expect("masked source stays utf-8")
}

/// Script blocks of an SFC, from the parsed document.
pub fn script_blocks(doc: &ast::Document) -> Vec<ScriptBlock> {
    doc.top_level()
        .filter(|e| e.name == "script")
        .map(|e| {
            let start = e.start_tag.span.end;
            let end = e.end_tag.map_or(e.span.end, |t| t.start);
            let lang = e.attr("lang").and_then(|a| match &a.value {
                Some(ast::AttributeValue::Literal(l)) => Some(l.value.clone()),
                _ => None,
            });
            ScriptBlock {
                span: e.span,
                content: Span::new(start, end),
                setup: e.attr("setup").is_some(),
                lang,
            }
        })
        .collect()
}

/// Cheap guess of the script language from `<script … lang="…">` tags (ts, jsx).
fn guess_script_lang(source: &str) -> (bool, bool) {
    let (mut ts, mut jsx) = (false, false);
    let mut rest = source;
    while let Some(i) = rest.find("<script") {
        let tag = &rest[i..];
        let end = tag.find('>').unwrap_or(tag.len());
        let attrs = &tag[7..end];
        for (pat, t, j) in [
            ("\"tsx\"", true, true),
            ("'tsx'", true, true),
            ("\"ts\"", true, false),
            ("'ts'", true, false),
            ("\"jsx\"", false, true),
            ("'jsx'", false, true),
        ] {
            if attrs.contains("lang") && attrs.contains(pat) {
                ts |= t;
                jsx |= j;
            }
        }
        rest = &tag[end.max(1)..];
    }
    (ts, jsx)
}

/// Rules whose upstream reports take their location from ESLint `sourceCode.getLocFromIndex`
/// (`utils/html-comments.ts`, explicit `loc`s), so lines also break on CR and U+2028/9 in an SFC.
/// Other reports use parser node locations ([`LineIndex::sfc`]).
const ESLINT_LOC: &[&str] = &[
    "html-comment-content-spacing",
    "html-comment-content-newline",
    "html-comment-indent",
    "no-irregular-whitespace",
    "no-content-outside-block",
];

/// `Diagnostic::rule` of a fatal script parse error (ESLint: `ruleId: null`, `fatal: true`).
pub const PARSING_ERROR: &str = "";

/// Lint one source text with the given rules. A script syntax error yields only one
/// [`PARSING_ERROR`] diagnostic, like ESLint.
pub fn lint_source(
    source: &str,
    path: &Path,
    opts: &FileOptions,
    rules: &[ConfiguredRule],
) -> Vec<Diagnostic> {
    let alloc = Allocator::default();
    let is_sfc = opts
        .sfc
        .unwrap_or_else(|| path.extension().is_some_and(|e| e == "vue"));
    let lines = LineIndex::for_source(source, is_sfc);
    let eslint_lines =
        (is_sfc && source.contains(['\r', '\u{2028}', '\u{2029}'])).then(|| LineIndex::new(source));
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

    let mut doc = None;
    let mut scripts = Vec::new();
    let mut ts = opts.ts || matches!(ext, "ts" | "tsx" | "mts" | "cts");
    let mut jsx = opts.jsx || matches!(ext, "jsx" | "tsx");
    let script_src: &str = if is_sfc {
        // Template expressions follow the script language: pre-scan `<script lang>` so the document is
        // parsed once (a wrong guess only costs a re-parse).
        let (gts, gjsx) = guess_script_lang(source);
        let guess = parser::ParserOptions {
            ts: ts || gts,
            jsx: jsx || gjsx,
        };
        let first = parser::parse_document(&alloc, source, &guess);
        scripts = script_blocks(&first);
        for s in &scripts {
            match s.lang.as_deref() {
                Some("ts") => ts = true,
                Some("tsx") => {
                    ts = true;
                    jsx = true;
                }
                Some("jsx") => jsx = true,
                _ => {}
            }
        }
        let same = ts == guess.ts && jsx == guess.jsx;
        doc = Some(if same {
            first
        } else {
            parser::parse_document(&alloc, source, &parser::ParserOptions { ts, jsx })
        });
        if scripts.is_empty() {
            ""
        } else {
            alloc.alloc_str(&mask_scripts(source, &scripts))
        }
    } else {
        source
    };

    let mut program = None;
    let mut tokens: &[oxc::parser::Token] = &[];
    if !script_src.is_empty() {
        let st = SourceType::default()
            .with_module(true)
            .with_typescript(ts)
            .with_jsx(jsx);
        let ret = Parser::new(&alloc, script_src, st)
            .with_options(ParseOptions {
                allow_return_outside_function: true,
                ..ParseOptions::default()
            })
            .with_config(TokensParserConfig)
            .parse();
        // ESLint: a script syntax error is a fatal "Parsing error" for the file and no rule runs.
        // Only oxc's fatal errors: recoverable ones include checks typescript-eslint / espree skip
        // (e.g. TS modifier order).
        if ret.fatal_error {
            let d = ret.diagnostics.errors().next();
            let label = d.and_then(|d| d.labels.first());
            let at = label.map_or(0, |l| l.offset() as usize);
            let text = label
                .and_then(|l| script_src.get(at..at + l.len() as usize))
                .unwrap_or("");
            let (msg, at) = parser::script::espree_like_message(
                d.map_or("Parsing error.", |d| &d.message),
                text,
                at,
            );
            let at = at.min(source.len()) as u32;
            return vec![Diagnostic {
                rule: PARSING_ERROR,
                span: Span::new(at, at),
                message: format!("Parsing error: {msg}"),
                message_id: None,
                fix: None,
                suggestions: Vec::new(),
                severity: Severity::Error,
                loc: None,
            }];
        }
        tokens = alloc.alloc_slice_copy(&ret.tokens);
        let mut prog = ret.program;
        // The script is parsed from a masked copy of the file, so a `//` comment right before `</script>`
        // would run on over the masked tag. vue-eslint-parser ends it at the block end: clamp it.
        if is_sfc {
            for c in prog.comments.iter_mut() {
                if c.is_line()
                    && let Some(b) = scripts
                        .iter()
                        .find(|b| b.content.start <= c.span.start && c.span.start < b.content.end)
                {
                    c.span.end = c.span.end.min(b.content.end);
                }
            }
        }
        let p: &oxc::ast::ast::Program = alloc.alloc(prog);
        program = Some(p);
    }
    let semantic_holder = program.map(|p| {
        SemanticBuilder::new()
            .with_build_nodes(true)
            .build(p)
            .semantic
    });
    let semantic = semantic_holder.as_ref();

    let file = LintFile {
        source,
        path,
        is_sfc,
        doc,
        scripts,
        program,
        semantic,
        script_tokens: tokens,
        ts,
        settings: &opts.settings,
        globals: &opts.globals,
        lines: &lines,
        eslint_lines: eslint_lines.as_ref().unwrap_or(&lines),
    };
    let mut out = Vec::new();
    for r in rules {
        let mut ctx = Ctx {
            file: &file,
            rule: r.name,
            severity: r.severity,
            out: &mut out,
        };
        r.rule.run(&mut ctx);
    }
    if let Some(el) = &eslint_lines {
        for d in out
            .iter_mut()
            .filter(|d| d.loc.is_none() && ESLINT_LOC.contains(&d.rule))
        {
            let (l, c) = el.line_col(source, d.span.start);
            let (e, ec) = el.line_col(source, d.span.end);
            d.loc = Some([l, c, e, ec]);
        }
    }
    out.sort_by_key(|d| (d.span.start, d.span.end));
    if opts.no_inline_config {
        return out;
    }
    directives::filter(&file, out, &opts.prefixes, opts.template_directives)
}

/// Apply fixes once, ESLint style (sorted, overlapping ones skipped). `None` = nothing to fix.
pub fn apply_fixes(source: &str, diags: &[Diagnostic]) -> Option<String> {
    let mut fixes: Vec<&Fix> = diags.iter().filter_map(|d| d.fix.as_ref()).collect();
    if fixes.is_empty() {
        return None;
    }
    fixes.sort_by_key(|f| (f.span.start, f.span.end));
    let mut out = String::with_capacity(source.len());
    let mut pos = 0usize;
    let mut last: i64 = -1;
    for f in fixes {
        if last >= f.span.start as i64 {
            continue;
        }
        out.push_str(&source[pos..f.span.start as usize]);
        out.push_str(&f.text);
        pos = f.span.end as usize;
        last = f.span.end as i64;
    }
    out.push_str(&source[pos..]);
    Some(out)
}
