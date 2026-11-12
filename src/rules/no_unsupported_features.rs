//! vue/no-unsupported-features


use oxc::span::{GetSpan, Span};

use crate::ast::*;
use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::template::{directive_value, is_vbind_same_name_shorthand, literal_value, value_span};
use crate::utils::tpl_misc::{body_attributes, style_v_binds};
use crate::utils::tpl_valid::{arg_span, expr_span};
use crate::utils::vue_component as vc;

// ---------------------------------------------------------------------------------------------
/// `semver.subset(sub, dom)`: every version allowed by `sub` is allowed by `dom` (node-semver semantics).
fn subset(sub: &node_semver::Range, dom: &node_semver::Range) -> bool {
    dom.allows_all(sub)
}

fn parse_range(s: &str) -> Option<node_semver::Range> {
    node_semver::Range::parse(s).ok()
}

// ---------------------------------------------------------------------------------------------
// Features
// ---------------------------------------------------------------------------------------------

const FEATURES: [(&str, &str, &str); 16] = [
    ("slot-scope-attribute", ">=2.5.0 <3.0.0", "`slot-scope` are not supported except Vue.js \">=2.5.0 <3.0.0\"."),
    ("dynamic-directive-arguments", ">=2.6.0", "Dynamic arguments are not supported until Vue.js \"2.6.0\"."),
    ("v-slot", ">=2.6.0", "`v-slot` are not supported until Vue.js \"2.6.0\"."),
    ("script-setup", ">=2.7.0", "`<script setup>` is not supported until Vue.js \"2.7.0\"."),
    (
        "style-css-vars-injection",
        ">=3.0.3 || >=2.7.0 <3.0.0",
        "SFC CSS variable injection is not supported until Vue.js \">=3.0.3 || >=2.7.0 <3.0.0\".",
    ),
    ("v-model-argument", ">=3.0.0", "Argument on `v-model` is not supported until Vue.js \"3.0.0\"."),
    ("v-model-custom-modifiers", ">=3.0.0", "Custom modifiers on `v-model` are not supported until Vue.js \"3.0.0\"."),
    ("v-is", ">=3.0.0", "`v-is` are not supported until Vue.js \"3.0.0\"."),
    ("is-attribute-with-vue-prefix", ">=3.1.0", "`is=\"vue:\"` are not supported until Vue.js \"3.1.0\"."),
    ("v-memo", ">=3.2.0", "`v-memo` are not supported until Vue.js \"3.2.0\"."),
    (
        "v-bind-prop-modifier-shorthand",
        ">=3.2.0 || >=2.6.0-beta.1 <=2.6.0-beta.3",
        "`.prop` shorthand are not supported until Vue.js \"3.2.0\".",
    ),
    ("v-bind-attr-modifier", ">=3.2.0", "`.attr` modifiers on `v-bind` are not supported until Vue.js \"3.2.0\"."),
    ("define-options", ">=3.3.0", "`defineOptions()` macros are not supported until Vue.js \"3.3.0\"."),
    ("define-slots", ">=3.3.0", "`defineSlots()` macros are not supported until Vue.js \"3.3.0\"."),
    ("define-model", ">=3.4.0", "`defineModel()` macros are not supported until Vue.js \"3.4.0\"."),
    (
        "v-bind-same-name-shorthand",
        ">=3.4.0",
        "`v-bind` same-name shorthand is not supported until Vue.js \"3.4.0\".",
    ),
];

struct R {
    /// Enabled feature names.
    on: Vec<&'static str>,
}

fn msg(name: &str) -> &'static str {
    FEATURES.iter().find(|f| f.0 == name).map_or("", |f| f.2)
}

