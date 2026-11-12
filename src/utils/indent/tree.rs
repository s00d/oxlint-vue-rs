//! A small ESTree / typescript-estree / vue-eslint-parser shaped tree built from the oxc AST and
//! the template [`Document`], so the indent engine can follow `indent-common.ts` literally:
//! ESTree node types, child order (visitor keys), and ESTree ranges (no `ParenthesizedExpression`
//! / `TSParenthesizedType`, identifiers spanning their type annotation, `TemplateElement` with its
//! delimiters, directives as `ExpressionStatement`s, rest elements inside element lists, …).

use oxc::ast::ast::*;
use oxc::span::{GetSpan, Span};

use crate::ast::{
    AttributeKey, AttributeValue, Child, DirectiveArgument, Document, ElementId, ExprId,
    VExpression,
};

pub const NONE: u32 = u32::MAX;

macro_rules! tys {
    ($($n:ident),* $(,)?) => {
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        pub enum Ty { $($n),* }
        impl Ty {
            pub fn name(self) -> &'static str {
                match self { $(Ty::$n => stringify!($n)),* }
            }
        }
    };
}

tys!(
    Program,
    ExpressionStatement,
    BlockStatement,
    BreakStatement,
    ContinueStatement,
    DebuggerStatement,
    DoWhileStatement,
    EmptyStatement,
    ForInStatement,
    ForOfStatement,
    ForStatement,
    IfStatement,
    LabeledStatement,
    ReturnStatement,
    SwitchStatement,
    SwitchCase,
    ThrowStatement,
    TryStatement,
    CatchClause,
    WhileStatement,
    WithStatement,
    VariableDeclaration,
    VariableDeclarator,
    FunctionDeclaration,
    FunctionExpression,
    ArrowFunctionExpression,
    ClassDeclaration,
    ClassExpression,
    ClassBody,
    MethodDefinition,
    PropertyDefinition,
    AccessorProperty,
    StaticBlock,
    ImportDeclaration,
    ImportSpecifier,
    ImportDefaultSpecifier,
    ImportNamespaceSpecifier,
    ImportAttribute,
    ExportAllDeclaration,
    ExportDefaultDeclaration,
    ExportNamedDeclaration,
    ExportSpecifier,
    Identifier,
    PrivateIdentifier,
    Literal,
    TemplateLiteral,
    TemplateElement,
    TaggedTemplateExpression,
    Super,
    ThisExpression,
    ArrayExpression,
    ObjectExpression,
    Property,
    SpreadElement,
    RestElement,
    ArrayPattern,
    ObjectPattern,
    AssignmentPattern,
    AssignmentExpression,
    BinaryExpression,
    LogicalExpression,
    UnaryExpression,
    UpdateExpression,
    AwaitExpression,
    YieldExpression,
    CallExpression,
    NewExpression,
    ImportExpression,
    MemberExpression,
    MetaProperty,
    ChainExpression,
    ConditionalExpression,
    SequenceExpression,
    Decorator,
    TSTypeAnnotation,
    TSAsExpression,
    TSSatisfiesExpression,
    TSTypeAssertion,
    TSNonNullExpression,
    TSInstantiationExpression,
    TSTypeAliasDeclaration,
    TSInterfaceDeclaration,
    TSInterfaceBody,
    TSInterfaceHeritage,
    TSClassImplements,
    TSEnumDeclaration,
    TSEnumBody,
    TSEnumMember,
    TSModuleDeclaration,
    TSModuleBlock,
    TSImportEqualsDeclaration,
    TSExternalModuleReference,
    TSExportAssignment,
    TSNamespaceExportDeclaration,
    TSDeclareFunction,
    TSEmptyBodyFunctionExpression,
    TSAbstractMethodDefinition,
    TSAbstractPropertyDefinition,
    TSAbstractAccessorProperty,
    TSParameterProperty,
    TSPropertySignature,
    TSIndexSignature,
    TSCallSignatureDeclaration,
    TSConstructSignatureDeclaration,
    TSMethodSignature,
    TSTypeParameterDeclaration,
    TSTypeParameterInstantiation,
    TSTypeParameter,
    TSQualifiedName,
    TSTypeReference,
    TSArrayType,
    TSConditionalType,
    TSConstructorType,
    TSFunctionType,
    TSImportType,
    TSIndexedAccessType,
    TSInferType,
    TSIntersectionType,
    TSUnionType,
    TSLiteralType,
    TSMappedType,
    TSNamedTupleMember,
    TSOptionalType,
    TSRestType,
    TSTemplateLiteralType,
    TSThisType,
    TSTupleType,
    TSTypeLiteral,
    TSTypeOperator,
    TSTypePredicate,
    TSTypeQuery,
    TSAnyKeyword,
    TSBigIntKeyword,
    TSBooleanKeyword,
    TSIntrinsicKeyword,
    TSNeverKeyword,
    TSNullKeyword,
    TSNumberKeyword,
    TSObjectKeyword,
    TSStringKeyword,
    TSSymbolKeyword,
    TSUndefinedKeyword,
    TSUnknownKeyword,
    TSVoidKeyword,
    VElement,
    VStartTag,
    VEndTag,
    VAttribute,
    VDirectiveKey,
    VIdentifier,
    VLiteral,
    VExpressionContainer,
    VText,
    VFilterSequenceExpression,
    VFilter,
    VForExpression,
    VOnExpression,
    VSlotScopeExpression,
    // Nodes the indent visitor has no handler for (JSX, …): their tokens are ignored.
    Unknown,
);

impl Ty {
    /// esquery `:statement` (also `:declaration`).
    pub fn is_statement_class(self) -> bool {
        let n = self.name();
        n.ends_with("Statement") || n.ends_with("Declaration")
    }
    /// esquery `:expression` (our `MetaProperty` has no `Identifier` children).
    pub fn is_expression_class(self) -> bool {
        let n = self.name();
        n.ends_with("Expression")
            || n.ends_with("Literal")
            || matches!(self, Ty::Identifier | Ty::MetaProperty)
    }
    /// `isTypeNode` of `ts-utils/ts-ast.ts`.
    pub fn is_type_node(self) -> bool {
        use Ty::*;
        matches!(
            self,
            TSAnyKeyword
                | TSArrayType
                | TSBigIntKeyword
                | TSBooleanKeyword
                | TSConditionalType
                | TSConstructorType
                | TSFunctionType
                | TSImportType
                | TSIndexedAccessType
                | TSInferType
                | TSIntersectionType
                | TSIntrinsicKeyword
                | TSLiteralType
                | TSMappedType
                | TSNamedTupleMember
                | TSNeverKeyword
                | TSNullKeyword
                | TSNumberKeyword
                | TSObjectKeyword
                | TSOptionalType
                | TSQualifiedName
                | TSRestType
                | TSStringKeyword
                | TSSymbolKeyword
                | TSTemplateLiteralType
                | TSThisType
                | TSTupleType
                | TSTypeLiteral
                | TSTypeOperator
                | TSTypePredicate
                | TSTypeQuery
                | TSTypeReference
                | TSUndefinedKeyword
                | TSUnionType
                | TSUnknownKeyword
                | TSVoidKeyword
        )
    }
}

/// Child field names (visitor keys).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum F {
    Body,
    Expression,
    Left,
    Right,
    Test,
    Consequent,
    Alternate,
    Label,
    Argument,
    Discriminant,
    Cases,
    Block,
    Handler,
    Finalizer,
    Param,
    Init,
    Update,
    Declarations,
    Id,
    Params,
    TypeParameters,
    ReturnType,
    Decorators,
    SuperClass,
    SuperTypeArguments,
    Implements,
    Key,
    Value,
    Specifiers,
    Source,
    Attributes,
    Imported,
    Local,
    Exported,
    Declaration,
    Quasis,
    Expressions,
    Tag,
    TypeArguments,
    Quasi,
    Elements,
    Properties,
    Callee,
    Arguments,
    Object,
    Property,
    TypeAnnotation,
    Members,
    Extends,
    Initializer,
    ModuleReference,
    Parameter,
    Parameters,
    Constraint,
    Default,
    Name,
    TypeName,
    ElementType,
    CheckType,
    ExtendsType,
    TrueType,
    FalseType,
    Types,
    IndexType,
    ObjectType,
    TypeParameter,
    NameType,
    ParameterName,
    ExprName,
    Qualifier,
    Options,
    Literal,
    ElementTypes,
    StartTag,
    EndTag,
    Children,
    Filters,
    Modifiers,
}

impl F {
    pub fn from_name(s: &str) -> Option<F> {
        Some(match s {
            "body" => F::Body,
            "expression" => F::Expression,
            "left" => F::Left,
            "right" => F::Right,
            "test" => F::Test,
            "consequent" => F::Consequent,
            "alternate" => F::Alternate,
            "argument" => F::Argument,
            "id" => F::Id,
            "params" => F::Params,
            "key" => F::Key,
            "value" => F::Value,
            "object" => F::Object,
            "property" => F::Property,
            "callee" => F::Callee,
            "arguments" => F::Arguments,
            "elements" => F::Elements,
            "properties" => F::Properties,
            "init" => F::Init,
            "declarations" => F::Declarations,
            "typeAnnotation" => F::TypeAnnotation,
            _ => return None,
        })
    }
}

pub mod flag {
    pub const COMPUTED: u8 = 1;
    pub const ASYNC: u8 = 2;
    pub const DELEGATE: u8 = 4;
    pub const AWAIT: u8 = 8;
    pub const OPTIONAL: u8 = 16;
    /// `VElement` named `pre` / `textarea`.
    pub const PRE: u8 = 32;
    /// `VText` whose value is whitespace only.
    pub const EMPTY_TEXT: u8 = 64;
}

#[derive(Clone, Copy, Debug)]
enum Slot {
    One(u32),
    Many(u32, u32),
}

#[derive(Clone, Copy, Debug)]
pub struct Node {
    pub ty: Ty,
    pub start: u32,
    pub end: u32,
    pub parent: u32,
    pub flags: u8,
    fs: u32,
    fl: u32,
}

#[derive(Default)]
pub struct Tree {
    pub nodes: Vec<Node>,
    fields: Vec<(F, Slot)>,
    items: Vec<u32>,
}

