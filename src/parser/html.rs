//! Port of vue-eslint-parser `src/html/parser.ts`: builds the element tree from intermediate
//! tokens (implicit end tags, void elements, namespaces, RCDATA/RAWTEXT switching, `v-pre`).
//! Directive conversion and mustache parsing are deferred to post-processes like upstream.

use oxc::span::Span;

use super::intermediate::{IToken, Intermediate, RawAttr, StartTag as RawStartTag};
use super::tokenizer::{State, Tokenizer};
use crate::ast::*;

pub(super) const NS_HTML: &str = "http://www.w3.org/1999/xhtml";
pub(super) const NS_MATHML: &str = "http://www.w3.org/1998/Math/MathML";
pub(super) const NS_SVG: &str = "http://www.w3.org/2000/svg";
const NS_XLINK: &str = "http://www.w3.org/1999/xlink";

fn ns_url(ns: Namespace) -> &'static str {
    match ns {
        Namespace::Html => NS_HTML,
        Namespace::Svg => NS_SVG,
        Namespace::MathMl => NS_MATHML,
    }
}

fn is_void(name: &str) -> bool {
    matches!(
        name,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}
fn can_be_left_open(name: &str) -> bool {
    matches!(
        name,
        "colgroup" | "li" | "options" | "p" | "td" | "tfoot" | "th" | "thead" | "tr" | "source"
    )
}
fn is_non_phrasing(name: &str) -> bool {
    matches!(
        name,
        "address"
            | "article"
            | "aside"
            | "base"
            | "blockquote"
            | "body"
            | "caption"
            | "col"
            | "colgroup"
            | "dd"
            | "details"
            | "dialog"
            | "div"
            | "dl"
            | "dt"
            | "fieldset"
            | "figcaption"
            | "figure"
            | "footer"
            | "form"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "head"
            | "header"
            | "hgroup"
            | "hr"
            | "html"
            | "legend"
            | "li"
            | "menuitem"
            | "meta"
            | "optgroup"
            | "option"
            | "param"
            | "rp"
            | "rt"
            | "source"
            | "style"
            | "summary"
            | "tbody"
            | "td"
            | "tfoot"
            | "th"
            | "thead"
            | "title"
            | "tr"
            | "track"
    )
}
fn is_rcdata(name: &str) -> bool {
    matches!(name, "title" | "textarea")
}
fn is_rawtext(name: &str) -> bool {
    matches!(
        name,
        "style" | "xmp" | "iframe" | "noembed" | "noframes" | "noscript" | "script"
    )
}
fn is_dt_dd(name: &str) -> bool {
    name == "dd" || name == "dt"
}

/// `SVG_ELEMENT_NAME_MAP`: lowercase → camelCase SVG tag names.
fn adjust_svg_element(name: &str) -> Option<&'static str> {
    const NAMES: &[&str] = &[
        "altGlyph",
        "altGlyphDef",
        "altGlyphItem",
        "animateColor",
        "animateMotion",
        "animateTransform",
        "clipPath",
        "feBlend",
        "feColorMatrix",
        "feComponentTransfer",
        "feComposite",
        "feConvolveMatrix",
        "feDiffuseLighting",
        "feDisplacementMap",
        "feDistantLight",
        "feDropShadow",
        "feFlood",
        "feFuncA",
        "feFuncB",
        "feFuncG",
        "feFuncR",
        "feGaussianBlur",
        "feImage",
        "feMerge",
        "feMergeNode",
        "feMorphology",
        "feOffset",
        "fePointLight",
        "feSpecularLighting",
        "feSpotLight",
        "feTile",
        "feTurbulence",
        "foreignObject",
        "glyphRef",
        "linearGradient",
        "radialGradient",
        "solidColor",
        "textArea",
        "textPath",
    ];
    NAMES
        .iter()
        .copied()
        .find(|n| n.eq_ignore_ascii_case(name) && n.to_ascii_lowercase() == name)
}

