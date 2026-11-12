//! Runs the template parser on every vue-eslint-parser AST fixture (`tests/parser-fixtures/`,
//! MIT, see LICENSE there) and compares: `tree.json` (V* nodes + text of embedded ESTree nodes),
//! `token-ranges.json` (template token texts), and `expected.json` (extracted from upstream
//! `ast.json` by `scripts/extract-parser-fixtures.js`: tokens, comments, errors, elements,
//! references, variables — UTF-8 byte offsets).
//!
//! `FIXTURE=<name> cargo test --test parser_fixtures -- --nocapture` prints details for one fixture.

use std::fmt::Write as _;
use std::path::Path;

use oxc::allocator::Allocator;
use oxc::ast::ast::{Argument, Expression, Statement};
use oxc::span::{GetSpan, Span};
use oxlint_vue_rs::ast::*;
use oxlint_vue_rs::parser::{ParserOptions, parse_document};
use serde_json::Value;

/// Fixtures whose upstream expectations we knowingly do not reproduce.
const KNOWN_FAILURES: &[(&str, &str)] = &[];

#[derive(Debug, PartialEq)]
struct Node {
    ty: String,
    text: String,
    children: Vec<Node>,
}

fn from_json(v: &Value) -> Node {
    let ty = v["type"].as_str().unwrap_or("").to_string();
    let text = v["text"].as_str().unwrap_or("").to_string();
    if ty.starts_with('V') {
        let children = v["children"]
            .as_array()
            .map(|a| a.iter().map(from_json).collect())
            .unwrap_or_default();
        Node { ty, text, children }
    } else {
        Node {
            ty: "*".into(),
            text,
            children: vec![],
        }
    }
}

struct Tree<'d, 'a> {
    doc: &'d Document<'a>,
}

fn unparen<'x, 'a>(mut e: &'x Expression<'a>) -> &'x Expression<'a> {
    while let Expression::ParenthesizedExpression(p) = e {
        e = &p.expression;
    }
    e
}

impl Tree<'_, '_> {
    fn node(&self, ty: &str, span: Span, children: Vec<Node>) -> Node {
        Node {
            ty: ty.into(),
            text: self.doc.slice(span).to_string(),
            children,
        }
    }
    fn leaf(&self, span: Span) -> Node {
        Node {
            ty: "*".into(),
            text: self.doc.slice(span).to_string(),
            children: vec![],
        }
    }
    fn expr_leaf(&self, e: &Expression) -> Node {
        self.leaf(unparen(e).span())
    }
    fn arg_leaf(&self, a: &Argument) -> Node {
        match a {
            Argument::SpreadElement(s) => self.leaf(s.span),
            _ => self.expr_leaf(a.to_expression()),
        }
    }
    fn element(&self, id: ElementId) -> Node {
        let el = self.doc.element(id);
        let mut children = vec![self.start_tag(el)];
        for c in &el.children {
            children.push(self.child(*c));
        }
        if let Some(end) = el.end_tag {
            children.push(self.node("VEndTag", end, vec![]));
        }
        self.node("VElement", el.span, children)
    }
    fn child(&self, c: Child) -> Node {
        match c {
            Child::Element(e) => self.element(e),
            Child::Text(t) => self.node("VText", self.doc.text(t).span, vec![]),
            Child::Expr(x) => self.container(x),
        }
    }
    fn ident(&self, i: &VIdentifier) -> Node {
        self.node("VIdentifier", i.span, vec![])
    }
    fn start_tag(&self, el: &Element) -> Node {
        let mut attrs = vec![];
        for a in &el.start_tag.attributes {
            let mut ch = vec![];
            match &a.key {
                AttributeKey::Plain(k) => ch.push(self.ident(k)),
                AttributeKey::Directive(k) => {
                    let mut kc = vec![self.ident(&k.name)];
                    match &k.argument {
                        Some(DirectiveArgument::Static(i)) => kc.push(self.ident(i)),
                        Some(DirectiveArgument::Dynamic(x)) => kc.push(self.container(*x)),
                        None => {}
                    }
                    kc.extend(k.modifiers.iter().map(|m| self.ident(m)));
                    ch.push(self.node("VDirectiveKey", k.span, kc));
                }
            }
            match &a.value {
                Some(AttributeValue::Literal(l)) => ch.push(self.node("VLiteral", l.span, vec![])),
                Some(AttributeValue::Expr(x)) => ch.push(self.container(*x)),
                None => {}
            }
            attrs.push(self.node("VAttribute", a.span, ch));
        }
        self.node("VStartTag", el.start_tag.span, attrs)
    }
    fn container(&self, x: ExprId) -> Node {
        let c = self.doc.expr(x);
        let mut ch = vec![];
        match &c.expression {
            None => {}
            Some(VExpression::Expression(e)) => ch.push(self.expr_leaf(e)),
            Some(VExpression::FilterSequence {
                span,
                expression,
                filters,
            }) => {
                let mut fc = vec![self.expr_leaf(expression)];
                for f in filters {
                    let mut c2 = vec![self.leaf(f.callee.1)];
                    c2.extend(f.arguments.iter().map(|a| self.arg_leaf(a)));
                    fc.push(self.node("VFilter", f.span, c2));
                }
                ch.push(self.node("VFilterSequenceExpression", *span, fc));
            }
            Some(VExpression::For {
                span,
                left,
                rest,
                right,
            }) => {
                let mut fc: Vec<Node> =
                    left.iter().flatten().map(|p| self.leaf(p.span())).collect();
                if let Some(r) = rest {
                    fc.push(self.leaf(r.span));
                }
                fc.push(self.expr_leaf(right));
                ch.push(self.node("VForExpression", *span, fc));
            }
            Some(VExpression::On { span, body }) => {
                let fc = body
                    .iter()
                    .map(|s: &Statement| self.leaf(s.span()))
                    .collect();
                ch.push(self.node("VOnExpression", *span, fc));
            }
            Some(VExpression::SlotScope { span, params }) => {
                let mut fc: Vec<Node> = params.items.iter().map(|p| self.leaf(p.span)).collect();
                if let Some(r) = &params.rest {
                    fc.push(self.leaf(r.span));
                }
                ch.push(self.node("VSlotScopeExpression", *span, fc));
            }
            Some(VExpression::Generic { span, params, .. }) => {
                let fc = params.params.iter().map(|p| self.leaf(p.span)).collect();
                ch.push(self.node("VGenericExpression", *span, fc));
            }
        }
        self.node("VExpressionContainer", c.span, ch)
    }
}

