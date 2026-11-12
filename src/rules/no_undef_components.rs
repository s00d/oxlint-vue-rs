//! vue/no-undef-components

use std::collections::HashSet;

use oxc::ast::ast::{Expression, ObjectExpression};
use oxc::semantic::SymbolFlags;
use oxc::span::Span;
use crate::utils::regexp::JsRegex as Regex;

use crate::ast::{AttributeKey, AttributeValue, Namespace, VExpression};
use crate::rule::{Ctx, Rule, RuleOptions, fmt_msg};
use crate::utils::casing::{capitalize, is_pascal_case, kebab_case, pascal_case};
use crate::utils::js;
use crate::utils::template::{
    directive_value, is_builtin_component_name, is_html_well_known_element_name,
    is_math_well_known_element_name, is_svg_well_known_element_name,
};
use crate::utils::tpl_analysis::{global_names, module_bindings, registered_components};
use crate::utils::tpl_valid::body_elements;
use crate::utils::vue_component::{self as vc, MacroKind, VueObjectType};

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let ignore = opts.get(0)["ignorePatterns"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str()).filter_map(|p| Regex::new(p).ok()).collect())
        .unwrap_or_default();
    Some(Box::new(NoUndefComponents { ignore }))
}

struct NoUndefComponents {
    ignore: Vec<Regex>,
}

/// Vue 3 `camelize`: `/-(\w)/g` → uppercase.
fn camelize(s: &str) -> String {
    let c: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        if c[i] == '-' && c.get(i + 1).is_some_and(|n| n.is_ascii_alphanumeric() || *n == '_') {
            out.extend(c[i + 1].to_uppercase());
            i += 2;
        } else {
            out.push(c[i]);
            i += 1;
        }
    }
    out
}

fn setup_defined(names: &HashSet<String>, raw: &str) -> bool {
    if names.contains(raw) {
        return true;
    }
    let camel = camelize(raw);
    if names.contains(&camel) || names.contains(&capitalize(&camel)) {
        return true;
    }
    matches!(raw.find('.'), Some(i) if i > 0 && setup_defined(names, &raw[..i]))
}

#[derive(Default)]
struct OptionNames {
    names: HashSet<String>,
    kebab: HashSet<String>,
}

impl OptionNames {
    fn add(&mut self, n: String) {
        self.kebab.insert(kebab_case(&n));
        self.names.insert(n);
    }
    fn defined(&self, raw: &str) -> bool {
        self.names.contains(raw) || (self.kebab.contains(&kebab_case(raw)) && !is_pascal_case(raw))
    }
    fn add_self_name(&mut self, obj: &ObjectExpression) {
        if let Some(p) = js::find_property(obj, "name")
            && js::is_string_literal(&p.value)
            && let Some(n) = js::string_literal_value(&p.value).filter(|n| !n.is_empty())
        {
            self.add(n);
        }
    }
}

impl NoUndefComponents {
    fn is_target(&self, raw: &str) -> bool {
        let kebab = kebab_case(raw);
        if is_html_well_known_element_name(raw)
            || is_svg_well_known_element_name(raw)
            || is_math_well_known_element_name(raw)
            || is_builtin_component_name(&kebab)
        {
            return false;
        }
        let pascal = pascal_case(raw);
        self.ignore
            .iter()
            .all(|r| !r.is_match(raw) && !r.is_match(&kebab) && !r.is_match(&pascal))
    }
}

impl Rule for NoUndefComponents {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let file = ctx.file;
        let Some(doc) = ctx.doc() else { return };
        let mut options = OptionNames::default();
        let setup = vc::is_script_setup(file);
        let mut setup_names = HashSet::new();
        let mut type_only = HashSet::new();
        if setup {
            if let Some(sem) = file.semantic {
                let sc = sem.scoping();
                for (name, sym) in module_bindings(file) {
                    let f = sc.symbol_flags(sym);
                    let is_type_only = (f.is_type() && !f.is_value() && !f.is_import())
                        || (f.contains(SymbolFlags::TypeImport) && !f.contains(SymbolFlags::Import));
                    if is_type_only {
                        type_only.insert(name.to_string());
                    } else {
                        setup_names.insert(name.to_string());
                    }
                }
            }
            setup_names.extend(global_names(file));
            if let Some(stem) = file.path.file_stem().and_then(|s| s.to_str()) {
                setup_names.insert(stem.to_string());
            }
            for o in vc::vue_objects(file) {
                if o.ty == VueObjectType::Export {
                    options.add_self_name(o.obj);
                }
            }
            for m in vc::macros_of(file, MacroKind::DefineOptions) {
                if let Some(Expression::ObjectExpression(o)) = m.arg(0) {
                    options.add_self_name(o);
                }
            }
        } else {
            for o in vc::vue_objects(file) {
                for (_, n) in registered_components(o.obj) {
                    options.add(n);
                }
                options.add_self_name(o.obj);
            }
        }
        let verify = |ctx: &mut Ctx, raw: &str, span: Span| {
            if !self.is_target(raw)
                || (setup && setup_defined(&setup_names, raw))
                || options.defined(raw)
            {
                return;
            }
            let (id, msg) = if setup && type_only.contains(raw) {
                ("typeOnly", "The '<{{name}}>' component has been used, but '{{name}}' only refers to a type.")
            } else {
                ("undef", "The '<{{name}}>' component has been used, but not defined.")
            };
            ctx.report_id(span, id, fmt_msg(msg, &[("name", raw)]));
        };
        for e in body_elements(doc) {
            if matches!(e.namespace, Namespace::Html | Namespace::Svg | Namespace::MathMl) {
                verify(ctx, &e.raw_name, e.start_tag.span);
            }
            for a in e.attributes() {
                match &a.key {
                    AttributeKey::Plain(k) if k.name == "is" => {
                        if let Some(AttributeValue::Literal(l)) = &a.value {
                            let v = l.value.strip_prefix("vue:").unwrap_or(&l.value);
                            verify(ctx, v, a.span);
                        }
                    }
                    AttributeKey::Directive(k)
                        if !setup
                            && (k.name.name == "is"
                                || (k.name.name == "bind" && k.static_argument() == Some("is"))) =>
                    {
                        if let Some(VExpression::Expression(x)) =
                            directive_value(doc, a).and_then(|c| c.expression.as_ref())
                        {
                            let v = match js::skip_parens(x) {
                                Expression::NullLiteral(_) => Some("null".to_string()),
                                x @ (Expression::StringLiteral(_)
                                | Expression::NumericLiteral(_)
                                | Expression::BooleanLiteral(_)
                                | Expression::RegExpLiteral(_)
                                | Expression::BigIntLiteral(_)) => js::string_literal_value(x),
                                _ => None,
                            };
                            if let Some(v) = v {
                                verify(ctx, &v, a.span);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}
