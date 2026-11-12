//! vue/key-spacing

use serde_json::Value;

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::indent::tree::{F, Ty, flag};
use crate::utils::tpl_ext::*;

/// One `initOptionProperty` result. Numbers may be NaN (`+undefined` of a raw `align` object).
#[derive(Clone)]
struct Line {
    mode: String,
    before: f64,
    after: f64,
}

#[derive(Clone)]
struct Align {
    on: String,
    mode: String,
    before: f64,
    after: f64,
}

struct KeySpacing {
    single: Line,
    multi: Line,
    align: Option<Align>,
    ignored: Vec<String>,
}

fn num(v: Option<&Value>, default: f64) -> f64 {
    match v {
        None => default,
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Value::Null) => 0.0,
        Some(_) => f64::NAN,
    }
}

fn str_or<'v>(v: Option<&'v Value>, d: &'v str) -> &'v str {
    v.and_then(Value::as_str).filter(|s| !s.is_empty()).unwrap_or(d)
}

/// `initOptionProperty`: returns the line options and the raw/derived `align`.
fn init_prop(from: &Value) -> (Line, Option<Align>) {
    let line = Line {
        mode: str_or(from.get("mode"), "strict").to_string(),
        before: num(from.get("beforeColon"), 0.0),
        after: num(from.get("afterColon"), 1.0),
    };
    let align = match from.get("align") {
        None => None,
        Some(a) if a.is_object() => Some(Align {
            on: a.get("on").and_then(Value::as_str).unwrap_or("").to_string(),
            mode: a.get("mode").and_then(Value::as_str).unwrap_or("").to_string(),
            before: num(a.get("beforeColon"), f64::NAN),
            after: num(a.get("afterColon"), f64::NAN),
        }),
        Some(a) => Some(Align {
            on: a.as_str().unwrap_or("").to_string(),
            mode: line.mode.clone(),
            before: line.before,
            after: line.after,
        }),
    };
    (line, align)
}

fn truthy(n: f64) -> bool {
    n != 0.0 && !n.is_nan()
}

/// `getStringLength` (ponytail: code points, not grapheme clusters).
fn str_len(s: &str) -> usize {
    s.chars().count()
}

