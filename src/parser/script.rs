//! Port of vue-eslint-parser `src/script/index.ts` + `scope-analyzer.ts`: template expression
//! parsing with oxc (wrapper snippets like upstream), references and variables, token mapping.

use oxc::allocator::{Allocator, Box as ArenaBox, Vec as ArenaVec};
use oxc::ast::ast::*;
use oxc::ast::builder::AstBuilder;
use oxc::ast_visit::{Visit, VisitMut};
use oxc::parser::{Kind, Token as OxcToken};
use oxc::parser::{ParseOptions, Parser, config::TokensParserConfig};
use oxc::semantic::{ScopeId, SemanticBuilder};
use oxc::span::{GetSpan, SourceType, Span};

use crate::ast::{
    Reference, ReferenceMode, Token, TokenKind, VExpression, VFilter, Variable, VariableKind,
};

// ---- locations --------------------------------------------------------------------------

/// Port of `LocationCalculatorForHtml`: maps an index in decoded (entity-resolved, CRLF→LF)
/// code back to an absolute byte offset in the source, using the tokenizer's gap offsets.
#[derive(Clone, Copy)]
pub(super) struct Mapper<'g> {
    gaps: &'g [u32],
    base: i64,
    base_index_of_gap: i64,
    shift: i64,
}

fn sorted_last_index(gaps: &[u32], v: i64) -> usize {
    gaps.partition_point(|&g| (g as i64) <= v)
}

impl<'g> Mapper<'g> {
    pub fn new(gaps: &'g [u32], base: u32) -> Self {
        let base = base as i64;
        let base_index_of_gap = if base == 0 {
            0
        } else {
            sorted_last_index(gaps, base) as i64
        };
        Self {
            gaps,
            base,
            base_index_of_gap,
            shift: 0,
        }
    }
    pub fn after(&self, offset: i64) -> Self {
        let mut m = Self::new(self.gaps, (self.base + offset).max(0) as u32);
        m.shift = self.shift;
        m
    }
    pub fn shifted(&self, offset: i64) -> Self {
        Self {
            shift: self.shift + offset,
            ..*self
        }
    }
    fn gap(&self, index: i64) -> i64 {
        let gaps = self.gaps;
        if gaps.is_empty() {
            return 0;
        }
        let mut g0 = sorted_last_index(gaps, index + self.base);
        let mut pos = index + self.base + g0 as i64 - self.base_index_of_gap;
        while g0 < gaps.len() && (gaps[g0] as i64) <= pos {
            g0 += 1;
            pos += 1;
        }
        g0 as i64 - self.base_index_of_gap
    }
    /// `getOffsetWithGap`.
    pub fn map(&self, index: i64) -> u32 {
        let fix = self.base + self.gap(index + self.shift) + self.shift;
        (index + fix).max(0) as u32
    }
}

struct Remap<'m> {
    m: Mapper<'m>,
    prefix: i64,
}

impl<'a> VisitMut<'a> for Remap<'_> {
    fn visit_span(&mut self, it: &mut Span) {
        let s = self.m.map(it.start as i64 - self.prefix);
        let e = self.m.map(it.end as i64 - self.prefix);
        *it = Span::new(s, e.max(s));
    }
}

// ---- results ----------------------------------------------------------------------------

#[derive(Debug)]
pub(super) struct ExprError {
    pub message: String,
    pub offset: u32,
}

pub(super) struct ExprResult<'a> {
    pub expression: Option<VExpression<'a>>,
    pub tokens: Vec<Token>,
    pub comments: Vec<Token>,
    pub references: Vec<Reference>,
    pub variables: Vec<Variable>,
}

impl ExprResult<'_> {
    pub fn empty() -> Self {
        Self {
            expression: None,
            tokens: Vec::new(),
            comments: Vec::new(),
            references: Vec::new(),
            variables: Vec::new(),
        }
    }
}

type PResult<'a> = Result<ExprResult<'a>, ExprError>;

/// One parsed wrapper snippet.
struct Wrapped<'a> {
    program: Program<'a>,
    src: &'a str,
    tokens: Vec<Token>,
    comments: Vec<Token>,
    references: Vec<Reference>,
    variables: Vec<Variable>,
}

/// Which scope's bindings become template variables.
#[derive(Clone, Copy, PartialEq, Eq)]
enum VarScope {
    None,
    /// `for(let … in …)` statement scope.
    For,
    /// The wrapper `function` scope (slot-scope / generic).
    Function,
}

pub(super) struct ExprParser<'a, 'g> {
    pub alloc: &'a Allocator,
    pub gaps: &'g [u32],
    pub ts: bool,
    pub jsx: bool,
}