/// `syntaxes/v-slot.ts` fix.
fn fix_v_slot_to_slot(doc: &Document, e: &Element, a: &Attribute, k: &DirectiveKey) -> Option<Fix> {
    if e.name != "template" || !k.modifiers.is_empty() {
        return None;
    }
    let mut attrs = Vec::new();
    match &k.argument {
        Some(DirectiveArgument::Static(id)) => attrs.push(format!("slot=\"{}\"", id.raw_name)),
        Some(DirectiveArgument::Dynamic(c)) => match &doc.expr(*c).expression {
            Some(VExpression::Expression(x)) => attrs.push(format!(":slot=\"{}\"", doc.slice(expr_span(x)))),
            Some(other) => attrs.push(format!(":slot=\"{}\"", doc.slice(vexpr_span(other)))),
            None => return None,
        },
        None => {}
    }
    if let Some(v) = value_span(doc, a) {
        attrs.push(format!("slot-scope={}", doc.slice(v)));
    }
    if attrs.is_empty() {
        attrs.push("slot".into());
    }
    Some(Fix::replace(a.span, attrs.join(" ")))
}

fn vexpr_span(x: &VExpression) -> Span {
    match x {
        VExpression::Expression(e) => expr_span(e),
        VExpression::FilterSequence { span, .. }
        | VExpression::For { span, .. }
        | VExpression::On { span, .. }
        | VExpression::SlotScope { span, .. }
        | VExpression::Generic { span, .. } => *span,
    }
}

/// `syntaxes/define-options.ts` fix.
fn fix_define_options(ctx: &Ctx, m: &vc::Macro) -> Option<Fix> {
    let file = ctx.file;
    let arg = m.call.arguments.first()?;
    let setup = vc::script_setup_block(file)?;
    let doc = file.doc.as_ref()?;
    if doc.top_level().any(|e| e.name == "script" && e.attr("setup").is_none()) {
        return None;
    }
    let arg_span = arg.as_expression().map_or(arg.span(), expr_span);
    let stmt = m.statement.span();
    let line_start = file.lines.line_start(file.lines.line_of(stmt.start)).unwrap_or(stmt.start);
    let start = if file.source[line_start as usize..stmt.start as usize].trim().is_empty() {
        line_start
    } else {
        stmt.start
    };
    Fix::merge(
        file.source,
        vec![
            Fix::insert(
                setup.span.start,
                format!("<script>\nexport default {}\n</script>\n", file.slice(arg_span)),
            ),
            Fix::remove(Span::new(start, stmt.end)),
        ],
    )
}

