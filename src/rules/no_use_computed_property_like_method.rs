//! vue/no-use-computed-property-like-method

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use oxc::ast::AstKind;
use oxc::ast::ast::*;
use oxc::ast_visit::Visit;
use oxc::semantic::{NodeId, Semantic};
use oxc::span::{GetSpan, Span};
use oxc::syntax::scope::ScopeFlags;

use crate::rule::{Ctx, Rule, RuleOptions, fmt_msg};
use crate::utils::js;
use crate::utils::template::template_expr_containers;
use crate::utils::tpl_analysis::{Nodes, Tree, innermost_vue};
use crate::utils::tpl_misc::is_this;
use crate::utils::vue_component::{self as vc, FnNode};

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(NoUseComputedPropertyLikeMethod))
}

struct NoUseComputedPropertyLikeMethod;

const NATIVE_NOT_FUNCTION_TYPES: &[&str] = &["String", "Number", "BigInt", "Boolean", "Object", "Array", "Symbol"];

fn key(e: &Expression) -> usize {
    e as *const Expression as usize
}

/// `resolvedExpressions(node)`.
fn resolved<'a>(sem: Option<&Semantic<'a>>, e: &'a Expression<'a>) -> Vec<&'a Expression<'a>> {
    let mut map: HashMap<usize, Vec<&'a Expression<'a>>> = HashMap::new();
    resolved_internal(sem, e, &mut map)
}

fn push_unique<'a>(v: &mut Vec<&'a Expression<'a>>, e: &'a Expression<'a>) {
    if !v.iter().any(|x| key(x) == key(e)) {
        v.push(e);
    }
}

#[allow(clippy::map_entry)] // the entry must exist while the recursion below runs
fn resolved_internal<'a>(
    sem: Option<&Semantic<'a>>,
    e: &'a Expression<'a>,
    map: &mut HashMap<usize, Vec<&'a Expression<'a>>>,
) -> Vec<&'a Expression<'a>> {
    let e = js::skip_parens(e);
    let k = key(e);
    if !map.contains_key(&k) {
        map.insert(k, Vec::new());
        let mut found = Vec::new();
        match e {
            Expression::Identifier(id) => {
                if let Some(sem) = sem
                    && let Some(sym) = js::ref_symbol(sem, id)
                {
                    let sc = sem.scoping();
                    // The declaration `const x = <init>` (a write reference in ESLint).
                    if let AstKind::VariableDeclarator(d) = sem.nodes().kind(sc.symbol_declaration(sym))
                        && matches!(&d.id, BindingPattern::BindingIdentifier(b) if b.symbol_id() == sym)
                        && let Some(init) = &d.init
                    {
                        found.extend(resolved_internal(Some(sem), init, map));
                    }
                    let ids: Vec<NodeId> = sc.get_resolved_references(sym).map(|r| r.node_id()).collect();
                    for rid in ids {
                        let span = sem.nodes().kind(rid).span();
                        if let Some(p) = sem.parent_of(rid)
                            && let AstKind::AssignmentExpression(a) = sem.nodes().kind(p)
                            && a.left.span() == span
                        {
                            found.extend(resolved_internal(Some(sem), &a.right, map));
                        }
                    }
                }
            }
            Expression::ConditionalExpression(c) => {
                found.extend(resolved_internal(sem, &c.consequent, map));
                found.extend(resolved_internal(sem, &c.alternate, map));
            }
            Expression::LogicalExpression(l) => {
                found.extend(resolved_internal(sem, &l.left, map));
                found.extend(resolved_internal(sem, &l.right, map));
            }
            _ => {}
        }
        let entry = map.get_mut(&k).unwrap();
        for f in found {
            push_unique(entry, f);
        }
    }
    let entry = map.get_mut(&k).unwrap();
    if entry.is_empty() {
        entry.push(e);
    }
    entry.clone()
}

/// `canBeFunction`.
// ponytail: `getStaticValue` is approximated — only the `undefined` / `NaN` / `Infinity` globals
// are known non-functions; a full static evaluator if a corpus needs it.
fn can_be_function<'a>(sem: Option<&Semantic<'a>>, e: &'a Expression<'a>) -> bool {
    for x in resolved(sem, e) {
        let non_fn = match x {
            Expression::ObjectExpression(_)
            | Expression::ArrayExpression(_)
            | Expression::StringLiteral(_)
            | Expression::NumericLiteral(_)
            | Expression::BooleanLiteral(_)
            | Expression::NullLiteral(_)
            | Expression::RegExpLiteral(_)
            | Expression::BigIntLiteral(_)
            | Expression::TemplateLiteral(_)
            | Expression::BinaryExpression(_)
            | Expression::UnaryExpression(_)
            | Expression::UpdateExpression(_) => true,
            Expression::ConditionalExpression(c) => {
                !can_be_function(sem, &c.consequent) && !can_be_function(sem, &c.alternate)
            }
            Expression::Identifier(id) => {
                matches!(id.name.as_str(), "undefined" | "NaN" | "Infinity")
                    && sem.is_none_or(|s| js::ref_symbol(s, id).is_none())
            }
            _ => false,
        };
        if !non_fn {
            return true;
        }
    }
    false
}