impl KeySpacing {
    fn is_key_value(&self, e: &Ext, p: u32) -> bool {
        e.ty(p) == Ty::Property && !e.prop_method_or_accessor(p) && !e.prop_shorthand(p)
    }
    fn colon(&self, e: &Ext, key: u32) -> Option<u32> {
        e.after_f(e.range(key), |t| e.is_punct(t, ":"))
    }
    fn whitespace(&self, e: &Ext, p: u32) -> Option<(String, String)> {
        let (k, v) = (e.get(p, F::Key)?, e.get(p, F::Value)?);
        let (a, b) = (e.range(k).1, e.range(v).0);
        if a >= b {
            return None;
        }
        let t = e.text((a, b));
        let i = t.find(':')?;
        let before: String = t[..i]
            .chars()
            .rev()
            .take_while(|&c| crate::utils::tpl_style::is_js_ws(c))
            .collect();
        let after: String = t[i + 1..]
            .chars()
            .take_while(|&c| crate::utils::tpl_style::is_js_ws(c))
            .collect();
        Some((before, after))
    }
    fn key_text(&self, e: &Ext, p: u32) -> String {
        let Some(k) = e.get(p, F::Key) else {
            return "null".into();
        };
        if e.tree.has(p, flag::COMPUTED) {
            return e.text(e.range(k)).to_string();
        }
        e.static_key_name(p).unwrap_or_else(|| "null".into())
    }
    fn report(&self, e: &Ext, p: u32, key_side: bool, ws: &str, (expected, mode): (f64, &str), out: &mut Vec<Report>) {
        let wl = ws.encode_utf16().count() as f64;
        let diff = wl - expected;
        let cond = (truthy(diff) && mode == "strict")
            || (diff < 0.0 && mode == "minimum")
            || (diff > 0.0 && !truthy(expected) && mode == "minimum");
        if !cond || (truthy(expected) && has_linebreak(ws)) {
            return;
        }
        let Some(k) = e.get(p, F::Key) else { return };
        let Some(colon) = self.colon(e, k) else { return };
        let cr = e.tr(colon);
        let (Some(tb), Some(ta)) = (e.before_c(cr), e.after_c(cr)) else {
            return;
        };
        let (tbr, tar) = (e.tr(tb), e.tr(ta));
        let extra = diff > 0.0;
        let n = diff.abs() as usize;
        let loc = if extra {
            if key_side { (tbr.1, cr.0) } else { (cr.0, tar.0) }
        } else if key_side {
            tbr
        } else {
            tar
        };
        let fix = if extra {
            if key_side {
                let len: usize = e.text((tbr.1, cr.0)).chars().take(n).map(char::len_utf8).sum();
                del(tbr.1, tbr.1 + len as u32)
            } else {
                let len: usize = e.text((cr.1, tar.0)).chars().rev().take(n).map(char::len_utf8).sum();
                del(tar.0 - len as u32, tar.0)
            }
        } else if key_side {
            ins(tbr.1, &" ".repeat(n))
        } else {
            ins(tar.0, &" ".repeat(n))
        };
        let computed = if e.tree.has(p, flag::COMPUTED) { "computed " } else { "" };
        let key = self.key_text(e, p);
        let msg = match (extra, key_side) {
            (true, true) => format!("Extra space after {computed}key '{key}'."),
            (true, false) => format!("Extra space before value for {computed}key '{key}'."),
            (false, true) => format!("Missing space after {computed}key '{key}'."),
            (false, false) => format!("Missing space before value for {computed}key '{key}'."),
        };
        out.push(rep(loc, msg, fix));
    }
    fn verify(&self, e: &Ext, p: u32, line: &Line, out: &mut Vec<Report>) {
        if e
            .parent(p)
            .is_some_and(|q| self.ignored.iter().any(|i| i == e.ty(q).name()))
        {
            return;
        }
        if let Some((b, a)) = self.whitespace(e, p) {
            self.report(e, p, true, &b, (line.before, &line.mode), out);
            self.report(e, p, false, &a, (line.after, &line.mode), out);
        }
    }
    fn continues_group(&self, e: &Ext, last: u32, cand: u32) -> bool {
        let end_line = e.line(e.range(last).0) as i64;
        let vstart = if self.is_key_value(e, cand) {
            e.get(cand, F::Key)
                .and_then(|k| self.colon(e, k))
                .and_then(|c| e.after(e.tr(c)))
                .map_or(e.range(cand).0, |t| e.tr(t).0)
        } else {
            e.range(cand).0
        };
        let vline = e.line(vstart) as i64;
        if vline - end_line <= 1 {
            return true;
        }
        // getCommentsBefore(candidate)
        let mut comments = Vec::new();
        let mut cur = e.range(cand);
        while let Some(t) = e.before_c(cur) {
            if !e.store.tok(t).comment {
                break;
            }
            comments.push(e.tr(t));
            cur = e.tr(t);
        }
        comments.reverse();
        if let (Some(f), Some(l)) = (comments.first(), comments.last())
            && e.line(f.0) as i64 - end_line <= 1
            && vline - e.line(l.1) as i64 <= 1
        {
            return comments
                .windows(2)
                .all(|w| e.line(w[1].0) as i64 - e.line(w[0].1) as i64 <= 1);
        }
        false
    }
    fn group_alignment(&self, e: &Ext, props: &[u32], al: &Align, out: &mut Vec<Report>) {
        let widths: Vec<f64> = props
            .iter()
            .map(|&p| {
                let st = e.first(e.range(p));
                let en = e
                    .get(p, F::Key)
                    .and_then(|k| self.colon(e, k))
                    .and_then(|c| e.before(e.tr(c)));
                match (st, en) {
                    (Some(s), Some(t)) => str_len(e.text((e.tr(s).0, e.tr(t).1))) as f64,
                    _ => 0.0,
                }
            })
            .collect();
        let mut target = widths.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let (before, after, mode) = if props.len() > 1 {
            (al.before, al.after, al.mode.as_str())
        } else {
            (self.multi.before, self.multi.after, al.mode.as_str())
        };
        target += if al.on == "colon" { before } else { after };
        for (i, &p) in props.iter().enumerate() {
            if let Some((b, a)) = self.whitespace(e, p) {
                let w = widths[i];
                if al.on == "value" {
                    self.report(e, p, true, &b, (before, mode), out);
                    self.report(e, p, false, &a, (target - w, mode), out);
                } else {
                    self.report(e, p, true, &b, (target - w, mode), out);
                    self.report(e, p, false, &a, (after, mode), out);
                }
            }
        }
    }
}

