//! vue/object-shorthand (core `object-shorthand` in `<template>`, dynamic arguments skipped)

use oxc::ast::ast::*;
use oxc::ast_visit::{Visit, walk};
use oxc::semantic::ScopeFlags;
use oxc::span::{GetSpan, Span};
use crate::utils::regexp::JsRegex as Regex;

use crate::ast::{Token, TokenKind};
use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::js::{key_name, skip_parens};
use crate::utils::tpl_core::{
    Scope, first_token_between, for_each_root, has_comment_in, token_before, token_index,
    visit_root,
};

#[derive(Clone, Copy, PartialEq)]
enum Apply {
    Always,
    Methods,
    Properties,
    Never,
    Consistent,
    ConsistentAsNeeded,
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let apply = match opts.get(0).as_str() {
        Some("methods") => Apply::Methods,
        Some("properties") => Apply::Properties,
        Some("never") => Apply::Never,
        Some("consistent") => Apply::Consistent,
        Some("consistent-as-needed") => Apply::ConsistentAsNeeded,
        _ => Apply::Always,
    };
    let p = opts.get(1);
    let flag = |k: &str| p.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
    Some(Box::new(ObjectShorthand {
        apply,
        ignore_constructors: flag("ignoreConstructors"),
        methods_ignore: p
            .get("methodsIgnorePattern")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .and_then(|s| Regex::new(s).ok()),
        avoid_quotes: flag("avoidQuotes"),
        avoid_explicit_return_arrows: flag("avoidExplicitReturnArrows"),
    }))
}

struct ObjectShorthand {
    apply: Apply,
    ignore_constructors: bool,
    methods_ignore: Option<Regex>,
    avoid_quotes: bool,
    avoid_explicit_return_arrows: bool,
}

/// `isConstructor`: first char after a `_$0-9` prefix is an uppercase letter.
fn is_constructor(name: &str) -> bool {
    match name.chars().find(|c| !matches!(c, '_' | '$' | '0'..='9')) {
        Some(c) => c.is_alphabetic() && c.to_uppercase().eq(std::iter::once(c)),
        None => false,
    }
}

fn is_proto_key(p: &ObjectProperty) -> bool {
    !p.computed && key_name(&p.key).as_deref() == Some("__proto__")
}

fn is_string_key(p: &ObjectProperty) -> bool {
    matches!(p.key, PropertyKey::StringLiteral(_))
}

fn can_have_shorthand<'x, 'b>(p: &'x ObjectPropertyKind<'b>) -> Option<&'x ObjectProperty<'b>> {
    match p {
        ObjectPropertyKind::ObjectProperty(p) if p.kind == PropertyKind::Init => Some(p),
        _ => None,
    }
}

fn is_redundant(p: &ObjectProperty) -> bool {
    if is_proto_key(p) {
        return false;
    }
    match skip_parens(&p.value) {
        Expression::FunctionExpression(f) => f.id.is_none(),
        Expression::Identifier(id) => key_name(&p.key).as_deref() == Some(id.name.as_str()),
        _ => false,
    }
}

struct V<'r, 'c, 'f, 'a> {
    rule: &'r ObjectShorthand,
    ctx: &'c mut Ctx<'f, 'a>,
    tokens: &'r [Token],
    comments: &'r [Token],
    /// Open arrow functions per non-arrow function frame (`lexicalScopeStack`).
    lexical: Vec<Vec<Span>>,
    lexical_arrows: Vec<Span>,
    /// Innermost non-arrow function: the method property it is the value of.
    method_stack: Vec<Option<Span>>,
    methods_with_super: Vec<Span>,
    /// Next function visited is the value of this method property.
    pending_method: Option<Span>,
}

impl V<'_, '_, '_, '_> {
    fn text(&self, a: u32, b: u32) -> &str {
        &self.ctx.source()[a as usize..b as usize]
    }

    fn first_tok(&self, span: Span) -> Option<&Token> {
        self.tokens
            .get(token_index(self.tokens, span.start))
            .filter(|t| t.span.end <= span.end)
    }

    fn key_tokens(&self, p: &ObjectProperty, value: Span) -> Option<(Span, Span)> {
        if p.computed {
            let first = first_token_between(self.tokens, p.span.start, p.span.end, |t| {
                t.kind == TokenKind::Punctuator && t.value == "["
            })?;
            let last = first_token_between(self.tokens, p.key.span().end, value.start, |t| {
                t.kind == TokenKind::Punctuator && t.value == "]"
            })?;
            Some((first.span, last.span))
        } else {
            let k = p.key.span();
            Some((self.first_tok(k)?.span, token_before(self.tokens, k.end)?.span))
        }
    }

