//! vue/no-dupe-v-else-if

use oxc::ast::ast::{Expression, LogicalOperator};

use crate::ast::Document;
use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::js::skip_parens;
use crate::utils::template::{directive_expression, sibling_elements};
use crate::utils::tpl_valid::{body_directives, equal_tokens, expr_span, same_type};

fn split<'b, 'a>(op: LogicalOperator, e: &'b Expression<'a>, out: &mut Vec<&'b Expression<'a>>) {
    match skip_parens(e) {
        Expression::LogicalExpression(l) if l.operator == op => {
            split(op, &l.left, out);
            split(op, &l.right, out);
        }
        x => out.push(x),
    }
}

/// `{ node, operands: AndOperands[] }` — OR operands, each split by AND.
struct OrOperands<'b, 'a> {
    node: &'b Expression<'a>,
    operands: Vec<Vec<&'b Expression<'a>>>,
}

fn build<'b, 'a>(e: &'b Expression<'a>) -> OrOperands<'b, 'a> {
    let mut ors = Vec::new();
    split(LogicalOperator::Or, e, &mut ors);
    OrOperands {
        node: skip_parens(e),
        operands: ors
            .into_iter()
            .map(|o| {
                let mut ands = Vec::new();
                split(LogicalOperator::And, o, &mut ands);
                ands
            })
            .collect(),
    }
}

fn is_equal(doc: &Document, a: &Expression, b: &Expression) -> bool {
    let (a, b) = (skip_parens(a), skip_parens(b));
    same_type(a, b)
        && (equal_tokens(doc, expr_span(a), expr_span(b))
            || match (a, b) {
                (Expression::LogicalExpression(x), Expression::LogicalExpression(y))
                    if matches!(x.operator, LogicalOperator::Or | LogicalOperator::And)
                        && x.operator == y.operator =>
                {
                    (is_equal(doc, &x.left, &y.left) && is_equal(doc, &x.right, &y.right))
                        || (is_equal(doc, &x.left, &y.right) && is_equal(doc, &x.right, &y.left))
                }
                _ => false,
            })
}

fn is_subset(doc: &Document, a: &[&Expression], b: &[&Expression]) -> bool {
    a.iter().all(|x| b.iter().any(|y| is_equal(doc, x, y)))
}

struct NoDupeVElseIf;

impl Rule for NoDupeVElseIf {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (e, _, a, _) in body_directives(doc, "else-if") {
            let Some(test) = directive_expression(doc, a) else {
                continue;
            };
            let test = skip_parens(test);
            let mut conditions = Vec::new();
            if let Expression::LogicalExpression(l) = test
                && l.operator == LogicalOperator::And
            {
                split(LogicalOperator::And, test, &mut conditions);
            }
            conditions.push(test);
            let mut list: Vec<OrOperands> = conditions.into_iter().map(build).collect();

            let sib = sibling_elements(doc, e);
            let Some(pos) = sib.iter().position(|s| s.id == e.id) else {
                continue;
            };
            'chain: for cur in sib[..pos].iter().rev() {
                let vif = cur.directive("if", None);
                let Some(dir) = vif.or_else(|| cur.directive("else-if", None)) else {
                    break;
                };
                if let Some(cur_expr) = directive_expression(doc, dir) {
                    let cur_ops = build(cur_expr);
                    for cond in &mut list {
                        cond.operands
                            .retain(|or| cur_ops.operands.iter().all(|c| !is_subset(doc, c, or)));
                        if cond.operands.is_empty() {
                            ctx.report(expr_span(cond.node), "This branch can never execute. Its condition is a duplicate or covered by previous conditions in the `v-if` / `v-else-if` chain.");
                            break 'chain;
                        }
                    }
                }
                if vif.is_some() {
                    break;
                }
            }
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(NoDupeVElseIf))
}
