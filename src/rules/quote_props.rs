//! vue/quote-props

use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::indent::tree::{F, Ty, flag};
use crate::utils::js::js_number;
use crate::utils::tpl_ext::*;

const ES3_KEYWORDS: &[&str] = &[
    "abstract", "boolean", "break", "byte", "case", "catch", "char", "class", "const", "continue",
    "debugger", "default", "delete", "do", "double", "else", "enum", "export", "extends", "false",
    "final", "finally", "float", "for", "function", "goto", "if", "implements", "import", "in",
    "instanceof", "int", "interface", "long", "native", "new", "null", "package", "private",
    "protected", "public", "return", "short", "static", "super", "switch", "synchronized", "this",
    "throw", "throws", "transient", "true", "try", "typeof", "var", "void", "volatile", "while",
    "with",
];

struct QuoteProps {
    mode: String,
    keywords: bool,
    unnecessary: bool,
    numbers: bool,
}

/// What `espree.tokenize(s)` makes of a key string: `None` = not exactly one token covering
/// the whole string; `Some(true)` = Identifier/Keyword/Null/Boolean, `Some(false)` = Numeric.
// ponytail: identifier chars approximated by `char::is_alphabetic` / `is_alphanumeric`.
fn single_token(s: &str) -> Option<bool> {
    let mut cs = s.chars();
    let c0 = cs.next()?;
    if (c0.is_alphabetic() || c0 == '$' || c0 == '_')
        && cs.all(|c| c.is_alphanumeric() || c == '$' || c == '_' || c == '\u{200c}' || c == '\u{200d}')
    {
        return Some(true);
    }
    let n = crate::utils::tpl_misc::js_to_number(s)?;
    // Only a canonical decimal is both one numeric token and `String(+s) === s`.
    (js_number(n) == s && !s.starts_with('-')).then_some(false)
}

/// `areQuotesRedundant`.
fn redundant(s: &str, skip_numbers: bool) -> bool {
    match single_token(s) {
        Some(true) => true,
        Some(false) => !skip_numbers,
        None => false,
    }
}

enum Key {
    Ident(String),
    Str(String),
    Num(String),
    Other,
}

impl QuoteProps {
    fn key(&self, e: &Ext, k: u32) -> Key {
        match e.ty(k) {
            Ty::Identifier => Key::Ident(e.text(e.range(k)).to_string()),
            Ty::Literal => match e.literal(k) {
                Lit::Str(s) => Key::Str(s),
                Lit::Num(n) => Key::Num(js_number(n)),
                Lit::Other => {
                    let raw = e.text(e.range(k));
                    match raw.strip_suffix('n') {
                        Some(b) if raw.starts_with(|c: char| c.is_ascii_digit()) => Key::Num(b.to_string()),
                        _ => Key::Other,
                    }
                }
            },
            _ => Key::Other,
        }
    }
    /// `getQuotedKey`.
    fn quoted(&self, e: &Ext, k: u32) -> String {
        match self.key(e, k) {
            Key::Str(_) => e.text(e.range(k)).to_string(),
            Key::Ident(s) | Key::Num(s) => format!("\"{s}\""),
            Key::Other => {
                if e.ty(k) == Ty::TemplateLiteral {
                    e.text(e.range(k)).to_string()
                } else {
                    "\"undefined\"".to_string()
                }
            }
        }
    }
    fn name(&self, e: &Ext, k: u32) -> String {
        match self.key(e, k) {
            Key::Ident(s) | Key::Str(s) | Key::Num(s) => s,
            Key::Other => "undefined".into(),
        }
    }
    fn skip(&self, e: &Ext, p: u32) -> bool {
        e.prop_method(p) || e.tree.has(p, flag::COMPUTED) || e.prop_shorthand(p)
    }
    fn fix(&self, k: (u32, u32), text: String) -> Option<Fix> {
        repl(k.0, k.1, text)
    }
    fn omitted(&self, e: &Ext, p: u32, key: u32, out: &mut Vec<(u32, Report)>) {
        if self.skip(e, p) {
            return;
        }
        if let Key::Str(_) = self.key(e, key) {
            return;
        }
        let msg = format!("Unquoted property '{}' found.", self.name(e, key));
        out.push((p, rep(e.range(p), msg, self.fix(e.range(key), self.quoted(e, key)))));
    }
    fn unnecessary(&self, e: &Ext, p: u32, key: u32, out: &mut Vec<(u32, Report)>) {
        if self.skip(e, p) {
            return;
        }
        match self.key(e, key) {
            Key::Str(s) => {
                if single_token(&s).is_none() {
                    return;
                }
                if self.keywords && ES3_KEYWORDS.contains(&s.as_str()) {
                    return;
                }
                if self.unnecessary && redundant(&s, self.numbers) {
                    let msg = format!("Unnecessarily quoted property '{s}' found.");
                    out.push((p, rep(e.range(p), msg, self.fix(e.range(key), s))));
                }
            }
            Key::Ident(s) if self.keywords && ES3_KEYWORDS.contains(&s.as_str()) => {
                let msg = format!("Unquoted reserved word '{s}' used as key.");
                out.push((p, rep(e.range(p), msg, self.fix(e.range(key), self.quoted(e, key)))));
            }
            Key::Num(s) if self.numbers => {
                let msg = format!("Unquoted number literal '{s}' used as key.");
                out.push((p, rep(e.range(p), msg, self.fix(e.range(key), self.quoted(e, key)))));
            }
            _ => {}
        }
    }
    fn consistency(&self, e: &Ext, props: &[u32], redundancy: bool, out: &mut Vec<(u32, Report)>) {
        let props: Vec<(u32, u32)> = props
            .iter()
            .filter(|&&p| e.ty(p) != Ty::SpreadElement && !self.skip(e, p))
            .filter_map(|&p| e.get(p, F::Key).map(|k| (p, k)))
            .collect();
        let mut quoted = Vec::new();
        let mut unquoted = Vec::new();
        let mut keyword_name: Option<String> = None;
        let mut necessary = false;
        for &(p, k) in &props {
            match self.key(e, k) {
                Key::Str(s) => {
                    quoted.push((p, k));
                    if redundancy {
                        necessary = necessary
                            || !redundant(&s, false)
                            || (self.keywords && ES3_KEYWORDS.contains(&s.as_str()));
                    }
                }
                Key::Ident(s) if self.keywords && redundancy && ES3_KEYWORDS.contains(&s.as_str()) => {
                    unquoted.push((p, k));
                    necessary = true;
                    keyword_name = Some(s);
                }
                _ => unquoted.push((p, k)),
            }
        }
        if redundancy && !quoted.is_empty() && !necessary {
            for (p, k) in quoted {
                let Key::Str(s) = self.key(e, k) else { continue };
                out.push((p, rep(e.range(p), "Properties shouldn't be quoted as all quotes are redundant.", self.fix(e.range(k), s))));
            }
        } else if !unquoted.is_empty() && keyword_name.is_some() {
            let kw = keyword_name.unwrap();
            for (p, k) in unquoted {
                let msg = format!("Properties should be quoted as '{kw}' is a reserved word.");
                out.push((p, rep(e.range(p), msg, self.fix(e.range(k), self.quoted(e, k)))));
            }
        } else if !quoted.is_empty() && !unquoted.is_empty() {
            for (p, k) in unquoted {
                let msg = format!("Inconsistently quoted property '{}' found.", self.name(e, k));
                out.push((p, rep(e.range(p), msg, self.fix(e.range(k), self.quoted(e, k)))));
            }
        }
    }
}

