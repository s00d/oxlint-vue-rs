//! vue/v-on-handler-style

use std::collections::HashMap;

use oxc::ast::ast::*;
use oxc::span::{GetSpan, Span};

use crate::ast::{AttributeValue, ExprContainer, TokenKind, VExpression};
use crate::rule::{Ctx, Fix, Rule, RuleOptions, Suggestion};
use crate::utils::js::{self, skip_parens};
use crate::utils::tpl_style::tokens_with_comments;
use crate::utils::tpl_valid::body_directives;
use crate::utils::vue_component as vc;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Method,
    Inline,
    InlineFunction,
}

struct R {
    allows: Vec<Kind>,
    ignore_comment: bool,
}

fn kind(s: &str) -> Option<Kind> {
    match s {
        "method" => Some(Kind::Method),
        "inline" => Some(Kind::Inline),
        "inline-function" => Some(Kind::InlineFunction),
        _ => None,
    }
}

/// `getParamCount`: `None` = Infinity (rest parameter).
fn param_count(p: &FormalParameters) -> Option<usize> {
    if p.rest.is_some() {
        None
    } else {
        Some(p.items.len())
    }
}

fn id_call<'b, 'a>(e: &'b Expression<'a>) -> Option<&'b CallExpression<'a>> {
    match skip_parens(e) {
        Expression::CallExpression(c)
            if !c.optional && matches!(skip_parens(&c.callee), Expression::Identifier(_)) =>
        {
            Some(c)
        }
        _ => None,
    }
}

fn callee_span(c: &CallExpression) -> Span {
    skip_parens(&c.callee).span()
}

fn callee_name<'a>(c: &CallExpression<'a>) -> &'a str {
    match skip_parens(&c.callee) {
        Expression::Identifier(id) => id.name.as_str(),
        _ => "",
    }
}

/// `getIdentifierCallExpression(VOnExpression | BlockStatement)`.
fn id_call_of_body<'b, 'a>(mut body: &'b [Statement<'a>]) -> Option<&'b CallExpression<'a>> {
    loop {
        let mut it = body
            .iter()
            .filter(|s| !matches!(s, Statement::EmptyStatement(_)));
        let (Some(st), None) = (it.next(), it.next()) else {
            return None;
        };
        match st {
            Statement::ExpressionStatement(e) => return id_call(&e.expression),
            Statement::BlockStatement(b) => body = &b.body,
            _ => return None,
        }
    }
}

struct Info {
    range: Span,
    has_comment: bool,
    has_quote: bool,
}

fn token_info(ctx: &Ctx, c: &ExprContainer) -> Info {
    let Some(doc) = ctx.doc() else {
        return Info {
            range: Span::empty(0),
            has_comment: false,
            has_quote: false,
        };
    };
    let toks = tokens_with_comments(doc, c.span);
    let (Some(first), Some(last)) = (toks.first(), toks.last()) else {
        return Info {
            range: Span::empty(0),
            has_comment: false,
            has_quote: false,
        };
    };
    let has_quote =
        first.kind == TokenKind::Punctuator && matches!(doc.slice(first.span), "\"" | "'");
    Info {
        range: if has_quote {
            Span::new(first.span.end, last.span.start)
        } else {
            Span::new(first.span.start, last.span.end)
        },
        has_comment: toks
            .iter()
            .any(|t| matches!(t.kind, TokenKind::LineComment | TokenKind::BlockComment)),
        has_quote,
    }
}

