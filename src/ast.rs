//! Vue template AST — a Rust mirror of vue-eslint-parser's `V*` nodes.
//!
//! Every span is a byte span into the WHOLE `.vue` source (absolute), including
//! spans inside embedded oxc expressions (they are shifted after parsing).
//! Nodes live in flat arenas inside [`Document`] and refer to each other by id,
//! so parents are cheap to reach.

use oxc::allocator::Vec as ArenaVec;
use oxc::ast::ast::{
    BindingPattern, BindingRestElement, Expression, FormalParameters, Statement,
    TSTypeParameterDeclaration,
};
use oxc::span::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ElementId(pub u32);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TextId(pub u32);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ExprId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Namespace {
    Html,
    Svg,
    MathMl,
}

/// A child of the document fragment or of an element.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Child {
    Element(ElementId),
    Text(TextId),
    /// `{{ … }}` mustache.
    Expr(ExprId),
}

/// `VIdentifier`: attribute name, directive name, static argument, modifier.
#[derive(Debug, Clone)]
pub struct VIdentifier {
    pub span: Span,
    /// Normalized name (directive name without `v-`; `:`→`bind`, `.`→`bind`, `@`→`on`, `#`→`slot`).
    /// Attribute names, directive arguments and modifiers are ASCII-lowercased like
    /// vue-eslint-parser does (`v-bind:fooBar` → `foobar`); SVG/MathML plain attribute names are
    /// camelCased back (`viewBox`). The synthetic `prop` modifier of `.foo` has an empty span and
    /// empty `raw_name`.
    pub name: String,
    /// Exactly as written in source (`rawName`).
    pub raw_name: String,
}

#[derive(Debug, Clone)]
pub enum DirectiveArgument {
    Static(VIdentifier),
    /// `v-bind:[expr]` — container of the dynamic argument.
    Dynamic(ExprId),
}

/// `VDirectiveKey`.
#[derive(Debug, Clone)]
pub struct DirectiveKey {
    pub span: Span,
    pub name: VIdentifier,
    pub argument: Option<DirectiveArgument>,
    pub modifiers: Vec<VIdentifier>,
}

#[derive(Debug, Clone)]
pub enum AttributeKey {
    Plain(VIdentifier),
    Directive(DirectiveKey),
}

/// `VLiteral`.
#[derive(Debug, Clone)]
pub struct VLiteral {
    /// Includes quotes when quoted.
    pub span: Span,
    /// Decoded value (entities resolved, no quotes).
    pub value: String,
}

#[derive(Debug, Clone)]
pub enum AttributeValue {
    Literal(VLiteral),
    Expr(ExprId),
}

/// `VAttribute` (`directive: false`) or `VDirective` (`directive: true`).
#[derive(Debug, Clone)]
pub struct Attribute {
    pub span: Span,
    pub key: AttributeKey,
    pub value: Option<AttributeValue>,
}

impl Attribute {
    pub fn is_directive(&self) -> bool {
        matches!(self.key, AttributeKey::Directive(_))
    }
}

/// `VStartTag`.
#[derive(Debug, Clone)]
pub struct StartTag {
    pub span: Span,
    pub self_closing: bool,
    pub attributes: Vec<Attribute>,
}

/// `VElement`.
#[derive(Debug, Clone)]
pub struct Element {
    pub id: ElementId,
    /// `None` = direct child of the document fragment (top-level SFC block).
    pub parent: Option<ElementId>,
    pub span: Span,
    pub namespace: Namespace,
    /// vue-eslint-parser `name`: the tag name ASCII-lowercased by the HTML tokenizer (for every
    /// tag, components included), then camelCased back for known SVG tags (`foreignObject`).
    /// Use `raw_name` for case-sensitive checks (vue-eslint-parser compares `rawName` in SFCs).
    pub name: String,
    /// As written.
    pub raw_name: String,
    pub start_tag: StartTag,
    /// `VEndTag` span; `None` when omitted/implicitly closed/self-closing/void.
    pub end_tag: Option<Span>,
    pub children: Vec<Child>,
    /// Variables declared by `v-for` / `v-slot` / `slot-scope` / `scope` / `generic` on this element.
    pub variables: Vec<Variable>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariableKind {
    VFor,
    Scope,
    Generic,
}

#[derive(Debug, Clone)]
pub struct Variable {
    pub name: String,
    /// Span of the declaring identifier.
    pub span: Span,
    pub kind: VariableKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceMode {
    Read,
    Write,
    ReadWrite,
}

/// A free identifier reference inside a template expression.
#[derive(Debug, Clone)]
pub struct Reference {
    pub name: String,
    pub span: Span,
    pub mode: ReferenceMode,
    /// Resolved template variable: (declaring element, index into `Element::variables`).
    pub variable: Option<(ElementId, u32)>,
    /// TS: identifier used only in a type position.
    pub is_type_reference: bool,
}

/// `VText`.
#[derive(Debug, Clone)]
pub struct Text {
    pub span: Span,
    pub parent: Option<ElementId>,
    /// Decoded text value.
    pub value: String,
}

/// Where an expression container sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExprParent {
    /// Mustache child of an element (or fragment when `None`).
    Element(Option<ElementId>),
    /// Value of a directive: (element, attribute index in `start_tag.attributes`).
    DirectiveValue(ElementId, u32),
    /// Dynamic argument of a directive key.
    DirectiveArgument(ElementId, u32),
}

/// `VFilter` (Vue 2 `{{ a | f(b) }}`).
#[derive(Debug)]
pub struct VFilter<'a> {
    pub span: Span,
    /// Callee identifier name + span.
    pub callee: (String, Span),
    pub arguments: ArenaVec<'a, oxc::ast::ast::Argument<'a>>,
}

