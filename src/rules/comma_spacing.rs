//! vue/comma-spacing

use std::collections::HashSet;

use crate::ast::TokenKind;
use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::indent::tree::{F, NONE, Ty};
use crate::utils::tpl_ext::*;

struct CommaSpacing {
    before: bool,
    after: bool,
}

impl Rule for CommaSpacing {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let Some(e) = Ext::new(ctx, true) else { return };
        let comma = |t: u32| e.is_punct(t, ",");
        let mut ignored: HashSet<u32> = HashSet::new();
        for n in e.nodes(true) {
            match e.ty(n) {
                Ty::ArrayExpression | Ty::ArrayPattern => {
                    let mut prev = e.first(e.range(n));
                    for &el in e.list(n, F::Elements) {
                        let tok = if el == NONE {
                            let t = prev.and_then(|p| e.after(e.tr(p)));
                            if let Some(t) = t.filter(|&t| comma(t)) {
                                ignored.insert(t);
                            }
                            t
                        } else {
                            e.after(e.range(el))
                        };
                        prev = tok;
                    }
                }
                Ty::TSTypeParameterDeclaration => {
                    if let Some(&p) = e.list(n, F::Params).last()
                        && let Some(a) = e.after(e.range(p)).filter(|&t| comma(t))
                    {
                        ignored.insert(a);
                    }
                }
                _ => {}
            }
        }
        let all = &e.store.all;
        let mut out = Vec::new();
        for i in 0..all.len() {
            let t = i as u32;
            if !comma(t) {
                continue;
            }
            let ign = ignored.contains(&t);
            let prev = (i > 0)
                .then(|| t - 1)
                .filter(|&p| !comma(p) && !ign);
            let next = (i + 1 < all.len())
                .then(|| t + 1)
                .filter(|&n| !comma(n) && !ign);
            let c = e.tr(t);
            if let Some(p) = prev {
                let pr = e.tr(p);
                if e.same_line(pr, c) && self.before != e.space_between(pr, c) {
                    if self.before {
                        out.push(rep(c, "A space is required before ','.", ins(c.0, " ")));
                    } else {
                        out.push(rep(c, "There should be no space before ','.", repl(pr.1, c.0, "")));
                    }
                }
            }
            if let Some(nx) = next {
                let nr = e.tr(nx);
                if e.same_line(c, nr)
                    && !e.is_punct(nx, ")")
                    && !e.is_punct(nx, "]")
                    && !e.is_punct(nx, "}")
                    && !(!self.after && e.kind(nx) == TokenKind::LineComment)
                    && self.after != e.space_between(c, nr)
                {
                    if self.after {
                        out.push(rep(c, "A space is required after ','.", ins(c.1, " ")));
                    } else {
                        out.push(rep(c, "There should be no space after ','.", repl(c.1, nr.0, "")));
                    }
                }
            }
        }
        emit(ctx, &e, out, true);
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(CommaSpacing {
        before: opt_bool(opts, 0, "before").unwrap_or(false),
        after: opt_bool(opts, 0, "after").unwrap_or(true),
    }))
}
