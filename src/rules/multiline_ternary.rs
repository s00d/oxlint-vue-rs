//! vue/multiline-ternary

use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::indent::tree::{F, Ty};
use crate::utils::tpl_ext::*;

struct MultilineTernary {
    multiline: bool,
    allow_single_line: bool,
}

impl Rule for MultilineTernary {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let Some(e) = Ext::new(ctx, true) else { return };
        let mut out = Vec::new();
        let not_close = |t: u32| !e.is_punct(t, ")");
        for n in e.nodes(true) {
            if e.ty(n) != Ty::ConditionalExpression {
                continue;
            }
            let (Some(test), Some(cons)) = (e.get(n, F::Test), e.get(n, F::Consequent)) else {
                continue;
            };
            let (Some(q), Some(c)) = (
                e.after_f(e.range(test), not_close),
                e.after_f(e.range(cons), not_close),
            ) else {
                continue;
            };
            let (qr, cr) = (e.tr(q), e.tr(c));
            let (Some(ft), Some(lt), Some(fc), Some(lc), Some(fa)) = (
                e.first(e.range(n)),
                e.before(qr),
                e.after(qr),
                e.before(cr),
                e.after(cr),
            ) else {
                continue;
            };
            let (ft, lt, fc, lc, fa) = (e.tr(ft), e.tr(lt), e.tr(fc), e.tr(lc), e.tr(fa));
            let tc_same = e.same_line(lt, fc);
            let ca_same = e.same_line(lc, fa);
            let comments = e.has_comments_inside(e.range(n));
            if !self.multiline {
                if !tc_same {
                    let fix = (!comments).then(|| {
                        let mut v = Vec::new();
                        if !e.same_line(lt, qr) {
                            v.push(Fix::remove(oxc::span::Span::new(lt.1, qr.0)));
                        }
                        if !e.same_line(qr, fc) {
                            v.push(Fix::remove(oxc::span::Span::new(qr.1, fc.0)));
                        }
                        Fix::merge(e.src, v)
                    });
                    out.push(rep(
                        (ft.0, lt.1),
                        "Unexpected newline between test and consequent of ternary expression.",
                        fix.flatten(),
                    ));
                }
                if !ca_same {
                    let fix = (!comments).then(|| {
                        let mut v = Vec::new();
                        if !e.same_line(lc, cr) {
                            v.push(Fix::remove(oxc::span::Span::new(lc.1, cr.0)));
                        }
                        if !e.same_line(cr, fa) {
                            v.push(Fix::remove(oxc::span::Span::new(cr.1, fa.0)));
                        }
                        Fix::merge(e.src, v)
                    });
                    out.push(rep(
                        (fc.0, lc.1),
                        "Unexpected newline between consequent and alternate of ternary expression.",
                        fix.flatten(),
                    ));
                }
            } else {
                if self.allow_single_line && e.single_line(e.range(n)) {
                    continue;
                }
                if tc_same {
                    let fix = if comments { None } else { repl(lt.1, qr.0, "\n") };
                    out.push(rep(
                        (ft.0, lt.1),
                        "Expected newline between test and consequent of ternary expression.",
                        fix,
                    ));
                }
                if ca_same {
                    let fix = if comments { None } else { repl(lc.1, cr.0, "\n") };
                    out.push(rep(
                        (fc.0, lc.1),
                        "Expected newline between consequent and alternate of ternary expression.",
                        fix,
                    ));
                }
            }
        }
        emit(ctx, &e, out, false);
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let style = opt_str(opts, 0, "always");
    Some(Box::new(MultilineTernary {
        multiline: style != "never",
        allow_single_line: style == "always-multiline",
    }))
}
