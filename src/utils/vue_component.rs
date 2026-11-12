//! Vue component detection and component-definition helpers (ports of eslint-plugin-vue
//! `lib/utils/index.ts`: `getVueObjectType`, `executeOnVue*`, `defineVueVisitor`,
//! `defineScriptSetupVisitor`, `getComponentPropsFrom*`, `getComponentEmitsFrom*`, …).
//!
//! Everything works on the merged script [`Program`] and [`Semantic`] of a [`LintFile`]; returned
//! references live as long as the file's AST.

use std::collections::HashMap;

use oxc::ast::AstKind;
use oxc::ast::ast::*;
use oxc::semantic::{NodeId, Semantic};
use oxc::span::{GetSpan, Span};

use super::js::{self, ObjectOrArray};
use crate::rule::{LintFile, ScriptBlock};

// ---------------------------------------------------------------------------------------------
// Files and `<script setup>`
// ---------------------------------------------------------------------------------------------

/// `isVueFile`: `.vue`, `.jsx`, `.tsx`.
pub fn is_vue_file(file: &LintFile) -> bool {
    matches!(ext(file), "vue" | "jsx" | "tsx")
}

/// `isTypeScriptFile`: `.ts`, `.tsx`, `.mts`.
pub fn is_ts_file(file: &LintFile) -> bool {
    matches!(ext(file), "ts" | "tsx" | "mts")
}

/// File extension without the dot (`""` when none).
pub fn ext<'f>(file: &LintFile<'f>) -> &'f str {
    file.path.extension().and_then(|e| e.to_str()).unwrap_or("")
}

/// `getScriptSetupElement`: with two `<script>` blocks the one with `setup`; with one, it must
/// have `setup`; otherwise none.
pub fn script_setup_block<'f>(file: &'f LintFile) -> Option<&'f ScriptBlock> {
    if !file.is_sfc {
        return None;
    }
    match file.scripts.len() {
        2 => file.scripts.iter().find(|s| s.setup),
        1 => file.scripts.first().filter(|s| s.setup),
        _ => None,
    }
}

/// `isScriptSetup`.
pub fn is_script_setup(file: &LintFile) -> bool {
    script_setup_block(file).is_some()
}

/// Whether a span lies inside the `<script setup>` element.
pub fn in_script_setup(file: &LintFile, span: Span) -> bool {
    script_setup_block(file).is_some_and(|s| js::in_range(s.span, span))
}

// ---------------------------------------------------------------------------------------------
// Vue objects
// ---------------------------------------------------------------------------------------------

/// `VueObjectType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VueObjectType {
    /// Preceded by a `// @vue/component` comment.
    Mark,
    /// `export default {}` in a `.vue` / `.jsx` / `.tsx` file.
    Export,
    /// `Vue.component('x', {})`, `defineComponent({})`, `app.mixin({})`, …
    Definition,
    /// `new Vue({})`.
    Instance,
}

impl VueObjectType {
    /// `executeOnVueComponent` types (everything but `Instance`).
    pub fn is_component(self) -> bool {
        self != VueObjectType::Instance
    }
}

/// `getVueComponentDefinitionType` result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefinitionType {
    Component,
    Mixin,
    Extend,
    CreateApp,
    DefineComponent,
    DefineNuxtComponent,
}

/// A detected component/instance object.
#[derive(Clone, Copy)]
pub struct VueObject<'a> {
    pub obj: &'a ObjectExpression<'a>,
    pub ty: VueObjectType,
}

impl<'a> VueObject<'a> {
    pub fn node_id(&self) -> NodeId {
        self.obj.node_id()
    }
    /// `vueStack.functional`.
    pub fn functional(&self) -> bool {
        js::find_property(self.obj, "functional").is_some_and(
            |p| !matches!(js::skip_parens(&p.value), Expression::BooleanLiteral(b) if !b.value),
        )
    }
}

fn is_object_argument(call_args: &[Argument]) -> bool {
    call_args
        .last()
        .and_then(|a| a.as_expression())
        .is_some_and(|e| matches!(js::skip_ts_as(e), Expression::ObjectExpression(_)))
}

fn last_arg_is<'a>(args: &[Argument<'a>], obj: &ObjectExpression<'a>) -> bool {
    args.last().and_then(|a| a.as_expression()).is_some_and(
        |e| matches!(js::skip_ts_as(e), Expression::ObjectExpression(o) if o.span == obj.span),
    )
}

const VUE_LIKE: [&str; 3] = ["vue", "@vue/composition-api", "@nuxtjs/composition-api"];

/// `isGlobalOrImportedFromVue(callee)`.
pub fn is_global_or_imported_from_vue(sem: &Semantic, callee: &IdentifierReference) -> bool {
    let Some(sym) = js::ref_symbol(sem, callee) else {
        return true;
    };
    let nodes = sem.nodes();
    let decl = js::declaration_id(sem, sym);
    if !sem.scoping().symbol_redeclarations(sym).is_empty() {
        return true;
    }
    match nodes.kind(decl) {
        AstKind::ImportSpecifier(spec) => {
            let AstKind::ImportDeclaration(imp) = nodes.parent_kind(decl) else {
                return true;
            };
            let src = imp.source.value.as_str().unwrap_or_default();
            let imported_matches = spec.imported.name() == callee.name.as_str();
            if src == "#imports" && imported_matches {
                return true;
            }
            match &spec.imported {
                ModuleExportName::IdentifierName(_) | ModuleExportName::IdentifierReference(_) => {
                    VUE_LIKE.contains(&src)
                }
                ModuleExportName::StringLiteral(_) => true,
            }
        }
        AstKind::BindingIdentifier(_) | AstKind::VariableDeclarator(_) => {
            // `const { name } = require('x')`
            let declarator = std::iter::once(nodes.get_node(decl))
                .chain(nodes.ancestors(decl))
                .find_map(|n| match n.kind() {
                    AstKind::VariableDeclarator(d) => Some(d),
                    _ => None,
                });
            match declarator {
                Some(d) if matches!(d.id, BindingPattern::ObjectPattern(_)) => {
                    match d.init.as_ref().map(js::skip_parens) {
                        Some(Expression::CallExpression(c))
                            if c.callee.is_specific_id("require") =>
                        {
                            let src = c
                                .arguments
                                .first()
                                .and_then(|a| a.as_expression())
                                .filter(|e| js::is_string_literal(e))
                                .and_then(js::string_literal_value);
                            src.is_some_and(|s| VUE_LIKE.contains(&s.as_str()))
                        }
                        _ => true,
                    }
                }
                _ => true,
            }
        }
        AstKind::Function(_) | AstKind::Class(_) | AstKind::CatchParameter(_) => false,
        _ => {
            // catch (name) {} binds through a CatchParameter ancestor.
            !nodes
                .ancestors(decl)
                .take(3)
                .any(|n| matches!(n.kind(), AstKind::CatchParameter(_)))
        }
    }
}