pub(crate) fn token_kind(kind: Kind) -> TokenKind {
    use Kind::*;
    match kind {
        Ident | Await | Async | From | Get | Meta | Of | Set | Target | Accessor | Source
        | Defer | Abstract | As | Asserts | Assert | Any | Boolean | Constructor | Declare
        | Infer | Intrinsic | Is | KeyOf | Module | Namespace | Never | Out | Readonly
        | Require | Number | Object | Satisfies | String | Symbol | Type | Undefined | Unique
        | Using | Unknown | Global | BigInt | Override | Implements | Interface | Package
        | Private | Protected | Public => TokenKind::Identifier,
        Break | Case | Catch | Class | Const | Continue | Debugger | Default | Delete | Do
        | Else | Enum | Export | Extends | Finally | For | Function | If | Import | In
        | Instanceof | New | Return | Super | Switch | This | Throw | Try | Typeof | Var | Void
        | While | With | Let | Yield | Static => TokenKind::Keyword,
        True | False => TokenKind::Boolean,
        Null => TokenKind::Null,
        Decimal | Float | Binary | Octal | Hex | PositiveExponential | NegativeExponential
        | DecimalBigInt | BinaryBigInt | OctalBigInt | HexBigInt => TokenKind::Numeric,
        Str => TokenKind::String,
        RegExp => TokenKind::RegularExpression,
        NoSubstitutionTemplate | TemplateHead | TemplateMiddle | TemplateTail => {
            TokenKind::Template
        }
        PrivateIdentifier => TokenKind::PrivateIdentifier,
        JSXText => TokenKind::JSXText,
        _ => TokenKind::Punctuator,
    }
}

/// JS `\s` (WhiteSpace + LineTerminator).
pub(super) fn is_js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