impl R {
    fn method_fix(
        &self,
        ctx: &Ctx,
        counts: &HashMap<String, Option<usize>>,
        call: &CallExpression,
        range: Span,
    ) -> (Option<Fix>, Vec<Suggestion>) {
        let fix = Fix::replace(range, ctx.slice(callee_span(call)));
        if counts.get(callee_name(call)) == Some(&Some(call.arguments.len())) {
            (Some(fix), Vec::new())
        } else {
            (
                None,
                vec![Suggestion {
                    message_id: Some("replaceWithMethodHandler"),
                    desc: "Replace with method handler. Note that the method will receive the event arguments."
                        .into(),
                    fix,
                }],
            )
        }
    }
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let file = ctx.file;
        let Some(doc) = ctx.doc() else { return };
        let mut counts: HashMap<String, Option<usize>> = HashMap::new();
        if self.allows.contains(&Kind::Method) {
            for o in vc::vue_objects(file) {
                for p in js::properties(o.obj) {
                    if js::prop_name(p).as_deref() != Some("methods") {
                        continue;
                    }
                    let Expression::ObjectExpression(m) = skip_parens(&p.value) else {
                        continue;
                    };
                    for q in js::properties(m) {
                        let (Some(name), Some(n)) = (
                            js::prop_name(q),
                            match &q.value {
                                Expression::FunctionExpression(f) => Some(param_count(&f.params)),
                                Expression::ArrowFunctionExpression(f) => {
                                    Some(param_count(&f.params))
                                }
                                _ => None,
                            },
                        ) else {
                            continue;
                        };
                        counts.insert(name, n);
                    }
                }
            }
            if let (Some(prog), Some(setup)) = (file.program, file.script_setup()) {
                let inside = |s: Span| setup.content.start <= s.start && s.end <= setup.content.end;
                for st in &prog.body {
                    match st {
                        Statement::FunctionDeclaration(f) if inside(f.span) => {
                            if let Some(id) = &f.id {
                                counts.insert(id.name.to_string(), param_count(&f.params));
                            }
                        }
                        Statement::VariableDeclaration(v)
                            if inside(v.span) && v.kind == VariableDeclarationKind::Const =>
                        {
                            for d in &v.declarations {
                                let BindingPattern::BindingIdentifier(id) = &d.id else {
                                    continue;
                                };
                                let n = match d.init.as_ref() {
                                    Some(Expression::FunctionExpression(f)) => {
                                        param_count(&f.params)
                                    }
                                    Some(Expression::ArrowFunctionExpression(f)) => {
                                        param_count(&f.params)
                                    }
                                    _ => continue,
                                };
                                counts.insert(id.name.to_string(), n);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        for (_, _, a, k) in body_directives(doc, "on") {
            if k.argument.is_none() {
                continue;
            }
            let Some(AttributeValue::Expr(id)) = &a.value else {
                continue;
            };
            let c = doc.expr(*id);
            let Some(expr) = &c.expression else { continue };
            let has_upper_ref = c.references.iter().any(|r| r.variable.is_some());
            match expr {
                VExpression::On { span, body } => {
                    if self.allows[0] == Kind::Inline {
                        continue;
                    }
                    for allow in &self.allows {
                        match allow {
                            Kind::Method => {
                                let info = token_info(ctx, c);
                                if self.ignore_comment && info.has_comment {
                                    continue;
                                }
                                let call = id_call_of_body(body);
                                if call.is_none_or(|c| !c.arguments.is_empty()) && has_upper_ref {
                                    continue;
                                }
                                let (fix, sugg) = match call {
                                    Some(cl) if !info.has_comment && cl.arguments.is_empty() => {
                                        self.method_fix(ctx, &counts, cl, info.range)
                                    }
                                    _ => (None, Vec::new()),
                                };
                                let (id, msg) = if call.is_some() {
                                    (
                                        "preferMethodOverInline",
                                        "Prefer method handler over inline handler in v-on.",
                                    )
                                } else {
                                    (
                                        "preferMethodOverInlineWithoutIdCall",
                                        "Prefer method handler over inline handler in v-on. Note that you may need to create a new method.",
                                    )
                                };
                                ctx.push(*span, Some(id), msg.into(), fix, sugg);
                                break;
                            }
                            Kind::InlineFunction => {
                                let has_event = doc.tokens_in(*span).iter().any(|t| {
                                    t.kind == TokenKind::Identifier && doc.slice(t.span) == "$event"
                                });
                                let info = token_info(ctx, c);
                                let fix = (!has_event && info.has_quote).then(|| {
                                    let mut edits = vec![Fix::insert(info.range.start, "() => ")];
                                    let toks = doc.tokens_in(*span);
                                    if let (Some(f), Some(l)) = (toks.first(), toks.last()) {
                                        let (fv, lv) = (doc.slice(f.span), doc.slice(l.span));
                                        let simple = (fv == "{" && lv == "}")
                                            || (lv != ";"
                                                && body.len() == 1
                                                && matches!(
                                                    body[0],
                                                    Statement::ExpressionStatement(_)
                                                ));
                                        if !simple {
                                            edits.push(Fix::insert(f.span.start, "{"));
                                            edits.push(Fix::insert(l.span.end, "}"));
                                        }
                                    }
                                    Fix::merge(doc.source, edits)
                                });
                                ctx.report_fix(
                                    *span,
                                    Some("preferInlineFunctionOverInline"),
                                    "Prefer inline function over inline handler in v-on.",
                                    fix.flatten(),
                                );
                                break;
                            }
                            Kind::Inline => {}
                        }
                    }
                }
                VExpression::Expression(Expression::Identifier(ident)) => {
                    if self.allows[0] == Kind::Method {
                        continue;
                    }
                    for allow in &self.allows {
                        let (id, msg) = match allow {
                            Kind::Inline => (
                                "preferInlineOverMethod",
                                "Prefer inline handler over method handler in v-on.",
                            ),
                            Kind::InlineFunction => (
                                "preferInlineFunctionOverMethod",
                                "Prefer inline function over method handler in v-on.",
                            ),
                            Kind::Method => continue,
                        };
                        ctx.report_id(ident.span, id, msg);
                        break;
                    }
                }
                VExpression::Expression(
                    e
                    @ (Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_)),
                ) => {
                    if self.allows[0] == Kind::InlineFunction {
                        continue;
                    }
                    let (params, fspan, block, expr_body): (
                        &FormalParameters,
                        Span,
                        Option<&FunctionBody>,
                        Option<&Expression>,
                    ) = match e {
                        Expression::ArrowFunctionExpression(f) => (
                            &f.params,
                            f.span,
                            match &f.body {
                                ArrowFunctionBody::FunctionBody(b) => Some(&**b),
                                _ => None,
                            },
                            f.body.as_expression(),
                        ),
                        Expression::FunctionExpression(f) => {
                            (&f.params, f.span, f.body.as_deref(), None)
                        }
                        _ => unreachable!(),
                    };
                    for allow in &self.allows {
                        match allow {
                            Kind::Method => {
                                let info = token_info(ctx, c);
                                if self.ignore_comment && info.has_comment {
                                    continue;
                                }
                                let call = match (block, expr_body) {
                                    (Some(b), _) => id_call_of_body(&b.statements),
                                    (None, Some(x)) => id_call(x),
                                    _ => None,
                                };
                                let same = |cl: &CallExpression| {
                                    params.rest.is_none()
                                        && params.items.len() == cl.arguments.len()
                                        && params.items.iter().zip(&cl.arguments).all(|(p, a)| {
                                            matches!((&p.pattern, a),
                                                (BindingPattern::BindingIdentifier(pi), Argument::Identifier(ai)) if pi.name == ai.name)
                                                && p.initializer.is_none()
                                        })
                                };
                                if call.is_none_or(|cl| !same(cl)) && has_upper_ref {
                                    continue;
                                }
                                let (fix, sugg) = match call {
                                    Some(cl) if !info.has_comment && same(cl) => {
                                        self.method_fix(ctx, &counts, cl, info.range)
                                    }
                                    _ => (None, Vec::new()),
                                };
                                let (id, msg) = if call.is_some() {
                                    (
                                        "preferMethodOverInlineFunction",
                                        "Prefer method handler over inline function in v-on.",
                                    )
                                } else {
                                    (
                                        "preferMethodOverInlineFunctionWithoutIdCall",
                                        "Prefer method handler over inline function in v-on. Note that you may need to create a new method.",
                                    )
                                };
                                ctx.push(fspan, Some(id), msg.into(), fix, sugg);
                                break;
                            }
                            Kind::Inline => {
                                let n = params.items.len() + usize::from(params.rest.is_some());
                                let fix = (n == 0).then(|| {
                                    let text = match (block, expr_body) {
                                        (Some(b), _) => {
                                            let t = doc.slice(b.span);
                                            t.get(1..t.len().saturating_sub(1))
                                                .unwrap_or("")
                                                .to_string()
                                        }
                                        (None, Some(x)) => {
                                            doc.slice(skip_parens(x).span()).to_string()
                                        }
                                        _ => String::new(),
                                    };
                                    Fix::replace(fspan, text)
                                });
                                let (id, msg) = if n > 1 {
                                    (
                                        "preferInlineOverInlineFunctionWithMultipleParams",
                                        "Prefer inline handler over inline function in v-on. Note that the custom event must be changed to a single payload.",
                                    )
                                } else {
                                    (
                                        "preferInlineOverInlineFunction",
                                        "Prefer inline handler over inline function in v-on.",
                                    )
                                };
                                ctx.report_fix(fspan, Some(id), msg, fix);
                                break;
                            }
                            Kind::InlineFunction => {}
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let o = opts.get(0);
    let mut allows: Vec<Kind> = match o {
        serde_json::Value::Array(a) => a.iter().filter_map(|v| v.as_str().and_then(kind)).collect(),
        serde_json::Value::String(s) => kind(s).into_iter().collect(),
        _ => Vec::new(),
    };
    if allows.is_empty() {
        allows = vec![Kind::Method, Kind::InlineFunction];
    }
    Some(Box::new(R {
        allows,
        ignore_comment: opts.get(1)["ignoreIncludesComment"].as_bool() == Some(true),
    }))
}