fn adjust_attribute_name(name: &str, ns: Namespace) -> Option<&'static str> {
    const SVG: &[&str] = &[
        "attributeName",
        "attributeType",
        "baseFrequency",
        "baseProfile",
        "calcMode",
        "clipPathUnits",
        "diffuseConstant",
        "edgeMode",
        "filterUnits",
        "glyphRef",
        "gradientTransform",
        "gradientUnits",
        "kernelMatrix",
        "kernelUnitLength",
        "keyPoints",
        "keySplines",
        "keyTimes",
        "lengthAdjust",
        "limitingConeAngle",
        "markerHeight",
        "markerUnits",
        "markerWidth",
        "maskContentUnits",
        "maskUnits",
        "numOctaves",
        "pathLength",
        "patternContentUnits",
        "patternTransform",
        "patternUnits",
        "pointsAtX",
        "pointsAtY",
        "pointsAtZ",
        "preserveAlpha",
        "preserveAspectRatio",
        "primitiveUnits",
        "refX",
        "refY",
        "repeatCount",
        "repeatDur",
        "requiredExtensions",
        "requiredFeatures",
        "specularConstant",
        "specularExponent",
        "spreadMethod",
        "startOffset",
        "stdDeviation",
        "stitchTiles",
        "surfaceScale",
        "systemLanguage",
        "tableValues",
        "targetX",
        "targetY",
        "textLength",
        "viewBox",
        "viewTarget",
        "xChannelSelector",
        "yChannelSelector",
        "zoomAndPan",
    ];
    match ns {
        Namespace::Svg => SVG.iter().copied().find(|n| n.to_ascii_lowercase() == name),
        Namespace::MathMl => (name == "definitionurl").then_some("definitionUrl"),
        Namespace::Html => None,
    }
}

/// `/^(?:v-|[.:@#]).*[^.:@#]$/u`
fn is_directive_name(name: &str) -> bool {
    let starts = name.starts_with("v-") || name.starts_with(['.', ':', '@', '#']);
    if !starts {
        return false;
    }
    // `.*` does not match line terminators.
    if name.contains(['\n', '\r', '\u{2028}', '\u{2029}']) {
        return false;
    }
    let last = name.chars().last();
    name.chars().count() >= 2 && !matches!(last, Some('.' | ':' | '@' | '#'))
}

/// A deferred post-process (run after the whole tree is built, like upstream).
#[derive(Debug)]
pub(super) enum Post {
    /// Convert `start_tag.attributes[attr]` of `element` to a directive.
    Directive { element: ElementId, attr: u32 },
    /// Parse a mustache.
    Mustache {
        expr: ExprId,
        value: String,
        start: u32,
        end: u32,
    },
}

pub(super) struct TreeBuilder<'a> {
    pub itok: Intermediate<'a>,
    pub elements: Vec<Element>,
    pub texts: Vec<Text>,
    /// Mustache containers (expressions filled in later).
    pub mustaches: Vec<(ExprId, Span, Option<ElementId>)>,
    pub children: Vec<Child>,
    pub posts: Vec<Post>,
    pub next_expr: u32,
    stack: Vec<ElementId>,
    v_pre: Option<ElementId>,
}

