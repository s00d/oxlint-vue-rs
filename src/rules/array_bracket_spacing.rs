//! vue/array-bracket-spacing

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::indent::tree::{F, Ty};
use crate::utils::tpl_ext::*;

struct ArrayBracketSpacing {
    spaced: bool,
    single: bool,
    objects: bool,
    arrays: bool,
}

impl Rule for ArrayBracketSpacing {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let Some(e) = Ext::new(ctx, false) else { return };
        let mut out = Vec::new();
        for n in e.nodes(true) {
            if !matches!(e.ty(n), Ty::ArrayExpression | Ty::ArrayPattern) {
                continue;
            }
            let els = e.list(n, F::Elements);
            if self.spaced && els.is_empty() {
                continue;
            }
            let r = e.range(n);
            let (Some(first), Some(second)) = (e.first(r), e.first_skip(r, 1)) else {
                continue;
            };
            let last = match e.get(n, F::TypeAnnotation) {
                Some(ta) if e.ty(n) == Ty::ArrayPattern => e.before(e.range(ta)),
                _ => e.last(r),
            };
            let Some(last) = last else { continue };
            let Some(pen) = e.before(e.tr(last)) else {
                continue;
            };
            let is_obj = |c: Option<&u32>| {
                c.is_some_and(|&c| {
                    c != u32::MAX && matches!(e.ty(c), Ty::ObjectExpression | Ty::ObjectPattern)
                })
            };
            let is_arr = |c: Option<&u32>| {
                c.is_some_and(|&c| {
                    c != u32::MAX && matches!(e.ty(c), Ty::ArrayExpression | Ty::ArrayPattern)
                })
            };
            let (fe, le) = (els.first(), els.last());
            let open_sp = if (self.objects && is_obj(fe))
                || (self.arrays && is_arr(fe))
                || (self.single && els.len() == 1)
            {
                !self.spaced
            } else {
                self.spaced
            };
            let close_sp = if (self.objects && is_obj(le))
                || (self.arrays && is_arr(le))
                || (self.single && els.len() == 1)
            {
                !self.spaced
            } else {
                self.spaced
            };
            let (f, s, l, p) = (e.tr(first), e.tr(second), e.tr(last), e.tr(pen));
            let fv = e.val(first);
            let lv = e.val(last);
            if e.same_line(f, s) {
                let sp = e.space_between(f, s);
                if open_sp && !sp {
                    out.push(rep(f, format!("A space is required after '{fv}'."), ins(f.1, " ")));
                }
                if !open_sp && sp {
                    out.push(rep((f.1, s.0), format!("There should be no space after '{fv}'."), del(f.1, s.0)));
                }
            }
            if first != pen && e.same_line(p, l) {
                let sp = e.space_between(p, l);
                if close_sp && !sp {
                    out.push(rep(l, format!("A space is required before '{lv}'."), ins(l.0, " ")));
                }
                if !close_sp && sp {
                    out.push(rep((p.1, l.0), format!("There should be no space before '{lv}'."), del(p.1, l.0)));
                }
            }
        }
        emit(ctx, &e, out, false);
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let spaced = opt_str(opts, 0, "never") == "always";
    let set = |k: &str| opt_bool(opts, 1, k) == Some(!spaced);
    Some(Box::new(ArrayBracketSpacing {
        spaced,
        single: set("singleValue"),
        objects: set("objectsInArrays"),
        arrays: set("arraysInArrays"),
    }))
}