fn js_trim(s: &str) -> &str {
    s.trim_matches(is_js_space)
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

impl<'a, 'g> ExprParser<'a, 'g> {
    fn source_type(&self) -> SourceType {
        SourceType::default()
            .with_module(true)
            .with_typescript(self.ts)
            .with_jsx(self.jsx)
    }

    fn empty_error(&self, m: &Mapper, expected: &str) -> ExprError {
        ExprError {
            message: format!("Expected to be {expected}, but got empty."),
            offset: m.map(0),
        }
    }

    /// `throwErrorAsAdjustingOutsideOfCode`.
    fn adjust_outside(&self, mut err: ExprError, code: &str, m: &Mapper) -> ExprError {
        if err.offset >= m.map(code.len() as i64) {
            err.message = "Unexpected end of expression.".to_string();
        }
        err
    }

    /// Parse `src` (a wrapper around the code) and analyze it. `prefix` = wrapper bytes before
    /// code index 0, so wrapper offset `w` maps to `m.map(w - prefix)`.
    fn parse_wrapped(
        &self,
        src: String,
        prefix: i64,
        m: &Mapper,
        vars: VarScope,
        kind: VariableKind,
    ) -> Result<Wrapped<'a>, ExprError> {
        let src: &'a str = self.alloc.alloc_str(&src);
        let ret = Parser::new(self.alloc, src, self.source_type())
            .with_options(ParseOptions::default())
            .with_config(TokensParserConfig)
            .parse();
        if ret.fatal_error || ret.diagnostics.has_errors() {
            let d = ret.diagnostics.errors().next();
            let label = d.and_then(|d| d.labels.first());
            let w = label.map_or(src.len(), |l| l.offset() as usize);
            let text = label
                .and_then(|l| src.get(l.offset() as usize..l.offset() as usize + l.len() as usize))
                .unwrap_or("");
            let (message, w) =
                espree_like_message(d.map_or("Parsing error.", |d| &d.message), text, w);
            return Err(ExprError {
                message,
                offset: m.map(w as i64 - prefix),
            });
        }
        let program = ret.program;
        let map = |sp: Span| {
            Span::new(
                m.map(sp.start as i64 - prefix),
                m.map(sp.end as i64 - prefix)
                    .max(m.map(sp.start as i64 - prefix)),
            )
        };

        let tokens: Vec<Token> = ret
            .tokens
            .iter()
            .filter(|t: &&OxcToken| !t.kind().is_eof())
            .map(|t| Token {
                kind: token_kind(t.kind()),
                span: map(t.span()),
                value: src[t.start() as usize..t.end() as usize].to_string(),
            })
            .collect();
        let comments: Vec<Token> = program
            .comments
            .iter()
            .map(|c| {
                let text = &src[c.span.start as usize..c.span.end as usize];
                let (kind, value) = if c.is_line() {
                    (TokenKind::LineComment, text.get(2..).unwrap_or(""))
                } else {
                    (
                        TokenKind::BlockComment,
                        text.get(2..text.len().saturating_sub(2)).unwrap_or(""),
                    )
                };
                Token {
                    kind,
                    span: map(c.span),
                    value: value.to_string(),
                }
            })
            .collect();

        // Scope analysis.
        let scoping = SemanticBuilder::new()
            .build(&program)
            .semantic
            .into_scoping();
        let mut ids = IdCollector::default();
        ids.visit_program(&program);
        let decl_scope = match vars {
            VarScope::None => None,
            VarScope::For => ids.for_scope,
            VarScope::Function => ids.function_scope,
        };
        let mut references = Vec::new();
        for (name, span, rid) in &ids.refs {
            let Some(rid) = rid else { continue };
            let r = scoping.get_reference(*rid);
            if r.symbol_id().is_some() {
                continue;
            }
            if name == "arguments" {
                // eslint-scope declares `arguments` in every non-arrow function scope.
                let mut scope = Some(r.scope_id());
                let mut in_fn = false;
                while let Some(s) = scope {
                    let f = scoping.scope_flags(s);
                    if f.is_function() && !f.is_arrow() {
                        in_fn = true;
                        break;
                    }
                    scope = scoping.scope_parent_id(s);
                }
                if in_fn {
                    continue;
                }
            }
            let flags = r.flags();
            let mode = if flags.is_read() && flags.is_write() {
                ReferenceMode::ReadWrite
            } else if flags.is_write() {
                ReferenceMode::Write
            } else {
                ReferenceMode::Read
            };
            references.push(Reference {
                name: name.clone(),
                span: map(*span),
                mode,
                variable: None,
                is_type_reference: flags.is_type() && !flags.is_value(),
            });
        }
        let mut variables = Vec::new();
        if let Some(scope) = decl_scope {
            for (_, &sym) in scoping.get_bindings(scope).iter() {
                variables.push(Variable {
                    name: scoping.symbol_name(sym).to_string(),
                    span: map(scoping.symbol_span(sym)),
                    kind,
                });
            }
            variables.sort_by_key(|v| v.span.start);
        }
        Ok(Wrapped {
            program,
            src,
            tokens,
            comments,
            references,
            variables,
        })
    }

    fn remap<T>(&self, node: &mut T, m: &Mapper, prefix: i64, f: impl FnOnce(&mut Remap, &mut T)) {
        let mut r = Remap { m: *m, prefix };
        f(&mut r, node);
    }

    /// `parseExpressionBody`: `0(${code})`.
    fn parse_expression_body(&self, code: &str, m: &Mapper, allow_empty: bool) -> PResult<'a> {
        let w = self
            .parse_wrapped(
                format!("0({code})"),
                2,
                m,
                VarScope::None,
                VariableKind::VFor,
            )
            .map_err(|e| self.adjust_outside(e, code, m))?;
        let Wrapped {
            mut program,
            mut tokens,
            comments,
            references,
            ..
        } = w;
        let Some(Statement::ExpressionStatement(stmt)) = program.body.pop() else {
            return Err(self.adjust_outside(
                ExprError {
                    message: "Unexpected token.".into(),
                    offset: m.map(0),
                },
                code,
                m,
            ));
        };
        let Expression::CallExpression(call) = stmt.unbox().expression else {
            return Err(self.adjust_outside(
                ExprError {
                    message: "Unexpected token.".into(),
                    offset: m.map(0),
                },
                code,
                m,
            ));
        };
        let mut args = call.unbox().arguments;
        if args.is_empty() {
            if !allow_empty {
                return Err(self.empty_error(m, "an expression"));
            }
            strip(&mut tokens, 2, 1);
            return Ok(ExprResult {
                expression: None,
                tokens,
                comments,
                references,
                variables: Vec::new(),
            });
        }
        if args.len() >= 2 {
            let second = args[1].span().start;
            let second = m.map(second as i64 - 2);
            let comma = tokens.iter().rev().find(|t| {
                t.span.start <= second && t.kind == TokenKind::Punctuator && t.value == ","
            });
            return Err(ExprError {
                message: "Unexpected token ','.".into(),
                offset: comma.map_or(second, |t| t.span.start),
            });
        }
        let arg = args.pop().expect("one argument");
        if let Argument::SpreadElement(s) = &arg {
            return Err(ExprError {
                message: "Unexpected token '...'.".into(),
                offset: m.map(s.span.start as i64 - 2),
            });
        }
        let mut expr = arg.into_expression();
        self.remap(&mut expr, m, 2, |r, e| r.visit_expression(e));
        strip(&mut tokens, 2, 1);
        Ok(ExprResult {
            expression: Some(VExpression::Expression(expr)),
            tokens,
            comments,
            references,
            variables: Vec::new(),
        })
    }

    /// `parseExpression` (with optional Vue 2 filters).
    pub fn parse_expression(
        &self,
        code: &str,
        m: &Mapper,
        allow_empty: bool,
        allow_filters: bool,
    ) -> PResult<'a> {
        let parts = if allow_filters {
            split_filters(code)
        } else {
            vec![code]
        };
        if parts.len() <= 1 {
            return self.parse_expression_body(code, m, allow_empty);
        }
        let main = parts[0];
        let mut ret = self.parse_expression_body(main, m, false)?;
        let Some(VExpression::Expression(expression)) = ret.expression.take() else {
            return Ok(ret);
        };
        let start = expression.span().start;
        let mut filters = Vec::new();
        let mut prev = main.len() as i64;
        for f in &parts[1..] {
            ret.tokens.push(Token {
                kind: TokenKind::Punctuator,
                span: Span::new(m.map(prev), m.map(prev + 1)),
                value: "|".into(),
            });
            let fr = self.parse_filter(f, &m.shifted(prev + 1))?;
            ret.tokens.extend(fr.tokens);
            ret.comments.extend(fr.comments);
            ret.references.extend(fr.references);
            filters.push(fr.filter);
            prev += 1 + f.len() as i64;
        }
        let end = ret.tokens.last().map_or(start, |t| t.span.end);
        ret.expression = Some(VExpression::FilterSequence {
            span: Span::new(start, end),
            expression,
            filters,
        });
        Ok(ret)
    }

    fn parse_filter(&self, code: &str, m: &Mapper) -> Result<FilterResult<'a>, ExprError> {
        let paren = code.find('(');
        let callee_code = paren.map_or(code, |p| &code[..p]);
        let args_code = paren.map(|p| &code[p..]);
        let trimmed = js_trim(callee_code);
        if trimmed.is_empty() {
            return Err(self.empty_error(m, "a filter name"));
        }
        let spaces = callee_code.len() - callee_code.trim_start_matches(is_js_space).len();
        let sub = m.shifted(spaces as i64);
        // Upstream parses `"${callee}"` as a string literal; a quote / line break inside fails.
        if let Some(i) = trimmed.find(['"', '\\', '\n', '\r', '\u{2028}', '\u{2029}']) {
            let offset = sub.map(i as i64 + 1);
            return Err(self.adjust_outside(
                ExprError {
                    message: "Unexpected token '\"'.".into(),
                    offset,
                },
                code,
                m,
            ));
        }
        let callee_span = Span::new(sub.map(0), sub.map(trimmed.len() as i64));
        let mut tokens = vec![Token {
            kind: TokenKind::Identifier,
            span: callee_span,
            value: trimmed.to_string(),
        }];
        let mut comments = Vec::new();
        let mut references = Vec::new();
        let mut arguments = ArenaVec::new_in(&self.alloc);
        if let (Some(p), Some(args_code)) = (paren, args_code) {
            let am = m.after(p as i64);
            let w = self
                .parse_wrapped(
                    format!("0{args_code}"),
                    1,
                    &am,
                    VarScope::None,
                    VariableKind::VFor,
                )
                .map_err(|e| self.adjust_outside(e, code, m))?;
            let Wrapped {
                mut program,
                tokens: mut atoks,
                comments: acomments,
                references: arefs,
                ..
            } = w;
            if !atoks.is_empty() {
                atoks.remove(0);
            }
            let call = match program.body.pop() {
                Some(Statement::ExpressionStatement(s)) => match s.unbox().expression {
                    Expression::CallExpression(c)
                        if matches!(c.callee, Expression::NumericLiteral(_)) =>
                    {
                        Some(c)
                    }
                    _ => None,
                },
                _ => None,
            };
            let Some(call) = call else {
                // Report the next token of `)`.
                let mut nest = 1;
                for t in atoks.iter().skip(1) {
                    if nest == 0 {
                        return Err(ExprError {
                            message: format!("Unexpected token '{}'.", t.value),
                            offset: t.span.start,
                        });
                    }
                    if t.kind == TokenKind::Punctuator && t.value == "(" {
                        nest += 1;
                    }
                    if t.kind == TokenKind::Punctuator && t.value == ")" {
                        nest -= 1;
                    }
                }
                let (value, offset) = atoks.last().map_or((String::new(), am.map(0)), |t| {
                    (t.value.clone(), t.span.start)
                });
                return Err(ExprError {
                    message: format!("Unexpected token '{value}'."),
                    offset,
                });
            };
            arguments = call.unbox().arguments;
            for a in arguments.iter_mut() {
                self.remap(a, &am, 1, |r, a| r.visit_argument(a));
            }
            tokens.extend(atoks);
            comments = acomments;
            references = arefs;
        }
        let span = Span::new(
            tokens[0].span.start,
            tokens.last().map_or(callee_span.end, |t| t.span.end),
        );
        Ok(FilterResult {
            filter: VFilter {
                span,
                callee: (trimmed.to_string(), callee_span),
                arguments,
            },
            tokens,
            comments,
            references,
        })
    }

    /// `parseVForExpression`.
    pub fn parse_v_for(&self, code: &str, m: &Mapper) -> PResult<'a> {
        if js_trim(code).is_empty() {
            return Err(self.empty_error(m, "'<alias> in <expression>'"));
        }
        let p = process_v_for_alias_and_iterator(code);
        if js_trim(&p.aliases).is_empty() {
            return Err(self.empty_error(m, "an alias"));
        }
        let prefix: i64 = if p.has_parens { 8 } else { 9 };
        let src = format!(
            "for(let {}{}{});",
            p.aliases_with_brackets, p.delimiter, p.iterator
        );
        let w = self
            .parse_wrapped(src, prefix, m, VarScope::For, VariableKind::VFor)
            .map_err(|e| self.adjust_outside(e, code, m))?;
        let Wrapped {
            mut program,
            mut tokens,
            comments,
            references,
            variables,
            ..
        } = w;
        let bad = || ExprError {
            message: "Unexpected token.".into(),
            offset: m.map(0),
        };
        let (left, right) = match program.body.pop() {
            Some(Statement::ForInStatement(s)) => {
                let s = s.unbox();
                (s.left, s.right)
            }
            Some(Statement::ForOfStatement(s)) => {
                let s = s.unbox();
                (s.left, s.right)
            }
            _ => return Err(self.adjust_outside(bad(), code, m)),
        };
        let ForStatementLeft::VariableDeclaration(decl) = left else {
            return Err(bad());
        };
        let left_end = decl.span.end;
        let mut decl = decl.unbox();
        let Some(d) = decl.declarations.pop() else {
            return Err(bad());
        };
        let BindingPattern::ArrayPattern(arr) = d.id else {
            return Err(bad());
        };
        let mut arr = arr.unbox();
        if !p.has_parens && arr.elements.is_empty() && arr.rest.is_none() {
            return Err(self.empty_error(m, "an alias"));
        }
        // Remove `for` `(` `let` … `)` `;`.
        strip(&mut tokens, 3, 2);
        let close = m.map(left_end as i64 - 1 - prefix);
        let close_index = tokens.iter().position(|t| t.span.start == close);
        if p.has_parens {
            if let Some(t) = tokens.first_mut() {
                t.value = "(".into();
            }
            if let Some(i) = close_index {
                tokens[i].value = ")".into();
            }
        } else {
            if let Some(i) = close_index {
                tokens.remove(i);
            }
            if !tokens.is_empty() {
                tokens.remove(0);
            }
        }
        for e in arr.elements.iter_mut().flatten() {
            self.remap(e, m, prefix, |r, e| r.visit_binding_pattern(e));
        }
        if let Some(rest) = arr.rest.as_mut() {
            self.remap(rest, m, prefix, |r, e| r.visit_binding_rest_element(e));
        }
        let mut right = right;
        self.remap(&mut right, m, prefix, |r, e| r.visit_expression(e));
        let first = tokens.first().map_or(close, |t| t.span.start);
        let last = tokens.last().map_or(right.span().end, |t| t.span.end);
        Ok(ExprResult {
            expression: Some(VExpression::For {
                span: Span::new(first, last),
                left: arr.elements,
                rest: arr.rest,
                right,
            }),
            tokens,
            comments,
            references,
            variables,
        })
    }

    /// `parseVOnExpression`.
    pub fn parse_v_on(&self, code: &str, m: &Mapper) -> PResult<'a> {
        if is_function_expression(code) || is_simple_path(code) {
            return self.parse_expression_body(code, m, false);
        }
        if js_trim(code).is_empty() {
            return Err(self.empty_error(m, "statements"));
        }
        let prefix = 22;
        let w = self
            .parse_wrapped(
                format!("void function($event){{{code}}}"),
                prefix,
                m,
                VarScope::None,
                VariableKind::VFor,
            )
            .map_err(|e| self.adjust_outside(e, code, m))?;
        let Wrapped {
            mut program,
            mut tokens,
            comments,
            references,
            ..
        } = w;
        let bad = || ExprError {
            message: "Unexpected token.".into(),
            offset: m.map(0),
        };
        let Some(Statement::ExpressionStatement(stmt)) = program.body.pop() else {
            return Err(bad());
        };
        let Expression::UnaryExpression(unary) = stmt.unbox().expression else {
            return Err(bad());
        };
        let Expression::FunctionExpression(func) = unary.unbox().argument else {
            return Err(bad());
        };
        let Some(body) = func.unbox().body else {
            return Err(bad());
        };
        let body = body.unbox();
        let block = body.span;
        let ast = AstBuilder::new(self.alloc);
        let mut stmts = ArenaVec::new_in(&self.alloc);
        for d in body.directives {
            let lit = d.expression;
            stmts.push(Statement::new_expression_statement(
                d.span,
                Expression::StringLiteral(ArenaBox::new_in(lit, &self.alloc)),
                &ast,
            ));
        }
        stmts.extend(body.statements);
        for s in stmts.iter_mut() {
            self.remap(s, m, prefix, |r, s| r.visit_statement(s));
        }
        let start = stmts.first().map_or_else(
            || m.map(block.start as i64 + 1 - prefix),
            |s| s.span().start,
        );
        let end = stmts
            .last()
            .map_or_else(|| m.map(block.end as i64 - 1 - prefix), |s| s.span().end);
        strip(&mut tokens, 6, 1);
        Ok(ExprResult {
            expression: Some(VExpression::On {
                span: Span::new(start, end.max(start)),
                body: stmts,
            }),
            tokens,
            comments,
            references,
            variables: Vec::new(),
        })
    }

    /// `parseSlotScopeExpression`.
    pub fn parse_slot_scope(&self, code: &str, m: &Mapper) -> PResult<'a> {
        if js_trim(code).is_empty() {
            return Err(self.empty_error(m, "an identifier or an array/object pattern"));
        }
        let prefix = 14;
        let w = self
            .parse_wrapped(
                format!("void function({code}) {{}}"),
                prefix,
                m,
                VarScope::Function,
                VariableKind::Scope,
            )
            .map_err(|e| self.adjust_outside(e, code, m))?;
        let Wrapped {
            mut program,
            mut tokens,
            comments,
            references,
            variables,
            ..
        } = w;
        let bad = || ExprError {
            message: "Unexpected token.".into(),
            offset: m.map(0),
        };
        let Some(Statement::ExpressionStatement(stmt)) = program.body.pop() else {
            return Err(bad());
        };
        let Expression::UnaryExpression(unary) = stmt.unbox().expression else {
            return Err(bad());
        };
        let Expression::FunctionExpression(func) = unary.unbox().argument else {
            return Err(bad());
        };
        let mut params = func.unbox().params;
        if params.items.is_empty() && params.rest.is_none() {
            return Ok(ExprResult::empty());
        }
        self.remap(&mut *params, m, prefix, |r, p| r.visit_formal_parameters(p));
        let first = params
            .items
            .first()
            .map(|p| p.span)
            .or_else(|| params.rest.as_ref().map(|r| r.span))
            .unwrap_or(params.span);
        let last = params
            .rest
            .as_ref()
            .map(|r| r.span)
            .or_else(|| params.items.last().map(|p| p.span))
            .unwrap_or(params.span);
        strip(&mut tokens, 3, 3);
        Ok(ExprResult {
            expression: Some(VExpression::SlotScope {
                span: Span::new(first.start, last.end),
                params,
            }),
            tokens,
            comments,
            references,
            variables,
        })
    }

    /// `parseGenericExpression` (always TypeScript).
    pub fn parse_generic(&self, code: &str, m: &Mapper) -> PResult<'a> {
        if js_trim(code).is_empty() {
            return Err(self.empty_error(m, "a type parameter"));
        }
        let ts = ExprParser {
            alloc: self.alloc,
            gaps: self.gaps,
            ts: true,
            jsx: self.jsx,
        };
        let prefix = 14;
        let w = ts
            .parse_wrapped(
                format!("void function<{code}>(){{}}"),
                prefix,
                m,
                VarScope::Function,
                VariableKind::Generic,
            )
            .map_err(|e| self.adjust_outside(e, code, m))?;
        let Wrapped {
            mut program,
            mut tokens,
            comments,
            references,
            variables,
            src,
            ..
        } = w;
        let bad = || ExprError {
            message: "Unexpected token.".into(),
            offset: m.map(0),
        };
        let Some(Statement::ExpressionStatement(stmt)) = program.body.pop() else {
            return Err(bad());
        };
        let Expression::UnaryExpression(unary) = stmt.unbox().expression else {
            return Err(bad());
        };
        let Expression::FunctionExpression(func) = unary.unbox().argument else {
            return Err(bad());
        };
        let Some(mut params) = func.unbox().type_parameters else {
            return Ok(ExprResult::empty());
        };
        if params.params.is_empty() {
            return Ok(ExprResult::empty());
        }
        let raw_params = params
            .params
            .iter()
            .map(|p| src[p.span.start as usize..p.span.end as usize].to_string())
            .collect();
        self.remap(&mut *params, m, prefix, |r, p| {
            r.visit_ts_type_parameter_declaration(p)
        });
        let span = Span::new(
            params.params[0].span.start,
            params.params[params.params.len() - 1].span.end,
        );
        strip(&mut tokens, 3, 5);
        Ok(ExprResult {
            expression: Some(VExpression::Generic {
                span,
                params,
                raw_params,
            }),
            tokens,
            comments,
            references,
            variables,
        })
    }

    /// v-bind same-name shorthand: parse the camelized argument as an identifier.
    pub fn parse_same_name_identifier(
        &self,
        name: &str,
        span: Span,
    ) -> Option<(Expression<'a>, Reference)> {
        let src: &'a str = self.alloc.alloc_str(name);
        let ret = Parser::new(self.alloc, src, self.source_type()).parse();
        if ret.fatal_error || ret.diagnostics.has_errors() || !ret.program.directives.is_empty() {
            return None;
        }
        let mut program = ret.program;
        if program.body.len() != 1 {
            return None;
        }
        let Some(Statement::ExpressionStatement(stmt)) = program.body.pop() else {
            return None;
        };
        let mut expr = stmt.unbox().expression;
        let Expression::Identifier(id) = &mut expr else {
            return None;
        };
        id.span = span;
        let reference = Reference {
            name: id.name.to_string(),
            span,
            mode: ReferenceMode::Read,
            variable: None,
            is_type_reference: false,
        };
        Some((expr, reference))
    }
}