/// `getVueComponentDefinitionType(obj)`.
pub fn definition_type<'a>(
    file: &LintFile<'a>,
    obj: &ObjectExpression<'a>,
) -> Option<DefinitionType> {
    let sem = file.semantic?;
    let (_, parent) = js::parent_skip_ts_as(sem, obj.node_id());
    let AstKind::CallExpression(call) = parent else {
        return None;
    };
    match js::skip_parens(&call.callee) {
        callee if callee.as_member_expression().is_some() => {
            let m = callee.as_member_expression().unwrap();
            let Expression::Identifier(object) = js::skip_ts_as(m.object()) else {
                return None;
            };
            let prop = js::member_name(m)?;
            if !is_object_argument(&call.arguments) {
                return None;
            }
            match prop.as_str() {
                "component" => Some(DefinitionType::Component),
                "mixin" => Some(DefinitionType::Mixin),
                "extend" if object.name == "Vue" => Some(DefinitionType::Extend),
                _ => None,
            }
        }
        Expression::Identifier(callee) => {
            if callee.name == "defineNuxtComponent" {
                return is_object_argument(&call.arguments)
                    .then_some(DefinitionType::DefineNuxtComponent);
            }
            let ty = match callee.name.as_str() {
                "component" => DefinitionType::Component,
                "createApp" => DefinitionType::CreateApp,
                "defineComponent" => DefinitionType::DefineComponent,
                _ => return None,
            };
            (is_global_or_imported_from_vue(sem, callee) && is_object_argument(&call.arguments))
                .then_some(ty)
        }
        _ => None,
    }
}

/// Comments containing `@vue/component`.
fn component_comment_end_lines(file: &LintFile) -> Vec<usize> {
    let Some(p) = file.program else {
        return Vec::new();
    };
    p.comments
        .iter()
        .filter(|c| file.slice(c.span).contains("@vue/component"))
        .map(|c| file.lines.line_of(c.span.end))
        .collect()
}

/// `getVueObjectType(obj)`.
pub fn vue_object_type<'a>(
    file: &LintFile<'a>,
    obj: &ObjectExpression<'a>,
) -> Option<VueObjectType> {
    let sem = file.semantic?;
    let (_, parent) = js::parent_skip_ts_as(sem, obj.node_id());
    match parent {
        AstKind::ExportDefaultDeclaration(ed) => {
            let direct = matches!(
                &ed.declaration,
                ExportDefaultDeclarationKind::ObjectExpression(_)
            ) || ed
                .declaration
                .as_expression()
                .is_some_and(|e| matches!(js::skip_parens(e), Expression::ObjectExpression(_)));
            if is_vue_file(file) && direct {
                return if in_script_setup(file, ed.span) {
                    None
                } else {
                    Some(VueObjectType::Export)
                };
            }
        }
        AstKind::CallExpression(call) => {
            if definition_type(file, obj).is_some() && last_arg_is(&call.arguments, obj) {
                return Some(VueObjectType::Definition);
            }
        }
        AstKind::NewExpression(n) => {
            let first_is = n.arguments.first().and_then(|a| a.as_expression()).is_some_and(|e| matches!(js::skip_ts_as(e), Expression::ObjectExpression(o) if o.span == obj.span));
            if n.callee.is_specific_id("Vue") && first_is {
                return Some(VueObjectType::Instance);
            }
        }
        _ => {}
    }
    let start_line = file.lines.line_of(obj.span.start);
    (start_line > 0 && component_comment_end_lines(file).contains(&(start_line - 1)))
        .then_some(VueObjectType::Mark)
}

/// Every Vue component/instance object of the file in source order (`executeOnVue`).
pub fn vue_objects<'a>(file: &LintFile<'a>) -> Vec<VueObject<'a>> {
    let Some(sem) = file.semantic else {
        return Vec::new();
    };
    sem.nodes()
        .iter()
        .filter_map(|n| match n.kind() {
            AstKind::ObjectExpression(obj) => {
                vue_object_type(file, obj).map(|ty| VueObject { obj, ty })
            }
            _ => None,
        })
        .collect()
}

/// The innermost Vue object containing `id` (the `vueStack` top of `defineVueVisitor`).
pub fn enclosing_vue_object<'a>(file: &LintFile<'a>, id: NodeId) -> Option<VueObject<'a>> {
    let sem = file.semantic?;
    sem.nodes().ancestors(id).find_map(|n| match n.kind() {
        AstKind::ObjectExpression(obj) => {
            vue_object_type(file, obj).map(|ty| VueObject { obj, ty })
        }
        _ => None,
    })
}

/// `isSFCObject(obj)`.
pub fn is_sfc_object<'a>(file: &LintFile<'a>, obj: &ObjectExpression<'a>) -> bool {
    let e = ext(file);
    if !(e == "vue" || e.is_empty()) {
        return false;
    }
    let Some(sem) = file.semantic else {
        return false;
    };
    is_sfc(file, sem, obj.node_id(), obj.span, 0)
}

fn is_sfc<'a>(file: &LintFile<'a>, sem: &Semantic<'a>, id: NodeId, span: Span, depth: u32) -> bool {
    if depth > 32 {
        return false;
    }
    let (pid, parent) = js::parent_skip_ts_as(sem, id);
    match parent {
        AstKind::ExportDefaultDeclaration(ed) => {
            let decl_is = ed
                .declaration
                .as_expression()
                .is_some_and(|e| js::skip_ts_as(e).span() == span);
            decl_is && !in_script_setup(file, ed.span)
        }
        AstKind::CallExpression(call) => {
            if !call.arguments.iter().any(|a| {
                a.as_expression()
                    .is_some_and(|e| js::skip_ts_as(e).span() == span)
            }) {
                return false;
            }
            let ok = match js::skip_parens(&call.callee) {
                Expression::Identifier(c) => {
                    c.name == "defineComponent" || c.name == "defineNuxtComponent"
                }
                Expression::StaticMemberExpression(m) => {
                    m.object.is_specific_id("Vue") && m.property.name == "extend"
                }
                _ => false,
            };
            ok && is_sfc(file, sem, pid, call.span, depth + 1)
        }
        AstKind::VariableDeclarator(d) => {
            let BindingPattern::BindingIdentifier(bid) = &d.id else {
                return false;
            };
            if d.init
                .as_ref()
                .is_none_or(|i| js::skip_ts_as(i).span() != span)
            {
                return false;
            }
            let s = sem.scoping();
            s.get_resolved_references(bid.symbol_id()).any(|r| {
                let rid = r.node_id();
                is_sfc(file, sem, rid, sem.nodes().kind(rid).span(), depth + 1)
            })
        }
        _ => false,
    }
}

/// A function node (`FunctionExpression` / `FunctionDeclaration` / arrow).
#[derive(Clone, Copy)]
pub enum FnNode<'a> {
    Function(&'a Function<'a>),
    Arrow(&'a ArrowFunctionExpression<'a>),
}