#[derive(PartialEq, Clone, Copy)]
enum Kind {
    Methods,
    Computed,
}

struct FnData<'a> {
    name: String,
    kind: Kind,
    returns: Vec<Option<&'a Expression<'a>>>,
    cache: RefCell<Option<bool>>,
}

struct Returns<'a>(Vec<Option<&'a Expression<'a>>>);
impl<'a> Visit<'a> for Returns<'a> {
    fn visit_return_statement(&mut self, r: &ReturnStatement<'a>) {
        // SAFETY-free: the AST outlives the rule run; `alloc` gives the `'a` reference.
        let arg: Option<&'a Expression<'a>> = r.argument.as_ref().map(|a| self.alloc(a));
        self.0.push(arg);
    }
    fn visit_function(&mut self, _f: &Function<'a>, _flags: ScopeFlags) {}
    fn visit_arrow_function_expression(&mut self, _f: &ArrowFunctionExpression<'a>) {}
}

struct Comp<'s, 'a> {
    sem: Option<&'s Semantic<'a>>,
    maybe_functions: RefCell<HashMap<String, bool>>,
    functions: Vec<FnData<'a>>,
    expr_cache: RefCell<HashMap<usize, bool>>,
}

impl<'s, 'a> Comp<'s, 'a> {
    fn new(sem: Option<&'s Semantic<'a>>, obj: &'a ObjectExpression<'a>) -> Self {
        let mut maybe = HashMap::new();
        let mut functions = Vec::new();
        let process = |name: &str, value: &'a Expression<'a>, kind: Kind, functions: &mut Vec<FnData<'a>>| {
            let Some(f) = FnNode::from_expr(value) else { return };
            let mut r = Returns(Vec::new());
            if let Some(b) = f.body() {
                for s in &b.statements {
                    r.visit_statement(s);
                }
            }
            if let Some(e) = f.expr_body() {
                r.0.insert(0, Some(e));
            }
            functions.push(FnData { name: name.to_string(), kind, returns: r.0, cache: RefCell::new(None) });
        };
        for p in vc::iterate_properties_full(obj, &["data", "props", "computed", "methods"]) {
            let Some(prop) = p.prop else { continue };
            match p.group {
                "data" => {
                    maybe.insert(p.name.clone(), can_be_function(sem, &prop.value));
                }
                "props" => {
                    let types = props_types(sem, &prop.value);
                    maybe.insert(
                        p.name.clone(),
                        types.is_none_or(|t| t.iter().any(|t| !NATIVE_NOT_FUNCTION_TYPES.contains(&t.as_str()))),
                    );
                }
                "computed" => {
                    let mut value: &'a Expression<'a> = &prop.value;
                    if let Expression::ObjectExpression(o) = js::skip_parens(value)
                        && let Some(g) = js::find_property(o, "get")
                    {
                        value = &g.value;
                    }
                    process(&p.name, value, Kind::Computed, &mut functions);
                }
                _ => {
                    process(&p.name, &prop.value, Kind::Methods, &mut functions);
                    maybe.insert(p.name.clone(), true);
                }
            }
        }
        Comp { sem, maybe_functions: RefCell::new(maybe), functions, expr_cache: RefCell::new(HashMap::new()) }
    }

    fn maybe_return_function(&self, f: &FnData<'a>) -> bool {
        if let Some(c) = *f.cache.borrow() {
            return c;
        }
        *f.cache.borrow_mut() = Some(true);
        let r = f.returns.iter().any(|rv| rv.is_some_and(|rv| self.maybe_function_expression(rv)));
        *f.cache.borrow_mut() = Some(r);
        r
    }

    fn maybe_function_expression(&self, e: &'a Expression<'a>) -> bool {
        let k = key(e);
        if let Some(c) = self.expr_cache.borrow().get(&k) {
            return *c;
        }
        self.expr_cache.borrow_mut().insert(k, true);
        let mut r = false;
        for x in resolved(self.sem, e) {
            if !can_be_function(self.sem, x) {
                continue;
            }
            if self.could_be_function(x) {
                r = true;
                break;
            }
        }
        self.expr_cache.borrow_mut().insert(k, r);
        r
    }

    fn this(&self, e: &Expression) -> bool {
        match self.sem {
            Some(s) => is_this(s, e),
            None => matches!(js::skip_parens(e), Expression::ThisExpression(_)),
        }
    }

    fn could_be_function(&self, e: &'a Expression<'a>) -> bool {
        match e {
            Expression::StaticMemberExpression(_)
            | Expression::ComputedMemberExpression(_)
            | Expression::PrivateFieldExpression(_) => {
                let m = e.as_member_expression().unwrap();
                if self.this(m.object())
                    && let Some(n) = js::member_name(m)
                    && !self.maybe_function_property(&n)
                {
                    return false;
                }
                true
            }
            Expression::CallExpression(c) => {
                if let Some(m) = js::skip_parens(&c.callee).as_member_expression()
                    && self.this(m.object())
                {
                    let n = js::member_name(m);
                    if let Some(f) = self.functions.iter().find(|f| Some(&f.name) == n.as_ref())
                        && f.kind == Kind::Methods
                        && !self.maybe_return_function(f)
                    {
                        return false;
                    }
                }
                true
            }
            Expression::ConditionalExpression(c) => {
                self.maybe_function_expression(&c.consequent) || self.maybe_function_expression(&c.alternate)
            }
            _ => true,
        }
    }

    fn maybe_function_property(&self, name: &str) -> bool {
        if let Some(c) = self.maybe_functions.borrow().get(name) {
            return *c;
        }
        self.maybe_functions.borrow_mut().insert(name.to_string(), true);
        let r = match self.functions.iter().find(|f| f.name == name) {
            Some(f) if f.kind == Kind::Computed => self.maybe_return_function(f),
            _ => true,
        };
        self.maybe_functions.borrow_mut().insert(name.to_string(), r);
        r
    }

    fn verify_call(&self, ctx: &mut Ctx, name: &str, span: Span, member: bool) {
        let Some(f) = self.functions.iter().find(|f| f.name == name && f.kind == Kind::Computed) else {
            return;
        };
        if self.maybe_return_function(f) {
            return;
        }
        let prefix = if member { "this." } else { "" };
        ctx.report_id(
            span,
            "unexpected",
            fmt_msg(
                "Use {{ likeProperty }} instead of {{ likeMethod }}.",
                &[("likeProperty", &format!("{prefix}{name}")), ("likeMethod", &format!("{prefix}{name}()"))],
            ),
        );
    }
}

