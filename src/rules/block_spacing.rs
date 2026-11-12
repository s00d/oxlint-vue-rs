//! vue/block-spacing

use crate::ast::TokenKind;
use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::indent::tree::Ty;
use crate::utils::tpl_ext::*;

struct BlockSpacing {
    always: bool,
}

impl Rule for BlockSpacing {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let Some(e) = Ext::new(ctx, false) else { return };
        let always = self.always;
        let (word, extra) = if always {
            ("Requires a space", false)
        } else {
            ("Unexpected space(s)", true)
        };
        let mut out = Vec::new();
        for n in e.nodes(true) {
            if !matches!(
                e.ty(n),
                Ty::BlockStatement | Ty::StaticBlock | Ty::SwitchStatement
            ) {
                continue;
            }
            let r = e.range(n);
            let (Some(open), Some(close)) = (e.first_f(r, |t| e.is_punct(t, "{")), e.last(r)) else {
                continue;
            };
            let (Some(ft), Some(lt)) = (e.after_c(e.tr(open)), e.before_c(e.tr(close))) else {
                continue;
            };
            if !e.is_punct(close, "}") || ft == close {
                continue;
            }
            if !always && e.kind(ft) == TokenKind::LineComment {
                continue;
            }
            let valid = |l: (u32, u32), r: (u32, u32)| !e.same_line(l, r) || e.space_between(l, r) == always;
            let (o, c, f, l) = (e.tr(open), e.tr(close), e.tr(ft), e.tr(lt));
            if !valid(o, f) {
                let loc = if extra { (o.1, f.0) } else { o };
                let fix = if always { ins(f.0, " ") } else { del(o.1, f.0) };
                out.push(rep(loc, format!("{word} after '{{'."), fix));
            }
            if !valid(l, c) {
                let loc = if extra { (l.1, c.0) } else { c };
                let fix = if always { ins(l.1, " ") } else { del(l.1, c.0) };
                out.push(rep(loc, format!("{word} before '}}'."), fix));
            }
        }
        emit(ctx, &e, out, false);
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(BlockSpacing {
        always: opt_str(opts, 0, "always") != "never",
    }))
}
