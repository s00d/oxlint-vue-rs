//! vue/require-expose

use oxc::ast::AstKind;
use oxc::ast::ast::*;
use oxc::semantic::{NodeId, Semantic};
use oxc::span::GetSpan;

use crate::rule::{Ctx, Fix, LintFile, Rule, RuleOptions, Suggestion};
use crate::utils::vue_component::{self as vc, DefinitionType, FnNode, VueObjectType};
use crate::utils::{js, tokens};

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(RequireExpose))
}

struct RequireExpose;

const FIX_EXPOSE_BEFORE_OPTIONS: [&str; 10] = [
    "name",
    "components",
    "directives",
    "extends",
    "mixins",
    "provide",
    "inject",
    "inheritAttrs",
    "props",
    "emits",
];
const MSG: &str = "The public properties of the component must be explicitly declared using `expose`. If the component does not have public properties, declare it empty.";
const EMPTY: &str = "Add the `expose` option to give an empty array.";
const ALL: &str = "Add the `expose` option to declare all properties.";

/// `isFunction(node)`: a function expression, or an identifier bound to one.
fn is_function(sem: &Semantic, e: &Expression, depth: u32) -> bool {
    match js::skip_parens(e) {
        Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_) => true,
        Expression::Identifier(id) if depth < 16 => {
            let Some(sym) = js::ref_symbol(sem, id) else {
                return false;
            };
            match sem.nodes().kind(js::declaration_id(sem, sym)) {
                AstKind::Function(_) => true,
                AstKind::VariableDeclarator(d) => d
                    .init
                    .as_ref()
                    .is_some_and(|i| is_function(sem, i, depth + 1)),
                _ => false,
            }
        }
        _ => false,
    }
}

fn innermost_fn(sem: &Semantic, id: NodeId) -> Option<NodeId> {
    sem.nodes()
        .ancestors(id)
        .find(|n| {
            matches!(
                n.kind(),
                AstKind::Function(_) | AstKind::ArrowFunctionExpression(_)
            )
        })
        .map(|n| n.id())
}

/// Read references of a binding identifier pattern.
fn read_refs(sem: &Semantic, p: &BindingPattern) -> Vec<NodeId> {
    match p {
        BindingPattern::BindingIdentifier(b) => sem
            .scoping()
            .get_resolved_references(b.symbol_id())
            .filter(|r| r.is_read())
            .map(|r| r.node_id())
            .collect(),
        _ => Vec::new(),
    }
}

/// Whether `setup(_, ctx)` calls `expose()` / `ctx.expose()` inside this Vue object.
fn calls_expose(file: &LintFile, sem: &Semantic, obj: &ObjectExpression, f: FnNode) -> bool {
    let Some(param) = f.params().items.get(1) else {
        return false;
    };
    if param.initializer.is_some() {
        return false;
    }
    let (expose_refs, ctx_refs) = match &param.pattern {
        BindingPattern::ObjectPattern(o) => match js::find_binding_property(o, "expose") {
            Some(p) => (read_refs(sem, &p.value), Vec::new()),
            None => return false,
        },
        p @ BindingPattern::BindingIdentifier(_) => (Vec::new(), read_refs(sem, p)),
        _ => return false,
    };
    sem.nodes().iter().any(|n| {
        let AstKind::CallExpression(call) = n.kind() else { return false };
        if !js::in_range(obj.span, call.span) || vc::enclosing_vue_object(file, n.id()).is_none_or(|v| v.obj.span != obj.span) {
            return false;
        }
        let callee = match js::skip_parens(&call.callee) {
            Expression::ChainExpression(c) => c.expression.as_member_expression().map(|m| (None, Some(m))),
            Expression::Identifier(id) => Some((Some(id.node_id()), None)),
            e => e.as_member_expression().map(|m| (None, Some(m))),
        };
        match callee {
            Some((Some(id), _)) => expose_refs.contains(&id),
            Some((None, Some(m))) => js::member_name(m).as_deref() == Some("expose") && matches!(m.object(), Expression::Identifier(o) if ctx_refs.contains(&o.node_id())),
            _ => false,
        }
    })
}

/// The setup function returns a render function.
fn returns_function(sem: &Semantic, f: FnNode) -> bool {
    if let Some(e) = f.expr_body() {
        return is_function(sem, e, 0);
    }
    let fid = f.node_id();
    sem.nodes().iter().any(|n| match n.kind() {
        AstKind::ReturnStatement(r) => r
            .argument
            .as_ref()
            .is_some_and(|a| innermost_fn(sem, n.id()) == Some(fid) && is_function(sem, a, 0)),
        _ => false,
    })
}

impl Rule for RequireExpose {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let file = ctx.file;
        if vc::is_script_setup(file) {
            return;
        }
        let Some(sem) = file.semantic else { return };
        for vo in vc::vue_objects(file) {
            let obj = vo.obj;
            let setups = vc::setup_functions(obj);
            if setups
                .iter()
                .any(|f| calls_expose(file, sem, obj, *f) || returns_function(sem, *f))
            {
                continue;
            }
            if vo.ty == VueObjectType::Definition
                && vc::definition_type(file, obj) == Some(DefinitionType::Mixin)
            {
                continue;
            }
            if js::properties(obj)
                .any(|p| !p.computed && js::prop_name(p).as_deref() == Some("expose"))
            {
                continue;
            }
            let suggestions = build_suggest(file, obj);
            ctx.push(
                obj.span,
                Some("requireExpose"),
                MSG.into(),
                None,
                suggestions,
            );
        }
    }
}

fn build_suggest(file: &LintFile, obj: &ObjectExpression) -> Vec<Suggestion> {
    let before = js::properties(obj).find(|p| {
        js::prop_name(p).is_some_and(|n| FIX_EXPOSE_BEFORE_OPTIONS.contains(&n.as_str()))
    });
    let all = vc::iterate_properties(
        obj,
        &["props", "data", "computed", "setup", "methods", "watch"],
    );
    let build = |text: String| -> Option<Fix> {
        if let Some(b) = before {
            return Some(Fix::insert(b.span.end, format!(",\n{text}")));
        }
        if !obj.properties.is_empty() {
            let after = js::properties(obj)
                .next()
                .map_or_else(|| obj.properties[0].span(), |p| p.span);
            let t = tokens::token_before(file, after.start)?;
            return Some(Fix::insert(t.end, format!("\n{text},")));
        }
        let l = tokens::first_token(file, obj.span)?;
        let r = tokens::last_token(file, obj.span)?;
        let nl = if tokens::line(file, l.end) < tokens::line(file, r.start) {
            ""
        } else {
            "\n"
        };
        Some(Fix::insert(l.end, format!("\n{text}{nl}")))
    };
    let mut out = Vec::new();
    if let Some(fix) = build("expose: []".into()) {
        out.push(Suggestion {
            message_id: Some("addExposeOptionForEmpty"),
            desc: EMPTY.into(),
            fix,
        });
    }
    if !all.is_empty() {
        let names: Vec<String> = all.iter().map(|p| js::json_string(&p.name)).collect();
        if let Some(fix) = build(format!("expose: [{}]", names.join(", "))) {
            out.push(Suggestion {
                message_id: Some("addExposeOptionForAll"),
                desc: ALL.into(),
                fix,
            });
        }
    }
    out
}