/// The parsed content of a `VExpressionContainer`.
#[derive(Debug)]
pub enum VExpression<'a> {
    Expression(Expression<'a>),
    FilterSequence {
        span: Span,
        expression: Expression<'a>,
        filters: Vec<VFilter<'a>>,
    },
    /// `v-for="(a, i) in list"`. `left` = alias patterns (`None` = hole, e.g. `(, i)`);
    /// `rest` = a trailing `...rest` alias (ESTree appends it to `left` as `RestElement`).
    For {
        span: Span,
        left: ArenaVec<'a, Option<BindingPattern<'a>>>,
        rest: Option<oxc::allocator::Box<'a, BindingRestElement<'a>>>,
        right: Expression<'a>,
    },
    /// `v-on` handler that is not a plain expression (statements).
    On {
        span: Span,
        body: ArenaVec<'a, Statement<'a>>,
    },
    /// `v-slot` / `slot-scope` / `scope` parameters.
    SlotScope {
        span: Span,
        params: oxc::allocator::Box<'a, FormalParameters<'a>>,
    },
    /// `<script setup generic="…">`.
    Generic {
        span: Span,
        params: oxc::allocator::Box<'a, TSTypeParameterDeclaration<'a>>,
        raw_params: Vec<String>,
    },
}

/// `VExpressionContainer`.
#[derive(Debug)]
pub struct ExprContainer<'a> {
    pub id: ExprId,
    /// Includes `{{ }}` or the attribute quotes.
    pub span: Span,
    pub parent: ExprParent,
    /// `None` = empty or failed to parse (see `Document::errors`).
    ///
    /// Embedded oxc nodes are parsed with oxc's default `preserve_parens: true` (same as the
    /// script program), so `{{ (a) }}` holds a `ParenthesizedExpression` (ESTree has none).
    pub expression: Option<VExpression<'a>>,
    pub references: Vec<Reference>,
}

/// Token kinds — names match vue-eslint-parser / ESLint token `type` strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenKind {
    HTMLAssociation,
    HTMLBogusComment,
    HTMLCDataText,
    HTMLComment,
    HTMLEndTagOpen,
    HTMLIdentifier,
    HTMLLiteral,
    HTMLRCDataText,
    HTMLRawText,
    HTMLSelfClosingTagClose,
    HTMLTagClose,
    HTMLTagOpen,
    HTMLText,
    HTMLWhitespace,
    VExpressionStart,
    VExpressionEnd,
    // Script tokens inside template expressions (ESLint types).
    Punctuator,
    Identifier,
    Keyword,
    String,
    Numeric,
    Template,
    RegularExpression,
    Boolean,
    Null,
    PrivateIdentifier,
    JSXText,
    JSXIdentifier,
    /// `//` or `/* */` comment inside an expression.
    LineComment,
    BlockComment,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
    /// Processed value (e.g. tag name for HTMLTagOpen, decoded text, comment body).
    pub value: String,
}

/// Template/HTML parse error (vue/no-parsing-error).
#[derive(Debug, Clone)]
pub struct ParseError {
    /// HTML spec error code (`eof-in-tag`, `x-invalid-end-tag`, …) when it is an HTML error.
    pub code: Option<String>,
    pub message: String,
    /// Byte offset.
    pub offset: u32,
}

