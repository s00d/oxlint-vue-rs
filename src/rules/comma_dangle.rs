//! vue/comma-dangle

use serde_json::Value;

use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::indent::tree::{F, NONE, Ty};
use crate::utils::tpl_ext::*;

struct CommaDangle {
    arrays: String,
    objects: String,
    functions: String,
    dynamic_imports: String,
    generics: String,
    tuples: String,
}

const CLOSE: [&str; 4] = ["}", "]", ")", ">"];

impl CommaDangle {
    fn trailing(&self, e: &Ext, n: u32, last: u32) -> Option<u32> {
        match e.ty(n) {
            Ty::ObjectExpression
            | Ty::ArrayExpression
            | Ty::CallExpression
            | Ty::NewExpression
            | Ty::ImportExpression => e.last_skip(e.range(n), 1),
            _ => {
                let next = e.after(e.range(last));
                if next.is_some_and(|t| e.is_punct(t, ",")) {
                    next
                } else {
                    e.last(e.range(last))
                }
            }
        }
    }
    fn multiline(&self, e: &Ext, n: u32, last: u32) -> bool {
        let Some(pen) = self.trailing(e, n, last) else {
            return false;
        };
        let Some(lt) = e.after(e.tr(pen)) else {
            return false;
        };
        e.line(e.tr(lt).1) != e.line(e.tr(pen).1)
    }
    fn forbid(&self, e: &Ext, n: u32, last: u32, out: &mut Vec<Report>) {
        let Some(t) = self.trailing(e, n, last) else { return };
        if !e.is_punct(t, ",") {
            return;
        }
        let tr = e.tr(t);
        let mut edits = vec![Fix::remove(oxc::span::Span::new(tr.0, tr.1))];
        if let Some(b) = e.before(tr) {
            edits.push(Fix::insert(e.tr(b).0, ""));
        }
        if let Some(a) = e.after(tr) {
            edits.push(Fix::insert(e.tr(a).1, ""));
        }
        out.push(rep(tr, "Unexpected trailing comma.", Fix::merge(e.src, edits)));
    }
    fn force(&self, e: &Ext, n: u32, last: u32, out: &mut Vec<Report>) {
        if e.ty(last) == Ty::RestElement {
            return self.forbid(e, n, last, out);
        }
        let Some(t) = self.trailing(e, n, last) else { return };
        if e.val(t) == "," {
            return;
        }
        let tr = e.tr(t);
        let Some(next) = e.after(tr) else { return };
        if !CLOSE.contains(&e.val(next)) {
            return;
        }
        let edits = vec![
            Fix::insert(tr.1, ","),
            Fix::insert(tr.0, ""),
            Fix::insert(e.tr(next).1, ""),
        ];
        out.push(rep(
            (tr.1, next_location(e.src, tr.1)),
            "Missing trailing comma.",
            Fix::merge(e.src, edits),
        ));
    }
    fn apply(&self, mode: &str, e: &Ext, n: u32, last: Option<u32>, out: &mut Vec<Report>) {
        let Some(last) = last.filter(|&l| l != NONE) else {
            return;
        };
        match mode {
            "always" => self.force(e, n, last, out),
            "always-multiline" => {
                if self.multiline(e, n, last) {
                    self.force(e, n, last, out)
                } else {
                    self.forbid(e, n, last, out)
                }
            }
            "only-multiline" => {
                if !self.multiline(e, n, last) {
                    self.forbid(e, n, last, out)
                }
            }
            "never" => self.forbid(e, n, last, out),
            _ => {}
        }
    }
}

impl Rule for CommaDangle {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let Some(e) = Ext::new(ctx, false) else { return };
        let mut out = Vec::new();
        for n in e.nodes(false) {
            let last = |f: F| e.list(n, f).last().copied();
            match e.ty(n) {
                Ty::ObjectExpression | Ty::ObjectPattern => {
                    self.apply(&self.objects, &e, n, last(F::Properties), &mut out)
                }
                Ty::ArrayExpression | Ty::ArrayPattern => {
                    self.apply(&self.arrays, &e, n, last(F::Elements), &mut out)
                }
                Ty::FunctionDeclaration
                | Ty::FunctionExpression
                | Ty::ArrowFunctionExpression
                | Ty::TSDeclareFunction
                | Ty::TSFunctionType => {
                    self.apply(&self.functions, &e, n, last(F::Params), &mut out)
                }
                Ty::CallExpression | Ty::NewExpression => {
                    self.apply(&self.functions, &e, n, last(F::Arguments), &mut out)
                }
                Ty::ImportExpression => {
                    let l = e.get(n, F::Options).or_else(|| e.get(n, F::Source));
                    self.apply(&self.dynamic_imports, &e, n, l, &mut out)
                }
                Ty::TSTypeParameterDeclaration => {
                    self.apply(&self.generics, &e, n, last(F::Params), &mut out)
                }
                Ty::TSTypeParameterInstantiation => {
                    self.apply("never", &e, n, last(F::Params), &mut out)
                }
                Ty::TSTupleType => self.apply(&self.tuples, &e, n, last(F::ElementTypes), &mut out),
                _ => {}
            }
        }
        emit(ctx, &e, out, false);
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let v = opts.get(0);
    // ponytail: `ecmaVersion` is taken as "latest" (functions / dynamicImports never "ignore"d).
    let pick = |k: &str| -> String {
        match v {
            Value::String(s) => s.clone(),
            Value::Object(_) => v.get(k).and_then(Value::as_str).unwrap_or("never").to_string(),
            _ => "never".to_string(),
        }
    };
    Some(Box::new(CommaDangle {
        arrays: pick("arrays"),
        objects: pick("objects"),
        functions: pick("functions"),
        dynamic_imports: pick("dynamicImports"),
        generics: pick("generics"),
        tuples: pick("tuples"),
    }))
}