    fn function_shorthand(&self, p: &ObjectProperty) -> Option<Fix> {
        let value = skip_parens(&p.value);
        let vspan = value.span();
        let (first, last) = self.key_tokens(p, vspan)?;
        if has_comment_in(self.comments, Span::new(last.end, vspan.start)) {
            return None;
        }
        let key_text = self.text(first.start, last.end);
        let range = Span::new(first.start, p.span.end);
        match value {
            Expression::FunctionExpression(f) => {
                let mut prefix = String::new();
                if f.r#async {
                    prefix.push_str("async ");
                }
                if f.generator {
                    prefix.push('*');
                }
                let func = first_token_between(self.tokens, vspan.start, vspan.end, |t| {
                    t.kind == TokenKind::Keyword && t.value == "function"
                })?;
                let before_params = if f.generator {
                    self.tokens.get(token_index(self.tokens, func.span.end))?
                } else {
                    func
                };
                let rest = self.text(before_params.span.end, vspan.end);
                Some(Fix::replace(range, format!("{prefix}{key_text}{rest}")))
            }
            Expression::ArrowFunctionExpression(a) => {
                let prefix = if a.r#async { "async " } else { "" };
                let arrow = self.tokens[..token_index(self.tokens, a.body.span().start)]
                    .iter()
                    .rev()
                    .find(|t| t.value == "=>")?;
                let body = self.text(arrow.span.end, vspan.end);
                let toks = &self.tokens[token_index(self.tokens, vspan.start)..];
                let first_value = toks.get(usize::from(a.r#async))?;
                let slice_start = first_value.span.start;
                let slice_end = token_before(self.tokens, arrow.span.start)?.span.end;
                let add_parens =
                    a.params.items.len() == 1 && a.params.items[0].span.start == slice_start;
                let old = self.text(slice_start, slice_end);
                let params = if add_parens {
                    format!("({old})")
                } else {
                    old.to_string()
                };
                Some(Fix::replace(
                    range,
                    format!("{prefix}{key_text}{params}{body}"),
                ))
            }
            _ => None,
        }
    }

    fn function_longform(&self, p: &ObjectProperty) -> Option<Fix> {
        let Expression::FunctionExpression(f) = skip_parens(&p.value) else {
            return None;
        };
        let (first, last) = self.key_tokens(p, f.span)?;
        let key_text = self.text(first.start, last.end);
        let mut header = "function".to_string();
        if f.r#async {
            header = format!("async {header}");
        }
        if f.generator {
            header.push('*');
        }
        Some(Fix::replace(
            Span::new(p.span.start, last.end),
            format!("{key_text}: {header}"),
        ))
    }

    /// A `/** @type … */`-like block comment inside the property.
    fn type_comment(&self, span: Span, jsdoc_regex: bool) -> bool {
        let i = self.comments.partition_point(|c| c.span.start < span.start);
        self.comments[i..]
            .iter()
            .take_while(|c| c.span.end <= span.end)
            .any(|c| {
                c.kind == TokenKind::BlockComment
                    && c.value.contains("@type")
                    && if jsdoc_regex {
                        c.value.trim_start().starts_with('*')
                    } else {
                        c.value.starts_with('*')
                    }
            })
    }

    fn report(&mut self, span: Span, id: &'static str, msg: &str, fix: Option<Fix>) {
        self.ctx.report_fix(span, Some(id), msg, fix);
    }

    fn property_exit(&mut self, p: &ObjectProperty) {
        let r = self.rule;
        let concise = p.method || p.shorthand;
        if p.kind != PropertyKind::Init || is_proto_key(p) {
            return;
        }
        let value = skip_parens(&p.value);
        let is_fn = matches!(
            value,
            Expression::FunctionExpression(_) | Expression::ArrowFunctionExpression(_)
        );
        if p.computed && !is_fn {
            return;
        }
        let never = r.apply == Apply::Never;
        let to_methods = matches!(r.apply, Apply::Methods | Apply::Always);
        let to_props = matches!(r.apply, Apply::Properties | Apply::Always);
        if concise {
            if p.method
                && !self.methods_with_super.contains(&p.span)
                && (never || (r.avoid_quotes && is_string_key(p)))
            {
                let (id, msg) = if never {
                    ("expectedMethodLongform", "Expected longform method syntax.")
                } else {
                    (
                        "expectedLiteralMethodLongform",
                        "Expected longform method syntax for string literal keys.",
                    )
                };
                let fix = self.function_longform(p);
                self.report(p.span, id, msg, fix);
            } else if never && !p.method {
                let fix = match &p.key {
                    PropertyKey::StaticIdentifier(k) => {
                        Some(Fix::insert(k.span.end, format!(": {}", k.name)))
                    }
                    _ => None,
                };
                self.report(
                    p.span,
                    "expectedPropertyLongform",
                    "Expected longform property syntax.",
                    fix,
                );
            }
            return;
        }
        let named_fn = matches!(value, Expression::FunctionExpression(f) if f.id.is_some());
        if to_methods && is_fn && !named_fn {
            let name = key_name(&p.key);
            if r.ignore_constructors && name.as_deref().is_some_and(is_constructor) {
                return;
            }
            if let (Some(re), Some(n)) = (&r.methods_ignore, &name)
                && re.is_match(n)
            {
                return;
            }
            if r.avoid_quotes && is_string_key(p) {
                return;
            }
            let report = match value {
                Expression::FunctionExpression(_) => true,
                Expression::ArrowFunctionExpression(a) => {
                    matches!(a.body, ArrowFunctionBody::FunctionBody(_))
                        && r.avoid_explicit_return_arrows
                        && !self.lexical_arrows.contains(&a.span)
                }
                _ => false,
            };
            if report {
                let fix = self.function_shorthand(p);
                self.report(
                    p.span,
                    "expectedMethodShorthand",
                    "Expected method shorthand.",
                    fix,
                );
            }
            return;
        }
        let Expression::Identifier(v) = value else {
            return;
        };
        if !to_props {
            return;
        }
        let ident_key = matches!(&p.key, PropertyKey::StaticIdentifier(k) if k.name == v.name);
        let literal_key = matches!(&p.key, PropertyKey::StringLiteral(s) if s.value == v.name);
        if ident_key || literal_key {
            if literal_key && r.avoid_quotes {
                return;
            }
            if self.type_comment(p.span, ident_key) {
                return;
            }
            let fix = (!has_comment_in(self.comments, p.span))
                .then(|| Fix::replace(p.span, v.name.as_str()));
            self.report(
                p.span,
                "expectedPropertyShorthand",
                "Expected property shorthand.",
                fix,
            );
        }
    }

