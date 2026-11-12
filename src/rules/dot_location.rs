//! vue/dot-location

use crate::ast::TokenKind;
use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::indent::tree::{F, Ty, flag};
use crate::utils::tpl_ext::*;

struct DotLocation {
    on_object: bool,
}

/// `DECIMAL_INTEGER_PATTERN`: `^(?:0|0[0-7]*[89]\d*|[1-9](?:_?\d)*)$`.
fn is_decimal_integer(s: &str) -> bool {
    let b = s.as_bytes();
    if s == "0" {
        return true;
    }
    if b.first() == Some(&b'0') {
        let rest = &s[1..];
        let oct = rest.bytes().take_while(|c| (b'0'..=b'7').contains(c)).count();
        let r2 = &rest[oct..];
        return matches!(r2.as_bytes().first(), Some(b'8' | b'9'))
            && r2.bytes().all(|c| c.is_ascii_digit());
    }
    if !matches!(b.first(), Some(b'1'..=b'9')) {
        return false;
    }
    let mut prev_us = false;
    for &c in &b[1..] {
        match c {
            b'_' if !prev_us => prev_us = true,
            b'0'..=b'9' => prev_us = false,
            _ => return false,
        }
    }
    !prev_us
}

impl Rule for DotLocation {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let Some(e) = Ext::new(ctx, false) else { return };
        let mut out = Vec::new();
        for n in e.nodes(false) {
            let prop = match e.ty(n) {
                Ty::MemberExpression if !e.tree.has(n, flag::COMPUTED) => e.get(n, F::Property),
                Ty::MetaProperty => e.get(n, F::Property),
                Ty::TSQualifiedName => e.get(n, F::Right),
                Ty::TSImportType => e.get(n, F::Qualifier),
                _ => continue,
            };
            let Some(prop) = prop else { continue };
            let pr = e.range(prop);
            let Some(dot) = e.before(pr) else { continue };
            let dr = e.tr(dot);
            let dv = e.val(dot);
            if self.on_object {
                let Some(tb) = e.before(dr) else { continue };
                let tbr = e.tr(tb);
                if !e.same_line(tbr, dr) {
                    let text = if dv.starts_with('.')
                        && e.kind(tb) == TokenKind::Numeric
                        && is_decimal_integer(e.val(tb))
                    {
                        format!(" {dv}")
                    } else {
                        dv.to_string()
                    };
                    let fix = Fix::merge(
                        e.src,
                        vec![
                            Fix::insert(tbr.1, text),
                            Fix::remove(oxc::span::Span::new(dr.0, dr.1)),
                        ],
                    );
                    out.push(rep(dr, "Expected dot to be on same line as object.", fix));
                }
            } else if !e.same_line(dr, pr) {
                let fix = Fix::merge(
                    e.src,
                    vec![
                        Fix::remove(oxc::span::Span::new(dr.0, dr.1)),
                        Fix::insert(pr.0, dv),
                    ],
                );
                out.push(rep(dr, "Expected dot to be on same line as property.", fix));
            }
        }
        emit(ctx, &e, out, false);
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(DotLocation {
        on_object: opt_str(opts, 0, "object") == "object",
    }))
}