/// Approximate espree's wording for the most common oxc syntax errors (vue/no-parsing-error
/// reports the message). Returns (message, wrapper offset).
fn espree_like_message(msg: &str, text: &str, w: usize) -> (String, usize) {
    // ponytail: a handful of mappings; other oxc messages pass through unchanged.
    if msg == "Unexpected token" && !text.is_empty() {
        return (format!("Unexpected token {text}"), w);
    }
    if let Some(i) = msg.find("but found `")
        && msg.starts_with("Expected")
    {
        // oxc names some tokens by kind (`Identifier`, `string`, …); espree prints the token text.
        let found = if text.is_empty() {
            msg[i + 11..].trim_end_matches('`')
        } else {
            text
        };
        return (format!("Unexpected token {found}"), w);
    }
    match msg {
        // acorn reports right after the opening `/`.
        "Unterminated regular expression" => (msg.to_string(), w + 1),
        "Cannot assign to this expression" => ("Assigning to rvalue".to_string(), w),
        "Unexpected JSX expression" => ("Unexpected token <".to_string(), w),
        _ => (msg.to_string(), w),
    }
}

pub(super) struct FilterResult<'a> {
    filter: VFilter<'a>,
    tokens: Vec<Token>,
    comments: Vec<Token>,
    references: Vec<Reference>,
}

