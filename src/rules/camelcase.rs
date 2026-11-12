//! vue/camelcase (core `camelcase` in `<template>`)

use std::collections::HashSet;

use oxc::ast::AstKind;
use oxc::ast::ast::*;
use oxc::ast_visit::{Visit, walk};
use oxc::semantic::ScopeFlags;
use oxc::span::{GetSpan, Span};
use crate::utils::regexp::JsRegex as Regex;

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::js::{self, skip_parens};
use crate::utils::tpl_core::{BODY, for_each_root, visit_root};

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let o = opts.get(0);
    let flag = |k: &str| o.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
    let allow = o
        .get("allow")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .map(|s| (s.to_string(), Regex::with_flags(s, "u").ok()))
                .collect()
        })
        .unwrap_or_default();
    Some(Box::new(Camelcase {
        allow,
        ignore_destructuring: flag("ignoreDestructuring"),
        ignore_globals: flag("ignoreGlobals"),
        properties_never: o.get("properties").and_then(|v| v.as_str()) == Some("never"),
    }))
}

struct Camelcase {
    allow: Vec<(String, Option<Regex>)>,
    ignore_destructuring: bool,
    ignore_globals: bool,
    properties_never: bool,
}

impl Camelcase {
    fn good(&self, name: &str) -> bool {
        let body = name.trim_matches('_');
        let underscored = body.contains('_') && body != body.to_uppercase();
        !underscored
            || self
                .allow
                .iter()
                .any(|(s, r)| s == name || r.as_ref().is_some_and(|r| r.is_match(name)))
    }
}

#[derive(Default)]
struct V<'r> {
    rule: Option<&'r Camelcase>,
    /// Inside a declaration (`getDeclaredVariables` of variable/function/class/catch nodes).
    decl_depth: usize,
    /// Declared bad names and their identifiers.
    bad_decls: Vec<(String, Span)>,
    refs: Vec<(String, Span)>,
    /// References that are callee/argument of a call or the default of a pattern.
    skip_refs: HashSet<Span>,
    /// Identifiers equal to the original property name (`equalsToOriginalName`).
    original: HashSet<Span>,
    /// Property keys / assignment-target member properties to report.
    props: Vec<(String, Span)>,
}

impl V<'_> {
    fn rule(&self) -> &Camelcase {
        self.rule.unwrap()
    }
    fn skip_ident(&mut self, e: &Expression) {
        if let Expression::Identifier(id) = skip_parens(e) {
            self.skip_refs.insert(id.span);
        }
    }
    fn check_target(&mut self, t: &AssignmentTarget) {
        if let AssignmentTarget::StaticMemberExpression(m) = t {
            self.member_prop(&m.property);
        }
    }
    fn check_maybe_default(&mut self, t: &AssignmentTargetMaybeDefault) {
        if let Some(t) = t.as_assignment_target() {
            self.check_target(t);
        }
    }
    fn member_prop(&mut self, p: &IdentifierName) {
        if !self.rule().properties_never && !self.rule().good(&p.name) {
            self.props.push((p.name.to_string(), p.span));
        }
    }
}

