//! vue/v-if-else-key

use std::collections::{HashMap, HashSet};

use crate::ast::{Child, Document, Element, ElementId};
use crate::rule::{Ctx, Fix, Rule, RuleOptions, fmt_msg};
use crate::utils::casing::kebab_case;
use crate::utils::template::is_custom_component;

struct R;

#[derive(Default)]
struct Family {
    r#if: Option<ElementId>,
    else_if: Vec<ElementId>,
    r#else: Option<ElementId>,
}

fn has_cond(e: &Element) -> bool {
    e.has_directive("if") || e.has_directive("else-if") || e.has_directive("else")
}

struct State<'d, 'c, 'f, 'a> {
    doc: &'d Document<'a>,
    ctx: &'c mut Ctx<'f, 'a>,
    families: HashMap<ElementId, Family>,
    stack: Vec<HashMap<String, (usize, Option<ElementId>)>>,
    pushed: HashSet<ElementId>,
}

const EXEMPT: [&str; 3] = ["component", "slot", "template"];

impl State<'_, '_, '_, '_> {
    fn check(&mut self, id: ElementId, name: &str, key: String) {
        let doc = self.doc;
        let e = doc.element(id);
        let Some(p) = e.parent else { return };
        let siblings = doc.element(p).children.iter().any(|c| {
            matches!(c, Child::Element(s) if *s != id && doc.element(*s).raw_name == name && has_cond(doc.element(*s)))
        });
        if !siblings {
            return;
        }
        let Some(f) = self.families.get(&p) else { return };
        if e.attr("key").is_some() || e.directive("bind", Some("key")).is_some() {
            return;
        }
        if f.r#if == Some(id) || f.r#else == Some(id) || f.else_if.contains(&id) {
            let at = e.start_tag.span.start + name.len() as u32 + 1;
            self.ctx.report_fix(
                e.start_tag.span,
                Some("requireKey"),
                fmt_msg(
                    "Conditionally rendered repeated component '{{componentName}}' expected to have a 'key' attribute.",
                    &[("componentName", name)],
                ),
                Some(Fix::insert(at, format!(" key=\"{key}\""))),
            );
        }
    }

    fn enter(&mut self, id: ElementId) {
        let doc = self.doc;
        let e = doc.element(id);
        if EXEMPT.contains(&e.raw_name.as_str()) {
            return;
        }
        let cond = ["if", "else-if", "else"].into_iter().find(|d| e.has_directive(d));
        if let (Some(ty), Some(p)) = (cond, e.parent) {
            let f = self.families.entry(p).or_insert_with(|| Family { r#if: Some(id), ..Default::default() });
            match ty {
                "if" => *f = Family { r#if: Some(id), ..Default::default() },
                "else-if" => f.else_if.push(id),
                _ => f.r#else = Some(id),
            }
        }
        let custom = is_custom_component(e, false);
        if custom && !has_cond(e) {
            self.stack.push(HashMap::new());
            return;
        }
        if !custom {
            return;
        }
        let name = e.raw_name.clone();
        let mut usage = self.stack.last().and_then(|m| m.get(&name)).copied().unwrap_or((0, None));
        // `custom && has_cond(e)` holds here.
        if usage.0 == 0 {
            usage.1 = Some(id);
        }
        if usage.0 > 0 {
            let k = kebab_case(&name);
            self.check(id, &name, format!("{k}-{}", usage.0 + 1));
            if usage.0 == 1
                && let Some(first) = usage.1
            {
                self.check(first, &name, format!("{k}-1"));
            }
        }
        usage.0 += 1;
        if let Some(m) = self.stack.last_mut() {
            m.insert(name, usage);
        }
        self.stack.push(HashMap::new());
        self.pushed.insert(id);
    }

    fn exit(&mut self, id: ElementId) {
        let e = self.doc.element(id);
        if EXEMPT.contains(&e.raw_name.as_str()) {
            return;
        }
        let custom = is_custom_component(e, false);
        if custom && !has_cond(e) {
            self.stack.pop();
            return;
        }
        if !custom || !self.pushed.remove(&id) {
            return;
        }
        self.stack.pop();
    }

    fn walk(&mut self, id: ElementId) {
        self.enter(id);
        let doc = self.doc;
        for c in &doc.element(id).children {
            if let Child::Element(c) = c {
                self.walk(*c);
            }
        }
        self.exit(id);
    }
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.file.doc.as_ref() else { return };
        let Some(root) = doc.template_body else { return };
        let mut st = State {
            doc,
            ctx,
            families: HashMap::new(),
            stack: vec![HashMap::new()],
            pushed: HashSet::new(),
        };
        st.walk(root);
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(R))
}
