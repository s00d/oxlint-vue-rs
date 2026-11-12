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
                for r in &c.references {
                    if !scope.contains(&r.name) {
                        ctx.report_fix(
                            r.span,
                            Some("expected"),
                            "Expected 'this'.",
                            Some(Fix::insert(r.span.start, "this.")),
                        );
                    }
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
