//! vue/operator-linebreak

use std::collections::HashMap;

use crate::ast::TokenKind;
use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::indent::tree::{F, Ty};
use crate::utils::tpl_ext::*;

struct OperatorLinebreak {
    global: String,
    overrides: HashMap<String, String>,
}

fn strip_linebreaks(s: &str) -> String {
    s.replace("\r\n", "")
        .replace(['\r', '\n', '\u{2028}', '\u{2029}'], "")
}

impl OperatorLinebreak {
    fn fixer(&self, e: &Ext, op: u32, style: &str) -> Option<Fix> {
        let or = e.tr(op);
        let (tb, ta) = (e.before(or)?, e.after(or)?);
        let (tbr, tar) = (e.tr(tb), e.tr(ta));
        let text_before = e.text((tbr.1, or.0));
        let text_after = e.text((or.1, tar.0));
        let lb_before = !e.same_line(tbr, or);
        let lb_after = !e.same_line(or, tar);
        let (nb, mut na);
        if lb_before != lb_after && style != "none" {
            if e.before_c(or) != Some(tb) && e.after_c(or) != Some(ta) {
                return None;
            }
            nb = text_after.to_string();
            na = text_before.to_string();
        } else {
            let trim = crate::utils::tpl_style::js_trim;
            nb = if style == "before" || !trim(text_before).is_empty() {
                text_before.to_string()
            } else {
                strip_linebreaks(text_before)
            };
            na = if style == "after" || !trim(text_after).is_empty() {
                text_after.to_string()
            } else {
                strip_linebreaks(text_after)
            };
            if nb == text_before && na == text_after {
                return None;
            }
        }
        let ov = e.val(op);
        if na.is_empty()
            && e.kind(ta) == TokenKind::Punctuator
            && (ov == "+" || ov == "-")
            && e.val(ta) == ov
        {
            na.push(' ');
        }
        repl(tbr.1, tar.0, format!("{nb}{ov}{na}"))
    }

    fn validate(&self, e: &Ext, right: u32, operator: &str, out: &mut Vec<Report>) {
        let Some(op) = e.before_f(e.range(right), |t| e.val(t) == operator) else {
            return;
        };
        let or = e.tr(op);
        let (Some(l), Some(r)) = (e.before(or), e.after(or)) else {
            return;
        };
        let ov = self.overrides.get(operator).map(String::as_str);
        let style = ov.unwrap_or(&self.global);
        let (ls, rs) = (e.same_line(e.tr(l), or), e.same_line(or, e.tr(r)));
        let msg = if ls && rs {
            return;
        } else if ov != Some("ignore") && !ls && !rs {
            format!("Bad line breaking before and after '{operator}'.")
        } else if style == "before" && ls {
            format!("'{operator}' should be placed at the beginning of the line.")
        } else if style == "after" && rs {
            format!("'{operator}' should be placed at the end of the line.")
        } else if style == "none" {
            format!("There should be no line break before or after '{operator}'.")
        } else {
            return;
        };
        out.push(rep(or, msg, self.fixer(e, op, style)));
    }
}

impl Rule for OperatorLinebreak {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let Some(e) = Ext::new(ctx, false) else { return };
        let mut out = Vec::new();
        for n in e.nodes(false) {
            match e.ty(n) {
                Ty::BinaryExpression | Ty::LogicalExpression | Ty::AssignmentExpression => {
                    if let (Some(op), Some(r)) = (e.operator_token(n), e.get(n, F::Right)) {
                        let o = e.val(op);
                        self.validate(&e, r, o, &mut out);
                    }
                }
                Ty::VariableDeclarator => {
                    if let Some(i) = e.get(n, F::Init) {
                        self.validate(&e, i, "=", &mut out);
                    }
                }
                Ty::PropertyDefinition | Ty::AccessorProperty => {
                    if let Some(v) = e.get(n, F::Value) {
                        self.validate(&e, v, "=", &mut out);
                    }
                }
                Ty::ConditionalExpression => {
                    if let Some(c) = e.get(n, F::Consequent) {
                        self.validate(&e, c, "?", &mut out);
                    }
                    if let Some(a) = e.get(n, F::Alternate) {
                        self.validate(&e, a, ":", &mut out);
                    }
                }
                Ty::TSConditionalType => {
                    if let Some(c) = e.get(n, F::TrueType) {
                        self.validate(&e, c, "?", &mut out);
                    }
                    if let Some(a) = e.get(n, F::FalseType) {
                        self.validate(&e, a, ":", &mut out);
                    }
                }
                Ty::TSIntersectionType | Ty::TSUnionType => {
                    let op = if e.ty(n) == Ty::TSUnionType { "|" } else { "&" };
                    let ts = e.list(n, F::Types);
                    for w in ts.windows(2) {
                        self.validate(&e, w[1], op, &mut out);
                    }
                }
                Ty::TSTypeParameter => {
                    if let Some(d) = e.get(n, F::Default) {
                        self.validate(&e, d, "=", &mut out);
                    }
                }
                _ => {}
            }
        }
        emit(ctx, &e, out, false);
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let first = opts.get(0).as_str().filter(|s| !s.is_empty());
    let mut overrides: HashMap<String, String> = opts
        .get(1)
        .get("overrides")
        .and_then(|v| v.as_object())
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();
    if first.is_none() {
        for k in ["?", ":"] {
            if overrides.get(k).is_none_or(|s| s.is_empty()) {
                overrides.insert(k.to_string(), "before".to_string());
            }
        }
    }
    Some(Box::new(OperatorLinebreak {
        global: first.unwrap_or("after").to_string(),
        overrides,
    }))
}