/// `getComponentPropsTypes`.
fn props_types<'a>(sem: Option<&Semantic<'a>>, value: &'a Expression<'a>) -> Option<Vec<String>> {
    let mut out = Vec::new();
    for x in resolved(sem, value) {
        let type_exprs: Vec<&Expression> = if let Expression::ObjectExpression(o) = x {
            let t = js::find_property(o, "type")?;
            resolved(sem, &t.value)
        } else {
            vec![x]
        };
        for t in type_exprs {
            match t {
                Expression::Identifier(i) => out.push(i.name.to_string()),
                Expression::ArrayExpression(a) => {
                    for el in &a.elements {
                        match el {
                            ArrayExpressionElement::Elision(_) => {}
                            ArrayExpressionElement::SpreadElement(_) => return None,
                            el => {
                                for r in resolved(sem, el.as_expression()?) {
                                    let Expression::Identifier(i) = r else { return None };
                                    out.push(i.name.to_string());
                                }
                            }
                        }
                    }
                }
                _ => return None,
            }
        }
    }
    Some(out)
}

impl Rule for NoUseComputedPropertyLikeMethod {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let file = ctx.file;
        let sem = file.semantic;
        let objs = vc::vue_objects(file);
        let comps: HashMap<NodeId, Comp> = objs.iter().map(|o| (o.node_id(), Comp::new(sem, o.obj))).collect();
        if let Some(sem) = sem {
            let vue_ids: HashSet<NodeId> = comps.keys().copied().collect();
            for n in sem.nodes().iter() {
                let AstKind::CallExpression(call) = n.kind() else { continue };
                let Some(m) = js::skip_parens(&call.callee).as_member_expression() else { continue };
                let obj = js::skip_parens(m.object());
                if !matches!(obj, Expression::ThisExpression(_) | Expression::Identifier(_)) || !is_this(sem, obj) {
                    continue;
                }
                let Some(name) = js::member_name(m) else { continue };
                if let Some(v) = innermost_vue(sem, &vue_ids, n.id()) {
                    comps[&v].verify_call(ctx, &name, call.span, true);
                }
            }
        }
        let Some(doc) = file.doc.as_ref() else { return };
        let Some(tc) = objs.iter().find(|o| {
            sem.is_some_and(|s| s.nodes().ancestor_kinds(o.node_id()).any(|k| matches!(k, AstKind::ExportDefaultDeclaration(_))))
        }) else {
            return;
        };
        let comp = &comps[&tc.node_id()];
        for c in template_expr_containers(doc) {
            if !c.references.iter().any(|r| r.variable.is_none()) {
                continue;
            }
            let nodes = Nodes::of(c);
            for (i, k) in nodes.kinds.iter().enumerate() {
                let AstKind::IdentifierReference(id) = k else { continue };
                if !c.references.iter().any(|r| r.variable.is_none() && r.span == id.span) {
                    continue;
                }
                if let Some(p) = nodes.parent_of(i)
                    && let AstKind::CallExpression(call) = nodes.kinds[p]
                    && js::skip_parens(&call.callee).span() == id.span
                {
                    comp.verify_call(ctx, &id.name, call.span, false);
                }
            }
        }
    }
}
