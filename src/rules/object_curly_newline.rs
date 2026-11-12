//! vue/object-curly-newline

use serde_json::Value;

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::indent::tree::{F, Ty};
use crate::utils::tpl_ext::*;

#[derive(Clone, Copy)]
struct O {
    multiline: bool,
    min_props: f64,
    consistent: bool,
}

fn value(v: Option<&Value>) -> O {
    let inf = f64::INFINITY;
    match v {
        None | Some(Value::Null) | Some(Value::Bool(false)) => O {
            multiline: false,
            min_props: inf,
            consistent: true,
        },
        Some(Value::String(s)) if s.is_empty() => O {
            multiline: false,
            min_props: inf,
            consistent: true,
        },
        Some(Value::String(s)) => O {
            multiline: false,
            min_props: if s == "always" { 0.0 } else { inf },
            consistent: false,
        },
        Some(o) => O {
            multiline: o.get("multiline").and_then(Value::as_bool).unwrap_or(false),
            min_props: o
                .get("minProperties")
                .and_then(Value::as_f64)
                .filter(|&m| m != 0.0)
                .unwrap_or(inf),
            consistent: o.get("consistent").and_then(Value::as_bool).unwrap_or(false),
        },
    }
}

struct ObjectCurlyNewline {
    expr: O,
    pat: O,
    type_lit: O,
}

impl Rule for ObjectCurlyNewline {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let Some(e) = Ext::new(ctx, false) else { return };
        let mut out = Vec::new();
        for n in e.nodes(true) {
            let (o, field) = match e.ty(n) {
                Ty::ObjectExpression => (self.expr, F::Properties),
                Ty::ObjectPattern => (self.pat, F::Properties),
                Ty::TSTypeLiteral => (self.type_lit, F::Members),
                _ => continue,
            };
            let r = e.range(n);
            let Some(open) = e.first_f(r, |t| e.val(t) == "{") else {
                continue;
            };
            let close = match e.get(n, F::TypeAnnotation) {
                Some(ta) if e.ty(n) == Ty::ObjectPattern => e.before(e.range(ta)),
                _ => e.last_f(r, |t| e.val(t) == "}"),
            };
            let Some(close) = close else { continue };
            let (or, cr) = (e.tr(open), e.tr(close));
            let (Some(fc), Some(lc)) = (e.after_c(or), e.before_c(cr)) else {
                continue;
            };
            let len = e.list(n, field).len() as f64;
            let need = len >= o.min_props
                || (o.multiline && len > 0.0 && !e.same_line(e.tr(lc), e.tr(fc)));
            let c_first = e.is_comment(fc);
            let c_last = e.is_comment(lc);
            let (Some(first), Some(last)) = (e.after(or), e.before(cr)) else {
                continue;
            };
            let (fr, lr) = (e.tr(first), e.tr(last));
            if need {
                if e.same_line(or, fr) {
                    let fix = if c_first { None } else { ins(or.1, "\n") };
                    out.push(rep(or, "Expected a line break after this opening brace.", fix));
                }
                if e.same_line(lr, cr) {
                    let fix = if c_last { None } else { ins(cr.0, "\n") };
                    out.push(rep(cr, "Expected a line break before this closing brace.", fix));
                }
            } else {
                let lb_open = !e.same_line(or, fr);
                let lb_close = !e.same_line(lr, cr);
                if (!o.consistent && lb_open) || (o.consistent && lb_open && !lb_close) {
                    let fix = if c_first { None } else { del(or.1, fr.0) };
                    out.push(rep(or, "Unexpected line break after this opening brace.", fix));
                }
                if (!o.consistent && lb_close) || (o.consistent && !lb_open && lb_close) {
                    let fix = if c_last { None } else { del(lr.1, cr.0) };
                    out.push(rep(cr, "Unexpected line break before this closing brace.", fix));
                }
            }
        }
        emit(ctx, &e, out, false);
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let v = opts.get(0);
    let specific = v
        .as_object()
        .is_some_and(|m| m.values().any(|x| x.is_object() || x.is_string()));
    let (expr, pat, type_lit) = if specific {
        (
            value(v.get("ObjectExpression")),
            value(v.get("ObjectPattern")),
            value(v.get("TSTypeLiteral")),
        )
    } else {
        let x = value(Some(v));
        (x, x, x)
    };
    Some(Box::new(ObjectCurlyNewline {
        expr,
        pat,
        type_lit,
    }))
}
