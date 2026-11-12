//! vue/no-empty-pattern (core `no-empty-pattern` in `<template>`)

use oxc::ast::ast::*;
use oxc::ast_visit::{Visit, walk};
use oxc::semantic::ScopeFlags;
use oxc::span::Span;

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::tpl_core::{BODY, for_each_root, visit_root};

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(NoEmptyPattern {
        allow_params: opts
            .get(0)
            .get("allowObjectPatternsAsParameters")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    }))
}

struct NoEmptyPattern {
    allow_params: bool,
}

struct V<'c, 'f, 'a> {
    ctx: &'c mut Ctx<'f, 'a>,
    allow_params: bool,
    /// Parameter lists of real functions (slot-scope params are not).
    fn_params: Vec<Span>,
    /// Object patterns allowed as parameters.
    allowed: Vec<Span>,
}

impl V<'_, '_, '_> {
    fn report(&mut self, span: Span, ty: &str) {
        self.ctx
            .report_id(span, "unexpected", format!("Unexpected empty {ty} pattern."));
    }
}

impl<'b> Visit<'b> for V<'_, '_, '_> {
    crate::skip_ts_types!('b);
    fn visit_function(&mut self, f: &Function<'b>, flags: ScopeFlags) {
        self.fn_params.push(f.params.span);
        walk::walk_function(self, f, flags);
    }
    fn visit_arrow_function_expression(&mut self, f: &ArrowFunctionExpression<'b>) {
        self.fn_params.push(f.params.span);
        walk::walk_arrow_function_expression(self, f);
    }
    fn visit_formal_parameters(&mut self, ps: &FormalParameters<'b>) {
        if self.allow_params && self.fn_params.contains(&ps.span) {
            for p in &ps.items {
                let BindingPattern::ObjectPattern(o) = &p.pattern else {
                    continue;
                };
                let ok = match p.initializer.as_deref() {
                    None => true,
                    Some(init) => matches!(
                        crate::utils::js::skip_parens(init),
                        Expression::ObjectExpression(e) if e.properties.is_empty()
                    ),
                };
                if ok {
                    self.allowed.push(o.span);
                }
            }
        }
        walk::walk_formal_parameters(self, ps);
    }
    fn visit_object_pattern(&mut self, it: &ObjectPattern<'b>) {
        walk::walk_object_pattern(self, it);
        if it.properties.is_empty() && it.rest.is_none() && !self.allowed.contains(&it.span) {
            self.report(it.span, "object");
        }
    }
    fn visit_array_pattern(&mut self, it: &ArrayPattern<'b>) {
        walk::walk_array_pattern(self, it);
        if it.elements.is_empty() && it.rest.is_none() {
            self.report(it.span, "array");
        }
    }
    fn visit_object_assignment_target(&mut self, it: &ObjectAssignmentTarget<'b>) {
        walk::walk_object_assignment_target(self, it);
        if it.properties.is_empty() && it.rest.is_none() {
            self.report(it.span, "object");
        }
    }
    fn visit_array_assignment_target(&mut self, it: &ArrayAssignmentTarget<'b>) {
        walk::walk_array_assignment_target(self, it);
        if it.elements.is_empty() && it.rest.is_none() {
            self.report(it.span, "array");
        }
    }
}

impl Rule for NoEmptyPattern {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        for_each_root(ctx, BODY, |ctx, root| {
            let mut v = V {
                ctx,
                allow_params: self.allow_params,
                fn_params: Vec::new(),
                allowed: Vec::new(),
            };
            visit_root(&mut v, root.expr);
        });
    }
}