impl<'b> Visit<'b> for V<'_> {
    crate::skip_ts_types!('b);
    fn visit_variable_declaration(&mut self, it: &VariableDeclaration<'b>) {
        self.decl_depth += 1;
        walk::walk_variable_declaration(self, it);
        self.decl_depth -= 1;
    }
    fn visit_function(&mut self, it: &Function<'b>, flags: ScopeFlags) {
        self.decl_depth += 1;
        walk::walk_function(self, it, flags);
        self.decl_depth -= 1;
    }
    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'b>) {
        self.decl_depth += 1;
        walk::walk_arrow_function_expression(self, it);
        self.decl_depth -= 1;
    }
    fn visit_class(&mut self, it: &Class<'b>) {
        self.decl_depth += 1;
        walk::walk_class(self, it);
        self.decl_depth -= 1;
    }
    fn visit_catch_clause(&mut self, it: &CatchClause<'b>) {
        self.decl_depth += 1;
        walk::walk_catch_clause(self, it);
        self.decl_depth -= 1;
    }
    fn visit_binding_identifier(&mut self, it: &BindingIdentifier<'b>) {
        if self.decl_depth > 0 && !self.rule().good(&it.name) {
            self.bad_decls.push((it.name.to_string(), it.span));
        }
    }
    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'b>) {
        self.refs.push((it.name.to_string(), it.span));
    }
    fn visit_binding_property(&mut self, it: &BindingProperty<'b>) {
        let id = match &it.value {
            BindingPattern::BindingIdentifier(b) => Some(b),
            BindingPattern::AssignmentPattern(a) => match &a.left {
                BindingPattern::BindingIdentifier(b) => Some(b),
                _ => None,
            },
            _ => None,
        };
        if let (Some(id), PropertyKey::StaticIdentifier(k), false) = (id, &it.key, it.computed)
            && k.name == id.name
        {
            self.original.insert(id.span);
        }
        walk::walk_binding_property(self, it);
    }
    fn visit_assignment_pattern(&mut self, it: &AssignmentPattern<'b>) {
        self.skip_ident(&it.right);
        walk::walk_assignment_pattern(self, it);
    }
    fn visit_formal_parameter(&mut self, it: &FormalParameter<'b>) {
        if let Some(init) = &it.initializer {
            self.skip_ident(init);
        }
        walk::walk_formal_parameter(self, it);
    }
    fn visit_call_expression(&mut self, it: &CallExpression<'b>) {
        self.skip_ident(&it.callee);
        for a in &it.arguments {
            if let Some(e) = a.as_expression() {
                self.skip_ident(e);
            }
        }
        walk::walk_call_expression(self, it);
    }
    fn visit_new_expression(&mut self, it: &NewExpression<'b>) {
        self.skip_ident(&it.callee);
        for a in &it.arguments {
            if let Some(e) = a.as_expression() {
                self.skip_ident(e);
            }
        }
        walk::walk_new_expression(self, it);
    }
    fn visit_object_property(&mut self, it: &ObjectProperty<'b>) {
        if !it.computed
            && let PropertyKey::StaticIdentifier(k) = &it.key
        {
            if !self.rule().properties_never && !self.rule().good(&k.name) {
                self.props.push((k.name.to_string(), k.span));
            }
            if let Expression::Identifier(v) = &it.value
                && v.name == k.name
            {
                self.original.insert(v.span);
            }
        }
        walk::walk_object_property(self, it);
    }
    fn visit_assignment_target_property_identifier(
        &mut self,
        it: &AssignmentTargetPropertyIdentifier<'b>,
    ) {
        self.original.insert(it.binding.span);
        if let Some(init) = &it.init {
            self.skip_ident(init);
        }
        walk::walk_assignment_target_property_identifier(self, it);
    }
    fn visit_assignment_target_property_property(
        &mut self,
        it: &AssignmentTargetPropertyProperty<'b>,
    ) {
        self.check_maybe_default(&it.binding);
        if let (PropertyKey::StaticIdentifier(k), false) = (&it.name, it.computed) {
            let t = match &it.binding {
                AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(d) => Some(&d.binding),
                b => b.as_assignment_target(),
            };
            if let Some(AssignmentTarget::AssignmentTargetIdentifier(id)) = t
                && id.name == k.name
            {
                self.original.insert(id.span);
            }
        }
        walk::walk_assignment_target_property_property(self, it);
    }
    fn visit_assignment_expression(&mut self, it: &AssignmentExpression<'b>) {
        self.check_target(&it.left);
        walk::walk_assignment_expression(self, it);
    }
    fn visit_assignment_target_with_default(&mut self, it: &AssignmentTargetWithDefault<'b>) {
        self.check_target(&it.binding);
        self.skip_ident(&it.init);
        walk::walk_assignment_target_with_default(self, it);
    }
    fn visit_array_assignment_target(&mut self, it: &ArrayAssignmentTarget<'b>) {
        for el in it.elements.iter().flatten() {
            self.check_maybe_default(el);
        }
        walk::walk_array_assignment_target(self, it);
    }
    fn visit_assignment_target_rest(&mut self, it: &AssignmentTargetRest<'b>) {
        self.check_target(&it.target);
        walk::walk_assignment_target_rest(self, it);
    }
    fn visit_labeled_statement(&mut self, it: &LabeledStatement<'b>) {
        if !self.rule().good(&it.label.name) {
            self.props.push((it.label.name.to_string(), it.label.span));
        }
        walk::walk_labeled_statement(self, it);
    }
    fn visit_break_statement(&mut self, it: &BreakStatement<'b>) {
        if let Some(l) = &it.label
            && !self.rule().good(&l.name)
        {
            self.props.push((l.name.to_string(), l.span));
        }
    }
    fn visit_continue_statement(&mut self, it: &ContinueStatement<'b>) {
        if let Some(l) = &it.label
            && !self.rule().good(&l.name)
        {
            self.props.push((l.name.to_string(), l.span));
        }
    }
}