impl<'a> FnNode<'a> {
    pub fn from_kind(k: AstKind<'a>) -> Option<Self> {
        match k {
            AstKind::Function(f) => Some(FnNode::Function(f)),
            AstKind::ArrowFunctionExpression(f) => Some(FnNode::Arrow(f)),
            _ => None,
        }
    }
    pub fn from_expr(e: &'a Expression<'a>) -> Option<Self> {
        match js::skip_parens(e) {
            Expression::FunctionExpression(f) => Some(FnNode::Function(f)),
            Expression::ArrowFunctionExpression(f) => Some(FnNode::Arrow(f)),
            _ => None,
        }
    }
    pub fn params(&self) -> &'a FormalParameters<'a> {
        match self {
            FnNode::Function(f) => &f.params,
            FnNode::Arrow(f) => &f.params,
        }
    }
    pub fn span(&self) -> Span {
        match self {
            FnNode::Function(f) => f.span,
            FnNode::Arrow(f) => f.span,
        }
    }
    pub fn node_id(&self) -> NodeId {
        match self {
            FnNode::Function(f) => f.node_id(),
            FnNode::Arrow(f) => f.node_id(),
        }
    }
    pub fn is_async(&self) -> bool {
        match self {
            FnNode::Function(f) => f.r#async,
            FnNode::Arrow(f) => f.r#async,
        }
    }
    /// Block body (`None` for an expression-bodied arrow or a body-less declaration).
    pub fn body(&self) -> Option<&'a FunctionBody<'a>> {
        match self {
            FnNode::Function(f) => f.body.as_deref(),
            FnNode::Arrow(f) => match &f.body {
                ArrowFunctionBody::FunctionBody(b) => Some(b),
                _ => None,
            },
        }
    }
    /// Expression body of a concise arrow.
    pub fn expr_body(&self) -> Option<&'a Expression<'a>> {
        match self {
            FnNode::Arrow(f) => f.body.as_expression(),
            FnNode::Function(_) => None,
        }
    }
}

/// `setup` functions of a Vue object (`onSetupFunctionEnter`): properties named `setup` whose value
/// `extends` / non-empty `mixins`: options merged in from objects we usually cannot see (imported), so the
/// component's property set is open.
// ponytail: mixin objects are not resolved, even same-file ones; resolve them if this hides too much.
pub fn inherits_options(obj: &ObjectExpression) -> bool {
    js::properties(obj).any(|p| match js::prop_name(p).as_deref() {
        Some("extends") => true,
        Some("mixins") => {
            !matches!(js::skip_parens(&p.value), Expression::ArrayExpression(a) if a.elements.is_empty())
        }
        _ => false,
    })
}

/// is a function expression or arrow.
pub fn setup_functions<'a>(obj: &'a ObjectExpression<'a>) -> Vec<FnNode<'a>> {
    js::properties(obj)
        .filter(|p| js::prop_name(p).as_deref() == Some("setup"))
        .filter_map(|p| FnNode::from_expr(&p.value))
        .collect()
}

/// `render` functions of a Vue object (`onRenderFunctionEnter`).
pub fn render_functions<'a>(obj: &'a ObjectExpression<'a>) -> Vec<FnNode<'a>> {
    js::properties(obj)
        .filter(|p| js::prop_name(p).as_deref() == Some("render"))
        .filter_map(|p| FnNode::from_expr(&p.value))
        .collect()
}

/// `executeOnCallVueComponent`: `x.component(…)` calls with at least one argument, where `x` is an identifier.
pub fn vue_component_calls<'a>(file: &LintFile<'a>) -> Vec<&'a CallExpression<'a>> {
    let Some(sem) = file.semantic else {
        return Vec::new();
    };
    sem.nodes()
        .iter()
        .filter_map(|n| match n.kind() {
            AstKind::CallExpression(c) if !c.arguments.is_empty() => {
                match js::skip_parens(&c.callee) {
                    Expression::StaticMemberExpression(m)
                        if m.property.name == "component"
                            && matches!(js::skip_ts_as(&m.object), Expression::Identifier(_)) =>
                    {
                        Some(c)
                    }
                    _ => None,
                }
            }
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Component properties (`iterateProperties`)
// ---------------------------------------------------------------------------------------------

/// `ComponentPropertyData`: a name defined by a component option group.
pub struct PropertyData {
    pub name: String,
    pub group: &'static str,
    /// Key node span (literal for array items).
    pub span: Span,
}

/// `iterateProperties(obj, groups)`.
pub fn iterate_properties<'a>(
    obj: &'a ObjectExpression<'a>,
    groups: &[&'static str],
) -> Vec<PropertyData> {
    iterate_properties_full(obj, groups)
        .into_iter()
        .map(|e| PropertyData {
            name: e.name,
            group: e.group,
            span: e.span,
        })
        .collect()
}

/// `ComponentArrayPropertyData` (`prop: None`, `span` = the literal) or
/// `ComponentObjectPropertyData` (`prop` = the property, `span` = its key).
pub struct PropertyEntry<'a> {
    pub name: String,
    pub group: &'static str,
    pub span: Span,
    pub prop: Option<&'a ObjectProperty<'a>>,
}

/// `iterateProperties(obj, groups)` with the property nodes.
pub fn iterate_properties_full<'a>(
    obj: &'a ObjectExpression<'a>,
    groups: &[&'static str],
) -> Vec<PropertyEntry<'a>> {
    let mut out = Vec::new();
    for p in js::properties(obj) {
        let Some(name) = js::prop_name(p) else {
            continue;
        };
        let Some(group) = groups.iter().copied().find(|g| *g == name) else {
            continue;
        };
        iterate_property_value(&p.value, group, &mut out);
    }
    out
}

fn iterate_property_value<'a>(
    value: &'a Expression<'a>,
    group: &'static str,
    out: &mut Vec<PropertyEntry<'a>>,
) {
    match value {
        Expression::ArrayExpression(a) => {
            for el in &a.elements {
                let Some(e) = el.as_expression() else {
                    continue;
                };
                if !matches!(
                    e,
                    Expression::StringLiteral(_) | Expression::TemplateLiteral(_)
                ) {
                    continue;
                }
                if let Some(name) = js::string_literal_value(e).filter(|n| !n.is_empty()) {
                    out.push(PropertyEntry {
                        name,
                        group,
                        span: e.span(),
                        prop: None,
                    });
                }
            }
        }
        Expression::ObjectExpression(o) => iterate_object(o, group, out),
        Expression::FunctionExpression(f) => {
            if let Some(body) = &f.body {
                iterate_returns(&body.statements, group, out);
            }
        }
        Expression::ArrowFunctionExpression(f) => match &f.body {
            ArrowFunctionBody::FunctionBody(b) => iterate_returns(&b.statements, group, out),
            body => {
                if let Some(Expression::ObjectExpression(o)) =
                    body.as_expression().map(js::skip_parens)
                {
                    iterate_object(o, group, out);
                }
            }
        },
        Expression::ParenthesizedExpression(p) => iterate_property_value(&p.expression, group, out),
        _ => {}
    }
}