fn diff_tree(a: &[Node], e: &[Node], path: &str, out: &mut String) -> bool {
    for (i, (x, y)) in a.iter().zip(e).enumerate() {
        let p = format!("{path}/{i}:{}", y.ty);
        if x.ty != y.ty || x.text != y.text {
            let _ = writeln!(
                out,
                "tree {p}: got {}[{:?}] expected {}[{:?}]",
                x.ty, x.text, y.ty, y.text
            );
            return false;
        }
        if !diff_tree(&x.children, &y.children, &p, out) {
            return false;
        }
    }
    if a.len() != e.len() {
        let _ = writeln!(
            out,
            "tree {path}: got {} children expected {} ({:?} vs {:?})",
            a.len(),
            e.len(),
            a.iter()
                .map(|n| format!("{}[{}]", n.ty, n.text))
                .collect::<Vec<_>>(),
            e.iter()
                .map(|n| format!("{}[{}]", n.ty, n.text))
                .collect::<Vec<_>>()
        );
        return false;
    }
    true
}

fn kind_name(k: TokenKind) -> String {
    match k {
        TokenKind::LineComment => "Line".into(),
        TokenKind::BlockComment => "Block".into(),
        k => format!("{k:?}"),
    }
}

fn mode_name(m: ReferenceMode) -> &'static str {
    match m {
        ReferenceMode::Read => "r",
        ReferenceMode::Write => "w",
        ReferenceMode::ReadWrite => "rw",
    }
}

/// Messages vue-eslint-parser produces itself (others come from espree; we only compare offsets).
fn own_message(m: &str) -> bool {
    m.starts_with("Expected to be")
        || m.starts_with("Unexpected token '")
        || m == "Unexpected end of expression."
        || m.starts_with("Dynamic argument")
        || m.bytes().all(|b| b.is_ascii_lowercase() || b == b'-')
}

