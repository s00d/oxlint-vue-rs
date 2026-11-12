//! vue/space-unary-ops

use std::collections::HashMap;

use crate::ast::TokenKind;
use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::indent::tree::{F, Ty, flag};
use crate::utils::tpl_ext::*;

struct SpaceUnaryOps {
    words: bool,
    nonwords: bool,
    overrides: HashMap<String, bool>,
}

/// `canTokensBeAdjacent` for two tokens.
fn can_be_adjacent(e: &Ext, l: u32, r: u32) -> bool {
    use TokenKind::*;
    let (lk, rk) = (e.kind(l), e.kind(r));
    let (lv, rv) = (e.val(l), e.val(r));
    if lk == Punctuator || rk == Punctuator {
        if lk == Punctuator && rk == Punctuator {
            let plus = |v: &str| v == "+" || v == "++";
            let minus = |v: &str| v == "-" || v == "--";
            return !((plus(lv) && plus(rv)) || (minus(lv) && minus(rv)));
        }
        if lk == Punctuator && lv == "/" {
            return !matches!(rk, BlockComment | LineComment | RegularExpression);
        }
        return true;
    }
    if matches!(lk, String | Template) || matches!(rk, String | Template) {
        return true;
    }
    if lk != Numeric && rk == Numeric && rv.starts_with('.') {
        return true;
    }
    if lk == BlockComment || rk == BlockComment || rk == LineComment {
        return true;
    }
    rk == PrivateIdentifier
}

impl SpaceUnaryOps {
    fn word(&self, e: &Ext, n: u32, f: u32, s: u32, word: &str, out: &mut Vec<Report>) {
        let (fr, sr) = (e.tr(f), e.tr(s));
        if self.overrides.get(word).copied().unwrap_or(self.words) {
            if sr.0 == fr.1 {
                out.push(rep(e.range(n), format!("Unary word operator '{word}' must be followed by whitespace."), ins(fr.1, " ")));
            }
        } else if can_be_adjacent(e, f, s) && sr.0 > fr.1 {
            out.push(rep(e.range(n), format!("Unexpected space after unary word operator '{word}'."), del(fr.1, sr.0)));
        }
    }
    fn have(&self, e: &Ext, n: u32, prefix: bool, f: u32, s: u32, out: &mut Vec<Report>) {
        let (fr, sr) = (e.tr(f), e.tr(s));
        if prefix {
            // isFirstBangInBangBangExpression
            if e.ty(n) == Ty::UnaryExpression
                && e.get(n, F::Argument).is_some_and(|a| {
                    e.ty(a) == Ty::UnaryExpression
                        && e.first(e.range(a)).is_some_and(|t| e.val(t) == "!")
                })
            {
                return;
            }
            if fr.1 == sr.0 {
                out.push(rep(e.range(n), format!("Unary operator '{}' must be followed by whitespace.", e.val(f)), ins(fr.1, " ")));
            }
        } else if fr.1 == sr.0 {
            out.push(rep(e.range(n), format!("Space is required before unary operator '{}'.", e.val(s)), ins(sr.0, " ")));
        }
    }
    fn dont(&self, e: &Ext, n: u32, prefix: bool, f: u32, s: u32, out: &mut Vec<Report>) {
        let (fr, sr) = (e.tr(f), e.tr(s));
        if prefix {
            if sr.0 > fr.1 {
                let fix = if can_be_adjacent(e, f, s) { del(fr.1, sr.0) } else { None };
                out.push(rep(e.range(n), format!("Unexpected space after unary operator '{}'.", e.val(f)), fix));
            }
        } else if sr.0 > fr.1 {
            out.push(rep(e.range(n), format!("Unexpected space before unary operator '{}'.", e.val(s)), del(fr.1, sr.0)));
        }
    }
}

impl Rule for SpaceUnaryOps {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let Some(e) = Ext::new(ctx, true) else { return };
        let mut out = Vec::new();
        for n in e.nodes(true) {
            let r = e.range(n);
            match e.ty(n) {
                Ty::UnaryExpression | Ty::UpdateExpression | Ty::NewExpression => {
                    let ty = e.ty(n);
                    let prefix = match ty {
                        Ty::UnaryExpression => true,
                        Ty::UpdateExpression => e
                            .first(r)
                            .is_some_and(|t| matches!(e.val(t), "++" | "--")),
                        _ => false,
                    };
                    let toks: Vec<u32> = if ty == Ty::UpdateExpression && !prefix {
                        let mut v: Vec<u32> = e.store.bwd(r.0 as i64, r.1 as i64, false).take(2).collect();
                        v.reverse();
                        v
                    } else {
                        e.store.fwd(r.0 as i64, r.1 as i64, false).take(2).collect()
                    };
                    let [f, s] = toks[..] else { continue };
                    if (ty == Ty::NewExpression || prefix) && e.kind(f) == TokenKind::Keyword {
                        self.word(&e, n, f, s, e.val(f), &mut out);
                        continue;
                    }
                    let key = if prefix { e.val(f) } else { e.val(s) };
                    if self.overrides.get(key).copied().unwrap_or(self.nonwords) {
                        self.have(&e, n, prefix, f, s, &mut out);
                    } else {
                        self.dont(&e, n, prefix, f, s, &mut out);
                    }
                }
                Ty::YieldExpression => {
                    if e.get(n, F::Argument).is_none() || e.tree.has(n, flag::DELEGATE) {
                        continue;
                    }
                    if let (Some(f), Some(s)) = (e.first(r), e.first_skip(r, 1)) {
                        self.word(&e, n, f, s, "yield", &mut out);
                    }
                }
                Ty::AwaitExpression => {
                    if let (Some(f), Some(s)) = (e.first(r), e.first_skip(r, 1)) {
                        self.word(&e, n, f, s, "await", &mut out);
                    }
                }
                Ty::TSNonNullExpression => {
                    let Some(op) = e.last_f(r, |t| e.val(t) == "!") else { continue };
                    let Some(p) = e.before(e.tr(op)) else { continue };
                    let have = self
                        .overrides
                        .get("ts-non-null")
                        .or_else(|| self.overrides.get("!"))
                        .copied()
                        .unwrap_or(self.nonwords);
                    if have {
                        self.have(&e, n, false, p, op, &mut out);
                    } else {
                        self.dont(&e, n, false, p, op, &mut out);
                    }
                }
                _ => {}
            }
        }
        emit(ctx, &e, out, false);
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let o = opts.get(0);
    let overrides = o
        .get("overrides")
        .and_then(|v| v.as_object())
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| v.as_bool().map(|b| (k.clone(), b)))
                .collect()
        })
        .unwrap_or_default();
    Some(Box::new(SpaceUnaryOps {
        words: o.get("words").and_then(|v| v.as_bool()).unwrap_or(true),
        nonwords: o.get("nonwords").and_then(|v| v.as_bool()).unwrap_or(false),
        overrides,
    }))
}
