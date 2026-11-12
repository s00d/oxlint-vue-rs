//! vue/object-curly-spacing

use serde_json::Value;

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::indent::tree::{F, Ty};
use crate::utils::tpl_ext::*;

struct ObjectCurlySpacing {
    spaced: bool,
    arrays_exc: bool,
    objects_exc: bool,
    overrides: Value,
    empty: String,
}

impl ObjectCurlySpacing {
    fn validate(&self, e: &Ext, ty: Ty, open: u32, close: u32, out: &mut Vec<Report>) {
        let (or, cr) = (e.tr(open), e.tr(close));
        let spaced = match self.overrides.get(ty.name()).and_then(Value::as_str) {
            Some(s) if !s.is_empty() => s == "always",
            _ => self.spaced,
        };
        let ov = e.val(open);
        let cv = e.val(close);
        if let Some(tao) = e.after_c(or)
            && e.same_line(or, e.tr(tao))
        {
            let tr = e.tr(tao);
            let first_sp = e.space_between(or, tr);
            let second = e.node_by_index(tr.0).map(|n| e.ty(n));
            let must = if self.arrays_exc
                && matches!(second, Some(Ty::TSMappedType | Ty::TSIndexSignature))
            {
                !spaced
            } else {
                spaced
            };
            if must && !first_sp {
                out.push(rep(or, format!("A space is required after '{ov}'."), ins(or.1, " ")));
            }
            if !must && first_sp && !(e.is_comment(tao) && !e.same_line(or, cr)) {
                out.push(rep((or.1, tr.0), format!("There should be no space after '{ov}'."), del(or.1, tr.0)));
            }
        }
        if let Some(tbc) = e.before_c(cr)
            && e.same_line(e.tr(tbc), cr)
        {
            let br = e.tr(tbc);
            let pen = if (self.arrays_exc && e.is_punct(tbc, "]"))
                || (self.objects_exc && e.is_punct(tbc, "}"))
            {
                e.node_by_index(br.0).map(|n| e.ty(n))
            } else {
                None
            };
            let must = if (self.arrays_exc && matches!(pen, Some(Ty::ArrayExpression | Ty::TSTupleType)))
                || (self.objects_exc
                    && matches!(
                        pen,
                        Some(Ty::ObjectExpression | Ty::ObjectPattern | Ty::TSMappedType | Ty::TSTypeLiteral)
                    ))
            {
                !spaced
            } else {
                spaced
            };
            let last_sp = e.space_between(br, cr);
            if must && !last_sp {
                out.push(rep(cr, format!("A space is required before '{cv}'."), ins(cr.0, " ")));
            }
            if !must && last_sp {
                out.push(rep((br.1, cr.0), format!("There should be no space before '{cv}'."), del(br.1, cr.0)));
            }
        }
    }
    fn empty(&self, e: &Ext, ty: Ty, open: u32, close: u32, out: &mut Vec<Report>) {
        let (or, cr) = (e.tr(open), e.tr(close));
        if self.empty == "ignore" || !e.same_line(or, cr) || e.comments_between(or, cr) {
            return;
        }
        let between = e.text((or.0 + 1, cr.1 - 1));
        if !crate::utils::tpl_style::js_trim(between).is_empty() {
            return;
        }
        let name = ty.name();
        if self.empty == "always" {
            if between == " " {
                return;
            }
            out.push(rep((or.1, cr.0), format!("A space is required in empty '{name}'."), repl(or.1, cr.0, " ")));
        } else if self.empty == "never" {
            if between.is_empty() {
                return;
            }
            out.push(rep((or.1, cr.0), format!("There should be no space in empty '{name}'."), del(or.1, cr.0)));
        }
    }
}

impl Rule for ObjectCurlySpacing {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let Some(e) = Ext::new(ctx, false) else { return };
        let mut out = Vec::new();
        for n in e.nodes(true) {
            let ty = e.ty(n);
            let r = e.range(n);
            let field = match ty {
                Ty::ObjectPattern | Ty::ObjectExpression => F::Properties,
                Ty::TSTypeLiteral => F::Members,
                Ty::TSMappedType => {
                    if let (Some(a), Some(b)) = (e.first(r), e.last(r)) {
                        self.validate(&e, ty, a, b, &mut out);
                    }
                    continue;
                }
                _ => continue,
            };
            let toks = e.tokens(r);
            let open = toks.iter().copied().find(|&t| e.is_punct(t, "{"));
            let close = match e.get(n, F::TypeAnnotation) {
                Some(ta) if ty == Ty::ObjectPattern => e.before(e.range(ta)),
                _ => toks.iter().rev().copied().find(|&t| e.is_punct(t, "}")),
            };
            let (Some(open), Some(close)) = (open, close) else {
                continue;
            };
            if e.list(n, field).is_empty() {
                self.empty(&e, ty, open, close, &mut out);
            } else {
                self.validate(&e, ty, open, close, &mut out);
            }
        }
        emit(ctx, &e, out, false);
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let spaced = opt_str(opts, 0, "never") == "always";
    let second = opts.get(1);
    let set = |k: &str| second.get(k).and_then(Value::as_bool) == Some(!spaced);
    Some(Box::new(ObjectCurlySpacing {
        spaced,
        arrays_exc: set("arraysInObjects"),
        objects_exc: set("objectsInObjects"),
        overrides: second.get("overrides").cloned().unwrap_or(Value::Null),
        empty: second
            .get("emptyObjects")
            .and_then(Value::as_str)
            .unwrap_or("ignore")
            .to_string(),
    }))
}
