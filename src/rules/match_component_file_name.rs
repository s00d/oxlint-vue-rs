//! vue/match-component-file-name

use oxc::ast::ast::Expression;
use oxc::span::{GetSpan, Span};

use crate::rule::{Ctx, Fix, LintFile, Rule, RuleOptions, Suggestion};
use crate::utils::vue_component::{self as vc, MacroKind};
use crate::utils::{casing, js};

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let o = opts.get(0);
    let extensions = match o.get("extensions").and_then(|v| v.as_array()) {
        Some(a) => a
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        None => vec!["jsx".to_string()],
    };
    Some(Box::new(MatchComponentFileName {
        extensions,
        match_case: o
            .get("shouldMatchCase")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    }))
}

struct MatchComponentFileName {
    extensions: Vec<String>,
    match_case: bool,
}

impl MatchComponentFileName {
    fn verify(
        &self,
        file: &LintFile,
        e: &Expression,
        stem: &str,
        errors: &mut Vec<(Span, String, Fix)>,
    ) {
        let Some(name) = js::string_literal_value(e) else {
            return;
        };
        let ok = if self.match_case {
            name == stem
        } else {
            casing::pascal_case(&name) == stem || casing::kebab_case(&name) == stem
        };
        if ok {
            return;
        }
        let span = e.span();
        let quote = if matches!(js::skip_parens(e), Expression::TemplateLiteral(_)) {
            "`".to_string()
        } else {
            file.slice(span)[..1].to_string()
        };
        errors.push((
            span,
            name,
            Fix::replace(span, format!("{quote}{stem}{quote}")),
        ));
    }
}

impl Rule for MatchComponentFileName {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let file = ctx.file;
        if !self.extensions.iter().any(|e| e == vc::ext(file)) {
            return;
        }
        let stem = file.path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let mut errors = Vec::new();
        let mut count = 0;
        for c in vc::vue_component_calls(file) {
            if c.arguments.len() == 2
                && let Some(a) = c.arguments[0]
                    .as_expression()
                    .filter(|e| js::is_string_literal(e))
            {
                self.verify(file, a, stem, &mut errors);
            }
        }
        for o in vc::vue_objects(file) {
            count += 1;
            if let Some(p) =
                js::find_property(o.obj, "name").filter(|p| js::is_string_literal(&p.value))
            {
                self.verify(file, &p.value, stem, &mut errors);
            }
        }
        for m in vc::macros_of(file, MacroKind::DefineOptions) {
            count += 1;
            if let Some(Expression::ObjectExpression(o)) = m.arg(0).map(js::skip_parens)
                && let Some(p) =
                    js::find_property(o, "name").filter(|p| js::is_string_literal(&p.value))
            {
                self.verify(file, &p.value, stem, &mut errors);
            }
        }
        if count > 1 {
            return;
        }
        for (span, name, fix) in errors {
            let sugg = Suggestion {
                message_id: None,
                desc: "Rename component to match file name.".into(),
                fix,
            };
            ctx.push(
                span,
                Some("shouldMatchFileName"),
                format!("Component name `{name}` should match file name `{stem}`."),
                None,
                vec![sugg],
            );
        }
    }
}