impl Tree {
    pub fn node(&self, n: u32) -> &Node {
        &self.nodes[n as usize]
    }
    pub fn ty(&self, n: u32) -> Ty {
        self.nodes[n as usize].ty
    }
    pub fn range(&self, n: u32) -> (u32, u32) {
        let x = &self.nodes[n as usize];
        (x.start, x.end)
    }
    pub fn parent(&self, n: u32) -> u32 {
        self.nodes[n as usize].parent
    }
    pub fn has(&self, n: u32, f: u8) -> bool {
        self.nodes[n as usize].flags & f != 0
    }
    fn node_fields(&self, n: u32) -> &[(F, Slot)] {
        let x = &self.nodes[n as usize];
        &self.fields[x.fs as usize..(x.fs + x.fl) as usize]
    }
    /// Single child (`NONE` when absent).
    pub fn get(&self, n: u32, f: F) -> u32 {
        for (k, s) in self.node_fields(n) {
            if *k == f {
                return match s {
                    Slot::One(c) => *c,
                    Slot::Many(..) => NONE,
                };
            }
        }
        NONE
    }
    /// List child (`NONE` entries = holes).
    pub fn list(&self, n: u32, f: F) -> &[u32] {
        for (k, s) in self.node_fields(n) {
            if *k == f
                && let Slot::Many(a, l) = s
            {
                return &self.items[*a as usize..(*a + *l) as usize];
            }
        }
        &[]
    }
    /// Children in visitor-key order.
    pub fn children(&self, n: u32, out: &mut Vec<u32>) {
        for (_, s) in self.node_fields(n) {
            match *s {
                Slot::One(c) => out.push(c),
                Slot::Many(a, l) => out.extend(
                    self.items[a as usize..(a + l) as usize]
                        .iter()
                        .copied()
                        .filter(|&c| c != NONE),
                ),
            }
        }
    }
    /// Children in visitor-key order, without allocating.
    pub fn child_iter(&self, n: u32) -> impl Iterator<Item = u32> + '_ {
        self.node_fields(n)
            .iter()
            .flat_map(move |(_, s)| {
                let (a, l, one) = match *s {
                    Slot::One(c) => (0, 0, Some(c)),
                    Slot::Many(a, l) => (a, l, None),
                };
                one.into_iter()
                    .chain(self.items[a as usize..(a + l) as usize].iter().copied())
            })
            .filter(|&c| c != NONE)
    }
    /// Which field of its parent holds `n`.
    pub fn field_of(&self, n: u32) -> Option<F> {
        let p = self.parent(n);
        if p == NONE {
            return None;
        }
        for (k, s) in self.node_fields(p) {
            match *s {
                Slot::One(c) if c == n => return Some(*k),
                Slot::Many(a, l) if self.items[a as usize..(a + l) as usize].contains(&n) => {
                    return Some(*k);
                }
                _ => {}
            }
        }
        None
    }
}

pub struct Builder<'s> {
    pub t: Tree,
    src: &'s str,
    scratch: Vec<(F, Slot)>,
    frames: Vec<usize>,
}

fn sp(s: Span) -> (u32, u32) {
    (s.start, s.end)
}

impl<'s> Builder<'s> {
    pub fn new(src: &'s str) -> Self {
        Builder {
            t: Tree::default(),
            src,
            scratch: Vec::new(),
            frames: Vec::new(),
        }
    }

    fn open(&mut self, ty: Ty, r: (u32, u32)) -> u32 {
        let id = self.t.nodes.len() as u32;
        self.t.nodes.push(Node {
            ty,
            start: r.0,
            end: r.1,
            parent: NONE,
            flags: 0,
            fs: 0,
            fl: 0,
        });
        self.frames.push(self.scratch.len());
        id
    }
    fn one(&mut self, f: F, c: u32) {
        if c != NONE {
            self.scratch.push((f, Slot::One(c)));
        }
    }
    fn many(&mut self, f: F, cs: &[u32]) {
        let a = self.t.items.len() as u32;
        self.t.items.extend_from_slice(cs);
        self.scratch.push((f, Slot::Many(a, cs.len() as u32)));
    }
    fn flag(&mut self, id: u32, f: u8, on: bool) {
        if on {
            self.t.nodes[id as usize].flags |= f;
        }
    }
    fn set_range(&mut self, id: u32, r: (u32, u32)) {
        let n = &mut self.t.nodes[id as usize];
        n.start = r.0;
        n.end = r.1;
    }
    fn close(&mut self, id: u32) -> u32 {
        let from = self.frames.pop().unwrap_or(0);
        let fs = self.t.fields.len() as u32;
        let fl = (self.scratch.len() - from) as u32;
        for i in from..self.scratch.len() {
            let (f, s) = self.scratch[i];
            match s {
                Slot::One(c) => self.t.nodes[c as usize].parent = id,
                Slot::Many(a, l) => {
                    for j in a..a + l {
                        let c = self.t.items[j as usize];
                        if c != NONE {
                            self.t.nodes[c as usize].parent = id;
                        }
                    }
                }
            }
            self.t.fields.push((f, s));
        }
        self.scratch.truncate(from);
        let n = &mut self.t.nodes[id as usize];
        n.fs = fs;
        n.fl = fl;
        id
    }
    fn leaf(&mut self, ty: Ty, r: (u32, u32)) -> u32 {
        let id = self.open(ty, r);
        self.close(id)
    }
    fn end_of(&self, id: u32) -> u32 {
        self.t.nodes[id as usize].end
    }
    fn start_of(&self, id: u32) -> u32 {
        self.t.nodes[id as usize].start
    }

    // ------------------------------------------------------------------
    // Script
    // ------------------------------------------------------------------

    /// `Program`: `range` is the ESTree program range.
    pub fn program(&mut self, p: &Program, range: (u32, u32)) -> u32 {
        let id = self.open(Ty::Program, range);
        let body = self.body(&p.directives, &p.body);
        self.many(F::Body, &body);
        self.close(id)
    }

    fn body(&mut self, dirs: &[Directive], stmts: &[Statement]) -> Vec<u32> {
        let mut v = Vec::with_capacity(dirs.len() + stmts.len());
        for d in dirs {
            let id = self.open(Ty::ExpressionStatement, sp(d.span));
            let e = self.leaf(Ty::Literal, sp(d.expression.span));
            self.one(F::Expression, e);
            v.push(self.close(id));
        }
        for s in stmts {
            v.push(self.stmt(s));
        }
        v
    }

    fn block(&mut self, b: &BlockStatement) -> u32 {
        let id = self.open(Ty::BlockStatement, sp(b.span));
        let body: Vec<u32> = b.body.iter().map(|s| self.stmt(s)).collect();
        self.many(F::Body, &body);
        self.close(id)
    }

    fn fn_body(&mut self, b: &FunctionBody) -> u32 {
        let id = self.open(Ty::BlockStatement, sp(b.span));
        let body = self.body(&b.directives, &b.statements);
        self.many(F::Body, &body);
        self.close(id)
    }

    fn ident(&mut self, s: Span) -> u32 {
        self.leaf(Ty::Identifier, sp(s))
    }

    fn opt_stmt(&mut self, s: Option<&Statement>) -> u32 {
        s.map_or(NONE, |s| self.stmt(s))
    }
    fn opt_expr(&mut self, e: Option<&Expression>) -> u32 {
        e.map_or(NONE, |e| self.expr(e))
    }

