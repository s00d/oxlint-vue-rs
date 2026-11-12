//! vue/keyword-spacing

use std::collections::{HashMap, HashSet};

use crate::ast::TokenKind;
use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::indent::tree::{F, Ty, flag};
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
const MORE_KEYWORDS: &[&str] = &[
    "arguments", "as", "async", "await", "eval", "from", "get", "let", "of", "set", "type",
    "using", "yield", "accessor", "satisfies",
];

#[derive(Clone, Copy)]
enum Pat {
    Prev,
    Next,
}

fn pat_match(p: Pat, v: &str) -> bool {
    match p {
        Pat::Prev => matches!(v, ")" | "]" | "}" | ">"),
        Pat::Next => matches!(v, "(" | "[" | "{" | "<" | "~" | "!" | "+" | "++" | "-" | "--"),
    }
}

fn check_type(k: TokenKind) -> bool {
    matches!(
        k,
        TokenKind::RegularExpression | TokenKind::String | TokenKind::Template | TokenKind::PrivateIdentifier
    )
}

struct KeywordSpacing {
    /// keyword → (before, after)
    map: HashMap<&'static str, (bool, bool)>,
}

struct St<'e, 'a> {
    e: &'e Ext<'a>,
    ignore: HashSet<u32>,
    out: Vec<Report>,
}

impl St<'_, '_> {
    fn check_before(&mut self, rule: &KeywordSpacing, t: u32) {
        let e = self.e;
        let Some(&(before, _)) = rule.map.get(e.val(t)) else { return };
        let tr = e.tr(t);
        let Some(p) = e.before(tr) else { return };
        let pr = e.tr(p);
        let ok = (check_type(e.kind(p)) || pat_match(Pat::Prev, e.val(p)))
            && !(e.kind(p) == TokenKind::Template && e.val(p).ends_with("${"))
            && !self.ignore.contains(&p)
            && e.same_line(pr, tr);
        if !ok {
            return;
        }
        let sp = e.space_between(pr, tr);
        let v = e.val(t);
        if before && !sp {
            self.out.push(rep(tr, format!("Expected space(s) before \"{v}\"."), ins(tr.0, " ")));
        } else if !before && sp {
            self.out.push(rep((pr.1, tr.0), format!("Unexpected space(s) before \"{v}\"."), del(pr.1, tr.0)));
        }
    }
    fn check_after(&mut self, rule: &KeywordSpacing, t: u32) {
        let e = self.e;
        let Some(&(_, after)) = rule.map.get(e.val(t)) else { return };
        let tr = e.tr(t);
        let Some(n) = e.after(tr) else { return };
        let nr = e.tr(n);
        let ok = (check_type(e.kind(n)) || pat_match(Pat::Next, e.val(n)))
            && !(e.kind(n) == TokenKind::Template && e.val(n).starts_with('}'))
            && !self.ignore.contains(&n)
            && e.same_line(tr, nr);
        if !ok {
            return;
        }
        let sp = e.space_between(tr, nr);
        let v = e.val(t);
        if after && !sp {
            self.out.push(rep(tr, format!("Expected space(s) after \"{v}\"."), ins(tr.1, " ")));
        } else if !after && sp {
            self.out.push(rep((tr.1, nr.0), format!("Unexpected space(s) after \"{v}\"."), del(tr.1, nr.0)));
        }
    }
    fn around(&mut self, rule: &KeywordSpacing, t: Option<u32>) {
        if let Some(t) = t {
            self.check_before(rule, t);
            self.check_after(rule, t);
        }
    }
    fn is_kw(&self, t: u32) -> bool {
        self.e.kind(t) == TokenKind::Keyword
    }
    fn around_first(&mut self, rule: &KeywordSpacing, n: Option<u32>) {
        let Some(n) = n else { return };
        let Some(f) = self.e.first(self.e.range(n)) else { return };
        if !self.is_kw(f) {
            if self.e.ty(n) == Ty::VariableDeclaration && self.e.kind(f) == TokenKind::Identifier {
                // `using` / `await using`
                let v = self.e.val(f);
                if v != "using" && v != "await" {
                    return;
                }
            } else {
                return;
            }
        }
        self.around(rule, Some(f));
    }
    fn before_first(&mut self, rule: &KeywordSpacing, n: u32) {
        if let Some(f) = self.e.first(self.e.range(n))
            && self.is_kw(f)
        {
            self.check_before(rule, f);
        }
    }
    fn around_before(&mut self, rule: &KeywordSpacing, n: Option<u32>) {
        if let Some(n) = n {
            let t = self.e.before_f(self.e.range(n), |t| self.is_kw(t));
            self.around(rule, t);
        }
    }
    fn function(&mut self, rule: &KeywordSpacing, n: u32) {
        if let Some(f) = self.e.first(self.e.range(n))
            && ((self.is_kw(f) && self.e.val(f) == "function") || self.e.val(f) == "async")
        {
            self.check_before(rule, f);
        }
    }
}

