//! vue/object-property-newline

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::indent::tree::{F, Ty};
use crate::utils::tpl_ext::*;

struct ObjectPropertyNewline {
    allow_same_line: bool,
}

impl Rule for ObjectPropertyNewline {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let Some(e) = Ext::new(ctx, false) else { return };
        let msg = if self.allow_same_line {
            "Object properties must go on a new line if they aren't all on the same line."
        } else {
            "Object properties must go on a new line."
        };
        let mut out = Vec::new();
        for n in e.nodes(true) {
            let kids = match e.ty(n) {
                Ty::ObjectExpression => e.list(n, F::Properties),
                Ty::TSTypeLiteral => e.list(n, F::Members),
                Ty::TSInterfaceBody => e.list(n, F::Body),
                _ => continue,
            };
            if self.allow_same_line
                && kids.len() > 1
                && let (Some(a), Some(b)) = (
                    e.first(e.range(kids[0])),
                    e.last(e.range(kids[kids.len() - 1])),
                )
                && e.same_line(e.tr(a), e.tr(b))
            {
                continue;
            }
            for i in 1..kids.len() {
                let (Some(lp), Some(fc)) = (e.last(e.range(kids[i - 1])), e.first(e.range(kids[i])))
                else {
                    continue;
                };
                let fr = e.tr(fc);
                if e.same_line(e.tr(lp), fr) {
                    let fix = e.before(fr).and_then(|b| {
                        let a = e.tr(b).1;
                        if crate::utils::tpl_style::js_trim(e.text((a, fr.0))).is_empty() {
                            repl(a, fr.0, "\n")
                        } else {
                            None
                        }
                    });
                    out.push(rep(fr, msg, fix));
                }
            }
        }
        emit(ctx, &e, out, false);
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(ObjectPropertyNewline {
        allow_same_line: opt_bool(opts, 0, "allowAllPropertiesOnSameLine").unwrap_or(false),
    }))
}
