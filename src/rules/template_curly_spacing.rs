//! vue/template-curly-spacing

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::indent::tree::Ty;
use crate::utils::tpl_ext::*;

struct TemplateCurlySpacing {
    always: bool,
}

impl Rule for TemplateCurlySpacing {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let Some(e) = Ext::new(ctx, true) else { return };
        let mut out = Vec::new();
        for n in e.nodes(true) {
            if e.ty(n) != Ty::TemplateElement {
                continue;
            }
            let Some(t) = e.first(e.range(n)) else { continue };
            let tr = e.tr(t);
            let v = e.val(t);
            if v.starts_with('}')
                && let Some(p) = e.before_c(tr)
            {
                let pr = e.tr(p);
                let sp = e.space_between(pr, tr);
                if e.same_line(pr, tr) {
                    if self.always && !sp {
                        out.push(rep((tr.0, tr.0 + 1), "Expected space(s) before '}'.", ins(tr.0, " ")));
                    }
                    if !self.always && sp {
                        out.push(rep((pr.1, tr.0), "Unexpected space(s) before '}'.", del(pr.1, tr.0)));
                    }
                }
            }
            if v.ends_with("${")
                && let Some(nx) = e.after_c(tr)
            {
                let nr = e.tr(nx);
                let sp = e.space_between(tr, nr);
                if e.same_line(tr, nr) {
                    if self.always && !sp {
                        out.push(rep((tr.1 - 2, tr.1), "Expected space(s) after '${'.", ins(tr.1, " ")));
                    }
                    if !self.always && sp {
                        out.push(rep((tr.1, nr.0), "Unexpected space(s) after '${'.", del(tr.1, nr.0)));
                    }
                }
            }
        }
        emit(ctx, &e, out, false);
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(TemplateCurlySpacing {
        always: opt_str(opts, 0, "never") == "always",
    }))
}
