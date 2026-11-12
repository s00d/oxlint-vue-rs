//! vue/require-valid-default-prop

use oxc::ast::ast::*;
use oxc::ast_visit::Visit;
use oxc::span::{GetSpan, Span};
use oxc::syntax::scope::ScopeFlags;

use crate::rule::{Ctx, Rule, RuleOptions, fmt_msg};
use crate::utils::js;
use crate::utils::vue_component::{self as vc, ComponentProp, MacroKind};

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(RequireValidDefaultProp))
}

struct RequireValidDefaultProp;

const NATIVE_TYPES: &[&str] = &["String", "Number", "Boolean", "Function", "Object", "Array", "Symbol", "BigInt"];
const FUNCTION_VALUE_TYPES: &[&str] = &["Function", "Object", "Array"];

/// `getPropertyNode`: non-computed identifier key.
fn property_node<'a>(o: &'a ObjectExpression<'a>, name: &str) -> Option<&'a ObjectProperty<'a>> {
    js::properties(o).find(|p| !p.computed && matches!(&p.key, PropertyKey::StaticIdentifier(i) if i.name == name))
}

fn get_types(e: &Expression) -> Vec<String> {
    match js::skip_ts_as(e) {
        Expression::Identifier(i) => vec![i.name.to_string()],
        Expression::ArrayExpression(a) => a
            .elements
            .iter()
            .filter_map(|el| match el {
                ArrayExpressionElement::Identifier(i) => Some(i.name.to_string()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

enum ValueType {
    Standard(String),
    /// Expression-bodied arrow: (body span, return type).
    FnExpr(Span, Option<String>),
    /// Block body: the return statements' value types.
    FnBlock(Vec<(String, Span)>),
}

/// Value types of the direct `return`s of a function body.
struct Returns(Vec<(String, Span)>);
impl<'a> Visit<'a> for Returns {
    fn visit_return_statement(&mut self, r: &ReturnStatement<'a>) {
        if let Some(a) = &r.argument
            && let Some(t) = value_type(a)
        {
            let ty = match t {
                ValueType::Standard(s) => s,
                _ => "Function".into(),
            };
            self.0.push((ty, js::skip_parens(a).span()));
        }
    }
    fn visit_function(&mut self, _f: &Function<'a>, _flags: ScopeFlags) {}
    fn visit_arrow_function_expression(&mut self, _f: &ArrowFunctionExpression<'a>) {}
}

fn value_type(e: &Expression) -> Option<ValueType> {
    let e = js::skip_parens(e);
    let e = match e {
        Expression::ChainExpression(c) => match &c.expression {
            ChainElement::CallExpression(call) => {
                return native_call(call);
            }
            _ => return None,
        },
        e => e,
    };
    Some(match e {
        Expression::CallExpression(c) => return native_call(c),
        Expression::TemplateLiteral(_) => ValueType::Standard("String".into()),
        Expression::NullLiteral(_) => return None,
        Expression::StringLiteral(_) => ValueType::Standard("String".into()),
        Expression::NumericLiteral(_) => ValueType::Standard("Number".into()),
        Expression::BooleanLiteral(_) => ValueType::Standard("Boolean".into()),
        Expression::BigIntLiteral(_) => ValueType::Standard("BigInt".into()),
        Expression::RegExpLiteral(_) => ValueType::Standard("Object".into()),
        Expression::ArrayExpression(_) => ValueType::Standard("Array".into()),
        Expression::ObjectExpression(_) => ValueType::Standard("Object".into()),
        Expression::FunctionExpression(f) => {
            let mut r = Returns(Vec::new());
            if let Some(b) = &f.body {
                for s in &b.statements {
                    r.visit_statement(s);
                }
            }
            ValueType::FnBlock(r.0)
        }
        Expression::ArrowFunctionExpression(f) => {
            if let Some(body) = f.body.as_expression() {
                let t = value_type(body).map(|t| match t {
                    ValueType::Standard(s) => s,
                    _ => "Function".into(),
                });
                ValueType::FnExpr(js::skip_parens(body).span(), t)
            } else {
                let mut r = Returns(Vec::new());
                if let Some(b) = f.body.as_function_body() {
                    for s in &b.statements {
                        r.visit_statement(s);
                    }
                }
                ValueType::FnBlock(r.0)
            }
        }
        _ => return None,
    })
}

fn native_call(c: &CallExpression) -> Option<ValueType> {
    match &c.callee {
        Expression::Identifier(i) if NATIVE_TYPES.contains(&i.name.as_str()) => {
            Some(ValueType::Standard(i.name.to_string()))
        }
        _ => None,
    }
}

#[derive(PartialEq)]
enum Src {
    Assignment,
    WithDefaults,
    DefaultProperty,
}

fn report(ctx: &mut Ctx, span: Span, name: &str, types: &[String]) {
    ctx.report_id(
        span,
        "invalidType",
        fmt_msg(
            "Type of the default value for '{{name}}' prop must be a {{types}}.",
            &[("name", name), ("types", &types.join(" or ").to_lowercase())],
        ),
    );
}

fn check(ctx: &mut Ctx, name: &str, types: &[String], defaults: &[(Src, &Expression)]) {
    if defaults.is_empty() {
        return;
    }
    let mut names: Vec<String> = Vec::new();
    for t in types {
        if NATIVE_TYPES.contains(&t.as_str()) && !names.contains(t) {
            names.push(t.clone());
        }
    }
    if names.is_empty() {
        return;
    }
    let has = |t: &str| names.iter().any(|n| n == t);
    for (src, e) in defaults {
        let Some(vt) = value_type(e) else { continue };
        let span = js::skip_parens(e).span();
        match vt {
            ValueType::Standard(t) => {
                if has(&t) && (*src == Src::Assignment || !FUNCTION_VALUE_TYPES.contains(&t.as_str())) {
                    continue;
                }
                let list: Vec<String> = if *src == Src::Assignment {
                    names.clone()
                } else {
                    names
                        .iter()
                        .map(|t| if FUNCTION_VALUE_TYPES.contains(&t.as_str()) { "Function".into() } else { t.clone() })
                        .collect()
                };
                report(ctx, span, name, &list);
            }
            _ if has("Function") => {}
            _ if *src == Src::Assignment => report(ctx, span, name, &names),
            ValueType::FnExpr(body, rt) => {
                if rt.is_some_and(|rt| !has(&rt)) {
                    report(ctx, body, name, &names);
                }
            }
            ValueType::FnBlock(rets) => {
                for (t, s) in rets {
                    if !has(&t) {
                        report(ctx, s, name, &names);
                    }
                }
            }
        }
    }
}

impl Rule for RequireValidDefaultProp {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let file = ctx.file;
        let src = file.source;
        let prop_label = |p: &ComponentProp| -> String {
            p.name().unwrap_or_else(|| match p {
                ComponentProp::Object { prop, .. } => format!("[{}]", &src[prop.key.span().start as usize..prop.key.span().end as usize]),
                _ => "[]".into(),
            })
        };
        for o in vc::vue_objects(file) {
            for p in vc::props_from_options(o.obj) {
                let ComponentProp::Object { value: Expression::ObjectExpression(v), .. } = &p else {
                    continue;
                };
                let Some(t) = property_node(v, "type") else { continue };
                let types = get_types(&t.value);
                let defaults: Vec<(Src, &Expression)> =
                    property_node(v, "default").map(|d| (Src::DefaultProperty, &d.value)).into_iter().collect();
                check(ctx, &prop_label(&p), &types, &defaults);
            }
        }
        for m in vc::script_setup_macros(file) {
            match m.kind {
                MacroKind::DefineProps => {
                    let (wd, _) = vc::with_defaults_prop_expressions(file, &m);
                    let ds = vc::destructure_default_expressions(&m);
                    for p in vc::props_from_define_props(file, m.call) {
                        let types = match &p {
                            ComponentProp::Object { value: Expression::ObjectExpression(v), .. } => {
                                let Some(t) = property_node(v, "type") else { continue };
                                get_types(&t.value)
                            }
                            ComponentProp::Object { value, .. } => get_types(value),
                            ComponentProp::Type(t) => t.types.clone(),
                            _ => continue,
                        };
                        let mut defaults: Vec<(Src, &Expression)> = Vec::new();
                        if let ComponentProp::Object { value: Expression::ObjectExpression(v), .. } = &p
                            && let Some(d) = property_node(v, "default")
                        {
                            defaults.push((Src::DefaultProperty, &d.value));
                        }
                        if let Some(n) = p.name() {
                            if let Some(e) = wd.get(&n) {
                                defaults.push((Src::WithDefaults, e));
                            }
                            if let Some((_, e)) = ds.get(&n) {
                                defaults.push((Src::Assignment, e));
                            }
                        }
                        check(ctx, &prop_label(&p), &types, &defaults);
                    }
                }
                MacroKind::DefineModel => {
                    let model = vc::model_from_define_model(m.call);
                    let options = model.options.map(js::skip_ts_as);
                    let opt_obj = match options {
                        Some(Expression::ObjectExpression(o)) => Some(&**o),
                        _ => None,
                    };
                    if let Some(t) = model.type_node {
                        let types = vc::infer_runtime_type(file, t, &mut Vec::new());
                        let defaults: Vec<(Src, &Expression)> = opt_obj
                            .and_then(|o| property_node(o, "default"))
                            .map(|d| (Src::DefaultProperty, &d.value))
                            .into_iter()
                            .collect();
                        check(ctx, &model.name, &types, &defaults);
                    } else if let Some(o) = opt_obj {
                        // Synthetic object prop: `type` / `default` of the options object.
                        let Some(t) = property_node(o, "type") else { continue };
                        let types = get_types(&t.value);
                        let defaults: Vec<(Src, &Expression)> =
                            property_node(o, "default").map(|d| (Src::DefaultProperty, &d.value)).into_iter().collect();
                        check(ctx, &model.name, &types, &defaults);
                    }
                }
                _ => {}
            }
        }
    }
}
