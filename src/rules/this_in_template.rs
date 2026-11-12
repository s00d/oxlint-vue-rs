//! vue/this-in-template

use oxc::ast::AstKind;
use oxc::span::GetSpan;

use crate::ast::ExprParent;
use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::js;
use crate::utils::template::{JS_RESERVED, template_expr_containers};
use crate::utils::tpl_analysis::{Nodes, container_element, scope_variable_names};

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(ThisInTemplate {
        always: opts.get(0).as_str() == Some("always"),
    }))
}

struct ThisInTemplate {
    always: bool,
}

/// Vue 3's `GLOBALS_ALLOWED` (`@vue/shared`): template expressions read these as globals.
const VUE_GLOBALS: &[&str] = &[
    "Infinity", "undefined", "NaN", "isFinite", "isNaN", "parseFloat", "parseInt", "decodeURI",
    "decodeURIComponent", "encodeURI", "encodeURIComponent", "Math", "Number", "Date", "Array",
    "Object", "Boolean", "String", "RegExp", "Map", "Set", "JSON", "Intl", "BigInt", "console",
    "Error", "Symbol",
];

/// `/^\d.*$|[^\w$]/`
fn bad_name(n: &str) -> bool {
    n.starts_with(|c: char| c.is_ascii_digit())
        || n.chars()
            .any(|c| !(c.is_ascii_alphanumeric() || c == '_' || c == '$'))
}

impl Rule for ThisInTemplate {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let Some(doc) = ctx.doc() else { return };
        for c in template_expr_containers(doc) {
            let Some(el) = container_element(c) else {
                continue;
            };
            let scope = scope_variable_names(doc, el);
            if self.always {
                if matches!(c.parent, ExprParent::DirectiveArgument(..)) {
                    continue;
                }
                let nodes = Nodes::of(c);
                for r in &c.references {
                    // Vue reads its allowed globals (`Math`, `console`, …) directly: `this.Math` is
                    // undefined (eslint-plugin-vue rewrites them).
                    if scope.contains(&r.name)
                        || VUE_GLOBALS.contains(&r.name.as_str())
                        || r.is_type_reference
                    {
                        continue;
                    }
                    // `{ a }` → `{ a: this.a }` (eslint-plugin-vue writes `{ this.a }`).
                    let shorthand = nodes.kinds.iter().enumerate().any(|(i, k)| {
                        matches!(k, AstKind::IdentifierReference(id) if id.span == r.span)
                            && nodes.parent(i).is_some_and(|p| {
                                matches!(nodes.kinds[p], AstKind::ObjectProperty(o) if o.shorthand)
                            })
                    });
                    let fix = if shorthand {
                        Fix::insert(r.span.start, format!("{}: this.", r.name))
                    } else {
                        Fix::insert(r.span.start, "this.")
                    };
                    ctx.report_fix(r.span, Some("expected"), "Expected 'this'.", Some(fix));
                }
                continue;
            }
            let nodes = Nodes::of(c);
            for (i, k) in nodes.kinds.iter().enumerate() {
                let AstKind::ThisExpression(t) = k else {
                    continue;
                };
                let Some(p) = nodes.parent_skip_parens(i) else {
                    continue;
                };
                let member = match nodes.kinds[p] {
                    AstKind::StaticMemberExpression(s) => {
                        Some((s.span, s.property.name.to_string()))
                    }
                    AstKind::ComputedMemberExpression(s) => {
                        js::string_literal_value(&s.expression).map(|n| (s.span, n))
                    }
                    _ => None,
                };
                let Some((span, name)) = member else { continue };
                if scope.contains(&name) || JS_RESERVED.contains(&name) || bad_name(&name) {
                    continue;
                }
                ctx.report_fix(
                    t.span(),
                    Some("unexpected"),
                    "Unexpected usage of 'this'.",
                    Some(Fix::replace(span, name)),
                );
            }
        }
    }
}