    pub fn stmt(&mut self, s: &Statement) -> u32 {
        match s {
            Statement::BlockStatement(b) => self.block(b),
            Statement::BreakStatement(b) => {
                let id = self.open(Ty::BreakStatement, sp(b.span));
                let l = b.label.as_ref().map_or(NONE, |l| self.ident(l.span));
                self.one(F::Label, l);
                self.close(id)
            }
            Statement::ContinueStatement(b) => {
                let id = self.open(Ty::ContinueStatement, sp(b.span));
                let l = b.label.as_ref().map_or(NONE, |l| self.ident(l.span));
                self.one(F::Label, l);
                self.close(id)
            }
            Statement::DebuggerStatement(d) => self.leaf(Ty::DebuggerStatement, sp(d.span)),
            Statement::DoWhileStatement(d) => {
                let id = self.open(Ty::DoWhileStatement, sp(d.span));
                let b = self.stmt(&d.body);
                self.one(F::Body, b);
                let t = self.expr(&d.test);
                self.one(F::Test, t);
                self.close(id)
            }
            Statement::EmptyStatement(e) => self.leaf(Ty::EmptyStatement, sp(e.span)),
            Statement::ExpressionStatement(e) => {
                let id = self.open(Ty::ExpressionStatement, sp(e.span));
                let x = self.expr(&e.expression);
                self.one(F::Expression, x);
                self.close(id)
            }
            Statement::ForInStatement(f) => {
                let id = self.open(Ty::ForInStatement, sp(f.span));
                let l = self.for_left(&f.left);
                self.one(F::Left, l);
                let r = self.expr(&f.right);
                self.one(F::Right, r);
                let b = self.stmt(&f.body);
                self.one(F::Body, b);
                self.close(id)
            }
            Statement::ForOfStatement(f) => {
                let id = self.open(Ty::ForOfStatement, sp(f.span));
                self.flag(id, flag::AWAIT, f.r#await);
                let l = self.for_left(&f.left);
                self.one(F::Left, l);
                let r = self.expr(&f.right);
                self.one(F::Right, r);
                let b = self.stmt(&f.body);
                self.one(F::Body, b);
                self.close(id)
            }
            Statement::ForStatement(f) => {
                let id = self.open(Ty::ForStatement, sp(f.span));
                let i = match &f.init {
                    None => NONE,
                    Some(ForStatementInit::VariableDeclaration(v)) => self.var_decl(v),
                    Some(init) => self.expr(init.to_expression()),
                };
                self.one(F::Init, i);
                let t = self.opt_expr(f.test.as_ref());
                self.one(F::Test, t);
                let u = self.opt_expr(f.update.as_ref());
                self.one(F::Update, u);
                let b = self.stmt(&f.body);
                self.one(F::Body, b);
                self.close(id)
            }
            Statement::IfStatement(i) => {
                let id = self.open(Ty::IfStatement, sp(i.span));
                let t = self.expr(&i.test);
                self.one(F::Test, t);
                let c = self.stmt(&i.consequent);
                self.one(F::Consequent, c);
                let a = self.opt_stmt(i.alternate.as_ref());
                self.one(F::Alternate, a);
                self.close(id)
            }
            Statement::LabeledStatement(l) => {
                let id = self.open(Ty::LabeledStatement, sp(l.span));
                let lb = self.ident(l.label.span);
                self.one(F::Label, lb);
                let b = self.stmt(&l.body);
                self.one(F::Body, b);
                self.close(id)
            }
            Statement::ReturnStatement(r) => {
                let id = self.open(Ty::ReturnStatement, sp(r.span));
                let a = self.opt_expr(r.argument.as_ref());
                self.one(F::Argument, a);
                self.close(id)
            }
            Statement::SwitchStatement(s) => {
                let id = self.open(Ty::SwitchStatement, sp(s.span));
                let d = self.expr(&s.discriminant);
                self.one(F::Discriminant, d);
                let cases: Vec<u32> = s
                    .cases
                    .iter()
                    .map(|c| {
                        let cid = self.open(Ty::SwitchCase, sp(c.span));
                        let t = self.opt_expr(c.test.as_ref());
                        self.one(F::Test, t);
                        let cs: Vec<u32> = c.consequent.iter().map(|s| self.stmt(s)).collect();
                        self.many(F::Consequent, &cs);
                        self.close(cid)
                    })
                    .collect();
                self.many(F::Cases, &cases);
                self.close(id)
            }
            Statement::ThrowStatement(t) => {
                let id = self.open(Ty::ThrowStatement, sp(t.span));
                let a = self.expr(&t.argument);
                self.one(F::Argument, a);
                self.close(id)
            }
            Statement::TryStatement(t) => {
                let id = self.open(Ty::TryStatement, sp(t.span));
                let b = self.block(&t.block);
                self.one(F::Block, b);
                let h = t.handler.as_ref().map_or(NONE, |h| {
                    let hid = self.open(Ty::CatchClause, sp(h.span));
                    let p = h.param.as_ref().map_or(NONE, |p| {
                        self.binding(&p.pattern, p.type_annotation.as_deref(), None, &[])
                    });
                    self.one(F::Param, p);
                    let b = self.block(&h.body);
                    self.one(F::Body, b);
                    self.close(hid)
                });
                self.one(F::Handler, h);
                let f = t.finalizer.as_ref().map_or(NONE, |f| self.block(f));
                self.one(F::Finalizer, f);
                self.close(id)
            }
            Statement::WhileStatement(w) => {
                let id = self.open(Ty::WhileStatement, sp(w.span));
                let t = self.expr(&w.test);
                self.one(F::Test, t);
                let b = self.stmt(&w.body);
                self.one(F::Body, b);
                self.close(id)
            }
            Statement::WithStatement(w) => {
                let id = self.open(Ty::WithStatement, sp(w.span));
                let o = self.expr(&w.object);
                self.one(F::Object, o);
                let b = self.stmt(&w.body);
                self.one(F::Body, b);
                self.close(id)
            }
            Statement::VariableDeclaration(_)
            | Statement::FunctionDeclaration(_)
            | Statement::ClassDeclaration(_)
            | Statement::TSTypeAliasDeclaration(_)
            | Statement::TSInterfaceDeclaration(_)
            | Statement::TSEnumDeclaration(_)
            | Statement::TSExternalModuleDeclaration(_)
            | Statement::TSNamespaceDeclaration(_)
            | Statement::TSGlobalDeclaration(_)
            | Statement::TSImportEqualsDeclaration(_) => self.decl(s.to_declaration()),
            Statement::ImportDeclaration(i) => {
                let id = self.open(Ty::ImportDeclaration, sp(i.span));
                let specs: Vec<u32> = i
                    .specifiers
                    .iter()
                    .flatten()
                    .map(|s| match s {
                        ImportDeclarationSpecifier::ImportSpecifier(s) => {
                            let sid = self.open(Ty::ImportSpecifier, sp(s.span));
                            let im = self.module_name(&s.imported);
                            self.one(F::Imported, im);
                            let l = self.ident(s.local.span);
                            self.one(F::Local, l);
                            self.close(sid)
                        }
                        ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                            let sid = self.open(Ty::ImportDefaultSpecifier, sp(s.span));
                            let l = self.ident(s.local.span);
                            self.one(F::Local, l);
                            self.close(sid)
                        }
                        ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                            let sid = self.open(Ty::ImportNamespaceSpecifier, sp(s.span));
                            let l = self.ident(s.local.span);
                            self.one(F::Local, l);
                            self.close(sid)
                        }
                    })
                    .collect();
                self.many(F::Specifiers, &specs);
                let s = self.leaf(Ty::Literal, sp(i.source.span));
                self.one(F::Source, s);
                let a = self.with_clause(i.with_clause.as_deref());
                self.many(F::Attributes, &a);
                self.close(id)
            }
            Statement::ExportAllDeclaration(e) => {
                let id = self.open(Ty::ExportAllDeclaration, sp(e.span));
                let x = e.exported.as_ref().map_or(NONE, |x| self.module_name(x));
                self.one(F::Exported, x);
                let s = self.leaf(Ty::Literal, sp(e.source.span));
                self.one(F::Source, s);
                let a = self.with_clause(e.with_clause.as_deref());
                self.many(F::Attributes, &a);
                self.close(id)
            }
            Statement::ExportDefaultDeclaration(e) => {
                let id = self.open(Ty::ExportDefaultDeclaration, sp(e.span));
                let d = match &e.declaration {
                    ExportDefaultDeclarationKind::FunctionDeclaration(f) => self.function(f),
                    ExportDefaultDeclarationKind::ClassDeclaration(c) => self.class(c),
                    ExportDefaultDeclarationKind::TSInterfaceDeclaration(t) => self.interface(t),
                    other => self.expr(other.to_expression()),
                };
                self.one(F::Declaration, d);
                self.close(id)
            }
            Statement::ExportDeclaration(e) => {
                let id = self.open(Ty::ExportNamedDeclaration, sp(e.span));
                let d = self.decl(&e.declaration);
                self.one(F::Declaration, d);
                self.many(F::Specifiers, &[]);
                self.close(id)
            }
            Statement::ExportNamedDeclaration(e) => {
                let id = self.open(Ty::ExportNamedDeclaration, sp(e.span));
                let specs = self.export_specs(&e.specifiers);
                self.many(F::Specifiers, &specs);
                self.close(id)
            }
            Statement::ExportFromDeclaration(e) => {
                let id = self.open(Ty::ExportNamedDeclaration, sp(e.span));
                let specs = self.export_specs(&e.specifiers);
                self.many(F::Specifiers, &specs);
                let s = self.leaf(Ty::Literal, sp(e.source.span));
                self.one(F::Source, s);
                let a = self.with_clause(e.with_clause.as_deref());
                self.many(F::Attributes, &a);
                self.close(id)
            }
            Statement::TSExportAssignment(e) => {
                let id = self.open(Ty::TSExportAssignment, sp(e.span));
                let x = self.expr(&e.expression);
                self.one(F::Expression, x);
                self.close(id)
            }
            Statement::TSNamespaceExportDeclaration(e) => {
                let id = self.open(Ty::TSNamespaceExportDeclaration, sp(e.span));
                let i = self.ident(e.id.span);
                self.one(F::Id, i);
                self.close(id)
            }
        }
    }

    fn decl(&mut self, d: &Declaration) -> u32 {
        match d {
            Declaration::VariableDeclaration(v) => self.var_decl(v),
            Declaration::FunctionDeclaration(f) => self.function(f),
            Declaration::ClassDeclaration(c) => self.class(c),
            Declaration::TSTypeAliasDeclaration(t) => {
                let id = self.open(Ty::TSTypeAliasDeclaration, sp(t.span));
                let i = self.ident(t.id.span);
                self.one(F::Id, i);
                let tp = self.type_params(t.type_parameters.as_deref());
                self.one(F::TypeParameters, tp);
                let ta = self.ts_type(&t.type_annotation);
                self.one(F::TypeAnnotation, ta);
                self.close(id)
            }
            Declaration::TSInterfaceDeclaration(t) => self.interface(t),
            Declaration::TSEnumDeclaration(e) => {
                let id = self.open(Ty::TSEnumDeclaration, sp(e.span));
                let i = self.ident(e.id.span);
                self.one(F::Id, i);
                let bid = self.open(Ty::TSEnumBody, sp(e.body.span));
                let ms: Vec<u32> = e
                    .body
                    .members
                    .iter()
                    .map(|m| {
                        let mid = self.open(Ty::TSEnumMember, sp(m.span));
                        let (k, computed) = match &m.id {
                            TSEnumMemberName::Identifier(i) => (self.ident(i.span), false),
                            TSEnumMemberName::String(s) => {
                                (self.leaf(Ty::Literal, sp(s.span)), false)
                            }
                            TSEnumMemberName::ComputedString(s) => {
                                (self.leaf(Ty::Literal, sp(s.span)), true)
                            }
                            TSEnumMemberName::ComputedTemplateString(t) => (self.template(t), true),
                        };
                        self.flag(mid, flag::COMPUTED, computed);
                        self.one(F::Id, k);
                        let init = self.opt_expr(m.initializer.as_ref());
                        self.one(F::Initializer, init);
                        self.close(mid)
                    })
                    .collect();
                self.many(F::Members, &ms);
                let b = self.close(bid);
                self.one(F::Body, b);
                self.close(id)
            }
            Declaration::TSExternalModuleDeclaration(m) => {
                let id = self.open(Ty::TSModuleDeclaration, sp(m.span));
                let i = self.leaf(Ty::Literal, sp(m.id.span));
                self.one(F::Id, i);
                let b = m.body.as_ref().map_or(NONE, |b| self.module_block(b));
                self.one(F::Body, b);
                self.close(id)
            }
            Declaration::TSNamespaceDeclaration(n) => {
                let id = self.open(Ty::TSModuleDeclaration, sp(n.span));
                // `namespace A.B {}` → `id: TSQualifiedName(A.B)`.
                let mut idn = self.ident(n.id.span);
                let mut cur: &TSNamespaceDeclaration = n;
                while let TSNamespaceDeclarationBody::TSNamespaceDeclaration(inner) = &cur.body {
                    let q = self.open(Ty::TSQualifiedName, (self.start_of(idn), inner.id.span.end));
                    self.one(F::Left, idn);
                    let r = self.ident(inner.id.span);
                    self.one(F::Right, r);
                    idn = self.close(q);
                    cur = inner;
                }
                self.one(F::Id, idn);
                let b = match &cur.body {
                    TSNamespaceDeclarationBody::TSModuleBlock(b) => self.module_block(b),
                    TSNamespaceDeclarationBody::TSNamespaceDeclaration(_) => NONE,
                };
                self.one(F::Body, b);
                self.close(id)
            }
            Declaration::TSGlobalDeclaration(g) => {
                let id = self.open(Ty::TSModuleDeclaration, sp(g.span));
                let i = self.ident(g.global_span);
                self.one(F::Id, i);
                let b = self.module_block(&g.body);
                self.one(F::Body, b);
                self.close(id)
            }
            Declaration::TSImportEqualsDeclaration(t) => {
                let id = self.open(Ty::TSImportEqualsDeclaration, sp(t.span));
                let i = self.ident(t.id.span);
                self.one(F::Id, i);
                let m = match &t.module_reference {
                    TSModuleReference::ExternalModuleReference(e) => {
                        let eid = self.open(Ty::TSExternalModuleReference, sp(e.span));
                        let l = self.leaf(Ty::Literal, sp(e.expression.span));
                        self.one(F::Expression, l);
                        self.close(eid)
                    }
                    TSModuleReference::IdentifierReference(i) => self.ident(i.span),
                    TSModuleReference::QualifiedName(q) => self.qualified(q),
                };
                self.one(F::ModuleReference, m);
                self.close(id)
            }
        }
    }

    fn export_specs(&mut self, specs: &[ExportSpecifier]) -> Vec<u32> {
        specs
            .iter()
            .map(|s| {
                let sid = self.open(Ty::ExportSpecifier, sp(s.span));
                let l = self.module_name(&s.local);
                self.one(F::Local, l);
                let e = self.module_name(&s.exported);
                self.one(F::Exported, e);
                self.close(sid)
            })
            .collect()
    }

    fn module_name(&mut self, m: &ModuleExportName) -> u32 {
        match m {
            ModuleExportName::IdentifierName(i) => self.ident(i.span),
            ModuleExportName::IdentifierReference(i) => self.ident(i.span),
            ModuleExportName::StringLiteral(s) => self.leaf(Ty::Literal, sp(s.span)),
        }
    }

    fn with_clause(&mut self, w: Option<&WithClause>) -> Vec<u32> {
        let Some(w) = w else { return Vec::new() };
        w.with_entries
            .iter()
            .map(|a| {
                let id = self.open(Ty::ImportAttribute, sp(a.span));
                let k = match &a.key {
                    ImportAttributeKey::Identifier(i) => self.ident(i.span),
                    ImportAttributeKey::StringLiteral(s) => self.leaf(Ty::Literal, sp(s.span)),
                };
                self.one(F::Key, k);
                let v = self.leaf(Ty::Literal, sp(a.value.span));
                self.one(F::Value, v);
                self.close(id)
            })
            .collect()
    }

    fn module_block(&mut self, b: &TSModuleBlock) -> u32 {
        let id = self.open(Ty::TSModuleBlock, sp(b.span));
        let body = self.body(&b.directives, &b.body);
        self.many(F::Body, &body);
        self.close(id)
    }

    fn interface(&mut self, t: &TSInterfaceDeclaration) -> u32 {
        let id = self.open(Ty::TSInterfaceDeclaration, sp(t.span));
        let i = self.ident(t.id.span);
        self.one(F::Id, i);
        let tp = self.type_params(t.type_parameters.as_deref());
        self.one(F::TypeParameters, tp);
        let ex: Vec<u32> = t
            .extends
            .iter()
            .map(|h| {
                let hid = self.open(Ty::TSInterfaceHeritage, sp(h.span));
                let e = self.type_name(&h.type_name);
                self.one(F::Expression, e);
                let ta = self.type_args(h.type_arguments.as_deref());
                self.one(F::TypeArguments, ta);
                self.close(hid)
            })
            .collect();
        self.many(F::Extends, &ex);
        let bid = self.open(Ty::TSInterfaceBody, sp(t.body.span));
        let ms: Vec<u32> = t.body.body.iter().map(|s| self.signature(s)).collect();
        self.many(F::Body, &ms);
        let b = self.close(bid);
        self.one(F::Body, b);
        self.close(id)
    }

    fn var_decl(&mut self, v: &VariableDeclaration) -> u32 {
        let id = self.open(Ty::VariableDeclaration, sp(v.span));
        let ds: Vec<u32> = v
            .declarations
            .iter()
            .map(|d| {
                let did = self.open(Ty::VariableDeclarator, sp(d.span));
                let i = self.binding(&d.id, d.type_annotation.as_deref(), None, &[]);
                self.one(F::Id, i);
                let init = self.opt_expr(d.init.as_ref());
                self.one(F::Init, init);
                self.close(did)
            })
            .collect();
        self.many(F::Declarations, &ds);
        self.close(id)
    }

    fn for_left(&mut self, l: &ForStatementLeft) -> u32 {
        match l {
            ForStatementLeft::VariableDeclaration(v) => self.var_decl(v),
            other => self.target(other.to_assignment_target()),
        }
    }

    fn decorators(&mut self, ds: &[Decorator]) -> Vec<u32> {
        ds.iter()
            .map(|d| {
                let id = self.open(Ty::Decorator, sp(d.span));
                let e = self.expr(&d.expression);
                self.one(F::Expression, e);
                self.close(id)
            })
            .collect()
    }

    fn type_ann(&mut self, a: Option<&TSTypeAnnotation>) -> u32 {
        let Some(a) = a else { return NONE };
        let id = self.open(Ty::TSTypeAnnotation, sp(a.span));
        let t = self.ts_type(&a.type_annotation);
        self.one(F::TypeAnnotation, t);
        self.close(id)
    }

    /// A binding pattern carrying (ESTree) decorators and type annotation, its range extended over
    /// the annotation; `opt_end` = end of an optional `?`.
    fn binding(
        &mut self,
        p: &BindingPattern,
        ann: Option<&TSTypeAnnotation>,
        opt_end: Option<u32>,
        decos: &[Decorator],
    ) -> u32 {
        let base = p.span();
        let end = base
            .end
            .max(ann.map_or(0, |a| a.span.end))
            .max(opt_end.unwrap_or(0));
        match p {
            BindingPattern::BindingIdentifier(b) => {
                let id = self.open(Ty::Identifier, (b.span.start, end));
                let d = self.decorators(decos);
                self.many(F::Decorators, &d);
                let t = self.type_ann(ann);
                self.one(F::TypeAnnotation, t);
                self.close(id)
            }
            BindingPattern::ObjectPattern(o) => {
                let id = self.open(Ty::ObjectPattern, (o.span.start, end));
                let d = self.decorators(decos);
                self.many(F::Decorators, &d);
                let mut props: Vec<u32> = o
                    .properties
                    .iter()
                    .map(|bp| {
                        let pid = self.open(Ty::Property, sp(bp.span));
                        self.flag(pid, flag::COMPUTED, bp.computed);
                        let k = self.prop_key(&bp.key);
                        self.one(F::Key, k);
                        let v = self.binding(&bp.value, None, None, &[]);
                        self.one(F::Value, v);
                        self.close(pid)
                    })
                    .collect();
                if let Some(r) = &o.rest {
                    props.push(self.rest(r, None, &[]));
                }
                self.many(F::Properties, &props);
                let t = self.type_ann(ann);
                self.one(F::TypeAnnotation, t);
                self.close(id)
            }
            BindingPattern::ArrayPattern(a) => {
                let id = self.open(Ty::ArrayPattern, (a.span.start, end));
                let d = self.decorators(decos);
                self.many(F::Decorators, &d);
                let mut els: Vec<u32> = a
                    .elements
                    .iter()
                    .map(|e| {
                        e.as_ref()
                            .map_or(NONE, |e| self.binding(e, None, None, &[]))
                    })
                    .collect();
                if let Some(r) = &a.rest {
                    els.push(self.rest(r, None, &[]));
                }
                self.many(F::Elements, &els);
                let t = self.type_ann(ann);
                self.one(F::TypeAnnotation, t);
                self.close(id)
            }
            BindingPattern::AssignmentPattern(a) => {
                let id = self.open(Ty::AssignmentPattern, (a.span.start, end));
                let d = self.decorators(decos);
                self.many(F::Decorators, &d);
                let l = self.binding(&a.left, None, None, &[]);
                self.one(F::Left, l);
                let r = self.expr(&a.right);
                self.one(F::Right, r);
                let t = self.type_ann(ann);
                self.one(F::TypeAnnotation, t);
                self.close(id)
            }
        }
    }

    fn rest(
        &mut self,
        r: &BindingRestElement,
        ann: Option<&TSTypeAnnotation>,
        decos: &[Decorator],
    ) -> u32 {
        let end = r.span.end.max(ann.map_or(0, |a| a.span.end));
        let id = self.open(Ty::RestElement, (r.span.start, end));
        let d = self.decorators(decos);
        self.many(F::Decorators, &d);
        let a = self.binding(&r.argument, None, None, &[]);
        self.one(F::Argument, a);
        let t = self.type_ann(ann);
        self.one(F::TypeAnnotation, t);
        self.close(id)
    }

    /// End of a `?` right after `pos` (skipping whitespace).
    fn question_end(&self, pos: u32) -> Option<u32> {
        let rest = self.src.get(pos as usize..)?;
        let skip = rest.len() - rest.trim_start().len();
        rest[skip..]
            .starts_with('?')
            .then_some(pos + skip as u32 + 1)
    }

    fn params(&mut self, ps: &FormalParameters, this: Option<&TSThisParameter>) -> Vec<u32> {
        let mut v = Vec::with_capacity(ps.items.len() + 2);
        if let Some(t) = this {
            let id = self.open(Ty::Identifier, sp(t.span));
            let a = self.type_ann(t.type_annotation.as_deref());
            self.one(F::TypeAnnotation, a);
            v.push(self.close(id));
        }
        for fp in &ps.items {
            v.push(self.param(fp));
        }
        if let Some(r) = &ps.rest {
            v.push(self.rest(&r.rest, r.type_annotation.as_deref(), &r.decorators));
        }
        v
    }

    fn param(&mut self, fp: &FormalParameter) -> u32 {
        let ts_prop = fp.accessibility.is_some() || fp.readonly || fp.r#override;
        let opt_end = if fp.optional && fp.type_annotation.is_none() {
            self.question_end(fp.pattern.span().end)
        } else {
            None
        };
        let decos: &[Decorator] = if ts_prop { &[] } else { &fp.decorators };
        let inner = match &fp.initializer {
            Some(init) => {
                let id = self.open(Ty::AssignmentPattern, (0, 0));
                let l = self.binding(&fp.pattern, fp.type_annotation.as_deref(), opt_end, decos);
                self.one(F::Left, l);
                let r = self.expr(init);
                self.one(F::Right, r);
                let r = (self.start_of(l), init.span().end);
                self.set_range(id, r);
                self.close(id)
            }
            None => self.binding(&fp.pattern, fp.type_annotation.as_deref(), opt_end, decos),
        };
        if ts_prop {
            let id = self.open(Ty::TSParameterProperty, sp(fp.span));
            let d = self.decorators(&fp.decorators);
            self.many(F::Decorators, &d);
            self.one(F::Parameter, inner);
            self.close(id)
        } else {
            inner
        }
    }

    fn function(&mut self, f: &Function) -> u32 {
        let ty = match f.r#type {
            FunctionType::FunctionDeclaration => Ty::FunctionDeclaration,
            FunctionType::FunctionExpression => Ty::FunctionExpression,
            FunctionType::TSDeclareFunction => Ty::TSDeclareFunction,
            FunctionType::TSEmptyBodyFunctionExpression => Ty::TSEmptyBodyFunctionExpression,
        };
        // A body-less method value is a `TSEmptyBodyFunctionExpression`.
        let ty = if ty == Ty::FunctionExpression && f.body.is_none() {
            Ty::TSEmptyBodyFunctionExpression
        } else if ty == Ty::FunctionDeclaration && f.body.is_none() {
            Ty::TSDeclareFunction
        } else {
            ty
        };
        let id = self.open(ty, sp(f.span));
        self.flag(id, flag::ASYNC, f.r#async);
        let i = f.id.as_ref().map_or(NONE, |i| self.ident(i.span));
        self.one(F::Id, i);
        let tp = self.type_params(f.type_parameters.as_deref());
        self.one(F::TypeParameters, tp);
        let ps = self.params(&f.params, f.this_param.as_deref());
        self.many(F::Params, &ps);
        let rt = self.type_ann(f.return_type.as_deref());
        self.one(F::ReturnType, rt);
        let b = f.body.as_ref().map_or(NONE, |b| self.fn_body(b));
        self.one(F::Body, b);
        self.close(id)
    }

    fn class(&mut self, c: &Class) -> u32 {
        let ty = match c.r#type {
            ClassType::ClassDeclaration => Ty::ClassDeclaration,
            ClassType::ClassExpression => Ty::ClassExpression,
        };
        let id = self.open(ty, sp(c.span));
        let d = self.decorators(&c.decorators);
        self.many(F::Decorators, &d);
        let i = c.id.as_ref().map_or(NONE, |i| self.ident(i.span));
        self.one(F::Id, i);
        let tp = self.type_params(c.type_parameters.as_deref());
        self.one(F::TypeParameters, tp);
        if let Some(h) = &c.heritage {
            let s = self.expr(&h.expression);
            self.one(F::SuperClass, s);
            let ta = self.type_args(h.type_arguments.as_deref());
            self.one(F::SuperTypeArguments, ta);
        }
        let imps: Vec<u32> = c
            .implements
            .iter()
            .map(|h| {
                let hid = self.open(Ty::TSClassImplements, sp(h.span));
                let e = self.type_name(&h.expression);
                self.one(F::Expression, e);
                let ta = self.type_args(h.type_arguments.as_deref());
                self.one(F::TypeArguments, ta);
                self.close(hid)
            })
            .collect();
        self.many(F::Implements, &imps);
        let bid = self.open(Ty::ClassBody, sp(c.body.span));
        let els: Vec<u32> = c.body.body.iter().map(|e| self.class_element(e)).collect();
        self.many(F::Body, &els);
        let b = self.close(bid);
        self.one(F::Body, b);
        self.close(id)
    }

    fn class_element(&mut self, e: &ClassElement) -> u32 {
        match e {
            ClassElement::StaticBlock(s) => {
                let id = self.open(Ty::StaticBlock, sp(s.span));
                let b: Vec<u32> = s.body.iter().map(|s| self.stmt(s)).collect();
                self.many(F::Body, &b);
                self.close(id)
            }
            ClassElement::MethodDefinition(m) => {
                let ty = match m.r#type {
                    MethodDefinitionType::MethodDefinition => Ty::MethodDefinition,
                    MethodDefinitionType::TSAbstractMethodDefinition => {
                        Ty::TSAbstractMethodDefinition
                    }
                };
                let id = self.open(ty, sp(m.span));
                self.flag(id, flag::COMPUTED, m.computed);
                let d = self.decorators(&m.decorators);
                self.many(F::Decorators, &d);
                let k = self.prop_key(&m.key);
                self.one(F::Key, k);
                let v = self.function(&m.value);
                self.one(F::Value, v);
                self.close(id)
            }
            ClassElement::PropertyDefinition(p) => {
                let ty = match p.r#type {
                    PropertyDefinitionType::PropertyDefinition => Ty::PropertyDefinition,
                    PropertyDefinitionType::TSAbstractPropertyDefinition => {
                        Ty::TSAbstractPropertyDefinition
                    }
                };
                let id = self.open(ty, sp(p.span));
                self.flag(id, flag::COMPUTED, p.computed);
                let d = self.decorators(&p.decorators);
                self.many(F::Decorators, &d);
                let k = self.prop_key(&p.key);
                self.one(F::Key, k);
                let t = self.type_ann(p.type_annotation.as_deref());
                self.one(F::TypeAnnotation, t);
                let v = self.opt_expr(p.value.as_ref());
                self.one(F::Value, v);
                self.close(id)
            }
            ClassElement::AccessorProperty(p) => {
                let ty = match p.r#type {
                    AccessorPropertyType::AccessorProperty => Ty::AccessorProperty,
                    AccessorPropertyType::TSAbstractAccessorProperty => {
                        Ty::TSAbstractAccessorProperty
                    }
                };
                let id = self.open(ty, sp(p.span));
                self.flag(id, flag::COMPUTED, p.computed);
                let d = self.decorators(&p.decorators);
                self.many(F::Decorators, &d);
                let k = self.prop_key(&p.key);
                self.one(F::Key, k);
                let t = self.type_ann(p.type_annotation.as_deref());
                self.one(F::TypeAnnotation, t);
                let v = self.opt_expr(p.value.as_ref());
                self.one(F::Value, v);
                self.close(id)
            }
            ClassElement::TSIndexSignature(s) => self.index_signature(s),
        }
    }

    fn index_signature(&mut self, s: &TSIndexSignature) -> u32 {
        let id = self.open(Ty::TSIndexSignature, sp(s.span));
        let pid = self.open(Ty::Identifier, sp(s.parameter.span));
        let a = self.type_ann(Some(&s.parameter.type_annotation));
        self.one(F::TypeAnnotation, a);
        let p = self.close(pid);
        self.many(F::Parameters, &[p]);
        let t = self.type_ann(Some(&s.type_annotation));
        self.one(F::TypeAnnotation, t);
        self.close(id)
    }

    fn prop_key(&mut self, k: &PropertyKey) -> u32 {
        match k {
            PropertyKey::StaticIdentifier(i) => self.ident(i.span),
            PropertyKey::PrivateIdentifier(i) => self.leaf(Ty::PrivateIdentifier, sp(i.span)),
            other => self.expr(other.to_expression()),
        }
    }

    fn template(&mut self, t: &TemplateLiteral) -> u32 {
        let id = self.open(Ty::TemplateLiteral, sp(t.span));
        let qs: Vec<u32> = t
            .quasis
            .iter()
            .map(|q| {
                let r = (
                    q.span.start.saturating_sub(1),
                    q.span.end + if q.tail { 1 } else { 2 },
                );
                self.leaf(Ty::TemplateElement, r)
            })
            .collect();
        self.many(F::Quasis, &qs);
        let es: Vec<u32> = t.expressions.iter().map(|e| self.expr(e)).collect();
        self.many(F::Expressions, &es);
        self.close(id)
    }

    fn args(&mut self, a: &[Argument]) -> Vec<u32> {
        a.iter().map(|a| self.arg(a)).collect()
    }
    fn arg(&mut self, a: &Argument) -> u32 {
        match a {
            Argument::SpreadElement(s) => self.spread(s),
            other => self.expr(other.to_expression()),
        }
    }
    fn spread(&mut self, s: &SpreadElement) -> u32 {
        let id = self.open(Ty::SpreadElement, sp(s.span));
        let a = self.expr(&s.argument);
        self.one(F::Argument, a);
        self.close(id)
    }

    fn member(&mut self, m: &MemberExpression) -> u32 {
        match m {
            MemberExpression::ComputedMemberExpression(c) => {
                let id = self.open(Ty::MemberExpression, sp(c.span));
                self.flag(id, flag::COMPUTED, true);
                let o = self.expr(&c.object);
                self.one(F::Object, o);
                let p = self.expr(&c.expression);
                self.one(F::Property, p);
                self.close(id)
            }
            MemberExpression::StaticMemberExpression(s) => {
                let id = self.open(Ty::MemberExpression, sp(s.span));
                let o = self.expr(&s.object);
                self.one(F::Object, o);
                let p = self.ident(s.property.span);
                self.one(F::Property, p);
                self.close(id)
            }
            MemberExpression::PrivateFieldExpression(s) => {
                let id = self.open(Ty::MemberExpression, sp(s.span));
                let o = self.expr(&s.object);
                self.one(F::Object, o);
                let p = self.leaf(Ty::PrivateIdentifier, sp(s.field.span));
                self.one(F::Property, p);
                self.close(id)
            }
        }
    }

    fn call(&mut self, c: &CallExpression) -> u32 {
        let id = self.open(Ty::CallExpression, sp(c.span));
        let callee = self.expr(&c.callee);
        self.one(F::Callee, callee);
        let ta = self.type_args(c.type_arguments.as_deref());
        self.one(F::TypeArguments, ta);
        let a = self.args(&c.arguments);
        self.many(F::Arguments, &a);
        self.close(id)
    }

    fn ts_unary(&mut self, ty: Ty, span: Span, e: &Expression) -> u32 {
        let id = self.open(ty, sp(span));
        let x = self.expr(e);
        self.one(F::Expression, x);
        self.close(id)
    }
    fn ts_cast(&mut self, ty: Ty, span: Span, e: &Expression, t: &TSType) -> u32 {
        let id = self.open(ty, sp(span));
        let x = self.expr(e);
        self.one(F::Expression, x);
        let a = self.ts_type(t);
        self.one(F::TypeAnnotation, a);
        self.close(id)
    }
    fn type_assertion(&mut self, a: &TSTypeAssertion) -> u32 {
        let id = self.open(Ty::TSTypeAssertion, sp(a.span));
        let t = self.ts_type(&a.type_annotation);
        self.one(F::TypeAnnotation, t);
        let x = self.expr(&a.expression);
        self.one(F::Expression, x);
        self.close(id)
    }

    pub fn expr(&mut self, e: &Expression) -> u32 {
        match e {
            Expression::BooleanLiteral(l) => self.leaf(Ty::Literal, sp(l.span)),
            Expression::NullLiteral(l) => self.leaf(Ty::Literal, sp(l.span)),
            Expression::NumericLiteral(l) => self.leaf(Ty::Literal, sp(l.span)),
            Expression::BigIntLiteral(l) => self.leaf(Ty::Literal, sp(l.span)),
            Expression::RegExpLiteral(l) => self.leaf(Ty::Literal, sp(l.span)),
            Expression::StringLiteral(l) => self.leaf(Ty::Literal, sp(l.span)),
            Expression::TemplateLiteral(t) => self.template(t),
            Expression::Identifier(i) => self.ident(i.span),
            Expression::Super(s) => self.leaf(Ty::Super, sp(s.span)),
            Expression::ThisExpression(t) => self.leaf(Ty::ThisExpression, sp(t.span)),
            Expression::ArrayExpression(a) => {
                let id = self.open(Ty::ArrayExpression, sp(a.span));
                let els: Vec<u32> = a
                    .elements
                    .iter()
                    .map(|el| match el {
                        ArrayExpressionElement::SpreadElement(s) => self.spread(s),
                        ArrayExpressionElement::Elision(_) => NONE,
                        other => self.expr(other.to_expression()),
                    })
                    .collect();
                self.many(F::Elements, &els);
                self.close(id)
            }
            Expression::ArrowFunctionExpression(a) => {
                let id = self.open(Ty::ArrowFunctionExpression, sp(a.span));
                self.flag(id, flag::ASYNC, a.r#async);
                let tp = self.type_params(a.type_parameters.as_deref());
                self.one(F::TypeParameters, tp);
                let ps = self.params(&a.params, None);
                self.many(F::Params, &ps);
                let rt = self.type_ann(a.return_type.as_deref());
                self.one(F::ReturnType, rt);
                let b = match &a.body {
                    ArrowFunctionBody::FunctionBody(b) => self.fn_body(b),
                    other => self.expr(other.to_expression()),
                };
                self.one(F::Body, b);
                self.close(id)
            }
            Expression::AssignmentExpression(a) => {
                let id = self.open(Ty::AssignmentExpression, sp(a.span));
                let l = self.target(&a.left);
                self.one(F::Left, l);
                let r = self.expr(&a.right);
                self.one(F::Right, r);
                self.close(id)
            }
            Expression::AwaitExpression(a) => {
                let id = self.open(Ty::AwaitExpression, sp(a.span));
                let x = self.expr(&a.argument);
                self.one(F::Argument, x);
                self.close(id)
            }
            Expression::BinaryExpression(b) => {
                let id = self.open(Ty::BinaryExpression, sp(b.span));
                let l = self.expr(&b.left);
                self.one(F::Left, l);
                let r = self.expr(&b.right);
                self.one(F::Right, r);
                self.close(id)
            }
            Expression::PrivateInExpression(b) => {
                let id = self.open(Ty::BinaryExpression, sp(b.span));
                let l = self.leaf(Ty::PrivateIdentifier, sp(b.left.span));
                self.one(F::Left, l);
                let r = self.expr(&b.right);
                self.one(F::Right, r);
                self.close(id)
            }
            Expression::LogicalExpression(b) => {
                let id = self.open(Ty::LogicalExpression, sp(b.span));
                let l = self.expr(&b.left);
                self.one(F::Left, l);
                let r = self.expr(&b.right);
                self.one(F::Right, r);
                self.close(id)
            }
            Expression::CallExpression(c) => self.call(c),
            Expression::ChainExpression(c) => {
                let id = self.open(Ty::ChainExpression, sp(c.span));
                let x = match &c.expression {
                    ChainElement::CallExpression(c) => self.call(c),
                    ChainElement::TSNonNullExpression(n) => {
                        self.ts_unary(Ty::TSNonNullExpression, n.span, &n.expression)
                    }
                    other => self.member(other.to_member_expression()),
                };
                self.one(F::Expression, x);
                self.close(id)
            }
            Expression::ClassExpression(c) => self.class(c),
            Expression::ConditionalExpression(c) => {
                let id = self.open(Ty::ConditionalExpression, sp(c.span));
                let t = self.expr(&c.test);
                self.one(F::Test, t);
                let x = self.expr(&c.consequent);
                self.one(F::Consequent, x);
                let a = self.expr(&c.alternate);
                self.one(F::Alternate, a);
                self.close(id)
            }
            Expression::FunctionExpression(f) => self.function(f),
            Expression::ImportExpression(i) => {
                let id = self.open(Ty::ImportExpression, sp(i.span));
                let s = self.expr(&i.source);
                self.one(F::Source, s);
                let o = self.opt_expr(i.options.as_ref());
                self.one(F::Options, o);
                self.close(id)
            }
            Expression::NewExpression(n) => {
                let id = self.open(Ty::NewExpression, sp(n.span));
                let c = self.expr(&n.callee);
                self.one(F::Callee, c);
                let ta = self.type_args(n.type_arguments.as_deref());
                self.one(F::TypeArguments, ta);
                let a = self.args(&n.arguments);
                self.many(F::Arguments, &a);
                self.close(id)
            }
            Expression::ObjectExpression(o) => self.object(o),
            Expression::ParenthesizedExpression(p) => self.expr(&p.expression),
            Expression::SequenceExpression(s) => {
                let id = self.open(Ty::SequenceExpression, sp(s.span));
                let es: Vec<u32> = s.expressions.iter().map(|e| self.expr(e)).collect();
                self.many(F::Expressions, &es);
                self.close(id)
            }
            Expression::TaggedTemplateExpression(t) => {
                let id = self.open(Ty::TaggedTemplateExpression, sp(t.span));
                let tag = self.expr(&t.tag);
                self.one(F::Tag, tag);
                let ta = self.type_args(t.type_arguments.as_deref());
                self.one(F::TypeArguments, ta);
                let q = self.template(&t.quasi);
                self.one(F::Quasi, q);
                self.close(id)
            }
            Expression::UnaryExpression(u) => {
                let id = self.open(Ty::UnaryExpression, sp(u.span));
                let a = self.expr(&u.argument);
                self.one(F::Argument, a);
                self.close(id)
            }
            Expression::UpdateExpression(u) => {
                let id = self.open(Ty::UpdateExpression, sp(u.span));
                let a = self.simple_target(&u.argument);
                self.one(F::Argument, a);
                self.close(id)
            }
            Expression::YieldExpression(y) => {
                let id = self.open(Ty::YieldExpression, sp(y.span));
                self.flag(id, flag::DELEGATE, y.delegate);
                let a = self.opt_expr(y.argument.as_ref());
                self.one(F::Argument, a);
                self.close(id)
            }
            Expression::ImportMeta(m) => self.leaf(Ty::MetaProperty, sp(m.span)),
            Expression::NewTarget(m) => self.leaf(Ty::MetaProperty, sp(m.span)),
            Expression::TSAsExpression(a) => self.ts_cast(
                Ty::TSAsExpression,
                a.span,
                &a.expression,
                &a.type_annotation,
            ),
            Expression::TSSatisfiesExpression(a) => self.ts_cast(
                Ty::TSSatisfiesExpression,
                a.span,
                &a.expression,
                &a.type_annotation,
            ),
            Expression::TSTypeAssertion(a) => self.type_assertion(a),
            Expression::TSNonNullExpression(n) => {
                self.ts_unary(Ty::TSNonNullExpression, n.span, &n.expression)
            }
            Expression::TSInstantiationExpression(i) => {
                let id = self.open(Ty::TSInstantiationExpression, sp(i.span));
                let x = self.expr(&i.expression);
                self.one(F::Expression, x);
                let ta = self.type_args(Some(&i.type_arguments));
                self.one(F::TypeArguments, ta);
                self.close(id)
            }
            Expression::ComputedMemberExpression(_)
            | Expression::StaticMemberExpression(_)
            | Expression::PrivateFieldExpression(_) => self.member(e.to_member_expression()),
            Expression::JSXElement(_)
            | Expression::JSXFragment(_)
            | Expression::V8IntrinsicExpression(_) => self.leaf(Ty::Unknown, sp(e.span())),
        }
    }

    fn object(&mut self, o: &ObjectExpression) -> u32 {
        let id = self.open(Ty::ObjectExpression, sp(o.span));
        let ps: Vec<u32> = o
            .properties
            .iter()
            .map(|p| match p {
                ObjectPropertyKind::SpreadProperty(s) => self.spread(s),
                ObjectPropertyKind::ObjectProperty(p) => {
                    let pid = self.open(Ty::Property, sp(p.span));
                    self.flag(pid, flag::COMPUTED, p.computed);
                    let k = self.prop_key(&p.key);
                    self.one(F::Key, k);
                    let v = self.expr(&p.value);
                    self.one(F::Value, v);
                    self.close(pid)
                }
            })
            .collect();
        self.many(F::Properties, &ps);
        self.close(id)
    }

    fn simple_target(&mut self, t: &SimpleAssignmentTarget) -> u32 {
        match t {
            SimpleAssignmentTarget::AssignmentTargetIdentifier(i) => self.ident(i.span),
            SimpleAssignmentTarget::TSAsExpression(a) => self.ts_cast(
                Ty::TSAsExpression,
                a.span,
                &a.expression,
                &a.type_annotation,
            ),
            SimpleAssignmentTarget::TSSatisfiesExpression(a) => self.ts_cast(
                Ty::TSSatisfiesExpression,
                a.span,
                &a.expression,
                &a.type_annotation,
            ),
            SimpleAssignmentTarget::TSNonNullExpression(n) => {
                self.ts_unary(Ty::TSNonNullExpression, n.span, &n.expression)
            }
            SimpleAssignmentTarget::TSTypeAssertion(a) => self.type_assertion(a),
            other => self.member(other.to_member_expression()),
        }
    }

    fn target(&mut self, t: &AssignmentTarget) -> u32 {
        match t {
            AssignmentTarget::ArrayAssignmentTarget(a) => {
                let id = self.open(Ty::ArrayPattern, sp(a.span));
                let mut els: Vec<u32> = a
                    .elements
                    .iter()
                    .map(|e| e.as_ref().map_or(NONE, |e| self.maybe_default(e)))
                    .collect();
                if let Some(r) = &a.rest {
                    els.push(self.target_rest(r));
                }
                self.many(F::Elements, &els);
                self.close(id)
            }
            AssignmentTarget::ObjectAssignmentTarget(o) => {
                let id = self.open(Ty::ObjectPattern, sp(o.span));
                let mut ps: Vec<u32> = o
                    .properties
                    .iter()
                    .map(|p| match p {
                        AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(p) => {
                            let pid = self.open(Ty::Property, sp(p.span));
                            let k = self.ident(p.binding.span);
                            self.one(F::Key, k);
                            let v = match &p.init {
                                Some(init) => {
                                    let aid = self.open(
                                        Ty::AssignmentPattern,
                                        (p.binding.span.start, init.span().end),
                                    );
                                    let l = self.ident(p.binding.span);
                                    self.one(F::Left, l);
                                    let r = self.expr(init);
                                    self.one(F::Right, r);
                                    self.close(aid)
                                }
                                None => self.ident(p.binding.span),
                            };
                            self.one(F::Value, v);
                            self.close(pid)
                        }
                        AssignmentTargetProperty::AssignmentTargetPropertyProperty(p) => {
                            let pid = self.open(Ty::Property, sp(p.span));
                            self.flag(pid, flag::COMPUTED, p.computed);
                            let k = self.prop_key(&p.name);
                            self.one(F::Key, k);
                            let v = self.maybe_default(&p.binding);
                            self.one(F::Value, v);
                            self.close(pid)
                        }
                    })
                    .collect();
                if let Some(r) = &o.rest {
                    ps.push(self.target_rest(r));
                }
                self.many(F::Properties, &ps);
                self.close(id)
            }
            other => self.simple_target(other.to_simple_assignment_target()),
        }
    }

    fn target_rest(&mut self, r: &AssignmentTargetRest) -> u32 {
        let id = self.open(Ty::RestElement, sp(r.span));
        let a = self.target(&r.target);
        self.one(F::Argument, a);
        self.close(id)
    }

    fn maybe_default(&mut self, m: &AssignmentTargetMaybeDefault) -> u32 {
        match m {
            AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(d) => {
                let id = self.open(Ty::AssignmentPattern, sp(d.span));
                let l = self.target(&d.binding);
                self.one(F::Left, l);
                let r = self.expr(&d.init);
                self.one(F::Right, r);
                self.close(id)
            }
            other => self.target(other.to_assignment_target()),
        }
    }

    // ---- TypeScript ----

    fn type_params(&mut self, t: Option<&TSTypeParameterDeclaration>) -> u32 {
        let Some(t) = t else { return NONE };
        let id = self.open(Ty::TSTypeParameterDeclaration, sp(t.span));
        let ps: Vec<u32> = t.params.iter().map(|p| self.type_param(p)).collect();
        self.many(F::Params, &ps);
        self.close(id)
    }

    fn type_param(&mut self, p: &TSTypeParameter) -> u32 {
        let id = self.open(Ty::TSTypeParameter, sp(p.span));
        let n = self.ident(p.name.span);
        self.one(F::Name, n);
        let c = p.constraint.as_ref().map_or(NONE, |c| self.ts_type(c));
        self.one(F::Constraint, c);
        let d = p.default.as_ref().map_or(NONE, |d| self.ts_type(d));
        self.one(F::Default, d);
        self.close(id)
    }

    fn type_args(&mut self, t: Option<&TSTypeParameterInstantiation>) -> u32 {
        let Some(t) = t else { return NONE };
        let id = self.open(Ty::TSTypeParameterInstantiation, sp(t.span));
        let ps: Vec<u32> = t.params.iter().map(|p| self.ts_type(p)).collect();
        self.many(F::Params, &ps);
        self.close(id)
    }

    fn type_name(&mut self, n: &TSTypeName) -> u32 {
        match n {
            TSTypeName::IdentifierReference(i) => self.ident(i.span),
            TSTypeName::QualifiedName(q) => self.qualified(q),
            TSTypeName::ThisExpression(t) => self.leaf(Ty::ThisExpression, sp(t.span)),
        }
    }

    fn qualified(&mut self, q: &TSQualifiedName) -> u32 {
        let id = self.open(Ty::TSQualifiedName, sp(q.span));
        let l = self.type_name(&q.left);
        self.one(F::Left, l);
        let r = self.ident(q.right.span);
        self.one(F::Right, r);
        self.close(id)
    }

    fn import_qualifier(&mut self, q: &TSImportTypeQualifier) -> u32 {
        match q {
            TSImportTypeQualifier::Identifier(i) => self.ident(i.span),
            TSImportTypeQualifier::QualifiedName(q) => {
                let id = self.open(Ty::TSQualifiedName, sp(q.span));
                let l = self.import_qualifier(&q.left);
                self.one(F::Left, l);
                let r = self.ident(q.right.span);
                self.one(F::Right, r);
                self.close(id)
            }
        }
    }

    fn import_type(&mut self, t: &TSImportType) -> u32 {
        let id = self.open(Ty::TSImportType, sp(t.span));
        let lid = self.open(Ty::TSLiteralType, sp(t.source.span));
        let l = self.leaf(Ty::Literal, sp(t.source.span));
        self.one(F::Literal, l);
        let a = self.close(lid);
        self.one(F::Argument, a);
        let o = t.options.as_ref().map_or(NONE, |o| self.object(o));
        self.one(F::Options, o);
        let q = t
            .qualifier
            .as_ref()
            .map_or(NONE, |q| self.import_qualifier(q));
        self.one(F::Qualifier, q);
        let ta = self.type_args(t.type_arguments.as_deref());
        self.one(F::TypeArguments, ta);
        self.close(id)
    }

    fn fn_type(
        &mut self,
        ty: Ty,
        span: Span,
        tp: Option<&TSTypeParameterDeclaration>,
        params: &FormalParameters,
        this: Option<&TSThisParameter>,
        ret: Option<&TSTypeAnnotation>,
    ) -> u32 {
        let id = self.open(ty, sp(span));
        let t = self.type_params(tp);
        self.one(F::TypeParameters, t);
        let ps = self.params(params, this);
        self.many(F::Params, &ps);
        let r = self.type_ann(ret);
        self.one(F::ReturnType, r);
        self.close(id)
    }

    fn signature(&mut self, s: &TSSignature) -> u32 {
        match s {
            TSSignature::TSIndexSignature(s) => self.index_signature(s),
            TSSignature::TSPropertySignature(p) => {
                let id = self.open(Ty::TSPropertySignature, sp(p.span));
                self.flag(id, flag::COMPUTED, p.computed);
                self.flag(id, flag::OPTIONAL, p.optional);
                let k = self.prop_key(&p.key);
                self.one(F::Key, k);
                let t = self.type_ann(p.type_annotation.as_deref());
                self.one(F::TypeAnnotation, t);
                self.close(id)
            }
            TSSignature::TSCallSignatureDeclaration(c) => self.fn_type(
                Ty::TSCallSignatureDeclaration,
                c.span,
                c.type_parameters.as_deref(),
                &c.params,
                c.this_param.as_deref(),
                c.return_type.as_deref(),
            ),
            TSSignature::TSConstructSignatureDeclaration(c) => self.fn_type(
                Ty::TSConstructSignatureDeclaration,
                c.span,
                c.type_parameters.as_deref(),
                &c.params,
                None,
                c.return_type.as_deref(),
            ),
            TSSignature::TSMethodSignature(m) => {
                let id = self.open(Ty::TSMethodSignature, sp(m.span));
                self.flag(id, flag::COMPUTED, m.computed);
                self.flag(id, flag::OPTIONAL, m.optional);
                let k = self.prop_key(&m.key);
                self.one(F::Key, k);
                let t = self.type_params(m.type_parameters.as_deref());
                self.one(F::TypeParameters, t);
                let ps = self.params(&m.params, m.this_param.as_deref());
                self.many(F::Params, &ps);
                let r = self.type_ann(m.return_type.as_deref());
                self.one(F::ReturnType, r);
                self.close(id)
            }
        }
    }

    fn tuple_element(&mut self, e: &TSTupleElement) -> u32 {
        match e {
            TSTupleElement::TSOptionalType(o) => {
                let id = self.open(Ty::TSOptionalType, sp(o.span));
                let t = self.ts_type(&o.type_annotation);
                self.one(F::TypeAnnotation, t);
                self.close(id)
            }
            TSTupleElement::TSRestType(r) => {
                let id = self.open(Ty::TSRestType, sp(r.span));
                let t = self.ts_type(&r.type_annotation);
                self.one(F::TypeAnnotation, t);
                self.close(id)
            }
            other => self.ts_type(other.to_ts_type()),
        }
    }

    /// typescript-estree gives a type predicate's `typeAnnotation` the range
    /// `[type.getFullStart() - 1, type.end]`.
    fn predicate_annotation(&mut self, a: &TSTypeAnnotation) -> u32 {
        let t = a.type_annotation.span();
        let before = self.src.get(..t.start as usize).unwrap_or("");
        let full_start = before.trim_end().len() as u32;
        let id = self.open(
            Ty::TSTypeAnnotation,
            (full_start.saturating_sub(1), a.span.end),
        );
        let x = self.ts_type(&a.type_annotation);
        self.one(F::TypeAnnotation, x);
        self.close(id)
    }

    fn ts_type(&mut self, t: &TSType) -> u32 {
        let s = sp(t.span());
        match t {
            TSType::TSAnyKeyword(_) => self.leaf(Ty::TSAnyKeyword, s),
            TSType::TSBigIntKeyword(_) => self.leaf(Ty::TSBigIntKeyword, s),
            TSType::TSBooleanKeyword(_) => self.leaf(Ty::TSBooleanKeyword, s),
            TSType::TSIntrinsicKeyword(_) => self.leaf(Ty::TSIntrinsicKeyword, s),
            TSType::TSNeverKeyword(_) => self.leaf(Ty::TSNeverKeyword, s),
            TSType::TSNullKeyword(_) => self.leaf(Ty::TSNullKeyword, s),
            TSType::TSNumberKeyword(_) => self.leaf(Ty::TSNumberKeyword, s),
            TSType::TSObjectKeyword(_) => self.leaf(Ty::TSObjectKeyword, s),
            TSType::TSStringKeyword(_) => self.leaf(Ty::TSStringKeyword, s),
            TSType::TSSymbolKeyword(_) => self.leaf(Ty::TSSymbolKeyword, s),
            TSType::TSUndefinedKeyword(_) => self.leaf(Ty::TSUndefinedKeyword, s),
            TSType::TSUnknownKeyword(_) => self.leaf(Ty::TSUnknownKeyword, s),
            TSType::TSVoidKeyword(_) => self.leaf(Ty::TSVoidKeyword, s),
            TSType::TSThisType(_) => self.leaf(Ty::TSThisType, s),
            TSType::TSArrayType(a) => {
                let id = self.open(Ty::TSArrayType, s);
                let e = self.ts_type(&a.element_type);
                self.one(F::ElementType, e);
                self.close(id)
            }
            TSType::TSConditionalType(c) => {
                let id = self.open(Ty::TSConditionalType, s);
                let a = self.ts_type(&c.check_type);
                self.one(F::CheckType, a);
                let b = self.ts_type(&c.extends_type);
                self.one(F::ExtendsType, b);
                let x = self.ts_type(&c.true_type);
                self.one(F::TrueType, x);
                let y = self.ts_type(&c.false_type);
                self.one(F::FalseType, y);
                self.close(id)
            }
            TSType::TSConstructorType(c) => self.fn_type(
                Ty::TSConstructorType,
                c.span,
                c.type_parameters.as_deref(),
                &c.params,
                None,
                Some(&c.return_type),
            ),
            TSType::TSFunctionType(f) => self.fn_type(
                Ty::TSFunctionType,
                f.span,
                f.type_parameters.as_deref(),
                &f.params,
                f.this_param.as_deref(),
                Some(&f.return_type),
            ),
            TSType::TSImportType(i) => self.import_type(i),
            TSType::TSIndexedAccessType(i) => {
                let id = self.open(Ty::TSIndexedAccessType, s);
                let o = self.ts_type(&i.object_type);
                self.one(F::ObjectType, o);
                let x = self.ts_type(&i.index_type);
                self.one(F::IndexType, x);
                self.close(id)
            }
            TSType::TSInferType(i) => {
                let id = self.open(Ty::TSInferType, s);
                let p = self.type_param(&i.type_parameter);
                self.one(F::TypeParameter, p);
                self.close(id)
            }
            TSType::TSIntersectionType(u) => {
                let id = self.open(Ty::TSIntersectionType, s);
                let ts: Vec<u32> = u.types.iter().map(|t| self.ts_type(t)).collect();
                self.many(F::Types, &ts);
                self.close(id)
            }
            TSType::TSUnionType(u) => {
                let id = self.open(Ty::TSUnionType, s);
                let ts: Vec<u32> = u.types.iter().map(|t| self.ts_type(t)).collect();
                self.many(F::Types, &ts);
                self.close(id)
            }
            TSType::TSLiteralType(l) => {
                let id = self.open(Ty::TSLiteralType, s);
                let x = match &l.literal {
                    TSLiteral::TemplateLiteral(t) => self.template(t),
                    TSLiteral::UnaryExpression(u) => {
                        let uid = self.open(Ty::UnaryExpression, sp(u.span));
                        let a = self.expr(&u.argument);
                        self.one(F::Argument, a);
                        self.close(uid)
                    }
                    TSLiteral::BooleanLiteral(b) => self.leaf(Ty::Literal, sp(b.span)),
                    TSLiteral::NumericLiteral(b) => self.leaf(Ty::Literal, sp(b.span)),
                    TSLiteral::BigIntLiteral(b) => self.leaf(Ty::Literal, sp(b.span)),
                    TSLiteral::StringLiteral(b) => self.leaf(Ty::Literal, sp(b.span)),
                };
                self.one(F::Literal, x);
                self.close(id)
            }
            TSType::TSMappedType(m) => {
                let id = self.open(Ty::TSMappedType, s);
                let k = self.ident(m.key.span);
                self.one(F::Key, k);
                let c = self.ts_type(&m.constraint);
                self.one(F::Constraint, c);
                let n = m.name_type.as_ref().map_or(NONE, |n| self.ts_type(n));
                self.one(F::NameType, n);
                let a = m.type_annotation.as_ref().map_or(NONE, |a| self.ts_type(a));
                self.one(F::TypeAnnotation, a);
                self.close(id)
            }
            TSType::TSNamedTupleMember(n) => {
                let id = self.open(Ty::TSNamedTupleMember, s);
                let l = self.ident(n.label.span);
                self.one(F::Label, l);
                let e = self.tuple_element(&n.element_type);
                self.one(F::ElementType, e);
                self.close(id)
            }
            TSType::TSTemplateLiteralType(t) => {
                let id = self.open(Ty::TSTemplateLiteralType, s);
                let qs: Vec<u32> = t
                    .quasis
                    .iter()
                    .map(|q| {
                        let r = (
                            q.span.start.saturating_sub(1),
                            q.span.end + if q.tail { 1 } else { 2 },
                        );
                        self.leaf(Ty::TemplateElement, r)
                    })
                    .collect();
                self.many(F::Quasis, &qs);
                let ts: Vec<u32> = t.types.iter().map(|t| self.ts_type(t)).collect();
                self.many(F::Types, &ts);
                self.close(id)
            }
            TSType::TSTupleType(t) => {
                let id = self.open(Ty::TSTupleType, s);
                let es: Vec<u32> = t
                    .element_types
                    .iter()
                    .map(|e| self.tuple_element(e))
                    .collect();
                self.many(F::ElementTypes, &es);
                self.close(id)
            }
            TSType::TSTypeLiteral(t) => {
                let id = self.open(Ty::TSTypeLiteral, s);
                let ms: Vec<u32> = t.members.iter().map(|m| self.signature(m)).collect();
                self.many(F::Members, &ms);
                self.close(id)
            }
            TSType::TSTypeOperatorType(o) => {
                let id = self.open(Ty::TSTypeOperator, s);
                let x = self.ts_type(&o.type_annotation);
                self.one(F::TypeAnnotation, x);
                self.close(id)
            }
            TSType::TSTypePredicate(p) => {
                let id = self.open(Ty::TSTypePredicate, s);
                let a = p
                    .type_annotation
                    .as_ref()
                    .map_or(NONE, |a| self.predicate_annotation(a));
                self.one(F::TypeAnnotation, a);
                let n = match &p.parameter_name {
                    TSTypePredicateName::Identifier(i) => self.ident(i.span),
                    TSTypePredicateName::This(t) => self.leaf(Ty::TSThisType, sp(t.span)),
                };
                self.one(F::ParameterName, n);
                self.close(id)
            }
            TSType::TSTypeQuery(q) => {
                let id = self.open(Ty::TSTypeQuery, s);
                let e = match &q.expr_name {
                    TSTypeQueryExprName::TSImportType(i) => self.import_type(i),
                    other => self.type_name(other.to_ts_type_name()),
                };
                self.one(F::ExprName, e);
                let ta = self.type_args(q.type_arguments.as_deref());
                self.one(F::TypeArguments, ta);
                self.close(id)
            }
            TSType::TSTypeReference(r) => {
                let id = self.open(Ty::TSTypeReference, s);
                let n = self.type_name(&r.type_name);
                self.one(F::TypeName, n);
                let ta = self.type_args(r.type_arguments.as_deref());
                self.one(F::TypeArguments, ta);
                self.close(id)
            }
            TSType::TSParenthesizedType(p) => self.ts_type(&p.type_annotation),
            TSType::JSDocNullableType(_)
            | TSType::JSDocNonNullableType(_)
            | TSType::JSDocUnknownType(_) => self.leaf(Ty::Unknown, s),
        }
    }

    // ------------------------------------------------------------------
    // Template
    // ------------------------------------------------------------------

    pub fn element(&mut self, doc: &Document, eid: ElementId) -> u32 {
        let el = doc.element(eid);
        let id = self.open(Ty::VElement, sp(el.span));
        self.flag(
            id,
            flag::PRE,
            matches!(el.name.as_str(), "pre" | "textarea"),
        );
        // startTag
        let st = self.open(Ty::VStartTag, sp(el.start_tag.span));
        let attrs: Vec<u32> = el
            .start_tag
            .attributes
            .iter()
            .map(|a| {
                let aid = self.open(Ty::VAttribute, sp(a.span));
                let k = match &a.key {
                    AttributeKey::Plain(k) => self.leaf(Ty::VIdentifier, sp(k.span)),
                    AttributeKey::Directive(k) => {
                        let kid = self.open(Ty::VDirectiveKey, sp(k.span));
                        let n = self.leaf(Ty::VIdentifier, sp(k.name.span));
                        self.one(F::Name, n);
                        let arg = match &k.argument {
                            None => NONE,
                            Some(DirectiveArgument::Static(i)) => {
                                self.leaf(Ty::VIdentifier, sp(i.span))
                            }
                            Some(DirectiveArgument::Dynamic(x)) => self.container(doc, *x),
                        };
                        self.one(F::Argument, arg);
                        let ms: Vec<u32> = k
                            .modifiers
                            .iter()
                            .filter(|m| !m.raw_name.is_empty())
                            .map(|m| self.leaf(Ty::VIdentifier, sp(m.span)))
                            .collect();
                        self.many(F::Modifiers, &ms);
                        self.close(kid)
                    }
                };
                self.one(F::Key, k);
                let v = match &a.value {
                    None => NONE,
                    Some(AttributeValue::Literal(l)) => self.leaf(Ty::VLiteral, sp(l.span)),
                    Some(AttributeValue::Expr(x)) => self.container(doc, *x),
                };
                self.one(F::Value, v);
                self.close(aid)
            })
            .collect();
        self.many(F::Attributes, &attrs);
        let st = self.close(st);
        self.one(F::StartTag, st);
        let children: Vec<u32> = el
            .children
            .iter()
            .map(|c| match c {
                Child::Element(e) => self.element(doc, *e),
                Child::Text(t) => {
                    let tx = doc.text(*t);
                    let tid = self.leaf(Ty::VText, sp(tx.span));
                    let empty = crate::utils::tpl_style::js_trim(&tx.value).is_empty();
                    self.flag(tid, flag::EMPTY_TEXT, empty);
                    tid
                }
                Child::Expr(x) => self.container(doc, *x),
            })
            .collect();
        self.many(F::Children, &children);
        let et = el.end_tag.map_or(NONE, |s| self.leaf(Ty::VEndTag, sp(s)));
        self.one(F::EndTag, et);
        self.close(id)
    }

    fn container(&mut self, doc: &Document, x: ExprId) -> u32 {
        let c = doc.expr(x);
        let id = self.open(Ty::VExpressionContainer, sp(c.span));
        let e = match &c.expression {
            None => NONE,
            Some(VExpression::Expression(e)) => self.expr(e),
            Some(VExpression::FilterSequence {
                span,
                expression,
                filters,
            }) => {
                let fid = self.open(Ty::VFilterSequenceExpression, sp(*span));
                let e = self.expr(expression);
                self.one(F::Expression, e);
                let fs: Vec<u32> = filters
                    .iter()
                    .map(|f| {
                        let id = self.open(Ty::VFilter, sp(f.span));
                        let c = self.ident(f.callee.1);
                        self.one(F::Callee, c);
                        let a = self.args(&f.arguments);
                        self.many(F::Arguments, &a);
                        self.close(id)
                    })
                    .collect();
                self.many(F::Filters, &fs);
                self.close(fid)
            }
            Some(VExpression::For {
                span,
                left,
                rest,
                right,
            }) => {
                let fid = self.open(Ty::VForExpression, sp(*span));
                let mut ls: Vec<u32> = left
                    .iter()
                    .map(|p| {
                        p.as_ref()
                            .map_or(NONE, |p| self.binding(p, None, None, &[]))
                    })
                    .collect();
                if let Some(r) = rest {
                    ls.push(self.rest(r, None, &[]));
                }
                self.many(F::Left, &ls);
                let r = self.expr(right);
                self.one(F::Right, r);
                self.close(fid)
            }
            Some(VExpression::On { span, body }) => {
                let oid = self.open(Ty::VOnExpression, sp(*span));
                let b: Vec<u32> = body.iter().map(|s| self.stmt(s)).collect();
                self.many(F::Body, &b);
                self.close(oid)
            }
            Some(VExpression::SlotScope { span, params }) => {
                let sid = self.open(Ty::VSlotScopeExpression, sp(*span));
                let ps = self.params(params, None);
                self.many(F::Params, &ps);
                self.close(sid)
            }
            Some(VExpression::Generic { span, .. }) => self.leaf(Ty::Unknown, sp(*span)),
        };
        self.one(F::Expression, e);
        let _ = self.end_of(id);
        self.close(id)
    }
}

impl Builder<'_> {
    /// A `<style>` `v-bind()` container (vue-eslint-parser adds these to the `<style>` element).
    pub fn style_container(&mut self, span: Span, e: &Expression) -> u32 {
        let id = self.open(Ty::VExpressionContainer, sp(span));
        let x = self.expr(e);
        self.one(F::Expression, x);
        self.close(id)
    }

    /// A childless node (e.g. a top-level `VText` of the document fragment).
    pub fn leaf_node(&mut self, ty: Ty, span: Span) -> u32 {
        self.leaf(ty, sp(span))
    }
}