impl R {
    fn has(&self, f: &str) -> bool {
        self.on.contains(&f)
    }
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let file = ctx.file;
        if let Some(doc) = file.doc.as_ref() {
            for (e, a) in body_attributes(doc) {
                match &a.key {
                    AttributeKey::Plain(k) => {
                        if self.has("is-attribute-with-vue-prefix")
                            && k.name == "is"
                            && literal_value(a).is_some_and(|v| v.starts_with("vue:"))
                        {
                            ctx.report(value_span(doc, a).unwrap(), msg("is-attribute-with-vue-prefix"));
                        }
                    }
                    AttributeKey::Directive(k) => {
                        let d = k.name.name.as_str();
                        if self.has("slot-scope-attribute") && d == "slot-scope" {
                            ctx.report(k.span, msg("slot-scope-attribute"));
                        }
                        if self.has("dynamic-directive-arguments")
                            && let Some(DirectiveArgument::Dynamic(c)) = &k.argument
                        {
                            ctx.report(doc.expr(*c).span, msg("dynamic-directive-arguments"));
                        }
                        if self.has("v-slot") && d == "slot" {
                            let fix = fix_v_slot_to_slot(doc, e, a, k);
                            ctx.report_fix(k.span, None, msg("v-slot"), fix);
                        }
                        if d == "model" {
                            if self.has("v-model-argument")
                                && let Some(s) = arg_span(doc, k)
                            {
                                ctx.report(s, msg("v-model-argument"));
                            }
                            if self.has("v-model-custom-modifiers") {
                                for m in &k.modifiers {
                                    if !matches!(m.name.as_str(), "lazy" | "number" | "trim") {
                                        ctx.report(m.span, msg("v-model-custom-modifiers"));
                                    }
                                }
                            }
                        }
                        if self.has("v-is") && d == "is" {
                            ctx.report(k.span, msg("v-is"));
                        }
                        if self.has("v-memo") && d == "memo" {
                            ctx.report(k.span, msg("v-memo"));
                        }
                        if d == "bind" {
                            if self.has("v-bind-prop-modifier-shorthand")
                                && k.name.raw_name == "."
                                && let Some(DirectiveArgument::Static(id)) = &k.argument
                            {
                                let fix = Fix::replace(k.span, format!(":{}.prop", id.raw_name));
                                ctx.report_fix(k.span, None, msg("v-bind-prop-modifier-shorthand"), Some(fix));
                            }
                            if self.has("v-bind-attr-modifier")
                                && let Some(m) = k.modifiers.iter().find(|m| m.name == "attr")
                            {
                                ctx.report(m.span, msg("v-bind-attr-modifier"));
                            }
                            if self.has("v-bind-same-name-shorthand") && is_vbind_same_name_shorthand(doc, a) {
                                let name = directive_value(doc, a)
                                    .and_then(|c| match &c.expression {
                                        Some(VExpression::Expression(oxc::ast::ast::Expression::Identifier(i))) => {
                                            Some(i.name.to_string())
                                        }
                                        _ => None,
                                    })
                                    .unwrap_or_default();
                                let fix = Fix::insert(a.span.end, format!("=\"{name}\""));
                                ctx.report_fix(a.span, None, msg("v-bind-same-name-shorthand"), Some(fix));
                            }
                        }
                    }
                }
            }
            if self.has("script-setup")
                && let Some(s) = vc::script_setup_block(file)
                && let Some(el) = doc.top_level().find(|e| e.span == s.span)
            {
                ctx.report(el.attr("setup").map_or(el.start_tag.span, |a| a.span), msg("script-setup"));
            }
            if self.has("style-css-vars-injection") {
                for s in style_v_binds(doc) {
                    ctx.report(s, msg("style-css-vars-injection"));
                }
            }
        }
        for m in vc::script_setup_macros(file) {
            match m.kind {
                vc::MacroKind::DefineOptions if self.has("define-options") => {
                    let fix = fix_define_options(ctx, &m);
                    ctx.report_fix(m.call.span, None, msg("define-options"), fix);
                }
                vc::MacroKind::DefineSlots if self.has("define-slots") => ctx.report(m.call.span, msg("define-slots")),
                vc::MacroKind::DefineModel if self.has("define-model") => ctx.report(m.call.span, msg("define-model")),
                _ => {}
            }
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let o = opts.get(0);
    // Upstream: no `version` configured = the rule reports nothing.
    let Some(version) = o["version"].as_str().filter(|v| !v.is_empty()) else {
        return Some(Box::new(R { on: Vec::new() }));
    };
    let ignores: Vec<&str> = o["ignores"].as_array().map(|a| a.iter().filter_map(|v| v.as_str()).collect()).unwrap_or_default();
    // An invalid range is `null` in node-semver; `subset(null, …)` then treats it as `*`.
    let range = parse_range(version).unwrap_or_else(node_semver::Range::any);
    let on = FEATURES
        .iter()
        .filter(|f| !ignores.contains(&f.0) && !parse_range(f.1).is_some_and(|sup| subset(&range, &sup)))
        .map(|f| f.0)
        .collect();
    Some(Box::new(R { on }))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sub(a: &str, b: &str) -> bool {
        subset(&parse_range(a).unwrap(), &parse_range(b).unwrap())
    }
    #[test]
    fn semver_subset() {
        assert!(sub("^2.6.0", ">=2.5.0 <3.0.0"));
        assert!(!sub("^2.4.0", ">=2.5.0 <3.0.0"));
        assert!(!sub("^3.0.0", ">=2.5.0 <3.0.0"));
        assert!(sub("^3.0.3", ">=3.0.3 || >=2.7.0 <3.0.0"));
        assert!(!sub("^3.0.0", ">=3.0.3 || >=2.7.0 <3.0.0"));
        assert!(sub("^2.7.0", ">=3.0.3 || >=2.7.0 <3.0.0"));
        assert!(sub("2.6.0-beta.2", ">=3.2.0 || >=2.6.0-beta.1 <=2.6.0-beta.3"));
        assert!(!sub("2.5.99", ">=3.2.0 || >=2.6.0-beta.1 <=2.6.0-beta.3"));
        assert!(!sub(">=2.0.0", ">=2.6.0"));
        assert!(sub("3.4.0", ">=3.4.0"));
    }
}
