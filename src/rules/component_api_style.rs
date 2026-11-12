//! vue/component-api-style

use oxc::span::Span;

use crate::rule::{Ctx, Rule, RuleOptions, fmt_msg};
use crate::utils::js;
use crate::utils::vue_component as vc;

#[derive(Default, Clone, Copy)]
struct Allows {
    script_setup: bool,
    composition: bool,
    composition_vue2: bool,
    options: bool,
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let mut sfc = Allows::default();
    let mut other = Allows::default();
    let prefer: Vec<String> = match opts.get(0).as_array() {
        Some(a) => a
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        None => vec!["script-setup".into(), "composition".into()],
    };
    for p in &prefer {
        match p.as_str() {
            "script-setup" => sfc.script_setup = true,
            "composition" => (sfc.composition, other.composition) = (true, true),
            "composition-vue2" => (sfc.composition_vue2, other.composition_vue2) = (true, true),
            "options" => (sfc.options, other.options) = (true, true),
            _ => {}
        }
    }
    if !other.composition && !other.composition_vue2 && !other.options {
        other = Allows {
            script_setup: false,
            composition: true,
            composition_vue2: true,
            options: true,
        };
    }
    Some(Box::new(ComponentApiStyle { sfc, other }))
}

struct ComponentApiStyle {
    sfc: Allows,
    other: Allows,
}

const OPTIONS_API: &[&str] = &[
    "mixins",
    "extends",
    "data",
    "computed",
    "methods",
    "watch",
    "provide",
    "inject",
    "beforeCreate",
    "created",
    "beforeMount",
    "mounted",
    "beforeUpdate",
    "updated",
    "activated",
    "deactivated",
    "beforeDestroy",
    "beforeUnmount",
    "destroyed",
    "unmounted",
    "render",
    "renderTracked",
    "renderTriggered",
    "errorCaptured",
    "expose",
];
const COMPOSITION_API: &[&str] = &["setup"];
const COMPOSITION_API_VUE2: &[&str] = &["setup", "render", "renderTracked", "renderTriggered"];
const LIFECYCLE_HOOKS: &[&str] = &[
    "beforeCreate",
    "created",
    "beforeMount",
    "mounted",
    "beforeUpdate",
    "updated",
    "activated",
    "deactivated",
    "beforeDestroy",
    "beforeUnmount",
    "destroyed",
    "unmounted",
    "renderTracked",
    "renderTriggered",
    "errorCaptured",
];

fn allowed_phrase(a: Allows) -> String {
    let mut p = Vec::new();
    if a.script_setup {
        p.push("`<script setup>`");
    }
    if a.composition {
        p.push("Composition API");
    }
    if a.composition_vue2 {
        p.push("Composition API (Vue 2)");
    }
    if a.options {
        p.push("Options API");
    }
    if p.len() > 2 {
        format!("{} or {}", p[..p.len() - 1].join(", "), p[p.len() - 1])
    } else {
        p.join(" or ")
    }
}

fn option_phrase(name: &str) -> String {
    if LIFECYCLE_HOOKS.contains(&name) {
        format!("`{name}` lifecycle hook")
    } else if name == "setup" || name == "render" {
        format!("`{name}` function")
    } else {
        format!("`{name}` option")
    }
}

impl Rule for ComponentApiStyle {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let file = ctx.file;
        if !self.sfc.script_setup
            && let Some(s) = vc::script_setup_block(file)
        {
            let msg = fmt_msg(
                "`<script setup>` is not allowed in your project. Use {{allowedApis}} instead.",
                &[("allowedApis", &allowed_phrase(self.sfc))],
            );
            ctx.report_id(
                Span::new(s.span.start, s.content.start),
                "disallowScriptSetup",
                msg,
            );
        }
        for o in vc::vue_objects(file) {
            let a = if vc::is_sfc_object(file, o.obj) {
                self.sfc
            } else {
                self.other
            };
            if (a.composition || a.composition_vue2) && a.options {
                continue;
            }
            let apis = [
                (a.composition, COMPOSITION_API, "Composition API"),
                (a.options, OPTIONS_API, "Options API"),
                (
                    a.composition_vue2,
                    COMPOSITION_API_VUE2,
                    "Composition API (Vue 2)",
                ),
            ];
            for p in js::properties(o.obj) {
                let Some(name) = js::prop_name(p).filter(|n| !n.is_empty()) else {
                    continue;
                };
                let n = name.as_str();
                if apis
                    .iter()
                    .any(|(allow, opts, _)| *allow && opts.contains(&n))
                {
                    continue;
                }
                let Some((_, _, api)) = apis
                    .iter()
                    .find(|(allow, opts, _)| !*allow && opts.contains(&n))
                else {
                    continue;
                };
                let prefer_setup =
                    a.script_setup && !a.composition && !a.composition_vue2 && !a.options;
                let key = oxc::span::GetSpan::span(&p.key);
                if prefer_setup {
                    ctx.report_id(
                        key,
                        "disallowComponentOptionPreferScriptSetup",
                        format!(
                            "{api} is not allowed in your project. Use `<script setup>` instead."
                        ),
                    );
                } else {
                    let msg = format!(
                        "{api} is not allowed in your project. {} is part of the {api}. Use {} instead.",
                        option_phrase(n),
                        allowed_phrase(a)
                    );
                    ctx.report_id(key, "disallowComponentOption", msg);
                }
            }
        }
    }
}