fn iterate_returns<'a>(
    stmts: &'a [Statement<'a>],
    group: &'static str,
    out: &mut Vec<PropertyEntry<'a>>,
) {
    for s in stmts {
        if let Statement::ReturnStatement(r) = s
            && let Some(Expression::ObjectExpression(o)) = r.argument.as_ref().map(js::skip_parens)
        {
            iterate_object(o, group, out);
        }
    }
}

fn iterate_object<'a>(
    o: &'a ObjectExpression<'a>,
    group: &'static str,
    out: &mut Vec<PropertyEntry<'a>>,
) {
    let mut used_getters: Vec<Span> = Vec::new();
    for p in js::properties(o) {
        let key_ok = matches!(
            &p.key,
            PropertyKey::StaticIdentifier(_)
                | PropertyKey::Identifier(_)
                | PropertyKey::TemplateLiteral(_)
        ) || matches!(
            &p.key,
            PropertyKey::StringLiteral(_)
                | PropertyKey::NumericLiteral(_)
                | PropertyKey::BooleanLiteral(_)
                | PropertyKey::NullLiteral(_)
                | PropertyKey::BigIntLiteral(_)
                | PropertyKey::RegExpLiteral(_)
        );
        if !key_ok {
            continue;
        }
        let Some(name) = js::prop_name(p).filter(|n| !n.is_empty()) else {
            continue;
        };
        if p.kind == PropertyKind::Set {
            let pair = js::properties(o).find(|g| {
                g.kind == PropertyKind::Get
                    && !used_getters.contains(&g.span)
                    && js::prop_name(g).as_deref() == Some(&name)
            });
            if let Some(g) = pair {
                used_getters.push(g.span);
                continue;
            }
        }
        out.push(PropertyEntry {
            name,
            group,
            span: p.key.span(),
            prop: Some(p),
        });
    }
}

// ---------------------------------------------------------------------------------------------
// `<script setup>` macros
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacroKind {
    DefineProps,
    DefineEmits,
    DefineOptions,
    DefineSlots,
    DefineExpose,
    DefineModel,
}

impl MacroKind {
    pub fn name(self) -> &'static str {
        match self {
            MacroKind::DefineProps => "defineProps",
            MacroKind::DefineEmits => "defineEmits",
            MacroKind::DefineOptions => "defineOptions",
            MacroKind::DefineSlots => "defineSlots",
            MacroKind::DefineExpose => "defineExpose",
            MacroKind::DefineModel => "defineModel",
        }
    }
    fn from_name(n: &str) -> Option<Self> {
        Some(match n {
            "defineProps" => MacroKind::DefineProps,
            "defineEmits" => MacroKind::DefineEmits,
            "defineOptions" => MacroKind::DefineOptions,
            "defineSlots" => MacroKind::DefineSlots,
            "defineExpose" => MacroKind::DefineExpose,
            "defineModel" => MacroKind::DefineModel,
            _ => return None,
        })
    }
}

/// A compiler macro call found by `defineScriptSetupVisitor`.
#[derive(Clone, Copy)]
pub struct Macro<'a> {
    pub kind: MacroKind,
    /// The macro call itself (`defineProps(…)`, also inside `withDefaults`).
    pub call: &'a CallExpression<'a>,
    /// The enclosing `withDefaults(defineProps(…), …)` call.
    pub with_defaults: Option<&'a CallExpression<'a>>,
    /// `const x = <macro>` declarator whose init is the macro (or its `withDefaults`).
    pub declarator: Option<&'a VariableDeclarator<'a>>,
    /// The top-level statement holding the macro.
    pub statement: &'a Statement<'a>,
    /// Index of [`Self::statement`] in `program.body`.
    pub statement_index: usize,
}

impl<'a> Macro<'a> {
    /// `getLeftOfDefineProps`: the declarator id when assigned to a variable.
    pub fn left(&self) -> Option<&'a BindingPattern<'a>> {
        self.declarator.map(|d| &d.id)
    }
    /// First argument as an expression.
    pub fn arg(&self, i: usize) -> Option<&'a Expression<'a>> {
        self.call.arguments.get(i).and_then(|a| a.as_expression())
    }
    /// First type argument.
    pub fn type_arg(&self) -> Option<&'a TSType<'a>> {
        self.call
            .type_arguments
            .as_ref()
            .and_then(|t| t.params.first())
    }
}

/// All compiler macros in `<script setup>`, in source order. A macro is a call with an identifier
/// callee that is the init of a top-level declarator or the expression of a top-level expression
/// statement (`defineProps` may also be the first argument of such a `withDefaults(…)`).
pub fn script_setup_macros<'a>(file: &LintFile<'a>) -> Vec<Macro<'a>> {
    let (Some(setup), Some(program)) = (script_setup_block(file), file.program) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (i, stmt) in program.body.iter().enumerate() {
        let mut candidates: Vec<(&'a Expression<'a>, Option<&'a VariableDeclarator<'a>>)> =
            Vec::new();
        match stmt {
            Statement::VariableDeclaration(d) => {
                for decl in &d.declarations {
                    if let Some(init) = &decl.init {
                        candidates.push((init, Some(decl)));
                    }
                }
            }
            Statement::ExpressionStatement(s) => candidates.push((&s.expression, None)),
            _ => {}
        }
        for (cand, declarator) in candidates {
            let Expression::CallExpression(call) = js::skip_parens(cand) else {
                continue;
            };
            if !js::in_range(setup.span, call.span) {
                continue;
            }
            let Expression::Identifier(callee) = &call.callee else {
                continue;
            };
            if let Some(kind) = MacroKind::from_name(&callee.name) {
                out.push(Macro {
                    kind,
                    call,
                    with_defaults: None,
                    declarator,
                    statement: stmt,
                    statement_index: i,
                });
            } else if callee.name == "withDefaults"
                && let Some(Expression::CallExpression(inner)) =
                    call.arguments.first().and_then(|a| a.as_expression())
                && inner.callee.is_specific_id("defineProps")
            {
                out.push(Macro {
                    kind: MacroKind::DefineProps,
                    call: inner,
                    with_defaults: Some(call),
                    declarator,
                    statement: stmt,
                    statement_index: i,
                });
            }
        }
    }
    out
}

/// Macros of one kind.
pub fn macros_of<'a>(file: &LintFile<'a>, kind: MacroKind) -> Vec<Macro<'a>> {
    script_setup_macros(file)
        .into_iter()
        .filter(|m| m.kind == kind)
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Props / emits
// ---------------------------------------------------------------------------------------------

/// `ComponentProp`.
#[derive(Clone)]
pub enum ComponentProp<'a> {
    /// `props: ['a']` — `key` is the literal when the name is static.
    Array {
        name: Option<String>,
        key: Option<&'a Expression<'a>>,
        node: &'a ArrayExpressionElement<'a>,
    },
    /// `props: { a: … }` — `key` only when the name is static; `value` has `as` stripped.
    Object {
        key: Option<&'a PropertyKey<'a>>,
        prop: &'a ObjectProperty<'a>,
        value: &'a Expression<'a>,
    },
    /// `defineProps<{ a: T }>()`.
    Type(TypeProp<'a>),
    /// Anything else (`node` = the unknown node when there is one).
    Unknown { node: Option<Span> },
}

