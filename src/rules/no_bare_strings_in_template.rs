//! vue/no-bare-strings-in-template

use std::collections::HashMap;

use oxc::ast::ast::Expression;
use crate::utils::regexp::JsRegex as Regex;
use serde_json::Value;

use crate::ast::{AttributeKey, AttributeValue};
use crate::rule::{Ctx, Rule, RuleOptions, fmt_msg};
use crate::utils::casing::{is_kebab_case, pascal_case};
use crate::utils::js;
use crate::utils::regexp::{is_regexp, to_regexp};
use crate::utils::template::directive_expression;
use crate::utils::tpl_style::js_trim;
use crate::utils::tpl_valid::body_elements;

const DEFAULT_ALLOWLIST: &[&str] = &[
    "(", ")", ",", ".", "&", "+", "-", "=", "*", "/", "#", "%", "!", "?", ":", "[", "]", "{", "}",
    "<", ">", "\u{B7}", "\u{2022}", "\u{2010}", "\u{2013}", "\u{2014}", "\u{2212}", "|",
];

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let o = opts.get(0);
    let strs = |v: &Value| -> Vec<String> {
        v.as_array()
            .map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect())
            .unwrap_or_default()
    };
    let allowlist: Vec<String> = if o["allowlist"].is_array() {
        strs(&o["allowlist"])
    } else {
        DEFAULT_ALLOWLIST.iter().map(|s| s.to_string()).collect()
    };
    let mut words: Vec<String> = Vec::new();
    let mut regexps = Vec::new();
    for item in &allowlist {
        if is_regexp(item) {
            regexps.extend(to_regexp(item));
        } else {
            words.push(crate::utils::regexp::escape(item));
        }
    }
    words.sort_by_key(|w| std::cmp::Reverse(w.encode_utf16().count()));
    let allow_re = (!words.is_empty())
        .then(|| Regex::with_flags(&words.join("|"), "u").ok())
        .flatten();

    let mut names = HashMap::new();
    let mut attr_regexps = Vec::new();
    let attrs_opt = if o["attributes"].is_object() {
        o["attributes"].clone()
    } else {
        serde_json::json!({
            "/.+/": ["title", "aria-label", "aria-placeholder", "aria-roledescription", "aria-valuetext"],
            "input": ["placeholder"],
            "img": ["alt"]
        })
    };
    for (tag, list) in attrs_opt.as_object().unwrap() {
        let list = strs(list);
        if is_regexp(tag) {
            if let Some(r) = to_regexp(tag) {
                attr_regexps.push((r, list));
            }
        } else {
            names.insert(tag.clone(), list);
        }
    }
    let directives = if o["directives"].is_array() {
        strs(&o["directives"])
    } else {
        vec!["v-text".to_string()]
    };
    Some(Box::new(NoBareStrings {
        allow_re,
        regexps,
        names,
        attr_regexps,
        directives,
    }))
}

struct NoBareStrings {
    allow_re: Option<Regex>,
    regexps: Vec<Regex>,
    names: HashMap<String, Vec<String>>,
    attr_regexps: Vec<(Regex, Vec<String>)>,
    directives: Vec<String>,
}

impl NoBareStrings {
    fn is_bare(&self, s: &str) -> bool {
        let mut r = js_trim(s).to_string();
        if let Some(re) = &self.allow_re {
            r = re.replace_all(&r, "").into_owned();
        }
        for re in &self.regexps {
            r = re.replace_all(&r, "").into_owned();
        }
        !js_trim(&r).is_empty()
    }
    fn target_attrs(&self, tag: &str, out: &mut Vec<String>) {
        if let Some(l) = self.names.get(tag) {
            out.extend(l.iter().cloned());
        }
        for (re, l) in &self.attr_regexps {
            if re.is_match(tag) {
                out.extend(l.iter().cloned());
            }
        }
        if is_kebab_case(tag) {
            let p = pascal_case(tag);
            if p != tag {
                self.target_attrs(&p, out);
            }
        }
    }
}

impl Rule for NoBareStrings {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let Some(doc) = ctx.doc() else { return };
        for t in &doc.texts {
            if t.parent.is_some_and(|p| doc.in_template(p)) && self.is_bare(&t.value) {
                ctx.report_id(t.span, "unexpected", "Unexpected non-translated string used.");
            }
        }
        for e in body_elements(doc) {
            let mut attrs = Vec::new();
            self.target_attrs(&e.raw_name, &mut attrs);
            for a in e.attributes() {
                let Some(v) = &a.value else { continue };
                match (&a.key, v) {
                    (AttributeKey::Plain(k), AttributeValue::Literal(l)) => {
                        if attrs.contains(&k.raw_name) && self.is_bare(&l.value) {
                            ctx.report_id(
                                l.span,
                                "unexpectedInAttr",
                                fmt_msg(
                                    "Unexpected non-translated string used in `{{attr}}`.",
                                    &[("attr", &k.raw_name)],
                                ),
                            );
                        }
                    }
                    (AttributeKey::Directive(k), AttributeValue::Expr(id)) => {
                        let d = format!("v-{}", k.name.name);
                        if !self.directives.contains(&d) {
                            continue;
                        }
                        let Some(Expression::StringLiteral(s)) =
                            directive_expression(doc, a).map(js::skip_parens)
                        else {
                            continue;
                        };
                        if !s.value.is_empty() && self.is_bare(&s.value.display().to_string()) {
                            ctx.report_id(
                                doc.expr(*id).span,
                                "unexpectedInAttr",
                                fmt_msg(
                                    "Unexpected non-translated string used in `{{attr}}`.",
                                    &[("attr", &d)],
                                ),
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}