fn strip(tokens: &mut Vec<Token>, front: usize, back: usize) {
    let n = tokens.len();
    tokens.truncate(n.saturating_sub(back));
    let f = front.min(tokens.len());
    tokens.drain(..f);
}

/// Collects identifier references (in order) and the wrapper scopes.
#[derive(Default)]
struct IdCollector {
    refs: Vec<(String, Span, Option<oxc::semantic::ReferenceId>)>,
    for_scope: Option<ScopeId>,
    function_scope: Option<ScopeId>,
    seen_function: bool,
    seen_for: bool,
}

impl<'a> Visit<'a> for IdCollector {
    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        self.refs
            .push((it.name.to_string(), it.span, it.reference_id.get()));
    }
    fn visit_for_in_statement(&mut self, it: &ForInStatement<'a>) {
        if !self.seen_for {
            self.seen_for = true;
            self.for_scope = it.scope_id.get();
        }
        oxc::ast_visit::walk::walk_for_in_statement(self, it);
    }
    fn visit_for_of_statement(&mut self, it: &ForOfStatement<'a>) {
        if !self.seen_for {
            self.seen_for = true;
            self.for_scope = it.scope_id.get();
        }
        oxc::ast_visit::walk::walk_for_of_statement(self, it);
    }
    fn visit_function(&mut self, it: &Function<'a>, flags: oxc::semantic::ScopeFlags) {
        if !self.seen_function {
            self.seen_function = true;
            self.function_scope = it.scope_id.get();
        }
        oxc::ast_visit::walk::walk_function(self, it, flags);
    }
}