fn check(name: &str, dir: &Path, verbose: bool) -> Result<(), String> {
    let source = std::fs::read_to_string(dir.join("source.vue")).unwrap();
    let expected: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("expected.json")).unwrap()).unwrap();
    let ts = expected["ts"].as_bool().unwrap_or(false);
    let alloc = Allocator::default();
    let doc = parse_document(&alloc, &source, &ParserOptions { ts, jsx: false });
    let mut out = String::new();

    // tree.json
    let tree: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("tree.json")).unwrap()).unwrap();
    let exp_tree: Vec<Node> = tree.as_array().unwrap().iter().map(from_json).collect();
    let t = Tree { doc: &doc };
    let has_tb = expected["hasTemplateBody"].as_bool().unwrap_or(false);
    if has_tb != doc.template_body.is_some() {
        let _ = writeln!(
            out,
            "templateBody presence: got {} expected {has_tb}",
            doc.template_body.is_some()
        );
    }
    let got_tree: Vec<Node> = doc
        .template_body
        .map(|id| vec![t.element(id)])
        .unwrap_or_default();
    diff_tree(&got_tree, &exp_tree, "", &mut out);

    if has_tb {
        // tokens / comments
        for (label, got, exp) in [
            ("token", &doc.tokens, &expected["tokens"]),
            ("comment", &doc.comments, &expected["comments"]),
        ] {
            let exp = exp.as_array().unwrap();
            for (i, e) in exp.iter().enumerate() {
                let (ty, s, en, v) = (
                    e[0].as_str().unwrap(),
                    e[1].as_u64().unwrap() as u32,
                    e[2].as_u64().unwrap() as u32,
                    e[3].as_str().unwrap_or(""),
                );
                let Some(g) = got.get(i) else {
                    let _ = writeln!(out, "{label} #{i}: missing, expected {ty} {s}..{en} {v:?}");
                    break;
                };
                if kind_name(g.kind) != ty || g.span.start != s || g.span.end != en || g.value != v
                {
                    let _ = writeln!(
                        out,
                        "{label} #{i}: got {} {}..{} {:?} expected {ty} {s}..{en} {v:?} ({:?})",
                        kind_name(g.kind),
                        g.span.start,
                        g.span.end,
                        g.value,
                        source.get(s as usize..en as usize)
                    );
                    break;
                }
            }
            if got.len() != exp.len() && out.is_empty() {
                let _ = writeln!(
                    out,
                    "{label} count: got {} expected {}",
                    got.len(),
                    exp.len()
                );
            }
        }
        // token-ranges.json: suffix = template tokens + comments.
        let ranges: Vec<String> =
            serde_json::from_str(&std::fs::read_to_string(dir.join("token-ranges.json")).unwrap())
                .unwrap();
        let ours: Vec<&str> = doc
            .tokens
            .iter()
            .chain(&doc.comments)
            .map(|t| doc.slice(t.span))
            .collect();
        if ours.len() > ranges.len() || ranges[ranges.len() - ours.len()..] != ours[..] {
            let _ = writeln!(out, "token-ranges.json suffix mismatch");
        }
        // Same on Windows line endings (upstream "should have correct range on windows(CRLF)").
        let crlf = source.replace("\r\n", "\n").replace('\n', "\r\n");
        let alloc2 = Allocator::default();
        let doc2 = parse_document(&alloc2, &crlf, &ParserOptions { ts, jsx: false });
        let ours2: Vec<String> = doc2
            .tokens
            .iter()
            .chain(&doc2.comments)
            .map(|t| doc2.slice(t.span).replace("\r\n", "\n"))
            .collect();
        if ours2.len() > ranges.len() || ranges[ranges.len() - ours2.len()..] != ours2[..] {
            let first = ranges[ranges.len().saturating_sub(ours2.len())..]
                .iter()
                .zip(&ours2)
                .find(|(a, b)| a != b);
            let _ = writeln!(
                out,
                "CRLF token-ranges mismatch (count {} vs {}): {first:?}",
                ours2.len(),
                ours.len()
            );
        }
        // errors
        let exp_errors = expected["errors"].as_array().unwrap();
        let errs = |v: &mut String| {
            let _ = writeln!(
                v,
                "errors: got {:?} expected {:?}",
                doc.errors
                    .iter()
                    .map(|e| (e.message.as_str(), e.offset))
                    .collect::<Vec<_>>(),
                exp_errors
            );
        };
        if exp_errors.len() != doc.errors.len() {
            errs(&mut out);
        } else {
            for (g, e) in doc.errors.iter().zip(exp_errors) {
                let (m, off) = (e[0].as_str().unwrap(), e[1].as_u64().unwrap() as u32);
                if g.offset != off || (own_message(m) && g.message != m) {
                    errs(&mut out);
                    break;
                }
            }
        }
        // elements
        let mut els = vec![];
        if let Some(root) = doc.template_body {
            let mut stack = vec![root];
            while let Some(id) = stack.pop() {
                let el = doc.element(id);
                els.push(el);
                for c in el.children.iter().rev() {
                    if let Child::Element(e) = c {
                        stack.push(*e);
                    }
                }
            }
        }
        let exp_els = expected["elements"].as_array().unwrap();
        for (g, e) in els.iter().zip(exp_els) {
            let ns = match g.namespace {
                Namespace::Html => "xhtml",
                Namespace::Svg => "svg",
                Namespace::MathMl => "MathML",
            };
            if g.name != e[0].as_str().unwrap()
                || g.raw_name != e[1].as_str().unwrap()
                || ns != e[2].as_str().unwrap()
            {
                let _ = writeln!(
                    out,
                    "element: got {}/{}/{ns} expected {e}",
                    g.name, g.raw_name
                );
                break;
            }
        }
        // references (containers with refs, in tree order) and variables.
        let mut got_refs = vec![];
        let mut got_vars = vec![];
        let collect_container = |x: ExprId, got_refs: &mut Vec<String>| {
            let c = doc.expr(x);
            if !c.references.is_empty() {
                let rs: Vec<String> = c
                    .references
                    .iter()
                    .map(|r| format!("{}:{}@{}", r.name, mode_name(r.mode), r.span.start))
                    .collect();
                got_refs.push(format!("{}={}", c.span.start, rs.join(",")));
            }
        };
        for el in &els {
            for a in el.attributes() {
                if let AttributeKey::Directive(k) = &a.key
                    && let Some(DirectiveArgument::Dynamic(x)) = &k.argument
                {
                    collect_container(*x, &mut got_refs);
                }
                if let Some(AttributeValue::Expr(x)) = &a.value {
                    collect_container(*x, &mut got_refs);
                }
            }
            for c in &el.children {
                if let Child::Expr(x) = c {
                    collect_container(*x, &mut got_refs);
                }
            }
            if !el.variables.is_empty() {
                let kind = |k: VariableKind| match k {
                    VariableKind::VFor => "v-for",
                    VariableKind::Scope => "scope",
                    VariableKind::Generic => "generic",
                };
                let vs: Vec<String> = el
                    .variables
                    .iter()
                    .map(|v| format!("{}:{}@{}", v.name, kind(v.kind), v.span.start))
                    .collect();
                got_vars.push(format!("{}={}", el.span.start, vs.join(",")));
            }
        }
        // Upstream order is tree order (children interleaved); compare as sorted sets.
        got_refs.sort();
        let mut exp_refs: Vec<String> = expected["references"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                let rs: Vec<String> = c[1]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|r| {
                        format!(
                            "{}:{}@{}",
                            r[0].as_str().unwrap(),
                            r[1].as_str().unwrap(),
                            r[2]
                        )
                    })
                    .collect();
                format!("{}={}", c[0], rs.join(","))
            })
            .collect();
        exp_refs.sort();
        if got_refs != exp_refs {
            let _ = writeln!(out, "references: got {got_refs:?} expected {exp_refs:?}");
        }
        let exp_vars: Vec<String> = expected["variables"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                let vs: Vec<String> = c[1]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| {
                        format!(
                            "{}:{}@{}",
                            v[0].as_str().unwrap(),
                            v[1].as_str().unwrap(),
                            v[2]
                        )
                    })
                    .collect();
                format!("{}={}", c[0], vs.join(","))
            })
            .collect();
        let mut gv = got_vars.clone();
        gv.sort();
        let mut ev = exp_vars.clone();
        ev.sort();
        if gv != ev {
            let _ = writeln!(out, "variables: got {got_vars:?} expected {exp_vars:?}");
        }
    }
    if verbose {
        eprintln!("== {name}\n{out}");
    }
    if out.is_empty() { Ok(()) } else { Err(out) }
}