const MSG: &str = "Identifier '{{name}}' is not in camel case.";

fn report(ctx: &mut Ctx, reported: &mut HashSet<u32>, name: &str, span: Span) {
    if reported.insert(span.start) {
        ctx.report_id(span, "notCamelCase", MSG.replace("{{name}}", name));
    }
}

impl Camelcase {
    /// The core `Program` handler runs on the template root with the global scope: unresolved
    /// script references with bad names (`scope.through`).
    fn script_through(&self, ctx: &mut Ctx, reported: &mut HashSet<u32>) {
        let Some(sem) = ctx.file.semantic else { return };
        let scoping = sem.scoping();
        let mut found: Vec<(String, Span)> = Vec::new();
        for (name, ids) in scoping.root_unresolved_references() {
            if self.good(name) {
                continue;
            }
            for &rid in ids {
                let node = scoping.get_reference(rid).node_id();
                let AstKind::IdentifierReference(id) = sem.nodes().kind(node) else {
                    continue;
                };
                let (_, parent) = js::parent(sem, node);
                let skip = match parent {
                    AstKind::CallExpression(_) | AstKind::NewExpression(_) => true,
                    AstKind::AssignmentPattern(p) => p.right.span() == id.span,
                    AstKind::AssignmentTargetWithDefault(p) => p.init.span() == id.span,
                    AstKind::ObjectProperty(p) if self.ignore_destructuring => {
                        !p.computed
                            && matches!(&p.key, PropertyKey::StaticIdentifier(k) if k.name == id.name)
                    }
                    _ => false,
                };
                if !skip {
                    found.push((name.to_string(), id.span));
                }
            }
        }
        found.sort_by_key(|(_, s)| s.start);
        for (name, span) in found {
            report(ctx, reported, &name, span);
        }
        let _ = self.ignore_globals; // config globals have no bad builtin names
    }
}

impl Rule for Camelcase {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        if ctx.doc().is_none_or(|d| d.template_body.is_none()) {
            return;
        }
        let mut reported = HashSet::new();
        self.script_through(ctx, &mut reported);
        for_each_root(ctx, BODY, |ctx, root| {
            let mut v = V {
                rule: Some(self),
                ..V::default()
            };
            visit_root(&mut v, root.expr);
            let bad: HashSet<&str> = v.bad_decls.iter().map(|(n, _)| n.as_str()).collect();
            for (name, span) in &v.bad_decls {
                if !(self.ignore_destructuring && v.original.contains(span)) {
                    report(ctx, &mut reported, name, *span);
                }
            }
            // ponytail: references are matched to declarations by name within the container
            // (no scope analysis); bad names declared inside template expressions are rare.
            for (name, span) in &v.refs {
                if bad.contains(name.as_str())
                    && !v.skip_refs.contains(span)
                    && !(self.ignore_destructuring && v.original.contains(span))
                {
                    report(ctx, &mut reported, name, *span);
                }
            }
            for (name, span) in &v.props {
                report(ctx, &mut reported, name, *span);
            }
        });
    }
}
