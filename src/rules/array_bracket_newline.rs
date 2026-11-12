//! vue/array-bracket-newline

use serde_json::Value;

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::indent::tree::{F, Ty};
use crate::utils::tpl_ext::*;

struct O {
    consistent: bool,
    multiline: bool,
    min_items: f64,
}

struct ArrayBracketNewline(O);

fn normalize(v: &Value) -> O {
    let inf = f64::INFINITY;
    match v {
        Value::Null | Value::Bool(false) => O {
            consistent: false,
            multiline: true,
            min_items: inf,
        },
        Value::String(s) if s == "consistent" => O {
            consistent: true,
            multiline: false,
            min_items: inf,
        },
        Value::String(s) if s == "always" => O {
            consistent: false,
            multiline: false,
            min_items: 0.0,
        },
        Value::String(_) => O {
            consistent: false,
            multiline: false,
            min_items: inf,
        },
        o => {
            let mi = o.get("minItems").and_then(Value::as_f64);
            if mi == Some(0.0) {
                return O {
                    consistent: false,
                    multiline: false,
                    min_items: 0.0,
                };
            }
            O {
                consistent: false,
                multiline: o.get("multiline").and_then(Value::as_bool).unwrap_or(false),
                min_items: mi.filter(|&m| m != 0.0).unwrap_or(inf),
            }
        }
    }
}

impl Rule for ArrayBracketNewline {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let Some(e) = Ext::new(ctx, false) else { return };
        let o = &self.0;
        let mut out = Vec::new();
        for n in e.nodes(true) {
            if !matches!(e.ty(n), Ty::ArrayExpression | Ty::ArrayPattern) {
                continue;
            }
            let len = e.list(n, F::Elements).len() as f64;
            let r = e.range(n);
            let (Some(open), Some(close)) = (e.first(r), e.last(r)) else {
                continue;
            };
            let (Some(fic), Some(lic), Some(first), Some(last)) = (
                e.after_c(e.tr(open)),
                e.before_c(e.tr(close)),
                e.after(e.tr(open)),
                e.before(e.tr(close)),
            ) else {
                continue;
            };
            let (open_r, close_r) = (e.tr(open), e.tr(close));
            let need = len >= o.min_items
                || (o.multiline && len > 0.0 && !e.same_line(e.tr(lic), e.tr(fic)))
                || (len == 0.0
                    && e.kind(fic) == crate::ast::TokenKind::BlockComment
                    && !e.same_line(e.tr(lic), e.tr(fic))
                    && fic == lic)
                || (o.consistent && !e.same_line(open_r, e.tr(first)));
            if need {
                if e.same_line(open_r, e.tr(first)) {
                    out.push(rep(open_r, "A linebreak is required after '['.", ins(open_r.1, "\n")));
                }
                if e.same_line(e.tr(last), close_r) {
                    out.push(rep(close_r, "A linebreak is required before ']'.", ins(close_r.0, "\n")));
                }
            } else {
                if !e.same_line(open_r, e.tr(first)) {
                    let fix = e
                        .after_c(open_r)
                        .filter(|&t| !e.is_comment(t))
                        .and_then(|t| del(open_r.1, e.tr(t).0));
                    out.push(rep(open_r, "There should be no linebreak after '['.", fix));
                }
                if !e.same_line(e.tr(last), close_r) {
                    let fix = e
                        .before_c(close_r)
                        .filter(|&t| !e.is_comment(t))
                        .and_then(|t| del(e.tr(t).1, close_r.0));
                    out.push(rep(close_r, "There should be no linebreak before ']'.", fix));
                }
            }
        }
        emit(ctx, &e, out, false);
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(ArrayBracketNewline(normalize(opts.get(0)))))
}
