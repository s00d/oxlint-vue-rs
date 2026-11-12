//! vue/brace-style

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::indent::tree::{F, Ty};
use crate::utils::tpl_ext::*;

struct BraceStyle {
    style: String,
    allow_single_line: bool,
}

const NEXT_LINE_OPEN: &str =
    "Opening curly brace does not appear on the same line as controlling statement.";
const SAME_LINE_OPEN: &str = "Opening curly brace appears on the same line as controlling statement.";
const BLOCK_SAME_LINE: &str = "Statement inside of curly braces should be on next line.";
const NEXT_LINE_CLOSE: &str =
    "Closing curly brace does not appear on the same line as the subsequent block.";
const SINGLE_LINE_CLOSE: &str = "Closing curly brace should be on the same line as opening curly brace or on the line after the previous block.";
const SAME_LINE_CLOSE: &str = "Closing curly brace appears on the same line as the subsequent block.";

/// `safeReplaceTextBetween`.
fn safe_replace(e: &Ext, l: (u32, u32), r: (u32, u32), text: &str) -> Option<crate::rule::Fix> {
    if e.comments_between(l, r) {
        return None;
    }
    repl(l.1, r.0, text)
}

impl BraceStyle {
    fn pair(&self, e: &Ext, open: u32, close: u32, out: &mut Vec<Report>) {
        let (o, c) = (e.tr(open), e.tr(close));
        let (Some(tbo), Some(tbc), Some(tao)) = (e.before(o), e.before(c), e.after(o)) else {
            return;
        };
        let allman = self.style == "allman";
        let single = self.allow_single_line && e.same_line(o, c);
        let tbo_r = e.tr(tbo);
        if !allman && !e.same_line(tbo_r, o) {
            out.push(rep(o, NEXT_LINE_OPEN, safe_replace(e, tbo_r, o, " ")));
        }
        if allman && e.same_line(tbo_r, o) && !single {
            out.push(rep(o, SAME_LINE_OPEN, ins(o.0, "\n")));
        }
        if e.same_line(o, e.tr(tao)) && tao != close && !single {
            out.push(rep(o, BLOCK_SAME_LINE, ins(o.1, "\n")));
        }
        if e.same_line(e.tr(tbc), c) && tbc != open && !single {
            out.push(rep(c, SINGLE_LINE_CLOSE, ins(c.0, "\n")));
        }
    }
    fn before_keyword(&self, e: &Ext, curly: Option<u32>, out: &mut Vec<Report>) {
        let Some(curly) = curly else { return };
        let c = e.tr(curly);
        let Some(kw) = e.after(c) else { return };
        let k = e.tr(kw);
        if self.style == "1tbs" && !e.same_line(c, k) {
            out.push(rep(c, NEXT_LINE_CLOSE, safe_replace(e, c, k, " ")));
        }
        if self.style != "1tbs" && e.same_line(c, k) {
            out.push(rep(c, SAME_LINE_CLOSE, ins(c.1, "\n")));
        }
    }
}

impl Rule for BraceStyle {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let Some(e) = Ext::new(ctx, false) else { return };
        let mut out = Vec::new();
        for n in e.nodes(true) {
            let r = e.range(n);
            match e.ty(n) {
                Ty::BlockStatement => {
                    let p = e.parent(n).map(|p| e.ty(p));
                    if !matches!(
                        p,
                        Some(Ty::Program | Ty::BlockStatement | Ty::StaticBlock | Ty::SwitchCase)
                    ) && let (Some(a), Some(b)) = (e.first(r), e.last(r))
                    {
                        self.pair(&e, a, b, &mut out);
                    }
                }
                Ty::StaticBlock => {
                    if let (Some(a), Some(b)) = (e.first_skip(r, 1), e.last(r)) {
                        self.pair(&e, a, b, &mut out);
                    }
                }
                Ty::ClassBody | Ty::TSModuleBlock => {
                    if let (Some(a), Some(b)) = (e.first(r), e.last(r)) {
                        self.pair(&e, a, b, &mut out);
                    }
                }
                Ty::SwitchStatement => {
                    let Some(close) = e.last(r) else { continue };
                    let target = e
                        .list(n, F::Cases)
                        .first()
                        .map_or(e.tr(close), |&c| e.range(c));
                    if let Some(open) = e.before(target) {
                        self.pair(&e, open, close, &mut out);
                    }
                }
                Ty::IfStatement => {
                    if let (Some(c), Some(_)) = (e.get(n, F::Consequent), e.get(n, F::Alternate))
                        && e.ty(c) == Ty::BlockStatement
                    {
                        self.before_keyword(&e, e.last(e.range(c)), &mut out);
                    }
                }
                Ty::TryStatement => {
                    if let Some(b) = e.get(n, F::Block) {
                        self.before_keyword(&e, e.last(e.range(b)), &mut out);
                    }
                    if let (Some(h), Some(_)) = (e.get(n, F::Handler), e.get(n, F::Finalizer))
                        && let Some(hb) = e.get(h, F::Body)
                    {
                        self.before_keyword(&e, e.last(e.range(hb)), &mut out);
                    }
                }
                _ => {}
            }
        }
        emit(ctx, &e, out, false);
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(BraceStyle {
        style: opt_str(opts, 0, "1tbs").to_string(),
        allow_single_line: opt_bool(opts, 1, "allowSingleLine").unwrap_or(false),
    }))
}
