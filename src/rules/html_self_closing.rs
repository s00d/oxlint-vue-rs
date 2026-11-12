//! vue/html-self-closing

use oxc::span::Span;

use crate::ast::*;
use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::template::{
    is_custom_component, is_html_void_element_name, is_vue_builtin_element_name,
};
use crate::utils::tpl_style::{has_invalid_eof, last_token};
use crate::utils::tpl_valid::body_elements;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Always,
    Never,
    Any,
}

#[derive(Clone, Copy, PartialEq)]
enum Ty {
    Normal,
    Void,
    Component,
    Svg,
    Math,
}

struct R {
    normal: Mode,
    void: Mode,
    component: Mode,
    svg: Mode,
    math: Mode,
}

fn mode(v: &serde_json::Value, default: Mode) -> Mode {
    match v.as_str() {
        Some("always") => Mode::Always,
        Some("never") => Mode::Never,
        Some("any") => Mode::Any,
        _ => default,
    }
}

fn element_type(e: &Element) -> Ty {
    if is_custom_component(e, false) || is_vue_builtin_element_name(&e.raw_name) {
        return Ty::Component;
    }
    match e.namespace {
        Namespace::Html if is_html_void_element_name(&e.name) => Ty::Void,
        Namespace::Html => Ty::Normal,
        Namespace::Svg => Ty::Svg,
        Namespace::MathMl => Ty::Math,
    }
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        if has_invalid_eof(doc) {
            return;
        }
        let src = ctx.source();
        for e in body_elements(doc) {
            if e.parent.is_none() {
                continue;
            }
            let ty = element_type(e);
            let (m, label) = match ty {
                Ty::Normal => (self.normal, "HTML elements"),
                Ty::Void => (self.void, "HTML void elements"),
                Ty::Component => (self.component, "Vue.js custom components"),
                Ty::Svg => (self.svg, "SVG elements"),
                Ty::Math => (self.math, "MathML elements"),
            };
            let st = &e.start_tag;
            let close = last_token(doc, st.span, 0);
            if m == Mode::Always && !st.self_closing {
                let end = e.end_tag.map_or(e.span.end, |t| t.start);
                // HTML whitespace only: Vue keeps U+00A0, U+3000, … as text (JS `trim()` drops them).
                // In (an element inside) `<pre>` / `<textarea>` whitespace is content too.
                let content = &src[st.span.end as usize..end as usize];
                let mut pre = matches!(e.name.as_str(), "pre" | "textarea");
                let mut up = e.parent;
                while let Some(id) = up {
                    pre |= doc.element(id).name == "pre";
                    up = doc.element(id).parent;
                }
                if (pre && content.is_empty())
                    || (!pre
                        && content
                            .trim_matches(|c| matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0c'))
                            .is_empty())
                {
                    let fix = close
                        .filter(|c| c.kind == TokenKind::HTMLTagClose)
                        .map(|c| {
                            let ws = src[..c.span.start as usize]
                                .chars()
                                .next_back()
                                .is_some_and(char::is_whitespace);
                            Fix::replace(
                                Span::new(c.span.start, e.span.end),
                                if ws { "/>" } else { " />" },
                            )
                        });
                    ctx.report_fix(
                        e.end_tag.unwrap_or(e.span),
                        Some("requireSelfClosing"),
                        format!("Require self-closing on {label} (<{}>).", e.raw_name),
                        fix,
                    );
                }
            }
            if m == Mode::Never && st.self_closing {
                let fix = close
                    .filter(|c| c.kind == TokenKind::HTMLSelfClosingTagClose)
                    .map(|c| {
                        if ty == Ty::Void {
                            Fix::replace(c.span, ">")
                        } else {
                            let part = &src[e.span.start as usize..c.span.start as usize];
                            Fix::replace(e.span, format!("{part}></{}>", e.raw_name))
                        }
                    });
                ctx.report_fix(
                    Span::new(e.span.end.saturating_sub(2), e.span.end),
                    Some("disallowSelfClosing"),
                    format!("Disallow self-closing on {label} (<{}/>).", e.raw_name),
                    fix,
                );
            }
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let o = opts.get(0);
    Some(Box::new(R {
        normal: mode(&o["html"]["normal"], Mode::Always),
        void: mode(&o["html"]["void"], Mode::Never),
        component: mode(&o["html"]["component"], Mode::Always),
        svg: mode(&o["svg"], Mode::Always),
        math: mode(&o["math"], Mode::Always),
    }))
}