#[derive(Clone)]
pub struct TypeProp<'a> {
    pub name: String,
    pub key: &'a PropertyKey<'a>,
    pub node: &'a TSSignature<'a>,
    pub required: bool,
    pub types: Vec<String>,
}

impl<'a> ComponentProp<'a> {
    /// `propName` (static name).
    pub fn name(&self) -> Option<String> {
        match self {
            ComponentProp::Array { name, .. } => name.clone(),
            ComponentProp::Object { key, prop, .. } => key.and_then(|_| js::prop_name(prop)),
            ComponentProp::Type(t) => Some(t.name.clone()),
            ComponentProp::Unknown { .. } => None,
        }
    }
    /// `prop.node` span.
    pub fn node_span(&self) -> Option<Span> {
        match self {
            ComponentProp::Array { node, .. } => Some(node.span()),
            ComponentProp::Object { prop, .. } => Some(prop.span),
            ComponentProp::Type(t) => Some(t.node.span()),
            ComponentProp::Unknown { node } => *node,
        }
    }
    /// `prop.key` span (static keys only).
    pub fn key_span(&self) -> Option<Span> {
        match self {
            ComponentProp::Array { key, .. } => key.map(GetSpan::span),
            ComponentProp::Object { key, .. } => key.map(GetSpan::span),
            ComponentProp::Type(t) => Some(t.key.span()),
            ComponentProp::Unknown { .. } => None,
        }
    }
}

/// `ComponentEmit`.
#[derive(Clone)]
pub enum ComponentEmit<'a> {
    Array {
        name: Option<String>,
        key: Option<&'a Expression<'a>>,
        node: &'a ArrayExpressionElement<'a>,
    },
    Object {
        key: Option<&'a PropertyKey<'a>>,
        prop: &'a ObjectProperty<'a>,
        value: &'a Expression<'a>,
    },
    Type {
        name: String,
        key: Span,
        node: TypeEmitNode<'a>,
    },
    Unknown {
        node: Option<Span>,
    },
}

/// Declaration node of a type-based emit.
#[derive(Clone, Copy)]
pub enum TypeEmitNode<'a> {
    CallSignature(&'a TSCallSignatureDeclaration<'a>),
    FunctionType(&'a TSFunctionType<'a>),
    Property(&'a TSPropertySignature<'a>),
    Method(&'a TSMethodSignature<'a>),
}

impl<'a> TypeEmitNode<'a> {
    pub fn span(&self) -> Span {
        match self {
            TypeEmitNode::CallSignature(n) => n.span,
            TypeEmitNode::FunctionType(n) => n.span,
            TypeEmitNode::Property(n) => n.span,
            TypeEmitNode::Method(n) => n.span,
        }
    }
}

impl<'a> ComponentEmit<'a> {
    pub fn name(&self) -> Option<String> {
        match self {
            ComponentEmit::Array { name, .. } => name.clone(),
            ComponentEmit::Object { key, prop, .. } => key.and_then(|_| js::prop_name(prop)),
            ComponentEmit::Type { name, .. } => Some(name.clone()),
            ComponentEmit::Unknown { .. } => None,
        }
    }
    pub fn node_span(&self) -> Option<Span> {
        match self {
            ComponentEmit::Array { node, .. } => Some(node.span()),
            ComponentEmit::Object { prop, .. } => Some(prop.span),
            ComponentEmit::Type { node, .. } => Some(node.span()),
            ComponentEmit::Unknown { node } => *node,
        }
    }
}

fn array_entries<'a>(
    a: &'a ArrayExpression<'a>,
) -> Vec<(
    Option<String>,
    Option<&'a Expression<'a>>,
    &'a ArrayExpressionElement<'a>,
)> {
    a.elements
        .iter()
        .filter(|e| !matches!(e, ArrayExpressionElement::Elision(_)))
        .map(|el| {
            if let Some(e) = el.as_expression()
                && matches!(
                    e,
                    Expression::StringLiteral(_)
                        | Expression::TemplateLiteral(_)
                        | Expression::NumericLiteral(_)
                        | Expression::BooleanLiteral(_)
                        | Expression::NullLiteral(_)
                        | Expression::BigIntLiteral(_)
                        | Expression::RegExpLiteral(_)
                )
                && let Some(name) = js::string_literal_value(e)
            {
                return (Some(name), Some(e), el);
            }
            (None, None, el)
        })
        .collect()
}

/// `getComponentPropsFromDefine(objectOrArray)`.
pub fn props_from_define<'a>(def: ObjectOrArray<'a>) -> Vec<ComponentProp<'a>> {
    match def {
        ObjectOrArray::Object(o) => o
            .properties
            .iter()
            .map(|p| match p {
                ObjectPropertyKind::SpreadProperty(s) => {
                    ComponentProp::Unknown { node: Some(s.span) }
                }
                ObjectPropertyKind::ObjectProperty(p) => {
                    let key = js::prop_name(p).map(|_| &p.key);
                    ComponentProp::Object {
                        key,
                        prop: p,
                        value: js::skip_ts_as(&p.value),
                    }
                }
            })
            .collect(),
        ObjectOrArray::Array(a) => array_entries(a)
            .into_iter()
            .map(|(name, key, node)| ComponentProp::Array { name, key, node })
            .collect(),
    }
}

/// `getComponentEmitsFromDefine(objectOrArray)`.
pub fn emits_from_define<'a>(def: ObjectOrArray<'a>) -> Vec<ComponentEmit<'a>> {
    match def {
        ObjectOrArray::Object(o) => o
            .properties
            .iter()
            .map(|p| match p {
                ObjectPropertyKind::SpreadProperty(s) => {
                    ComponentEmit::Unknown { node: Some(s.span) }
                }
                ObjectPropertyKind::ObjectProperty(p) => {
                    let key = js::prop_name(p).map(|_| &p.key);
                    ComponentEmit::Object {
                        key,
                        prop: p,
                        value: js::skip_ts_as(&p.value),
                    }
                }
            })
            .collect(),
        ObjectOrArray::Array(a) => array_entries(a)
            .into_iter()
            .map(|(name, key, node)| ComponentEmit::Array { name, key, node })
            .collect(),
    }
}

/// `getComponentPropsFromOptions(obj)`.
pub fn props_from_options<'a>(obj: &'a ObjectExpression<'a>) -> Vec<ComponentProp<'a>> {
    let Some(p) = js::find_property(obj, "props") else {
        return Vec::new();
    };
    match js::skip_parens(&p.value) {
        Expression::ObjectExpression(o) => props_from_define(ObjectOrArray::Object(o)),
        Expression::ArrayExpression(a) => props_from_define(ObjectOrArray::Array(a)),
        v => vec![ComponentProp::Unknown {
            node: Some(v.span()),
        }],
    }
}