impl Rule for KeywordSpacing {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let Some(e) = Ext::new(ctx, false) else { return };
        let mut s = St {
            e: &e,
            ignore: HashSet::new(),
            out: Vec::new(),
        };
        let not_open = |t: u32| !e.is_punct(t, "(");
        for n in e.nodes(true) {
            let r = e.range(n);
            match e.ty(n) {
                Ty::DebuggerStatement
                | Ty::WithStatement
                | Ty::BreakStatement
                | Ty::ContinueStatement
                | Ty::ReturnStatement
                | Ty::ThrowStatement
                | Ty::SwitchStatement
                | Ty::SwitchCase
                | Ty::ForStatement
                | Ty::WhileStatement
                | Ty::VariableDeclaration
                | Ty::StaticBlock => s.around_first(self, Some(n)),
                Ty::TryStatement => {
                    s.around_first(self, Some(n));
                    if let Some(h) = e.get(n, F::Handler) {
                        if e.get(h, F::Param).is_some() {
                            s.before_first(self, h);
                        } else {
                            s.around_first(self, Some(h));
                        }
                    }
                    s.around_before(self, e.get(n, F::Finalizer));
                }
                Ty::IfStatement => {
                    s.around_first(self, Some(n));
                    s.around_before(self, e.get(n, F::Alternate));
                }
                Ty::DoWhileStatement => {
                    s.around_first(self, Some(n));
                    s.around_before(self, e.get(n, F::Test));
                }
                Ty::ForInStatement | Ty::ForOfStatement => {
                    if e.ty(n) == Ty::ForOfStatement && e.tree.has(n, flag::AWAIT) {
                        if let Some(t) = e.first(r) {
                            s.check_before(self, t);
                        }
                        if let Some(t) = e.first_skip(r, 1) {
                            s.check_after(self, t);
                        }
                    } else {
                        s.around_first(self, Some(n));
                    }
                    if let Some(right) = e.get(n, F::Right)
                        && let Some(t) = e.before_f(e.range(right), not_open)
                    {
                        if e.before(e.tr(t)).is_none_or(|p| e.kind(p) != TokenKind::PrivateIdentifier) {
                            s.check_before(self, t);
                        }
                        s.check_after(self, t);
                    }
                }
                Ty::ClassDeclaration | Ty::ClassExpression => {
                    s.around_first(self, Some(n));
                    s.around_before(self, e.get(n, F::SuperClass));
                }
                Ty::FunctionDeclaration | Ty::ArrowFunctionExpression | Ty::FunctionExpression => {
                    s.function(self, n)
                }
                Ty::AwaitExpression => {
                    if let Some(t) = e.first(r) {
                        s.check_before(self, t);
                    }
                }
                Ty::NewExpression
                | Ty::Super
                | Ty::ThisExpression
                | Ty::UnaryExpression
                | Ty::YieldExpression => s.before_first(self, n),
                Ty::Property | Ty::MethodDefinition | Ty::PropertyDefinition | Ty::AccessorProperty => {
                    let Some(k) = e.get(n, F::Key) else { continue };
                    let kind = if e.ty(n) == Ty::Property { e.prop_kind(n) } else { "init" };
                    let async_method = e.ty(n) == Ty::Property
                        && e.prop_method(n)
                        && e.get(n, F::Value).is_some_and(|v| e.tree.has(v, flag::ASYNC));
                    if kind != "init" || async_method {
                        let t = e.before_f(e.range(k), |t| {
                            matches!(e.val(t), "get" | "set" | "async" | "accessor")
                        });
                        s.around(self, t);
                    }
                }
                Ty::BinaryExpression => {
                    if let Some(right) = e.get(n, F::Right)
                        && let Some(t) = e.before_f(e.range(right), not_open)
                        && e.val(t) == ">"
                    {
                        s.ignore.insert(t);
                    }
                }
                Ty::TSAsExpression | Ty::TSSatisfiesExpression => {
                    let w = if e.ty(n) == Ty::TSAsExpression { "as" } else { "satisfies" };
                    if let Some(x) = e.get(n, F::Expression) {
                        let t = e.after_f(e.range(x), |t| e.val(t) == w);
                        s.around(self, t);
                    }
                }
                _ => {}
            }
        }
        let out = std::mem::take(&mut s.out);
        emit(ctx, &e, out, false);
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let o = opts.get(0);
    let before = o.get("before").and_then(|v| v.as_bool()).unwrap_or(true);
    let after = o.get("after").and_then(|v| v.as_bool()).unwrap_or(true);
    let ov = o.get("overrides");
    let mut map = HashMap::new();
    for &k in ES3_KEYWORDS.iter().chain(MORE_KEYWORDS) {
        let x = ov.and_then(|v| v.get(k)).filter(|v| v.is_object());
        let b = x.and_then(|x| x.get("before")).and_then(|v| v.as_bool()).unwrap_or(before);
        let a = x.and_then(|x| x.get("after")).and_then(|v| v.as_bool()).unwrap_or(after);
        map.insert(k, (b, a));
    }
    Some(Box::new(KeywordSpacing { map }))
}
