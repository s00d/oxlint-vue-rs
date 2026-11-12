//! vue/func-call-spacing (@stylistic `function-call-spacing`)

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::indent::tree::{F, Ty};
use crate::utils::tpl_ext::*;

struct FuncCallSpacing {
    never: bool,
    allow_newlines: bool,
    oc_before: bool,
    oc_after: bool,
}

/// `text.replace(/\/\*.*?\*\//gu, "")` (`.` stops at line terminators).
fn strip_block_comments(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("/*") {
        let after = &rest[i + 2..];
        let line_end = after
            .find(['\n', '\r', '\u{2028}', '\u{2029}'])
            .unwrap_or(after.len());
        match after[..line_end].find("*/") {
            Some(j) => {
                out.push_str(&rest[..i]);
                rest = &after[j + 2..];
            }
            None => {
                out.push_str(&rest[..i + 2]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn is_ws(c: char) -> bool {
    crate::utils::tpl_style::is_js_ws(c)
}

impl FuncCallSpacing {
    fn check(&self, e: &Ext, n: u32, l: (u32, u32), r: (u32, u32), optional: bool, out: &mut Vec<Report>) {
        let between = strip_block_comments(e.text((l.1, r.0)));
        let has_ws = between.chars().any(is_ws);
        let has_nl = has_ws && has_linebreak(&between);
        let loc = (l.1, r.0);
        let safe = |text: String| {
            if e.comments_between(l, r) {
                None
            } else {
                repl(l.1, r.0, text)
            }
        };
        let _ = n;
        if self.never {
            if has_ws {
                let t = if optional { "?." } else { "" };
                out.push(rep(loc, "Unexpected whitespace between function name and paren.", safe(t.into())));
            }
        } else if optional {
            let prefix = between.starts_with(is_ws);
            let suffix = between.ends_with(is_ws);
            let ok_prefix = if self.oc_before { prefix } else { !prefix };
            let ok_suffix = if self.oc_after { suffix } else { !suffix };
            let ok_nl = self.allow_newlines || !has_nl;
            if !ok_prefix || !ok_suffix || !ok_nl {
                let msg = if !ok_nl {
                    "Unexpected newline between function name and paren."
                } else if (!self.oc_before && prefix) || (!self.oc_after && suffix) {
                    "Unexpected whitespace between function name and paren."
                } else {
                    "Missing space between function name and paren."
                };
                let mut text = between.clone();
                if !self.allow_newlines {
                    text = text.replace("\r\n", " ").replace(['\r', '\n', '\u{2028}', '\u{2029}'], " ");
                }
                if !ok_prefix {
                    text = if self.oc_before {
                        format!(" {text}")
                    } else {
                        text.trim_start_matches(is_ws).to_string()
                    };
                }
                if !ok_suffix {
                    text = if self.oc_after {
                        format!("{text} ")
                    } else {
                        text.trim_end_matches(is_ws).to_string()
                    };
                }
                out.push(rep(loc, msg, safe(text)));
            }
        } else if !has_ws {
            out.push(rep(loc, "Missing space between function name and paren.", ins(r.0, " ")));
        } else if !self.allow_newlines && has_nl {
            out.push(rep(loc, "Unexpected newline between function name and paren.", safe(" ".into())));
        }
    }
}

impl Rule for FuncCallSpacing {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let Some(e) = Ext::new(ctx, true) else { return };
        let mut out = Vec::new();
        for n in e.nodes(true) {
            let r = e.range(n);
            match e.ty(n) {
                Ty::CallExpression | Ty::NewExpression => {
                    let Some(close) = e.last(r) else { continue };
                    let callee = e.get(n, F::TypeArguments).or_else(|| e.get(n, F::Callee));
                    let Some(lc) = callee.and_then(|c| e.last(e.range(c))) else {
                        continue;
                    };
                    let Some(open) =
                        e.first_between_f(e.tr(lc), e.tr(close), |t| e.is_punct(t, "("))
                    else {
                        continue;
                    };
                    if e.tr(open).1 >= r.1 {
                        continue;
                    }
                    let Some(left) = e.before_f(e.tr(open), |t| !e.is_punct(t, "?.")) else {
                        continue;
                    };
                    let optional = e.ty(n) == Ty::CallExpression
                        && e.first_between_f(e.tr(lc), e.tr(open), |t| e.is_punct(t, "?."))
                            .is_some();
                    self.check(&e, n, e.tr(left), e.tr(open), optional, &mut out);
                }
                Ty::ImportExpression => {
                    let Some(l) = e.first(r) else { continue };
                    let Some(rt) = e.after(e.tr(l)) else { continue };
                    self.check(&e, n, e.tr(l), e.tr(rt), false, &mut out);
                }
                _ => {}
            }
        }
        emit(ctx, &e, out, false);
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let c = opts.get(1);
    let oc = c.get("optionalChain");
    Some(Box::new(FuncCallSpacing {
        never: opt_str(opts, 0, "never") == "never",
        allow_newlines: c.get("allowNewlines").and_then(|v| v.as_bool()).unwrap_or(false),
        oc_before: oc.and_then(|o| o.get("before")).and_then(|v| v.as_bool()).unwrap_or(true),
        oc_after: oc.and_then(|o| o.get("after")).and_then(|v| v.as_bool()).unwrap_or(true),
    }))
}