/// `getComponentEmitsFromOptions(obj)`.
pub fn emits_from_options<'a>(obj: &'a ObjectExpression<'a>) -> Vec<ComponentEmit<'a>> {
    let Some(p) = js::find_property(obj, "emits") else {
        return Vec::new();
    };
    match js::skip_parens(&p.value) {
        Expression::ObjectExpression(o) => emits_from_define(ObjectOrArray::Object(o)),
        Expression::ArrayExpression(a) => emits_from_define(ObjectOrArray::Array(a)),
        v => vec![ComponentEmit::Unknown {
            node: Some(v.span()),
        }],
    }
}

/// `getComponentPropsFromDefineProps(call)`.
pub fn props_from_define_props<'a>(
    file: &LintFile<'a>,
    call: &'a CallExpression<'a>,
) -> Vec<ComponentProp<'a>> {
    if let Some(a) = call.arguments.first() {
        if let Some(def) = a.as_expression().and_then(|e| js::object_or_array(file, e)) {
            return props_from_define(def);
        }
        return vec![ComponentProp::Unknown {
            node: Some(a.span()),
        }];
    }
    if let Some(t) = call.type_arguments.as_ref().and_then(|t| t.params.first()) {
        return props_from_type(file, t);
    }
    vec![ComponentProp::Unknown { node: None }]
}

/// `getComponentEmitsFromDefineEmits(call)`.
pub fn emits_from_define_emits<'a>(
    file: &LintFile<'a>,
    call: &'a CallExpression<'a>,
) -> Vec<ComponentEmit<'a>> {
    if let Some(a) = call.arguments.first() {
        if let Some(def) = a.as_expression().and_then(|e| js::object_or_array(file, e)) {
            return emits_from_define(def);
        }
        return vec![ComponentEmit::Unknown {
            node: Some(a.span()),
        }];
    }
    if let Some(t) = call.type_arguments.as_ref().and_then(|t| t.params.first()) {
        return emits_from_type(file, t);
    }
    vec![ComponentEmit::Unknown { node: None }]
}

/// `getComponentModelFromDefineModel`.
pub struct ModelInfo<'a> {
    pub name: String,
    /// The name literal (`None` = default `modelValue`).
    pub name_node: Option<Span>,
    pub options: Option<&'a Expression<'a>>,
    pub type_node: Option<&'a TSType<'a>>,
}

pub fn model_from_define_model<'a>(call: &'a CallExpression<'a>) -> ModelInfo<'a> {
    let mut name = "modelValue".to_string();
    let mut name_node = None;
    let mut options = call.arguments.first().and_then(|a| a.as_expression());
    if let Some(first) = call.arguments.first().and_then(|a| a.as_expression()) {
        let cand = js::skip_ts_as(first);
        let lit = match cand {
            Expression::NullLiteral(_) => Some("null".to_string()),
            Expression::StringLiteral(_)
            | Expression::NumericLiteral(_)
            | Expression::BooleanLiteral(_)
            | Expression::BigIntLiteral(_)
            | Expression::RegExpLiteral(_) => js::string_literal_value(cand),
            _ => None,
        };
        if let Some(l) = lit {
            name = l;
            name_node = Some(cand.span());
            options = call.arguments.get(1).and_then(|a| a.as_expression());
        }
    }
    ModelInfo {
        name,
        name_node,
        options,
        type_node: call.type_arguments.as_ref().and_then(|t| t.params.first()),
    }
}

/// `hasWithDefaults` / `getWithDefaultsPropExpressions`: default expressions by prop name, and
/// whether some defaults could not be determined statically.
pub fn with_defaults_prop_expressions<'a>(
    file: &LintFile<'a>,
    m: &Macro<'a>,
) -> (HashMap<String, &'a Expression<'a>>, bool) {
    let mut out = HashMap::new();
    let Some(wd) = m.with_defaults else {
        return (out, false);
    };
    let Some(defaults) = wd.arguments.get(1) else {
        return (out, false);
    };
    let Some(e) = defaults.as_expression() else {
        return (out, true);
    };
    let mut visited = Vec::new();
    let resolved = collect_prop_expressions(file, e, &mut out, &mut visited);
    (out, !resolved)
}

fn collect_prop_expressions<'a>(
    file: &LintFile<'a>,
    e: &'a Expression<'a>,
    out: &mut HashMap<String, &'a Expression<'a>>,
    visited: &mut Vec<Span>,
) -> bool {
    let Some(ObjectOrArray::Object(o)) = js::object_or_array(file, e) else {
        return false;
    };
    if visited.contains(&o.span) {
        return true;
    }
    visited.push(o.span);
    let mut ok = true;
    for p in &o.properties {
        match p {
            ObjectPropertyKind::SpreadProperty(s) => {
                if !collect_prop_expressions(file, &s.argument, out, visited) {
                    ok = false;
                }
            }
            ObjectPropertyKind::ObjectProperty(p) => match js::prop_name(p) {
                Some(n) => {
                    out.insert(n, &p.value);
                }
                None => ok = false,
            },
        }
    }
    ok
}

/// `getWithDefaultsProps`: properties of an inline `withDefaults` defaults object by name.
pub fn with_defaults_props<'a>(m: &Macro<'a>) -> HashMap<String, &'a ObjectProperty<'a>> {
    let mut out = HashMap::new();
    if let Some(Expression::ObjectExpression(o)) = m
        .with_defaults
        .and_then(|w| w.arguments.get(1))
        .and_then(|a| a.as_expression())
    {
        for p in js::properties(o) {
            if let Some(n) = js::prop_name(p) {
                out.insert(n, p);
            }
        }
    }
    out
}

/// `getPropsDestructure`: destructured props (`const { a, b = 1 } = defineProps()`) by name.
pub fn props_destructure<'a>(m: &Macro<'a>) -> HashMap<String, &'a BindingProperty<'a>> {
    let mut out = HashMap::new();
    if let Some(BindingPattern::ObjectPattern(p)) = m.left() {
        for prop in &p.properties {
            if let Some(n) = js::binding_prop_name(prop) {
                out.insert(n, prop);
            }
        }
    }
    out
}

/// `getDefaultPropExpressionsForPropsDestructure`: `{ a = 1 }` defaults by name.
pub fn destructure_default_expressions<'a>(
    m: &Macro<'a>,
) -> HashMap<String, (&'a BindingProperty<'a>, &'a Expression<'a>)> {
    props_destructure(m)
        .into_iter()
        .filter_map(|(n, p)| match &p.value {
            BindingPattern::AssignmentPattern(a) => Some((n, (p, &a.right))),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Type-based definitions (`ts-utils`, without a type checker)
// ---------------------------------------------------------------------------------------------

/// A flattened type definition node (`flattenTypeNodes`).
#[derive(Clone, Copy)]
enum Flat<'a> {
    Interface(&'a TSInterfaceBody<'a>),
    Type(&'a TSType<'a>),
}

fn resolve_type_ref<'a>(file: &LintFile<'a>, r: &'a TSTypeReference<'a>) -> Option<AstKind<'a>> {
    let TSTypeName::IdentifierReference(id) = &r.type_name else {
        return None;
    };
    let sem = file.semantic?;
    let sym = js::ref_symbol(sem, id)?;
    js::single_declaration(sem, sym)
}