/// The whole `.vue` file as an HTML document fragment (`VDocumentFragment`).
///
/// Top-level `<template>` (without a non-HTML `lang`) content is parsed as a Vue template;
/// other top-level blocks (`<script>`, `<style>`, custom blocks) keep their content as raw text.
#[derive(Debug, Default)]
pub struct Document<'a> {
    pub source: &'a str,
    pub elements: Vec<Element>,
    pub texts: Vec<Text>,
    pub exprs: Vec<ExprContainer<'a>>,
    /// Children of the document fragment.
    pub children: Vec<Child>,
    /// The first top-level `<template>` element (`templateBody`).
    pub template_body: Option<ElementId>,
    /// All tokens of the fragment in source order (HTML + expression tokens).
    pub tokens: Vec<Token>,
    /// HTML comments (`<!-- -->`) and comments inside expressions.
    pub comments: Vec<Token>,
    pub errors: Vec<ParseError>,
}

impl<'a> Document<'a> {
    pub fn element(&self, id: ElementId) -> &Element {
        &self.elements[id.0 as usize]
    }
    pub fn text(&self, id: TextId) -> &Text {
        &self.texts[id.0 as usize]
    }
    pub fn expr(&self, id: ExprId) -> &ExprContainer<'a> {
        &self.exprs[id.0 as usize]
    }
    /// Top-level elements (SFC blocks).
    pub fn top_level(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|c| match c {
            Child::Element(id) => Some(self.element(*id)),
            _ => None,
        })
    }
    /// Elements inside the template body in document (pre-)order, excluding the `<template>` root itself.
    pub fn template_elements(&self) -> Vec<ElementId> {
        let mut out = Vec::new();
        if let Some(root) = self.template_body {
            self.collect(root, &mut out);
            out.remove(0);
        }
        out
    }
    fn collect(&self, id: ElementId, out: &mut Vec<ElementId>) {
        out.push(id);
        for c in &self.element(id).children {
            if let Child::Element(e) = c {
                self.collect(*e, out);
            }
        }
    }
    /// Ancestors from the parent upwards.
    pub fn ancestors(&self, id: ElementId) -> impl Iterator<Item = &Element> {
        let mut cur = self.element(id).parent;
        std::iter::from_fn(move || {
            let e = self.element(cur?);
            cur = e.parent;
            Some(e)
        })
    }
    /// Whether `id` is inside the template body (or is it).
    pub fn in_template(&self, id: ElementId) -> bool {
        let Some(root) = self.template_body else {
            return false;
        };
        id == root || self.ancestors(id).any(|e| e.id == root)
    }
    /// Text of a span.
    pub fn slice(&self, span: Span) -> &'a str {
        &self.source[span.start as usize..span.end as usize]
    }
    /// Tokens that lie fully inside `span`.
    pub fn tokens_in(&self, span: Span) -> &[Token] {
        let lo = self.tokens.partition_point(|t| t.span.start < span.start);
        let hi = self.tokens.partition_point(|t| t.span.end <= span.end);
        if hi <= lo { &[] } else { &self.tokens[lo..hi] }
    }
}

impl Element {
    pub fn attributes(&self) -> &[Attribute] {
        &self.start_tag.attributes
    }
    /// Plain (non-directive) attribute by name.
    pub fn attr(&self, name: &str) -> Option<&Attribute> {
        self.attributes()
            .iter()
            .find(|a| matches!(&a.key, AttributeKey::Plain(k) if k.name == name))
    }
    /// Directive by normalized name (`bind`, `on`, `if`, `for`, …) and optional static argument.
    pub fn directive<'s>(&'s self, name: &'s str, arg: Option<&str>) -> Option<&'s Attribute> {
        self.directives(name).find(|a| match (&a.key, arg) {
            (_, None) => true,
            (AttributeKey::Directive(k), Some(arg)) => {
                matches!(&k.argument, Some(DirectiveArgument::Static(id)) if id.name == arg)
            }
            _ => false,
        })
    }
    pub fn directives<'s>(&'s self, name: &'s str) -> impl Iterator<Item = &'s Attribute> + 's {
        self.attributes()
            .iter()
            .filter(move |a| matches!(&a.key, AttributeKey::Directive(k) if k.name.name == name))
    }
    pub fn has_directive(&self, name: &str) -> bool {
        self.directives(name).next().is_some()
    }
    /// Plain attribute or `v-bind` with that static argument.
    pub fn attr_or_bind(&self, name: &str) -> Option<&Attribute> {
        self.attr(name)
            .or_else(|| self.directive("bind", Some(name)))
    }
}

impl DirectiveKey {
    pub fn static_argument(&self) -> Option<&str> {
        match &self.argument {
            Some(DirectiveArgument::Static(id)) => Some(&id.name),
            _ => None,
        }
    }
    pub fn has_modifier(&self, m: &str) -> bool {
        self.modifiers.iter().any(|x| x.name == m)
    }
}