// ---- regex ports ------------------------------------------------------------------------

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b == b'$'
}
fn is_ident_part(b: u8) -> bool {
    is_word_byte(b) || b == b'$'
}

fn skip_ws(s: &str, i: usize) -> usize {
    let rest = &s[i..];
    i + (rest.len() - rest.trim_start_matches(is_js_space).len())
}

/// `IS_FUNCTION_EXPRESSION` (Vue's v-on heuristics).
pub(super) fn is_function_expression(code: &str) -> bool {
    let b = code.as_bytes();
    let i = skip_ws(code, 0);
    // `\s*(:[^=]+)?=>` after a parameter list.
    let arrow_tail = |mut j: usize| -> bool {
        j = skip_ws(code, j);
        if b.get(j) == Some(&b':') {
            let k = j + 1 + code[j + 1..].find('=').unwrap_or(code.len() - j - 1);
            if k > j + 1 && code[k..].starts_with("=>") {
                return true;
            }
        }
        code[j..].starts_with("=>")
    };
    // Alt 1a: `[\w$_]+`
    let mut j = i;
    while j < b.len() && is_ident_part(b[j]) {
        j += 1;
    }
    if j > i && arrow_tail(j) {
        return true;
    }
    // Alt 1b: `(async\s*)?\([^)]*?\)`
    let mut starts = vec![i];
    if code[i..].starts_with("async") {
        starts.push(skip_ws(code, i + 5));
    }
    for s in starts {
        if b.get(s) == Some(&b'(')
            && let Some(close) = code[s..].find(')')
            && arrow_tail(s + close + 1)
        {
            return true;
        }
    }
    // Alt 2: `(async\s+)?function(?:\s+[\w$]+)?\s*\(`
    let mut k = i;
    if code[k..].starts_with("async") {
        let after = skip_ws(code, k + 5);
        if after > k + 5 {
            k = after;
        }
    }
    if !code[k..].starts_with("function") {
        return false;
    }
    let k = k + 8;
    let after_ws = skip_ws(code, k);
    if after_ws > k {
        let mut n = after_ws;
        while n < b.len() && is_ident_part(b[n]) {
            n += 1;
        }
        if n > after_ws && b.get(skip_ws(code, n)) == Some(&b'(') {
            return true;
        }
    }
    b.get(after_ws) == Some(&b'(')
}