fn flatten<'a>(
    file: &LintFile<'a>,
    t: &'a TSType<'a>,
    cache: &mut HashMap<Span, (Vec<Flat<'a>>, bool)>,
    out: &mut Vec<Flat<'a>>,
) {
    match t {
        TSType::TSUnionType(u) => u.types.iter().for_each(|t| flatten(file, t, cache, out)),
        TSType::TSIntersectionType(u) => u.types.iter().for_each(|t| flatten(file, t, cache, out)),
        TSType::TSParenthesizedType(p) => flatten(file, &p.type_annotation, cache, out),
        TSType::TSTypeReference(r) => match resolve_type_ref(file, r) {
            Some(AstKind::TSInterfaceDeclaration(i)) => out.push(Flat::Interface(&i.body)),
            Some(AstKind::TSTypeAliasDeclaration(a)) => {
                let key = a.type_annotation.span();
                if let Some((nodes, finished)) = cache.get(&key) {
                    out.extend(nodes.iter().copied());
                    if !finished {
                        out.push(Flat::Type(&a.type_annotation));
                    }
                    return;
                }
                cache.insert(key, (Vec::new(), false));
                let mut inner = Vec::new();
                flatten(file, &a.type_annotation, cache, &mut inner);
                // Upstream collects an alias's nodes in a `Set`: `A<T> & X | A<T> & Y` yields `A`'s members once.
                let mut seen = Vec::new();
                inner.retain(|f| {
                    let id = match f {
                        Flat::Interface(b) => std::ptr::from_ref(*b).cast::<()>(),
                        Flat::Type(t) => std::ptr::from_ref(*t).cast::<()>(),
                    };
                    !seen.contains(&id) && {
                        seen.push(id);
                        true
                    }
                });
                cache.insert(key, (inner.clone(), true));
                out.extend(inner);
            }
            _ => out.push(Flat::Type(t)),
        },
        _ => out.push(Flat::Type(t)),
    }
}

fn flatten_type<'a>(file: &LintFile<'a>, t: &'a TSType<'a>) -> Vec<Flat<'a>> {
    let mut out = Vec::new();
    flatten(file, t, &mut HashMap::new(), &mut out);
    out
}

/// Name of a type member key: identifier (also computed `[foo]`) or string/number literal.
fn type_key_name(key: &PropertyKey) -> Option<String> {
    match key {
        PropertyKey::StaticIdentifier(i) => Some(i.name.to_string()),
        PropertyKey::Identifier(i) => Some(i.name.to_string()),
        PropertyKey::StringLiteral(s) => Some(s.value.display().to_string()),
        PropertyKey::NumericLiteral(n) => Some(js::js_number(n.value)),
        PropertyKey::BooleanLiteral(b) => Some(b.value.to_string()),
        PropertyKey::NullLiteral(_) => Some("null".into()),
        PropertyKey::BigIntLiteral(b) => Some(b.value.to_string()),
        PropertyKey::RegExpLiteral(r) => Some(r.regex.to_string()),
        _ => None,
    }
}

/// `getComponentPropsFromTypeDefine`.
pub fn props_from_type<'a>(file: &LintFile<'a>, t: &'a TSType<'a>) -> Vec<ComponentProp<'a>> {
    let mut out = Vec::new();
    for f in flatten_type(file, t) {
        let members: &'a [TSSignature<'a>] = match f {
            Flat::Interface(b) => &b.body,
            Flat::Type(TSType::TSTypeLiteral(l)) => &l.members,
            Flat::Type(t) => {
                out.push(ComponentProp::Unknown {
                    node: Some(t.span()),
                });
                continue;
            }
        };
        for m in members {
            let (key, optional, types) = match m {
                TSSignature::TSPropertySignature(p) => (
                    &p.key,
                    p.optional,
                    p.type_annotation
                        .as_ref()
                        .map(|a| infer_runtime_type(file, &a.type_annotation, &mut Vec::new())),
                ),
                TSSignature::TSMethodSignature(p) => {
                    (&p.key, p.optional, Some(vec!["Function".to_string()]))
                }
                _ => continue,
            };
            match type_key_name(key) {
                Some(name) => out.push(ComponentProp::Type(TypeProp {
                    name,
                    key,
                    node: m,
                    required: !optional,
                    types: types.unwrap_or_else(|| vec!["null".into()]),
                })),
                None => out.push(ComponentProp::Unknown {
                    node: Some(key.span()),
                }),
            }
        }
    }
    out
}

/// `getComponentEmitsFromTypeDefine`.
pub fn emits_from_type<'a>(file: &LintFile<'a>, t: &'a TSType<'a>) -> Vec<ComponentEmit<'a>> {
    let mut out = Vec::new();
    for f in flatten_type(file, t) {
        let members: &'a [TSSignature<'a>] = match f {
            Flat::Interface(b) => &b.body,
            Flat::Type(TSType::TSTypeLiteral(l)) => &l.members,
            Flat::Type(TSType::TSFunctionType(ft)) => {
                event_names(&ft.params, TypeEmitNode::FunctionType(ft), &mut out);
                continue;
            }
            Flat::Type(t) => {
                out.push(ComponentEmit::Unknown {
                    node: Some(t.span()),
                });
                continue;
            }
        };
        for m in members {
            match m {
                TSSignature::TSCallSignatureDeclaration(c) => {
                    event_names(&c.params, TypeEmitNode::CallSignature(c), &mut out)
                }
                TSSignature::TSPropertySignature(p) => {
                    push_type_emit(&p.key, TypeEmitNode::Property(p), &mut out)
                }
                TSSignature::TSMethodSignature(p) => {
                    push_type_emit(&p.key, TypeEmitNode::Method(p), &mut out)
                }
                _ => {}
            }
        }
    }
    out
}

fn push_type_emit<'a>(
    key: &'a PropertyKey<'a>,
    node: TypeEmitNode<'a>,
    out: &mut Vec<ComponentEmit<'a>>,
) {
    match type_key_name(key) {
        Some(name) => out.push(ComponentEmit::Type {
            name,
            key: key.span(),
            node,
        }),
        None => out.push(ComponentEmit::Unknown {
            node: Some(key.span()),
        }),
    }
}