#[test]
fn vue_eslint_parser_fixtures() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parser-fixtures");
    let only = std::env::var("FIXTURE").ok();
    let mut names: Vec<String> = std::fs::read_dir(&root)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    let (mut pass, mut skipped, mut known) = (0, vec![], 0);
    let mut failures = vec![];
    for name in &names {
        if only.as_deref().is_some_and(|o| o != name) {
            continue;
        }
        let dir = root.join(name);
        let expected: Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("expected.json")).unwrap())
                .unwrap();
        if let Some(reason) = expected["skip"].as_str()
            && only.is_none()
        {
            skipped.push(format!("{name} ({reason})"));
            continue;
        }
        let res = std::panic::catch_unwind(|| check(name, &dir, only.is_some()));
        match res {
            Ok(Ok(())) => pass += 1,
            Ok(Err(e)) if KNOWN_FAILURES.iter().any(|(n, _)| n == name) => {
                known += 1;
                let _ = e;
            }
            Ok(Err(e)) => failures.push(format!("--- {name}\n{e}")),
            Err(_) => failures.push(format!("--- {name}\nPANIC")),
        }
    }
    eprintln!(
        "parser fixtures: {pass} passed, {} failed, {known} known failures, {} skipped",
        failures.len(),
        skipped.len()
    );
    for s in &skipped {
        eprintln!("  skipped: {s}");
    }
    if !failures.is_empty() {
        let names: Vec<&str> = failures
            .iter()
            .map(|f| f.lines().next().unwrap_or(""))
            .collect();
        eprintln!("{}", names.join("\n"));
        if std::env::var("FIXTURE_DETAILS").is_ok() {
            eprintln!("{}", failures.join("\n"));
        }
        panic!("{} fixture(s) failed", failures.len());
    }
}