/// `IS_SIMPLE_PATH`.
pub(super) fn is_simple_path(code: &str) -> bool {
    let b = code.as_bytes();
    let ident = |mut i: usize| -> Option<usize> {
        if i < b.len() && is_ident_start(b[i]) {
            i += 1;
            while i < b.len() && is_ident_part(b[i]) {
                i += 1;
            }
            Some(i)
        } else {
            None
        }
    };
    let Some(mut i) = ident(0) else { return false };
    while i < b.len() {
        match b[i] {
            b'.' => match ident(i + 1) {
                Some(n) => i = n,
                None => return false,
            },
            b'[' => {
                let j = i + 1;
                match b.get(j) {
                    Some(&q @ (b'\'' | b'"')) => {
                        let Some(close) = code[j + 1..].find(q as char) else {
                            return false;
                        };
                        let end = j + 1 + close;
                        if b.get(end + 1) != Some(&b']') {
                            return false;
                        }
                        i = end + 2;
                    }
                    Some(c) if c.is_ascii_digit() => {
                        let mut n = j;
                        while n < b.len() && b[n].is_ascii_digit() {
                            n += 1;
                        }
                        if b.get(n) != Some(&b']') {
                            return false;
                        }
                        i = n + 1;
                    }
                    _ => match ident(j) {
                        Some(n) if b.get(n) == Some(&b']') => i = n + 1,
                        _ => return false,
                    },
                }
            }
            _ => return false,
        }
    }
    true
}

pub(super) struct VForParts {
    pub aliases: String,
    pub has_parens: bool,
    pub aliases_with_brackets: String,
    pub delimiter: String,
    pub iterator: String,
}

