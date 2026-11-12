//! vue/require-explicit-emits

use std::collections::{HashMap, HashSet};

use oxc::ast::AstKind;
use oxc::ast::ast::{CallExpression, Expression, ObjectExpression};
use oxc::semantic::NodeId;
use oxc::span::{GetSpan, Span};

use crate::rule::{Ctx, Fix, Rule, RuleOptions, Suggestion, fmt_msg};
use crate::utils::casing::capitalize;
use crate::utils::js;
use crate::utils::tokens;
use crate::utils::tpl_analysis::{
    TemplateDefine, callee_ident, emit_setup, innermost_vue, member_call_object, name_param,
    template_calls, template_define,
};
use crate::utils::tpl_misc::is_this;
use crate::utils::vue_component::{
    self as vc, ComponentEmit, ComponentProp, MacroKind,
};

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let o = opts.get(0);
    Some(Box::new(RequireExplicitEmits {
        allow_props: o["allowProps"].as_bool() == Some(true),
        ignore_mixins: o["ignoreMixins"].as_bool() == Some(true),
    }))
}

struct RequireExplicitEmits {
    allow_props: bool,
    ignore_mixins: bool,
}

const FIX_EMITS_AFTER_OPTIONS: &[&str] = &[
    "setup", "data", "computed", "watch", "methods", "template", "render", "renderError",
    "beforeCreate", "created", "beforeMount", "mounted", "beforeUpdate", "updated", "activated",
    "deactivated", "beforeUnmount", "unmounted", "beforeDestroy", "destroyed", "renderTracked",
    "renderTriggered", "errorCaptured",
];

struct Decl<'a> {
    emits: Vec<ComponentEmit<'a>>,
    props: Vec<ComponentProp<'a>>,
    /// `None` = `<script setup>` (Program).
    obj: Option<&'a ObjectExpression<'a>>,
}

impl RequireExplicitEmits {
    fn verify(&self, ctx: &mut Ctx, d: &Decl, models: &HashSet<String>, name: &str, span: Span) {
        if models.contains(name) || d.emits.iter().any(|e| e.name().is_none_or(|n| n == name)) {
            return;
        }
        if self.allow_props {
            let key = format!("on{}", capitalize(name));
            if d.props.iter().any(|p| p.name().is_none_or(|n| n == key)) {
                return;
            }
        }
        let kind = if d.obj.is_some() { "`emits` option" } else { "`defineEmits`" };
        let sugg = suggest(ctx, d, name, kind);
        ctx.push(
            span,
            Some("missing"),
            fmt_msg(
                "The \"{{name}}\" event has been triggered but not declared on {{emitsKind}}.",
                &[("name", name), ("emitsKind", kind)],
            ),
            None,
            sugg,
        );
    }
}

fn sug(id: &'static str, msg: &str, name: &str, kind: &str, fix: Fix) -> Suggestion {
    Suggestion {
        message_id: Some(id),
        desc: fmt_msg(msg, &[("name", name), ("emitsKind", kind)]),
        fix,
    }
}

const ADD_ONE: &str = "Add the \"{{name}}\" to {{emitsKind}}.";

fn suggest(ctx: &Ctx, d: &Decl, name: &str, kind: &str) -> Vec<Suggestion> {
    let last = d
        .emits
        .iter()
        .rev()
        .find(|e| matches!(e, ComponentEmit::Array { .. } | ComponentEmit::Object { .. }));
    if let Some(last) = last {
        let fix = match last {
            ComponentEmit::Array { node, .. } => Fix::insert(node.span().end, format!(", '{name}'")),
            ComponentEmit::Object { prop, .. } => {
                Fix::insert(prop.span.end, format!(", '{name}': null"))
            }
            _ => unreachable!(),
        };
        return vec![sug("addOneOption", ADD_ONE, name, kind, fix)];
    }
    let Some(obj) = d.obj else { return Vec::new() };
    if let Some(p) = js::properties(obj).find(|p| js::prop_name(p).as_deref() == Some("emits")) {
        return match &p.value {
            Expression::ArrayExpression(a) => {
                let sep = if a.elements.is_empty() { "" } else { "," };
                vec![sug(
                    "addOneOption",
                    ADD_ONE,
                    name,
                    kind,
                    Fix::insert(a.span.start + 1, format!("'{name}'{sep}")),
                )]
            }
            Expression::ObjectExpression(o) => {
                let sep = if o.properties.is_empty() { "" } else { "," };
                vec![sug(
                    "addOneOption",
                    ADD_ONE,
                    name,
                    kind,
                    Fix::insert(o.span.start + 1, format!("'{name}': null{sep}")),
                )]
            }
            _ => Vec::new(),
        };
    }
    let after = js::properties(obj).find(|p| {
        js::prop_name(p).is_some_and(|n| FIX_EMITS_AFTER_OPTIONS.contains(&n.as_str()))
    });
    let file = ctx.file;
    let mk = |value: &str| -> Fix {
        if let Some(a) = after {
            let at = tokens::token_before(file, a.span.start).map_or(a.span.start, |t| t.end);
            return Fix::insert(at, format!("\nemits: {value},"));
        }
        if let Some(lastp) = obj.properties.last() {
            let before = js::properties(obj).last().map_or(js::member_span(lastp), |p| p.span);
            return Fix::insert(before.end, format!(",\nemits: {value}"));
        }
        let l = file.lines.line_of(obj.span.start);
        let r = file.lines.line_of(obj.span.end.saturating_sub(1));
        let tail = if l < r { "" } else { "\n" };
        Fix::insert(obj.span.start + 1, format!("\nemits: {value}{tail}"))
    };
    vec![
        sug(
            "addArrayEmitsOption",
            "Add the {{emitsKind}} with array syntax and define \"{{name}}\" event.",
            name,
            kind,
            mk(&format!("['{name}']")),
        ),
        sug(
            "addObjectEmitsOption",
            "Add the {{emitsKind}} with object syntax and define \"{{name}}\" event.",
            name,
            kind,
            mk(&format!("{{'{name}': null}}")),
        ),
    ]
}