fn event_names<'a>(
    params: &'a FormalParameters<'a>,
    node: TypeEmitNode<'a>,
    out: &mut Vec<ComponentEmit<'a>>,
) {
    let Some(p) = params.items.first() else {
        return;
    };
    if !p.pattern.is_binding_identifier() || p.initializer.is_some() {
        return;
    }
    let Some(ann) = &p.type_annotation else {
        return;
    };
    let lit_name = |t: &TSType| match t {
        TSType::TSLiteralType(l) => match &l.literal {
            TSLiteral::StringLiteral(s) => Some((s.value.display().to_string(), l.span)),
            TSLiteral::NumericLiteral(n) => Some((js::js_number(n.value), l.span)),
            TSLiteral::BooleanLiteral(b) => Some((b.value.to_string(), l.span)),
            TSLiteral::BigIntLiteral(b) => Some((b.value.to_string(), l.span)),
            _ => None,
        },
        _ => None,
    };
    match &ann.type_annotation {
        TSType::TSUnionType(u) => {
            for t in &u.types {
                if let Some((n, s)) = lit_name(t) {
                    out.push(ComponentEmit::Type {
                        name: n,
                        key: s,
                        node,
                    });
                }
            }
        }
        t => {
            if let Some((n, s)) = lit_name(t) {
                out.push(ComponentEmit::Type {
                    name: n,
                    key: s,
                    node,
                });
            }
        }
    }
}

/// `inferRuntimeType` without type information (unknowns become `null`).
pub fn infer_runtime_type<'a>(
    file: &LintFile<'a>,
    t: &'a TSType<'a>,
    checked: &mut Vec<Span>,
) -> Vec<String> {
    let v = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    match t {
        TSType::TSStringKeyword(_) | TSType::TSTemplateLiteralType(_) => v(&["String"]),
        TSType::TSNumberKeyword(_) => v(&["Number"]),
        TSType::TSBooleanKeyword(_) => v(&["Boolean"]),
        TSType::TSObjectKeyword(_) => v(&["Object"]),
        TSType::TSTypeLiteral(l) => {
            let mut out: Vec<String> = Vec::new();
            for m in &l.members {
                let s = if matches!(
                    m,
                    TSSignature::TSCallSignatureDeclaration(_)
                        | TSSignature::TSConstructSignatureDeclaration(_)
                ) {
                    "Function"
                } else {
                    "Object"
                };
                if !out.iter().any(|x| x == s) {
                    out.push(s.into());
                }
            }
            if out.is_empty() { v(&["Object"]) } else { out }
        }
        TSType::TSFunctionType(_) => v(&["Function"]),
        TSType::TSArrayType(_) | TSType::TSTupleType(_) => v(&["Array"]),
        TSType::TSSymbolKeyword(_) => v(&["Symbol"]),
        TSType::TSParenthesizedType(p) => infer_runtime_type(file, &p.type_annotation, checked),
        TSType::TSLiteralType(l) => match &l.literal {
            TSLiteral::BooleanLiteral(_) => v(&["Boolean"]),
            TSLiteral::StringLiteral(_) => v(&["String"]),
            TSLiteral::NumericLiteral(_) | TSLiteral::BigIntLiteral(_) => v(&["Number"]),
            _ => v(&["null"]),
        },
        TSType::TSTypeReference(r) => {
            let TSTypeName::IdentifierReference(id) = &r.type_name else {
                return v(&["null"]);
            };
            match resolve_type_ref(file, r) {
                Some(AstKind::TSInterfaceDeclaration(_)) => return v(&["Object"]),
                Some(AstKind::TSTypeAliasDeclaration(a)) => {
                    let key = a.type_annotation.span();
                    if !checked.contains(&key) {
                        checked.push(key);
                        return infer_runtime_type(file, &a.type_annotation, checked);
                    }
                }
                Some(AstKind::TSEnumDeclaration(e)) => {
                    let mut out: Vec<String> = Vec::new();
                    for m in &e.body.members {
                        let Some(init) = &m.initializer else { continue };
                        let ty = match js::skip_parens(init) {
                            Expression::StringLiteral(_) => Some("String"),
                            Expression::NumericLiteral(_) | Expression::BigIntLiteral(_) => {
                                Some("Number")
                            }
                            Expression::BooleanLiteral(_) => Some("Boolean"),
                            Expression::NullLiteral(_) | Expression::RegExpLiteral(_) => None,
                            _ => Some("null"),
                        };
                        if let Some(ty) = ty
                            && !out.iter().any(|x| x == ty)
                        {
                            out.push(ty.into());
                        }
                    }
                    return if out.is_empty() { v(&["Number"]) } else { out };
                }
                _ => {}
            }
            let name = id.name.as_str();
            for n in [Some(name), name.strip_prefix("Readonly")]
                .into_iter()
                .flatten()
            {
                if matches!(
                    n,
                    "Array"
                        | "Function"
                        | "Object"
                        | "Set"
                        | "Map"
                        | "WeakSet"
                        | "WeakMap"
                        | "Date"
                ) {
                    return vec![n.to_string()];
                }
            }
            let targ = |i: usize| r.type_arguments.as_ref().and_then(|a| a.params.get(i));
            match name {
                "Record" | "Partial" | "Readonly" | "Pick" | "Omit" | "Required"
                | "InstanceType" => return v(&["Object"]),
                "Uppercase" | "Lowercase" | "Capitalize" | "Uncapitalize" => return v(&["String"]),
                "Parameters" | "ConstructorParameters" => return v(&["Array"]),
                "NonNullable" => {
                    if let Some(a) = targ(0) {
                        return infer_runtime_type(file, a, checked)
                            .into_iter()
                            .filter(|t| t != "null")
                            .collect();
                    }
                }
                "Extract" => {
                    if let Some(a) = targ(1) {
                        return infer_runtime_type(file, a, checked);
                    }
                }
                "Exclude" | "OmitThisParameter" => {
                    if let Some(a) = targ(0) {
                        return infer_runtime_type(file, a, checked);
                    }
                }
                _ => {}
            }
            v(&["null"])
        }
        TSType::TSUnionType(u) => union_types(file, &u.types, checked),
        TSType::TSIntersectionType(u) => union_types(file, &u.types, checked),
        _ => v(&["null"]),
    }
}

fn union_types<'a>(
    file: &LintFile<'a>,
    types: &'a [TSType<'a>],
    checked: &mut Vec<Span>,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in types {
        for tt in infer_runtime_type(file, t, checked) {
            if !out.contains(&tt) {
                out.push(tt);
            }
        }
    }
    out
}

/// `getComponentSlotsFromDefineSlots` without type information: `(slotName, node span)` of the
/// members of inline type literals / same-file interfaces (`None` name = unknown slot).
pub fn slots_from_define_slots<'a>(
    file: &LintFile<'a>,
    call: &'a CallExpression<'a>,
) -> Vec<(Option<String>, Span)> {
    let Some(t) = call.type_arguments.as_ref().and_then(|t| t.params.first()) else {
        return vec![(None, call.span)];
    };
    let mut out = Vec::new();
    for f in flatten_type(file, t) {
        let members: &[TSSignature] = match f {
            Flat::Interface(b) => &b.body,
            Flat::Type(TSType::TSTypeLiteral(l)) => &l.members,
            Flat::Type(t) => {
                out.push((None, t.span()));
                continue;
            }
        };
        for m in members {
            let (key, span) = match m {
                TSSignature::TSPropertySignature(p) => (&p.key, p.span),
                TSSignature::TSMethodSignature(p) => (&p.key, p.span),
                _ => continue,
            };
            out.push((type_key_name(key), span));
        }
    }
    out
}