/// `processVForAliasAndIterator` (`ALIAS_ITERATOR` + `PARENS` regexes).
pub(super) fn process_v_for_alias_and_iterator(code: &str) -> VForParts {
    let b = code.as_bytes();
    // Earliest p with code[p-1] ∈ \s|')' and `in`/`of` + word boundary at p.
    let mut found = None;
    for (idx, c) in code.char_indices() {
        let p = idx + c.len_utf8();
        if !(is_js_space(c) || c == ')') {
            continue;
        }
        let rest = &b[p..];
        if (rest.starts_with(b"in") || rest.starts_with(b"of"))
            && !rest.get(2).is_some_and(|&x| is_word_byte(x))
        {
            found = Some(p);
            break;
        }
    }
    let Some(p) = found else {
        return VForParts {
            aliases: String::new(),
            has_parens: false,
            aliases_with_brackets: String::new(),
            delimiter: String::new(),
            iterator: code.to_string(),
        };
    };
    let aliases = &code[..p];
    let delimiter = &code[p..p + 2];
    let iterator = &code[p + 2..];
    // PARENS: `^(\s*\()([\s\S]*?)(\)\s*)$`
    let lead = aliases.len() - aliases.trim_start_matches(is_js_space).len();
    let trimmed_end = aliases.trim_end_matches(is_js_space);
    let parens =
        aliases[lead..].starts_with('(') && trimmed_end.ends_with(')') && trimmed_end.len() > lead;
    let aliases_with_brackets = if parens {
        let close = trimmed_end.len() - 1;
        format!(
            "{}[{}]{}",
            &aliases[..lead],
            &aliases[lead + 1..close],
            &aliases[close + 1..]
        )
    } else {
        let last = aliases.chars().last().map_or(1, char::len_utf8);
        // Keep byte offsets aligned when the last char is multi-byte whitespace.
        format!(
            "[{}]{}",
            &aliases[..aliases.len() - last],
            " ".repeat(last - 1)
        )
    };
    VForParts {
        aliases: aliases.to_string(),
        has_parens: parens,
        aliases_with_brackets,
        delimiter: delimiter.to_string(),
        iterator: iterator.to_string(),
    }
}

/// `splitFilters` (Vue 2).
pub(super) fn split_filters(exp: &str) -> Vec<&str> {
    let b = exp.as_bytes();
    let mut result = Vec::new();
    let (mut in_single, mut in_double, mut in_tpl, mut in_regex) = (false, false, false, false);
    let (mut curly, mut square, mut paren) = (0i32, 0i32, 0i32);
    let mut last = 0usize;
    let mut c = 0u8;
    for i in 0..b.len() {
        let prev = c;
        c = b[i];
        if in_single {
            if c == b'\'' && prev != b'\\' {
                in_single = false;
            }
        } else if in_double {
            if c == b'"' && prev != b'\\' {
                in_double = false;
            }
        } else if in_tpl {
            if c == b'`' && prev != b'\\' {
                in_tpl = false;
            }
        } else if in_regex {
            if c == b'/' && prev != b'\\' {
                in_regex = false;
            }
        } else if c == b'|'
            && b.get(i + 1) != Some(&b'|')
            && (i == 0 || b[i - 1] != b'|')
            && curly == 0
            && square == 0
            && paren == 0
        {
            result.push(&exp[last..i]);
            last = i + 1;
        } else {
            match c {
                b'"' => in_double = true,
                b'\'' => in_single = true,
                b'`' => in_tpl = true,
                b'(' => paren += 1,
                b')' => paren -= 1,
                b'[' => square += 1,
                b']' => square -= 1,
                b'{' => curly += 1,
                b'}' => curly -= 1,
                _ => {}
            }
            if c == b'/' {
                let mut p = None;
                let mut j = i;
                while j > 0 {
                    j -= 1;
                    p = Some(b[j]);
                    if b[j] != b' ' {
                        break;
                    }
                }
                let valid = p.is_some_and(|p| {
                    is_word_byte(p) || matches!(p, b')' | b'.' | b'+' | b'-' | b'$' | b']')
                });
                if !valid {
                    in_regex = true;
                }
            }
        }
    }
    result.push(&exp[last..]);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regexes() {
        assert!(is_function_expression("() => a"));
        assert!(is_function_expression(" async (a, b) => a"));
        assert!(is_function_expression("e => a"));
        assert!(is_function_expression("(e): void => a"));
        assert!(is_function_expression("function () {}"));
        assert!(is_function_expression("async function foo () {}"));
        assert!(!is_function_expression("foo()"));
        assert!(!is_function_expression("functionfoo()"));
        assert!(is_simple_path("a.b['c'][0][d]"));
        assert!(!is_simple_path("a.b()"));
        assert!(!is_simple_path(" a"));
        let p = process_v_for_alias_and_iterator("(a, i) in list");
        assert!(p.has_parens);
        assert_eq!(p.aliases_with_brackets, "[a, i] ");
        assert_eq!(p.iterator, " list");
        let p = process_v_for_alias_and_iterator("a of list");
        assert_eq!(p.aliases_with_brackets, "[a]");
        assert_eq!(p.delimiter, "of");
        assert_eq!(split_filters("a | b(c) | d"), vec!["a ", " b(c) ", " d"]);
        assert_eq!(split_filters("a || b"), vec!["a || b"]);
    }

    #[test]
    fn mapper() {
        let gaps = [5u32, 6, 7];
        let m = Mapper::new(&gaps, 0);
        assert_eq!(m.map(4), 4);
        assert_eq!(m.map(5), 8);
        let m = Mapper::new(&[], 10).shifted(-2);
        assert_eq!(m.map(2), 10);
    }
}