impl Rule for KeySpacing {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let Some(e) = Ext::new(ctx, false) else { return };
        let mut out = Vec::new();
        for n in e.nodes(true) {
            match (&self.align, e.ty(n)) {
                (None, Ty::Property) => {
                    let single = e.parent(n).is_some_and(|q| e.single_line(e.range(q)));
                    let line = if single { &self.single } else { &self.multi };
                    self.verify(&e, n, line, &mut out);
                }
                (Some(al), Ty::ObjectExpression) => {
                    if self.ignored.iter().any(|i| i == "ObjectExpression") {
                        continue;
                    }
                    let props = e.list(n, F::Properties);
                    if e.single_line(e.range(n)) {
                        for &p in props.iter().filter(|&&p| self.is_key_value(&e, p)) {
                            self.verify(&e, p, &self.single, &mut out);
                        }
                        continue;
                    }
                    // createGroups
                    let mut groups: Vec<Vec<u32>> = vec![Vec::new()];
                    if props.len() == 1 {
                        groups = vec![props.to_vec()];
                    } else {
                        for &p in props {
                            let cur = groups.last_mut().unwrap();
                            match cur.last() {
                                Some(&prev) if !self.continues_group(&e, prev, p) => {
                                    groups.push(vec![p]);
                                }
                                _ => cur.push(p),
                            }
                        }
                    }
                    for g in groups {
                        let kv: Vec<u32> =
                            g.into_iter().filter(|&p| self.is_key_value(&e, p)).collect();
                        let single = match (kv.first(), kv.last()) {
                            (Some(&f), Some(&l)) => e.line(e.range(l).1) == e.line(e.range(f).0),
                            _ => false,
                        };
                        if single {
                            for &p in &kv {
                                self.verify(&e, p, &self.multi, &mut out);
                            }
                        } else {
                            self.group_alignment(&e, &kv, al, &mut out);
                        }
                    }
                }
                // ponytail: TS type-literal / class-body members (`validateBody`) are not
                // checked; they do not occur in template expressions in practice.
                _ => {}
            }
        }
        emit(ctx, &e, out, false);
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let from = opts.get(0);
    let empty = Value::Object(Default::default());
    let from = if from.is_object() { from } else { &empty };
    let sub = |k: &str| from.get(k).filter(|v| !v.is_null() && v.as_bool() != Some(false));
    let (multi, m_align) = init_prop(sub("multiLine").unwrap_or(from));
    let (single, _) = init_prop(sub("singleLine").unwrap_or(from));
    let align = match from.get("align") {
        Some(a) if a.is_object() => {
            let (l, _) = init_prop(a);
            Some(Align {
                on: str_or(a.get("on"), "colon").to_string(),
                mode: str_or(a.get("mode"), "strict").to_string(),
                before: l.before,
                after: l.after,
            })
        }
        _ => m_align.map(|a| Align {
            mode: if a.mode.is_empty() { multi.mode.clone() } else { a.mode },
            ..a
        }),
    };
    let ignored = from
        .get("ignoredNodes")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    Some(Box::new(KeySpacing {
        single,
        multi,
        align,
        ignored,
    }))
}
