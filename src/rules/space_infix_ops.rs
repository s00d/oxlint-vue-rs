//! vue/space-infix-ops

use crate::ast::TokenKind;
use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::indent::tree::{F, Ty};
use crate::utils::tpl_ext::*;

struct SpaceInfixOps {
    int32_hint: bool,
    ignore_types: bool,
}

impl SpaceInfixOps {
    fn report(&self, e: &Ext, op: u32, out: &mut Vec<Report>) {
        let or = e.tr(op);
        let (Some(p), Some(a)) = (e.before(or), e.after(or)) else {
            return;
        };
        let mut s = String::new();
        if or.0 == e.tr(p).1 {
            s.push(' ');
        }
        s.push_str(e.val(op));
        if e.tr(a).0 == or.1 {
            s.push(' ');
        }
        out.push(rep(or, format!("Operator '{}' must be spaced.", e.val(op)), repl(or.0, or.1, s)));
    }
    fn non_spaced(&self, e: &Ext, op: u32) -> bool {
        let or = e.tr(op);
        match (e.before(or), e.after(or)) {
            (Some(p), Some(a)) => !e.space_between(e.tr(p), or) || !e.space_between(or, e.tr(a)),
            _ => false,
        }
    }
    fn assignment_space(&self, e: &Ext, l: Option<u32>, r: Option<u32>, out: &mut Vec<Report>) {
        let (Some(l), Some(r)) = (l, r) else { return };
        let op = e.first_between_f(e.range(l), e.range(r), |t| {
            e.kind(t) == TokenKind::Punctuator && matches!(e.val(t), "=" | "?" | ":")
        });
        if let Some(op) = op
            && self.non_spaced(e, op)
        {
            self.report(e, op, out);
        }
    }
}

impl Rule for SpaceInfixOps {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let Some(e) = Ext::new(ctx, true) else { return };
        let mut out = Vec::new();
        for n in e.nodes(true) {
            match e.ty(n) {
                Ty::AssignmentExpression
                | Ty::AssignmentPattern
                | Ty::BinaryExpression
                | Ty::LogicalExpression => {
                    let (Some(l), Some(r)) = (e.get(n, F::Left), e.get(n, F::Right)) else {
                        continue;
                    };
                    let opv = if e.ty(n) == Ty::AssignmentPattern {
                        "="
                    } else {
                        match e.operator_token(n) {
                            Some(t) => e.val(t),
                            None => continue,
                        }
                    };
                    let Some(op) = e.first_between_f(e.range(l), e.range(r), |t| e.val(t) == opv)
                    else {
                        continue;
                    };
                    if self.non_spaced(&e, op)
                        && !(self.int32_hint && e.text(e.range(n)).ends_with("|0"))
                    {
                        self.report(&e, op, &mut out);
                    }
                }
                Ty::ConditionalExpression => {
                    let (t, c, a) = (e.get(n, F::Test), e.get(n, F::Consequent), e.get(n, F::Alternate));
                    self.assignment_space(&e, t, c, &mut out);
                    self.assignment_space(&e, c, a, &mut out);
                }
                Ty::VariableDeclarator => {
                    self.assignment_space(&e, e.get(n, F::Id), e.get(n, F::Init), &mut out);
                }
                Ty::TSUnionType | Ty::TSIntersectionType if !self.ignore_types => {
                    for &ty in e.list(n, F::Types) {
                        let r = e.range(ty);
                        let op = if e.ty(ty) == Ty::TSFunctionType {
                            e.before_f(r, |t| !e.is_punct(t, "("))
                        } else {
                            e.before(r)
                        };
                        if let Some(op) = op
                            && matches!(e.val(op), "|" | "&")
                            && self.non_spaced(&e, op)
                        {
                            self.report(&e, op, &mut out);
                        }
                    }
                }
                Ty::TSConditionalType => {
                    let (x, t, f) = (e.get(n, F::ExtendsType), e.get(n, F::TrueType), e.get(n, F::FalseType));
                    self.assignment_space(&e, x, t, &mut out);
                    self.assignment_space(&e, t, f, &mut out);
                }
                _ => {}
            }
        }
        emit(ctx, &e, out, false);
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(SpaceInfixOps {
        int32_hint: opt_bool(opts, 0, "int32Hint").unwrap_or(false),
        ignore_types: opt_bool(opts, 0, "ignoreTypes").unwrap_or(false),
    }))
}