    fn mark_lexical(&mut self) {
        if let Some(top) = self.lexical.last() {
            self.lexical_arrows.extend(top.iter().copied());
        }
    }
}

impl<'b> Visit<'b> for V<'_, '_, '_, '_> {
    crate::skip_ts_types!('b);
    fn visit_object_expression(&mut self, o: &ObjectExpression<'b>) {
        if matches!(
            self.rule.apply,
            Apply::Consistent | Apply::ConsistentAsNeeded
        ) {
            let props: Vec<_> = o.properties.iter().filter_map(can_have_shorthand).collect();
            let short = props.iter().filter(|p| p.shorthand || p.method).count();
            if !props.is_empty() && short != props.len() {
                if short > 0 {
                    self.ctx.report_id(
                        o.span,
                        "unexpectedMix",
                        "Unexpected mix of shorthand and non-shorthand properties.",
                    );
                } else if self.rule.apply == Apply::ConsistentAsNeeded
                    && props.iter().all(|p| is_redundant(p))
                {
                    self.ctx.report_id(
                        o.span,
                        "expectedAllPropertiesShorthanded",
                        "Expected shorthand for all properties.",
                    );
                }
            }
        }
        walk::walk_object_expression(self, o);
    }

    fn visit_object_property(&mut self, p: &ObjectProperty<'b>) {
        if p.method {
            self.pending_method = Some(p.span);
        }
        walk::walk_object_property(self, p);
        self.property_exit(p);
    }

    fn visit_function(&mut self, f: &Function<'b>, flags: ScopeFlags) {
        self.method_stack.push(self.pending_method.take());
        self.lexical.push(Vec::new());
        walk::walk_function(self, f, flags);
        self.lexical.pop();
        self.method_stack.pop();
    }

    fn visit_arrow_function_expression(&mut self, a: &ArrowFunctionExpression<'b>) {
        self.pending_method = None;
        if let Some(top) = self.lexical.last_mut() {
            top.push(a.span);
        }
        walk::walk_arrow_function_expression(self, a);
        if let Some(top) = self.lexical.last_mut() {
            top.retain(|s| *s != a.span);
        }
    }

    fn visit_this_expression(&mut self, _: &ThisExpression) {
        self.mark_lexical();
    }

    fn visit_super(&mut self, _: &Super) {
        self.mark_lexical();
        if let Some(Some(m)) = self.method_stack.last() {
            self.methods_with_super.push(*m);
        }
    }

    fn visit_new_target(&mut self, _: &NewTarget) {
        self.mark_lexical();
    }
}

impl Rule for ObjectShorthand {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let scope = Scope {
            document: false,
            skip_dynamic_args: true,
        };
        for_each_root(ctx, scope, |ctx, root| {
            let mut v = V {
                rule: self,
                ctx,
                tokens: root.tokens,
                comments: root.comments,
                lexical: vec![Vec::new()],
                lexical_arrows: Vec::new(),
                method_stack: Vec::new(),
                methods_with_super: Vec::new(),
                pending_method: None,
            };
            visit_root(&mut v, root.expr);
        });
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn constructor() {
        assert!(super::is_constructor("Foo"));
        assert!(super::is_constructor("_$1Foo"));
        assert!(!super::is_constructor("foo"));
        assert!(!super::is_constructor("__"));
    }
}