impl<'a> TreeBuilder<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            itok: Intermediate::new(Tokenizer::new(source)),
            elements: Vec::new(),
            texts: Vec::new(),
            mustaches: Vec::new(),
            children: Vec::new(),
            posts: Vec::new(),
            next_expr: 0,
            stack: Vec::new(),
            v_pre: None,
        }
    }

    pub fn parse(&mut self) {
        while let Some(t) = self.itok.next_token() {
            match t {
                IToken::StartTag(t) => self.start_tag(t),
                IToken::EndTag { start, end, name } => self.end_tag(start, end, &name),
                IToken::Text { start, end, value } => self.text(start, end, value),
                IToken::Mustache {
                    start,
                    end,
                    value,
                    start_tok_end,
                    end_tok_start,
                } => self.mustache(start, end, value, start_tok_end, end_tok_start),
            }
        }
        self.pop_until(0);
    }

    fn report(&mut self, offset: u32, code: &'static str) {
        self.itok.tokenizer.errors.push((code, offset));
    }

    fn current(&self) -> Option<ElementId> {
        self.stack.last().copied()
    }

    fn push_child(&mut self, parent: Option<ElementId>, child: Child) {
        match parent {
            Some(p) => self.elements[p.0 as usize].children.push(child),
            None => self.children.push(child),
        }
    }

    fn child_end(&self, c: Child) -> u32 {
        match c {
            Child::Element(e) => self.elements[e.0 as usize].span.end,
            Child::Text(t) => self.texts[t.0 as usize].span.end,
            Child::Expr(x) => self
                .mustaches
                .iter()
                .find(|m| m.0 == x)
                .map_or(0, |m| m.1.end),
        }
    }

    fn pop(&mut self) {
        let Some(id) = self.stack.pop() else { return };
        // propagateEndLocation
        let el = &self.elements[id.0 as usize];
        let end = el
            .end_tag
            .map(|t| t.end)
            .or_else(|| el.children.last().map(|&c| self.child_end(c)));
        if let Some(end) = end {
            self.elements[id.0 as usize].span.end = end;
        }
        let ns = self
            .current()
            .map_or(Namespace::Html, |c| self.elements[c.0 as usize].namespace);
        self.itok.set_namespace(ns);
        if self.v_pre == Some(id) {
            self.v_pre = None;
            self.itok.set_expression_enabled(true);
        }
        if self.stack.is_empty() {
            self.itok.set_expression_enabled(false);
        }
    }

    fn pop_until(&mut self, index: usize) {
        while self.stack.len() > index {
            self.pop();
        }
    }

    fn detect_namespace(&self, token: &RawStartTag) -> Namespace {
        let name = token.raw_name.as_str();
        let mut ns = self.itok.namespace();
        if (ns == Namespace::MathMl || ns == Namespace::Svg)
            && let Some(cur) = self.current()
        {
            let el = &self.elements[cur.0 as usize];
            if el.namespace == Namespace::MathMl && el.raw_name == "annotation-xml" && name == "svg"
            {
                return Namespace::Svg;
            }
            if is_html_integration_point(el)
                || (is_mathml_integration_point(el) && name != "mglyph" && name != "malignmark")
            {
                ns = Namespace::Html;
            }
        }
        if ns == Namespace::Html {
            if name == "svg" {
                return Namespace::Svg;
            }
            if name == "math" {
                return Namespace::MathMl;
            }
        }
        if name == "template"
            && let Some(v) = token
                .attributes
                .iter()
                .find(|a| a.name == "xmlns")
                .and_then(|a| a.value.as_ref())
        {
            match v.2.as_str() {
                NS_HTML => return Namespace::Html,
                NS_MATHML => return Namespace::MathMl,
                NS_SVG => return Namespace::Svg,
                _ => {}
            }
        }
        ns
    }

    fn close_current_element_if_necessary(&mut self, name: &str) {
        let Some(cur) = self.current() else { return };
        let element_name = self.elements[cur.0 as usize].raw_name.clone();
        if element_name == "p" && is_non_phrasing(name) {
            self.pop();
        }
        if element_name == name && can_be_left_open(name) {
            self.pop();
        }
        if is_dt_dd(&element_name) && is_dt_dd(name) {
            self.pop();
        }
    }

    fn start_tag(&mut self, token: RawStartTag) {
        self.close_current_element_if_necessary(&token.raw_name.clone());
        let parent = self.current();
        let namespace = self.detect_namespace(&token);
        let id = ElementId(self.elements.len() as u32);
        let text = self.itok.tokenizer.text();
        let name = match namespace {
            Namespace::Svg => {
                adjust_svg_element(&token.name).map_or_else(|| token.name.clone(), str::to_string)
            }
            _ => token.name.clone(),
        };
        let has_v_pre =
            self.v_pre.is_none() && token.attributes.iter().any(|a| key_raw(text, a) == "v-pre");
        if has_v_pre {
            self.itok.set_expression_enabled(false);
        }
        let attributes: Vec<Attribute> = token
            .attributes
            .iter()
            .map(|a| Attribute {
                span: Span::new(a.start, a.end),
                key: AttributeKey::Plain(VIdentifier {
                    span: Span::new(a.key_start, a.key_end),
                    name: a.name.clone(),
                    raw_name: key_raw(text, a).to_string(),
                }),
                value: a.value.as_ref().map(|(s, e, v)| {
                    AttributeValue::Literal(VLiteral {
                        span: Span::new(*s, *e),
                        value: v.clone(),
                    })
                }),
            })
            .collect();
        self.elements.push(Element {
            id,
            parent,
            span: Span::new(token.start, token.end),
            namespace,
            name,
            raw_name: token.raw_name.clone(),
            start_tag: StartTag {
                span: Span::new(token.start, token.end),
                self_closing: token.self_closing,
                attributes,
            },
            end_tag: None,
            children: Vec::new(),
            variables: Vec::new(),
        });
        self.push_child(parent, Child::Element(id));
        for i in 0..token.attributes.len() {
            self.process_attribute(id, i, namespace);
        }

        let is_void_el = namespace == Namespace::Html && is_void(&token.raw_name);
        if token.self_closing && !is_void_el && namespace == Namespace::Html {
            self.report(
                token.start,
                "non-void-html-element-start-tag-with-trailing-solidus",
            );
        }
        // Vue.js supports self-closing elements even if it's not one of void elements.
        if token.self_closing || is_void_el {
            self.itok.set_expression_enabled(self.v_pre.is_none());
            return;
        }
        self.stack.push(id);
        if has_v_pre {
            self.v_pre = Some(id);
        }
        self.itok.set_namespace(namespace);

        if namespace == Namespace::Html {
            let element_name = token.raw_name.as_str();
            if parent.is_none() {
                let lang = token
                    .attributes
                    .iter()
                    .find(|a| a.name == "lang")
                    .and_then(|a| a.value.as_ref())
                    .map(|v| v.2.as_str());
                if element_name == "template" {
                    self.itok.set_expression_enabled(true);
                    if lang.is_some_and(|l| !l.is_empty() && l != "html") {
                        // It is not an HTML template.
                        self.itok.set_state(State::Rawtext);
                        self.itok.set_expression_enabled(false);
                    }
                } else if lang != Some("html") {
                    // SFC custom block (or script/style): not HTML.
                    self.itok.set_state(State::Rawtext);
                }
            } else {
                if is_rcdata(element_name) {
                    self.itok.set_state(State::Rcdata);
                }
                if is_rawtext(element_name) {
                    self.itok.set_state(State::Rawtext);
                }
            }
        }
    }

    fn process_attribute(&mut self, element: ElementId, idx: usize, namespace: Namespace) {
        if self.need_convert_to_directive(element, idx) {
            self.posts.push(Post::Directive {
                element,
                attr: idx as u32,
            });
            return;
        }
        let el = &mut self.elements[element.0 as usize];
        let attr = &mut el.start_tag.attributes[idx];
        let AttributeKey::Plain(key) = &mut attr.key else {
            return;
        };
        if let Some(n) = adjust_attribute_name(&key.name, namespace) {
            key.name = n.to_string();
        }
        let raw = key.raw_name.clone();
        let value = match &attr.value {
            Some(AttributeValue::Literal(l)) => Some(l.value.clone()),
            _ => None,
        };
        let start = attr.span.start;
        let bad = (raw == "xmlns" && value.as_deref() != Some(ns_url(namespace)))
            || (raw == "xmlns:xlink" && value.as_deref() != Some(NS_XLINK));
        if bad {
            self.report(start, "x-invalid-namespace");
        }
    }

    fn need_convert_to_directive(&self, element: ElementId, idx: usize) -> bool {
        let el = &self.elements[element.0 as usize];
        let AttributeKey::Plain(key) = &el.start_tag.attributes[idx].key else {
            return false;
        };
        let attr_name = key.raw_name.as_str();
        if attr_name == "generic" && el.parent.is_none() && is_script_setup(el) && is_ts_lang(el) {
            return true;
        }
        let enabled =
            self.itok.expression_enabled() || (attr_name == "v-pre" && self.v_pre.is_none());
        if !enabled {
            return false;
        }
        is_directive_name(attr_name)
            || attr_name == "slot-scope"
            || (el.raw_name == "template" && attr_name == "scope")
    }

    fn end_tag(&mut self, start: u32, end: u32, name: &str) {
        let found = self
            .stack
            .iter()
            .rposition(|&e| self.elements[e.0 as usize].name.to_lowercase() == name);
        let Some(i) = found else {
            self.report(start, "x-invalid-end-tag");
            return;
        };
        let id = self.stack[i];
        self.elements[id.0 as usize].end_tag = Some(Span::new(start, end));
        self.pop_until(i);
    }

    fn text(&mut self, start: u32, end: u32, value: String) {
        let parent = self.current();
        let id = TextId(self.texts.len() as u32);
        self.texts.push(Text {
            span: Span::new(start, end),
            parent,
            value,
        });
        self.push_child(parent, Child::Text(id));
    }

    fn mustache(
        &mut self,
        start: u32,
        end: u32,
        value: String,
        start_tok_end: u32,
        end_tok_start: u32,
    ) {
        let parent = self.current();
        let id = ExprId(self.next_expr);
        self.next_expr += 1;
        self.mustaches.push((id, Span::new(start, end), parent));
        self.push_child(parent, Child::Expr(id));
        self.posts.push(Post::Mustache {
            expr: id,
            value,
            start: start_tok_end,
            end: end_tok_start,
        });
    }
}

