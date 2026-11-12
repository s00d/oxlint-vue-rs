//! vue/no-v-html

use oxc::ast::ast::Expression;
use crate::utils::regexp::JsRegex as Regex;

use crate::ast::VExpression;
use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::js::skip_parens;
use crate::utils::template::directive_value;
use crate::utils::tpl_valid::{body_directives, expr_span};

struct R {
    ignore: Option<Regex>,
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (_, _, a, _) in body_directives(doc, "html") {
            if let Some(re) = &self.ignore
                && let Some(x) = directive_value(doc, a).and_then(|c| c.expression.as_ref())
            {
                let text = match x {
                    VExpression::Expression(e) => match skip_parens(e) {
                        Expression::Identifier(id) => id.name.to_string(),
                        _ => doc.slice(expr_span(e)).to_string(),
                    },
                    VExpression::FilterSequence { span, .. }
                    | VExpression::For { span, .. }
                    | VExpression::On { span, .. }
                    | VExpression::SlotScope { span, .. }
                    | VExpression::Generic { span, .. } => doc.slice(*span).to_string(),
                };
                if re.is_match(&text) {
                    continue;
                }
            }
            ctx.report(a.span, "'v-html' directive can lead to XSS attack.");
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let ignore = opts.get(0)["ignorePattern"]
        .as_str()
        .filter(|s| !s.is_empty())
        .and_then(|s| Regex::with_flags(s, "u").ok());
    Some(Box::new(R { ignore }))
}
