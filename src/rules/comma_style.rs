//! vue/comma-style

use std::collections::HashMap;

use crate::ast::TokenKind;
use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::indent::tree::{F, NONE, Ty};
use crate::utils::tpl_ext::*;

struct CommaStyle {
    style: String,
    exceptions: HashMap<String, bool>,
}

impl CommaStyle {
    fn exc(&self, ty: Ty) -> bool {
        self.exceptions.get(ty.name()).copied().unwrap_or(false)
    }

    fn extract(&self, e: &Ext, n: u32, items: &[u32]) -> Vec<u32> {
        let comma = |t: u32| e.is_punct(t, ",");
        if items.is_empty() {
            return Vec::new();
        }
        let defined: Vec<u32> = items.iter().copied().filter(|&i| i != NONE).collect();
        let nr = e.range(n);
        if defined.is_empty() {
            return e.tokens(nr).into_iter().filter(|&t| comma(t)).collect();
        }
        let mut out = Vec::new();
        let mut prev = e.before(e.range(defined[0]));
        while let Some(p) = prev {
            if nr.0 > e.tr(p).0 {
                break;
            }
            if comma(p) {
                out.insert(0, p);
            } else if !e.is_punct(p, "(") {
                break;
            }
            prev = e.before(e.tr(p));
        }
        let mut prev_item: Option<u32> = None;
        for &it in &defined {
            if let Some(pi) = prev_item {
                out.extend(
                    e.between(e.range(pi), e.range(it))
                        .into_iter()
                        .filter(|&t| comma(t)),
                );
            }
            if let Some(l) = e.last(e.range(it)).filter(|&t| comma(t)) {
                out.push(l);
            }
            prev_item = Some(it);
        }
        let mut next = prev_item.and_then(|p| e.after(e.range(p)));
        while let Some(t) = next {
            if e.tr(t).1 > nr.1 {
                break;
            }
            if comma(t) {
                out.push(t);
            } else if !e.is_punct(t, ")") {
                break;
            }
            next = e.after(e.tr(t));
        }
        out
    }

    fn validate(&self, e: &Ext, n: u32, items: &[u32], out: &mut Vec<Report>) {
        for c in self.extract(e, n, items) {
            let cr = e.tr(c);
            let (Some(b), Some(a)) = (e.before(cr), e.after(cr)) else {
                continue;
            };
            if e.is_punct(b, "[") {
                continue;
            }
            if e.is_punct(b, ",")
                && e.before_f(e.tr(b), |t| !e.is_punct(t, ","))
                    .is_some_and(|t| e.is_punct(t, "["))
            {
                continue;
            }
            if e.is_punct(a, ",") && !e.same_line(cr, e.tr(a)) {
                continue;
            }
            let (br, ar) = (e.tr(b), e.tr(a));
            let fix = |style: &str| {
                let text = format!("{}{}", e.text((br.1, cr.0)), e.text((cr.1, ar.0)));
                let t = match style {
                    "between" => format!(",{}", remove_first_linebreak(&text)),
                    "first" => format!("{text},"),
                    "last" => format!(",{text}"),
                    _ => String::new(),
                };
                repl(br.1, ar.0, t)
            };
            let after_same = e.same_line(cr, ar);
            let before_same = e.same_line(br, cr);
            if after_same && before_same {
            } else if !after_same && !before_same {
                let comment = e
                    .after_c(cr)
                    .filter(|&t| e.store.tok(t).comment);
                let st = match comment {
                    Some(cm)
                        if e.kind(cm) == TokenKind::BlockComment && e.same_line(cr, e.tr(cm)) =>
                    {
                        self.style.as_str()
                    }
                    _ => "between",
                };
                out.push(rep(cr, "Bad line breaking before and after ','.", fix(st)));
            } else if self.style == "first" && !after_same {
                out.push(rep(cr, "',' should be placed first.", fix("first")));
            } else if self.style == "last" && after_same {
                out.push(rep(cr, "',' should be placed last.", fix("last")));
            }
        }
    }
}

fn remove_first_linebreak(s: &str) -> String {
    for (i, ch) in s.char_indices() {
        match ch {
            '\r' if s[i..].starts_with("\r\n") => return format!("{}{}", &s[..i], &s[i + 2..]),
            '\r' | '\n' | '\u{2028}' | '\u{2029}' => {
                return format!("{}{}", &s[..i], &s[i + ch.len_utf8()..]);
            }
            _ => {}
        }
    }
    s.to_string()
}

impl Rule for CommaStyle {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let Some(e) = Ext::new(ctx, false) else { return };
        let mut out = Vec::new();
        for n in e.nodes(false) {
            let ty = e.ty(n);
            let field = match ty {
                Ty::VariableDeclaration => F::Declarations,
                Ty::ObjectExpression | Ty::ObjectPattern => F::Properties,
                Ty::ArrayExpression | Ty::ArrayPattern => F::Elements,
                Ty::FunctionDeclaration
                | Ty::FunctionExpression
                | Ty::ArrowFunctionExpression
                | Ty::TSDeclareFunction
                | Ty::TSFunctionType
                | Ty::TSConstructorType
                | Ty::TSEmptyBodyFunctionExpression
                | Ty::TSMethodSignature
                | Ty::TSCallSignatureDeclaration
                | Ty::TSConstructSignatureDeclaration
                | Ty::TSTypeParameterDeclaration
                | Ty::TSTypeParameterInstantiation => F::Params,
                Ty::CallExpression | Ty::NewExpression => F::Arguments,
                Ty::SequenceExpression => F::Expressions,
                Ty::TSTypeLiteral | Ty::TSEnumBody => F::Members,
                Ty::TSTupleType => F::ElementTypes,
                Ty::TSIndexSignature => F::Parameters,
                Ty::TSInterfaceBody => F::Body,
                Ty::ImportExpression => {
                    if !self.exc(ty) {
                        let items = [
                            e.get(n, F::Source).unwrap_or(NONE),
                            e.get(n, F::Options).unwrap_or(NONE),
                        ];
                        self.validate(&e, n, &items, &mut out);
                    }
                    continue;
                }
                // The Vue wrapper: slot-scope params are checked like `FunctionExpression` params.
                Ty::VSlotScopeExpression => {
                    if !self.exc(Ty::FunctionExpression) {
                        self.validate(&e, n, e.list(n, F::Params), &mut out);
                    }
                    continue;
                }
                _ => continue,
            };
            if !self.exc(ty) {
                self.validate(&e, n, e.list(n, field), &mut out);
            }
        }
        emit(ctx, &e, out, false);
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let exceptions = opts
        .get(1)
        .get("exceptions")
        .and_then(|v| v.as_object())
        .map(|m| {
            m.iter()
                .map(|(k, v)| (k.clone(), v.as_bool().unwrap_or(false)))
                .collect()
        })
        .unwrap_or_default();
    Some(Box::new(CommaStyle {
        style: opt_str(opts, 0, "last").to_string(),
        exceptions,
    }))
}