fn key_raw<'s>(text: &'s str, a: &RawAttr) -> &'s str {
    text.get(a.key_start as usize..a.key_end as usize)
        .unwrap_or("")
}

pub(super) fn plain_attr<'e>(el: &'e Element, name: &str) -> Option<&'e Attribute> {
    el.start_tag
        .attributes
        .iter()
        .find(|a| matches!(&a.key, AttributeKey::Plain(k) if k.name == name))
}

pub(super) fn is_script_setup(el: &Element) -> bool {
    el.name == "script" && plain_attr(el, "setup").is_some()
}

pub(super) fn is_ts_lang(el: &Element) -> bool {
    let lang = plain_attr(el, "lang").and_then(|a| match &a.value {
        Some(AttributeValue::Literal(l)) => Some(l.value.as_str()),
        _ => None,
    });
    matches!(lang, Some("ts" | "tsx"))
}

fn is_mathml_integration_point(el: &Element) -> bool {
    el.namespace == Namespace::MathMl
        && matches!(el.raw_name.as_str(), "mi" | "mo" | "mn" | "ms" | "mtext")
}

fn is_html_integration_point(el: &Element) -> bool {
    match el.namespace {
        Namespace::MathMl => {
            el.raw_name == "annotation-xml"
                && el.start_tag.attributes.iter().any(|a| {
                    matches!(&a.key, AttributeKey::Plain(k) if k.name == "encoding")
                        && matches!(&a.value, Some(AttributeValue::Literal(l)) if l.value == "text/html" || l.value == "application/xhtml+xml")
                })
        }
        Namespace::Svg => matches!(el.raw_name.as_str(), "foreignObject" | "desc" | "title"),
        Namespace::Html => false,
    }
}