impl Rule for RequireExplicitEmits {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let file = ctx.file;
        let setup = emit_setup(file, true, false);
        let mut models = HashSet::new();
        let mut program: Option<(u32, Decl)> = None;
        let mut setup_props = Vec::new();
        for m in vc::script_setup_macros(file) {
            match m.kind {
                MacroKind::DefineEmits => {
                    program = Some((
                        m.call.span.start,
                        Decl {
                            emits: vc::emits_from_define_emits(file, m.call),
                            props: Vec::new(),
                            obj: None,
                        },
                    ));
                }
                MacroKind::DefineModel => {
                    models.insert(format!("update:{}", vc::model_from_define_model(m.call).name));
                }
                MacroKind::DefineProps if self.allow_props => {
                    setup_props = vc::props_from_define_props(file, m.call);
                }
                _ => {}
            }
        }
        if let Some((_, d)) = &mut program {
            d.props = setup_props.clone();
        }

        let objs = vc::vue_objects(file);
        let mut decls: HashMap<NodeId, Decl> = HashMap::new();
        for o in &objs {
            if self.ignore_mixins && js::find_property(o.obj, "mixins").is_some() {
                continue;
            }
            decls.insert(
                o.node_id(),
                Decl {
                    emits: vc::emits_from_options(o.obj),
                    props: if self.allow_props { vc::props_from_options(o.obj) } else { Vec::new() },
                    obj: Some(o.obj),
                },
            );
        }

        if let Some(sem) = file.semantic {
            let vue_ids: HashSet<NodeId> = decls.keys().copied().collect();
            for n in sem.nodes().iter() {
                let AstKind::CallExpression(call) = n.kind() else {
                    continue;
                };
                let Some((name, span)) = name_param(call) else {
                    continue;
                };
                if vc::in_script_setup(file, call.span)
                    && let Some((start, d)) = &program
                    && call.span.start >= *start
                {
                    let c = setup.program.as_ref().filter(|(s, _)| call.span.start >= *s);
                    self.check_script(ctx, sem, call, c.map(|(_, c)| c), d, &models, &name, span);
                }
                if let Some(v) = innermost_vue(sem, &vue_ids, n.id()) {
                    let d = &decls[&v];
                    self.check_script(ctx, sem, call, setup.vue.get(&v), d, &models, &name, span);
                }
            }
        }

        // Template.
        let Some(doc) = file.doc.as_ref() else { return };
        let empty = Decl { emits: Vec::new(), props: setup_props, obj: None };
        let define = template_define(file, &objs, |o| decls.contains_key(&o.node_id()));
        let d = match &define {
            None => return,
            Some(TemplateDefine::Setup) => program.as_ref().map_or(&empty, |(_, d)| d),
            Some(TemplateDefine::Vue(o)) => &decls[&o.node_id()],
        };
        for call in template_calls(doc) {
            let Some((name, span)) = name_param(call) else {
                continue;
            };
            if callee_ident(call).is_some_and(|c| c == "$emit" || setup.param_name.as_deref() == Some(c)) {
                self.verify(ctx, d, &models, &name, span);
            }
        }
    }
}

impl RequireExplicitEmits {
    #[allow(clippy::too_many_arguments)]
    fn check_script(
        &self,
        ctx: &mut Ctx,
        sem: &oxc::semantic::Semantic,
        call: &CallExpression,
        c: Option<&crate::utils::tpl_analysis::EmitCtx>,
        d: &Decl,
        models: &HashSet<String>,
        name: &str,
        span: Span,
    ) {
        if c.is_some_and(|c| c.matches(call)) {
            self.verify(ctx, d, models, name, span);
        }
        if member_call_object(call, "$emit").is_some_and(|o| is_this(sem, o)) {
            self.verify(ctx, d, models, name, span);
        }
    }
}
