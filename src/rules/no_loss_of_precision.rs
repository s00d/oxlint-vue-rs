//! vue/no-loss-of-precision (core `no-loss-of-precision` in `<template>`)

use oxc::ast::ast::NumericLiteral;
use oxc::ast_visit::Visit;

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::tpl_core::{DOCUMENT, for_each_root, visit_root};

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(NoLossOfPrecision))
}

struct NoLossOfPrecision;

fn trim_leading_zeros(s: &str) -> &str {
    let t = s.trim_start_matches('0');
    if t.is_empty() { s } else { t }
}
fn trim_trailing_zeros(s: &str) -> &str {
    let t = s.trim_end_matches('0');
    if t.is_empty() { s } else { t }
}

/// `convertNumberToScientificNotation` → (coefficient, magnitude).
fn scientific(s: &str, as_float: bool) -> (String, i64) {
    let mut parts = s.split('e');
    let coef = parts.next().unwrap_or("");
    let (c, mut m) = if as_float || s.contains('.') {
        let t = trim_leading_zeros(coef);
        match t.find('.') {
            Some(0) => {
                let sig = trim_leading_zeros(&t[1..]);
                (sig.to_string(), sig.len() as i64 - t.len() as i64)
            }
            None => (t.to_string(), t.len() as i64 - 1),
            Some(i) => (t.replacen('.', "", 1), i as i64 - 1),
        }
    } else {
        let t = trim_leading_zeros(coef);
        (trim_trailing_zeros(t).to_string(), t.len() as i64 - 1)
    };
    if let Some(e) = parts.next() {
        m += e.parse::<i64>().unwrap_or(0);
    }
    (c, m)
}

fn loses_precision(raw: &str, value: f64) -> bool {
    let is_base_ten = !["0x", "0X", "0b", "0B", "0o", "0O"]
        .iter()
        .any(|p| raw.starts_with(p))
        && !(raw.len() > 1 && raw.starts_with('0') && raw[1..].bytes().all(|b| (b'0'..=b'7').contains(&b)));
    let raw = raw.replace('_', "");
    if !is_base_ten {
        let upper = raw.to_ascii_uppercase();
        let base = if upper.starts_with("0B") {
            2
        } else if upper.starts_with("0X") {
            16
        } else {
            8
        };
        // ponytail: values beyond u128 are not checked (JS prints them exactly).
        if value >= 2f64.powi(128) {
            return false;
        }
        let mut v = value as u128;
        let mut digits = String::new();
        loop {
            digits.insert(0, char::from_digit((v % base) as u32, base as u32).unwrap().to_ascii_uppercase());
            v /= base;
            if v == 0 {
                break;
            }
        }
        return !upper.ends_with(&digits);
    }
    let mut lower = raw.to_ascii_lowercase();
    // `.replace(/\.(?=e|$)/u, "")`
    if let Some(i) = lower.find('.')
        && (i + 1 == lower.len() || lower.as_bytes()[i + 1] == b'e')
    {
        lower.remove(i);
    }
    let (coef, mag) = scientific(&lower, false);
    if value == 0.0 {
        return !coef.bytes().all(|b| b == b'0');
    }
    let p = coef.len();
    if p > 100 {
        return true;
    }
    let stored = format!("{:.*e}", p - 1, value);
    let (sc, sm) = scientific(&stored, true);
    mag != sm || coef != sc
}

struct V<'c, 'f, 'a>(&'c mut Ctx<'f, 'a>);

impl<'b> Visit<'b> for V<'_, '_, '_> {
    crate::skip_ts_types!('b);
    fn visit_numeric_literal(&mut self, n: &NumericLiteral<'b>) {
        let raw = self.0.slice(n.span);
        if loses_precision(raw, n.value) {
            self.0.report_id(
                n.span,
                "noLossOfPrecision",
                "This number literal will lose precision at runtime.",
            );
        }
    }
}

impl Rule for NoLossOfPrecision {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        for_each_root(ctx, DOCUMENT, |ctx, root| visit_root(&mut V(ctx), root.expr));
    }
}

#[cfg(test)]
mod tests {
    use super::loses_precision as l;
    #[test]
    fn cases() {
        assert!(l("12345678901234567890", "12345678901234567890".parse().unwrap()));
        assert!(l("0.12345678901234567890", "0.12345678901234567890".parse().unwrap()));
        assert!(!l("12345", 12345.0));
        assert!(!l("123.45", 123.45));
        assert!(!l("0.0", 0.0));
        assert!(!l("1e3", 1e3));
        assert!(!l("0x1F", 31.0));
        assert!(l("9007199254740993", 9007199254740992.0));
        assert!(l("0x20000000000001", 9007199254740992.0));
        assert!(!l("1.0", 1.0));
        assert!(!l("100", 100.0));
        assert!(!l(".5", 0.5));
        assert!(!l("5e-324", 5e-324));
    }
}