impl Rule for QuoteProps {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let Some(e) = Ext::new(ctx, false) else { return };
        let mode = self.mode.as_str();
        let mut found: Vec<(u32, Report)> = Vec::new();
        for n in e.nodes(true) {
            match e.ty(n) {
                Ty::Property | Ty::TSPropertySignature => {
                    let Some(k) = e.get(n, F::Key) else { continue };
                    if mode == "always" {
                        self.omitted(&e, n, k, &mut found);
                    }
                    if mode == "as-needed" {
                        self.unnecessary(&e, n, k, &mut found);
                    }
                }
                Ty::ObjectExpression | Ty::TSTypeLiteral => {
                    let f = if e.ty(n) == Ty::ObjectExpression { F::Properties } else { F::Members };
                    if mode == "consistent" {
                        self.consistency(&e, e.list(n, f), false, &mut found);
                    }
                    if mode == "consistent-as-needed" {
                        self.consistency(&e, e.list(n, f), true, &mut found);
                    }
                }
                _ => {}
            }
        }
        // The Vue wrapper: inside an attribute value, quotes in the fix text become the quote
        // the attribute does not use.
        let out = found
            .into_iter()
            .map(|(p, mut r)| {
                let mut cur = e.parent(p);
                while let Some(c) = cur {
                    if e.ty(c) == Ty::VExpressionContainer {
                        let in_value = e.parent(c).is_some_and(|a| e.ty(a) == Ty::VAttribute);
                        let q = e.text(e.range(c)).chars().next();
                        if in_value && matches!(q, Some('"' | '\'')) {
                            let to = if q == Some('"') { "'" } else { "\"" };
                            if let Some(f) = r.fix.as_mut() {
                                f.text = f.text.replace(['"', '\''], to);
                            }
                        }
                        break;
                    }
                    cur = e.parent(c);
                }
                r
            })
            .collect();
        emit(ctx, &e, out, false);
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let mode = opt_str(opts, 0, "always");
    let mode = if mode.is_empty() { "always" } else { mode };
    Some(Box::new(QuoteProps {
        mode: mode.to_string(),
        keywords: opt_bool(opts, 1, "keywords").unwrap_or(false),
        unnecessary: opt_bool(opts, 1, "unnecessary").unwrap_or(true),
        numbers: opt_bool(opts, 1, "numbers").unwrap_or(false),
    }))
}
