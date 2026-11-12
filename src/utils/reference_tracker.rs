//! Port of the ESM part of `@eslint-community/eslint-utils` `ReferenceTracker`
//! (`iterateEsmReferences` with `[ESM]: true` and `[CALL]` leaves).
//!
//! ponytail: only CALL leaves are reported, global/CJS tracking and destructuring *assignments*
//! (`({ a } = ns)`) are not followed; add them when a rule needs READ/CONSTRUCT or globals.

use oxc::ast::AstKind;
use oxc::ast::ast::*;
use oxc::semantic::{NodeId, Semantic, SymbolId};

use super::js;
use crate::rule::LintFile;

/// A trace map node: named children plus a `[CALL]` flag.
#[derive(Default, Debug, Clone)]
pub struct TraceMap {
    pub children: Vec<(String, TraceMap)>,
    pub call: bool,
}

impl TraceMap {
    /// `{ a: { [CALL]: true }, b: { [CALL]: true } }`.
    pub fn calls(names: &[&str]) -> Self {
        Self {
            children: names
                .iter()
                .map(|n| {
                    (
                        n.to_string(),
                        Self {
                            call: true,
                            ..Self::default()
                        },
                    )
                })
                .collect(),
            call: false,
        }
    }
    /// `{ p0: { p1: { … { [CALL]: true } } } }`.
    pub fn call_path(path: &[String]) -> Self {
        let mut m = Self {
            call: true,
            ..Self::default()
        };
        for p in path.iter().rev() {
            m = Self {
                children: vec![(p.clone(), m)],
                call: false,
            };
        }
        m
    }
    fn get(&self, key: &str) -> Option<&TraceMap> {
        self.children.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
}

/// A traced call: the call expression and the path (`[module, export, …]`).
pub struct TraceHit<'a> {
    pub call: &'a CallExpression<'a>,
    pub path: Vec<String>,
}

/// `tracker.iterateEsmReferences({ [module]: { [ESM]: true, ...map } })` for each `(module, map)`.
pub fn esm_calls<'a>(file: &LintFile<'a>, modules: &[(&str, &TraceMap)]) -> Vec<TraceHit<'a>> {
    let (Some(program), Some(sem)) = (file.program, file.semantic) else {
        return Vec::new();
    };
    let mut t = Tracker {
        sem,
        out: Vec::new(),
        stack: Vec::new(),
    };
    for stmt in &program.body {
        let Statement::ImportDeclaration(imp) = stmt else {
            continue;
        };
        let Some(src) = imp.source.value.as_str() else {
            continue;
        };
        for (module, map) in modules.iter().filter(|(m, _)| *m == src) {
            let path = vec![module.to_string()];
            for spec in imp.specifiers.iter().flatten() {
                match spec {
                    ImportDeclarationSpecifier::ImportSpecifier(s) => {
                        let key = s.imported.name();
                        if let Some(next) = map.get(key.as_str()) {
                            t.variable(s.local.symbol_id(), path_with(&path, key.as_str()), next);
                        }
                    }
                    ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                        if let Some(next) = map.get("default") {
                            t.variable(s.local.symbol_id(), path_with(&path, "default"), next);
                        }
                    }
                    ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                        t.variable(s.local.symbol_id(), path.clone(), map)
                    }
                }
            }
        }
    }
    t.out
}

fn path_with(path: &[String], key: &str) -> Vec<String> {
    let mut p = path.to_vec();
    p.push(key.to_string());
    p
}

struct Tracker<'s, 'a> {
    sem: &'s Semantic<'a>,
    out: Vec<TraceHit<'a>>,
    stack: Vec<SymbolId>,
}

impl<'a> Tracker<'_, 'a> {
    fn variable(&mut self, sym: SymbolId, path: Vec<String>, map: &TraceMap) {
        if self.stack.contains(&sym) {
            return;
        }
        self.stack.push(sym);
        let refs: Vec<NodeId> = self
            .sem
            .scoping()
            .get_resolved_references(sym)
            .filter(|r| r.is_read())
            .map(|r| r.node_id())
            .collect();
        for id in refs {
            self.property(id, &path, map);
        }
        self.stack.pop();
    }

    fn property(&mut self, mut cur: NodeId, path: &[String], map: &TraceMap) {
        let nodes = self.sem.nodes();
        loop {
            let pid = nodes.parent_id(cur);
            if pid == cur || !is_pass_through(nodes.kind(pid), cur) {
                break;
            }
            cur = pid;
        }
        let pid = nodes.parent_id(cur);
        if pid == cur {
            return;
        }
        match nodes.kind(pid) {
            AstKind::StaticMemberExpression(m) if m.object.node_id() == cur => {
                if let Some(next) = map.get(m.property.name.as_str()) {
                    self.property(pid, &path_with(path, m.property.name.as_str()), next);
                }
            }
            AstKind::ComputedMemberExpression(m) if m.object.node_id() == cur => {
                if let Some(key) = js::string_literal_value(&m.expression)
                    && let Some(next) = map.get(&key)
                {
                    self.property(pid, &path_with(path, &key), next);
                }
            }
            AstKind::CallExpression(c) if c.callee.node_id() == cur && map.call => {
                self.out.push(TraceHit {
                    call: c,
                    path: path.to_vec(),
                });
            }
            AstKind::AssignmentExpression(a) if a.right.node_id() == cur => {
                if let AssignmentTarget::AssignmentTargetIdentifier(id) = &a.left
                    && let Some(sym) = js::ref_symbol(self.sem, id)
                {
                    self.variable(sym, path.to_vec(), map);
                }
                self.property(pid, path, map);
            }
            AstKind::AssignmentPattern(a) if a.right.node_id() == cur => {
                self.lhs(&a.left, path, map)
            }
            AstKind::VariableDeclarator(d)
                if d.init.as_ref().is_some_and(|i| i.node_id() == cur) =>
            {
                self.lhs(&d.id, path, map)
            }
            _ => {}
        }
    }

    fn lhs(&mut self, pat: &BindingPattern<'a>, path: &[String], map: &TraceMap) {
        match pat {
            BindingPattern::BindingIdentifier(b) => {
                self.variable(b.symbol_id(), path.to_vec(), map)
            }
            BindingPattern::ObjectPattern(o) => {
                for p in &o.properties {
                    if let Some(key) = js::binding_prop_name(p)
                        && let Some(next) = map.get(&key)
                    {
                        self.lhs(&p.value, &path_with(path, &key), next);
                    }
                }
            }
            BindingPattern::AssignmentPattern(a) => self.lhs(&a.left, path, map),
            BindingPattern::ArrayPattern(_) => {}
        }
    }
}

/// `isPassThrough(node)` given the parent kind (+ oxc parentheses).
fn is_pass_through(parent: AstKind, cur: NodeId) -> bool {
    match parent {
        AstKind::ConditionalExpression(c) => {
            c.consequent.node_id() == cur || c.alternate.node_id() == cur
        }
        AstKind::SequenceExpression(s) => s.expressions.last().is_some_and(|e| e.node_id() == cur),
        AstKind::LogicalExpression(_)
        | AstKind::ChainExpression(_)
        | AstKind::ParenthesizedExpression(_)
        | AstKind::TSAsExpression(_)
        | AstKind::TSSatisfiesExpression(_)
        | AstKind::TSTypeAssertion(_)
        | AstKind::TSNonNullExpression(_)
        | AstKind::TSInstantiationExpression(_) => true,
        _ => false,
    }
}
