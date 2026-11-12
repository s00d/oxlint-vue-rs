//! vue/padding-lines-in-component-definition

use oxc::ast::ast::*;
use oxc::span::{GetSpan, Span};
use serde_json::Value;

use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::js::{self, ObjectOrArray};
use crate::utils::{tokens, vue_component as vc};

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let o = opts.get(0);
    let options = if o.is_null() {
        Value::String("always".into())
    } else {
        o.clone()
    };
    Some(Box::new(Padding { options }))
}

struct Padding {
    options: Value,
}

/// `parseOption`: a string option applies to every key, otherwise look the key up.
fn parse_option(v: &Value, key: &str) -> Value {
    if v.is_string() {
        v.clone()
    } else {
        v.get(key).cloned().unwrap_or(Value::Null)
    }
}

fn truthy(v: &Value) -> bool {
    !matches!(v, Value::Null | Value::Bool(false)) && v.as_str() != Some("")
}

fn split_last_line(s: &str) -> &str {
    let idx = s
        .char_indices()
        .filter(|(_, c)| matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}'))
        .map(|(i, c)| i + c.len_utf8())
        .next_back();
    &s[idx.unwrap_or(0)..]
}

impl Padding {
    fn verify(
        &self,
        ctx: &mut Ctx<'_, '_>,
        properties: &[ObjectPropertyKind],
        option: &Value,
        next_option: &Value,
    ) {
        let file = ctx.file;
        let group = match self.options.as_str() {
            Some("always") => true,
            Some("never") => false,
            _ => self
                .options
                .get("groupSingleLineProperties")
                .is_some_and(truthy),
        };
        for (i, current) in properties.iter().enumerate() {
            let cur = current.span();
            if let Some(next) = properties.get(i + 1)
                && option.as_str() != Some("ignore")
            {
                let next = next.span();
                let report = tokens::token_or_comment_before(file, next.start)
                    .filter(|t| tokens::is_comment(file, *t))
                    .unwrap_or(next);
                let range =
                    tokens::line(file, report.start) as i64 - tokens::line(file, cur.end) as i64;
                let comma_end = tokens::token_after_where(file, cur.end, |s| s == ",")
                    .map_or(cur.end, |c| c.end);
                let replace = || {
                    let padding = file.slice(Span::new(comma_end, report.start));
                    Fix::replace(
                        Span::new(comma_end, report.start),
                        format!("\n{}", split_last_line(padding)),
                    )
                };
                if group && tokens::is_single_line(file, cur) && tokens::is_single_line(file, next)
                {
                    if range > 1 {
                        ctx.report_fix(
                            report,
                            Some("groupSingleLineProperties"),
                            "Unexpected blank line between single line properties.",
                            Some(replace()),
                        );
                    }
                    continue;
                }
                if range <= 1 && option.as_str() == Some("always") {
                    let line_start = tokens::line_start(file, report.start) as usize;
                    let line = &file.source[line_start..];
                    let spaces = &line[..line.len() - line.trim_start().len()];
                    let text = if range == 0 {
                        format!("\n\n{spaces}")
                    } else {
                        "\n".to_string()
                    };
                    ctx.report_fix(
                        report,
                        Some("always"),
                        "Expected blank line before this definition.",
                        Some(Fix::insert(comma_end, text)),
                    );
                } else if range > 1 && option.as_str() == Some("never") {
                    ctx.report_fix(
                        report,
                        Some("never"),
                        "Unexpected blank line before this definition.",
                        Some(replace()),
                    );
                }
            }
            if !truthy(next_option) {
                continue;
            }
            let ObjectPropertyKind::ObjectProperty(p) = current else {
                continue;
            };
            let Some(name) = js::prop_name(p).filter(|n| !n.is_empty()) else {
                continue;
            };
            let prop_opt = parse_option(next_option, &name);
            if !truthy(&prop_opt) {
                continue;
            }
            let Expression::ObjectExpression(nested) = js::skip_parens(&p.value) else {
                continue;
            };
            self.verify(
                ctx,
                &nested.properties,
                &parse_option(&prop_opt, "betweenItems"),
                &parse_option(&prop_opt, "withinEach"),
            );
        }
    }

    fn verify_macro_object<'a>(
        &self,
        ctx: &mut Ctx<'_, 'a>,
        arg: Option<&'a Expression<'a>>,
        key: &str,
    ) {
        let Some(Some(ObjectOrArray::Object(o))) = arg.map(|a| js::object_or_array(ctx.file, a))
        else {
            return;
        };
        let within = parse_option(&self.options, "withinOption");
        if !truthy(&within) {
            return;
        }
        let opt = parse_option(&within, key);
        if !truthy(&opt) {
            return;
        }
        self.verify(
            ctx,
            &o.properties,
            &parse_option(&opt, "betweenItems"),
            &parse_option(&opt, "withinEach"),
        );
    }
}

impl Rule for Padding {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let file = ctx.file;
        let between = parse_option(&self.options, "betweenOptions");
        let within = parse_option(&self.options, "withinOption");
        for o in vc::vue_objects(file) {
            self.verify(ctx, &o.obj.properties, &between, &within);
        }
        for m in vc::script_setup_macros(file) {
            match m.kind {
                vc::MacroKind::DefineProps => self.verify_macro_object(ctx, m.arg(0), "props"),
                vc::MacroKind::DefineEmits => self.verify_macro_object(ctx, m.arg(0), "emits"),
                vc::MacroKind::DefineOptions => {
                    if let Some(Expression::ObjectExpression(o)) = m.arg(0).map(js::skip_parens) {
                        self.verify(ctx, &o.properties, &between, &within);
                    }
                }
                _ => {}
            }
        }
    }
}
