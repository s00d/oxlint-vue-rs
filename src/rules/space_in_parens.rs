//! vue/space-in-parens

use crate::ast::TokenKind;
use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::tpl_ext::*;

struct SpaceInParens {
    always: bool,
    openers: Vec<&'static str>,
    closers: Vec<&'static str>,
    empty: bool,
}

impl Rule for SpaceInParens {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let Some(e) = Ext::new(ctx, true) else { return };
        let always = self.always;
        let n = e.store.all.len();
        let mut out = Vec::new();
        for i in 0..n {
            let t = i as u32;
            let open = e.is_punct(t, "(");
            let close = e.is_punct(t, ")");
            if !open && !close {
                continue;
            }
            let tr = e.tr(t);
            if open && i + 1 < n {
                let nx = t + 1;
                let nr = e.tr(nx);
                let opener_exc = self.openers.contains(&e.val(nx));
                let sp = e.space_between(tr, nr);
                let missing = !sp
                    && (self.empty || !e.is_punct(nx, ")"))
                    && if always { !opener_exc } else { opener_exc };
                if missing {
                    out.push(rep(tr, "There must be a space after this paren.", ins(tr.1, " ")));
                }
                let rejects = e.same_line(tr, nr)
                    && e.kind(nx) != TokenKind::LineComment
                    && sp
                    && if always { opener_exc } else { !opener_exc };
                if rejects {
                    out.push(rep((tr.1, nr.0), "There should be no space after this paren.", del(tr.1, nr.0)));
                }
            }
            if close && i > 0 {
                let pv = t - 1;
                let pr = e.tr(pv);
                let closer_exc = self.closers.contains(&e.val(pv));
                let sp = e.space_between(pr, tr);
                let missing = !sp
                    && (self.empty || !e.is_punct(pv, "("))
                    && if always { !closer_exc } else { closer_exc };
                if missing {
                    out.push(rep(tr, "There must be a space before this paren.", ins(tr.0, " ")));
                }
                let rejects = e.same_line(pr, tr) && sp && if always { closer_exc } else { !closer_exc };
                if rejects {
                    out.push(rep((pr.1, tr.0), "There should be no space before this paren.", del(pr.1, tr.0)));
                }
            }
        }
        emit(ctx, &e, out, true);
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let exc: Vec<String> = opts
        .get(1)
        .get("exceptions")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let has = |s: &str| exc.iter().any(|x| x == s);
    let mut openers = Vec::new();
    let mut closers = Vec::new();
    for (k, o, c) in [("{}", "{", "}"), ("[]", "[", "]"), ("()", "(", ")"), ("empty", ")", "(")] {
        if has(k) {
            openers.push(o);
            closers.push(c);
        }
    }
    Some(Box::new(SpaceInParens {
        always: opt_str(opts, 0, "never") == "always",
        openers,
        closers,
        empty: has("empty"),
    }))
}
