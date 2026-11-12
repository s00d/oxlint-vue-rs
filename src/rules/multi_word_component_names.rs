//! vue/multi-word-component-names

use oxc::ast::ast::Expression;
use oxc::span::{GetSpan, Span};

use crate::rule::{Ctx, Rule, RuleOptions, fmt_msg};
use crate::utils::casing::{is_pascal_case, kebab_case};
use crate::utils::js::{self, skip_parens};
use crate::utils::template::VUE3_BUILTIN_COMPONENT_NAMES;
use crate::utils::vue_component as vc;

struct R {
    ignores: Vec<String>,
}

impl R {
    fn valid(&self, name: &str) -> bool {
        self.ignores.iter().any(|i| i == name)
            || VUE3_BUILTIN_COMPONENT_NAMES.contains(&name)
            || kebab_case(name).split('-').count() > 1
    }
    fn validate(&self, ctx: &mut Ctx, e: &Expression) {
        let e = skip_parens(e);
        if !e.is_literal() {
            return;
        }
        let name = match e {
            Expression::NullLiteral(_) => "null".to_string(),
            _ => match js::string_literal_value(e) {
                Some(s) => s,
                None => return,
            },
        };
        if !self.valid(&name) {
            ctx.report(e.span(), fmt_msg("Component name \"{{value}}\" should always be multi-word.", &[("value", &name)]));
        }
    }
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let file = ctx.file;
        let mut has_vue = vc::is_script_setup(file);
        let mut has_name = false;
        for call in vc::vue_component_calls(file) {
            has_vue = true;
            if call.arguments.len() != 2 {
                continue;
            }
            has_name = true;
            if let Some(a) = call.arguments[0].as_expression() {
                self.validate(ctx, a);
            }
        }
        for o in vc::vue_objects(file) {
            has_vue = true;
            if let Some(p) = js::find_property(o.obj, "name") {
                has_name = true;
                self.validate(ctx, &p.value);
            }
        }
        for m in vc::macros_of(file, vc::MacroKind::DefineOptions) {
            if let Some(Expression::ObjectExpression(o)) = m.arg(0).map(skip_parens)
                && let Some(p) = js::find_property(o, "name")
            {
                has_name = true;
                self.validate(ctx, &p.value);
            }
        }
        let body_len = file.program.map_or(0, |p| p.body.len());
        if has_name || (!has_vue && body_len > 0) {
            return;
        }
        let name = file.path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if vc::is_vue_file(file) && !self.valid(name) {
            ctx.report(Span::new(0, 0), fmt_msg("Component name \"{{value}}\" should always be multi-word.", &[("value", name)]));
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let mut ignores = vec!["App".to_string(), "app".to_string()];
    if let Some(a) = opts.get(0)["ignores"].as_array() {
        for i in a.iter().filter_map(|v| v.as_str()) {
            ignores.push(i.to_string());
            if is_pascal_case(i) {
                ignores.push(kebab_case(i));
            }
        }
    }
    Some(Box::new(R { ignores }))
}