/// The parser must never panic: feed every prefix (at char boundaries, sampled) of every fixture.
#[test]
fn never_panics_on_truncated_input() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parser-fixtures");
    for e in std::fs::read_dir(&root)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
    {
        let source = std::fs::read_to_string(e.path().join("source.vue")).unwrap();
        let step = (source.len() / 64).max(1);
        let mut i = 0;
        while i <= source.len() {
            if source.is_char_boundary(i) {
                for ts in [false, true] {
                    let r = std::panic::catch_unwind(|| {
                        let alloc = Allocator::default();
                        let doc =
                            parse_document(&alloc, &source[..i], &ParserOptions { ts, jsx: false });
                        // Every span must be a valid slice of the source.
                        for t in doc.tokens.iter().chain(&doc.comments) {
                            assert!(
                                source[..i]
                                    .get(t.span.start as usize..t.span.end as usize)
                                    .is_some(),
                                "bad token span {:?}",
                                t.span
                            );
                        }
                    });
                    assert!(
                        r.is_ok(),
                        "panic in {:?} at prefix {i} (ts={ts})",
                        e.file_name()
                    );
                }
            }
            i += step;
        }
    }
}

/// References resolve to the nearest element variable (v-for / slot scope); spans are absolute.
#[test]
fn references_resolve_to_element_variables() {
    let src = "<script setup lang=\"ts\">\n</script>\n<template>\n  <ul v-for=\"(item, i) in items\" :key=\"item.id\">\n    <Comp v-slot=\"{ row }\">{{ row + i + other }}</Comp>\n    <li @click=\"count++\">{{ item | upper(i) }}</li>\n  </ul>\n</template>\n";
    let alloc = Allocator::default();
    let doc = parse_document(
        &alloc,
        src,
        &ParserOptions {
            ts: true,
            jsx: false,
        },
    );
    assert!(doc.errors.is_empty(), "{:?}", doc.errors);
    let script = doc.top_level().find(|e| e.name == "script").unwrap();
    assert!(script.end_tag.is_some());
    let find = |name: &str| -> Vec<&Reference> {
        doc.exprs
            .iter()
            .flat_map(|c| &c.references)
            .filter(|r| r.name == name)
            .collect()
    };
    for r in find("item").into_iter().chain(find("i")).chain(find("row")) {
        let (el, idx) = r.variable.expect("resolved");
        let v = &doc.element(el).variables[idx as usize];
        assert_eq!(v.name, r.name);
        assert_eq!(&src[r.span.start as usize..r.span.end as usize], r.name);
    }
    assert!(find("items")[0].variable.is_none());
    assert!(find("other")[0].variable.is_none());
    let count = find("count")[0];
    assert_eq!(count.mode, ReferenceMode::ReadWrite);
    assert!(find("upper").is_empty(), "filter names are not references");
}
