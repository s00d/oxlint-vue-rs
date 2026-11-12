//! vue/arrow-spacing

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::indent::tree::{F, Ty};
use crate::utils::tpl_ext::*;

struct ArrowSpacing {
    before: bool,
    after: bool,
}

impl Rule for ArrowSpacing {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let Some(e) = Ext::new(ctx, false) else { return };
        let mut out = Vec::new();
        let is_arrow = |t: u32| e.is_punct(t, "=>");
        for n in e.nodes(false) {
            let arrow = match e.ty(n) {
                Ty::ArrowFunctionExpression => e
                    .get(n, F::Body)
                    .and_then(|b| e.before_f(e.range(b), is_arrow)),
                Ty::TSFunctionType | Ty::TSConstructorType => e
                    .get(n, F::ReturnType)
                    .and_then(|rt| e.first_f(e.range(rt), is_arrow)),
                _ => continue,
            };
            let Some(arrow) = arrow else { continue };
            let ar = e.tr(arrow);
            if let Some(b) = e.before_c(ar) {
                let br = e.tr(b);
                let sp = e.space_between(br, ar);
                if self.before && !sp {
                    out.push(rep(br, "Missing space before =>.", ins(ar.0, " ")));
                } else if !self.before && sp {
                    out.push(rep(br, "Unexpected space before =>.", del(br.1, ar.0)));
                }
            }
            if let Some(a) = e.after_c(ar) {
                let anr = e.tr(a);
                let sp = e.space_between(ar, anr);
                if self.after && !sp {
                    out.push(rep(anr, "Missing space after =>.", ins(ar.1, " ")));
                } else if !self.after && sp {
                    out.push(rep(anr, "Unexpected space after =>.", del(ar.1, anr.0)));
                }
            }
        }
        emit(ctx, &e, out, false);
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(ArrowSpacing {
        before: opt_bool(opts, 0, "before").unwrap_or(true),
        after: opt_bool(opts, 0, "after").unwrap_or(true),
    }))
}
