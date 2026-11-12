//! vue/array-element-newline

use serde_json::Value;

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::indent::tree::{F, NONE, Ty};
use crate::utils::tpl_ext::*;

#[derive(Clone, Copy)]
struct O {
    consistent: bool,
    multiline: bool,
    min_items: f64,
}

struct ArrayElementNewline {
    expr: Option<O>,
    pat: Option<O>,
}

fn value(v: &Value) -> O {
    let inf = f64::INFINITY;
    let falsy = matches!(v, Value::Null | Value::Bool(false))
        || v.as_str() == Some("")
        || v.as_f64() == Some(0.0);
    let o = O {
        consistent: false,
        multiline: false,
        min_items: 0.0,
    };
    if falsy || v.as_str() == Some("always") || v.get("minItems").and_then(Value::as_f64) == Some(0.0) {
        return o;
    }
    match v.as_str() {
        Some("never") => O { min_items: inf, ..o },
        Some("consistent") => O {
            consistent: true,
            min_items: inf,
            ..o
        },
        _ => O {
            consistent: v.get("consistent").and_then(Value::as_bool).unwrap_or(false),
            multiline: v.get("multiline").and_then(Value::as_bool).unwrap_or(false),
            min_items: v
                .get("minItems")
                .and_then(Value::as_f64)
                .filter(|&m| m != 0.0)
                .unwrap_or(inf),
        },
    }
}

fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) | Some(Value::Bool(false)) => false,
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Number(n)) => n.as_f64() != Some(0.0),
        _ => true,
    }
}

impl Rule for ArrayElementNewline {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let Some(e) = Ext::new(ctx, false) else { return };
        let mut out = Vec::new();
        for n in e.nodes(true) {
            let o = match e.ty(n) {
                Ty::ArrayExpression => self.expr,
                Ty::ArrayPattern => self.pat,
                _ => continue,
            };
            let Some(o) = o else { continue };
            let els = e.list(n, F::Elements);
            let element_break = o.multiline
                && els
                    .iter()
                    .any(|&c| c != NONE && !e.single_line(e.range(c)));
            let comma_of = |i: usize| {
                e.first_between_f(e.range(els[i - 1]), e.range(els[i]), |t| e.is_punct(t, ","))
            };
            let mut breaks = 0usize;
            for i in 1..els.len() {
                if els[i] == NONE || els[i - 1] == NONE {
                    continue;
                }
                let Some(c) = comma_of(i) else { continue };
                if let (Some(b), Some(a)) = (e.before(e.tr(c)), e.after(e.tr(c)))
                    && !e.same_line(e.tr(b), e.tr(a))
                {
                    breaks += 1;
                }
            }
            let need = els.len() as f64 >= o.min_items
                || (o.multiline && element_break)
                || (o.consistent && breaks > 0 && breaks < els.len());
            for i in 1..els.len() {
                if els[i] == NONE || els[i - 1] == NONE {
                    continue;
                }
                let Some(c) = comma_of(i) else { continue };
                let (Some(lp), Some(fc)) = (e.before(e.tr(c)), e.after(e.tr(c))) else {
                    continue;
                };
                let fcr = e.tr(fc);
                let Some(tb) = e.before_c(fcr) else { continue };
                let tbr = e.tr(tb);
                if need {
                    if e.same_line(e.tr(lp), fcr) {
                        out.push(rep(
                            (tbr.1, fcr.0),
                            "There should be a linebreak after this element.",
                            repl(tbr.1, fcr.0, "\n"),
                        ));
                    }
                } else if !e.same_line(e.tr(lp), fcr) {
                    let fix = if e.is_comment(tb) {
                        None
                    } else if !e.same_line(tbr, fcr) {
                        repl(tbr.1, fcr.0, " ")
                    } else {
                        match e.before_c(tbr) {
                            Some(t2) if !e.is_comment(t2) => repl(e.tr(t2).1, tbr.0, ""),
                            _ => None,
                        }
                    };
                    out.push(rep((tbr.1, fcr.0), "There should be no linebreak here.", fix));
                }
            }
        }
        emit(ctx, &e, out, false);
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let v = opts.get(0);
    let (ae, ap) = (v.get("ArrayExpression"), v.get("ArrayPattern"));
    let (expr, pat) = if v.is_object() && (truthy(ae) || truthy(ap)) {
        (
            truthy(ae).then(|| value(ae.unwrap())),
            truthy(ap).then(|| value(ap.unwrap())),
        )
    } else {
        (Some(value(v)), Some(value(v)))
    };
    Some(Box::new(ArrayElementNewline { expr, pat }))
}
