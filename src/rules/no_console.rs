//! vue/no-console (core `no-console` member check in `<template>`)

use oxc::ast::ast::{Expression, MemberExpression};
use oxc::ast_visit::{Visit, walk};

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::js::{member_name, skip_parens};
use crate::utils::tpl_core::{BODY, for_each_root, visit_root};

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let allow = opts
        .get(0)
        .get("allow")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    Some(Box::new(NoConsole { allow }))
}

struct NoConsole {
    allow: Vec<String>,
}

struct V<'r, 'c, 'f, 'a> {
    allow: &'r [String],
    ctx: &'c mut Ctx<'f, 'a>,
}

impl<'b> Visit<'b> for V<'_, '_, '_, '_> {
    crate::skip_ts_types!('b);
    fn visit_member_expression(&mut self, m: &MemberExpression<'b>) {
        walk::walk_member_expression(self, m);
        let Expression::Identifier(obj) = skip_parens(m.object()) else {
            return;
        };
        if obj.name != "console" {
            return;
        }
        if member_name(m).is_some_and(|n| !n.is_empty() && self.allow.contains(&n)) {
            return;
        }
        self.ctx
            .report_id(obj.span, "unexpected", "Unexpected console statement.");
    }
}

impl Rule for NoConsole {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        for_each_root(ctx, BODY, |ctx, root| {
            visit_root(&mut V { allow: &self.allow, ctx }, root.expr);
        });
    }
}
