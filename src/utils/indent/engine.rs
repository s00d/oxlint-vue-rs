//! Port of eslint-plugin-vue `lib/utils/indent-common.ts` (+ `indent-ts.ts`): token offsets are
//! collected by node handlers (called in ESLint selector order), then every line is validated.

use crate::ast::TokenKind;
use crate::utils::tpl_style::{is_js_ws, js_trim};

use super::store::{NONE, Store};
use super::tree::{F, Tree, Ty, flag};
use super::{Opts, Report, Selector};

#[derive(Clone, Copy, Debug)]
struct Off {
    base: u32,
    offset: i32,
    baseline: bool,
    expected: Option<i32>,
}

pub struct Engine<'e, 'a> {
    t: &'e Tree,
    s: &'e Store<'a>,
    o: &'e Opts,
    lines: &'e crate::LineIndex,
    /// token id → slot in `infos` (`NONE` = no offset). Slots may be shared (aliases).
    offs: Vec<u32>,
    infos: Vec<Off>,
    ignored: Vec<bool>,
    root: u32,
    pub out: Vec<Report>,
}

type R = (u32, u32);

fn is_p(s: &Store, t: u32, v: &str) -> bool {
    t != NONE && s.tok(t).kind == TokenKind::Punctuator && s.text(t) == v
}
/// `isNotWhitespace`.
fn not_ws(s: &Store, t: u32) -> bool {
    let k = s.tok(t).kind;
    k != TokenKind::HTMLWhitespace && !(k == TokenKind::JSXText && js_trim(s.text(t)).is_empty())
}
fn open_paren(s: &Store, t: u32) -> bool {
    is_p(s, t, "(")
}
fn close_paren(s: &Store, t: u32) -> bool {
    is_p(s, t, ")")
}
fn not_close_paren(s: &Store, t: u32) -> bool {
    !is_p(s, t, ")")
}
fn not_open_paren(s: &Store, t: u32) -> bool {
    !is_p(s, t, "(")
}
fn close_brace(s: &Store, t: u32) -> bool {
    is_p(s, t, "}")
}
fn close_bracket(s: &Store, t: u32) -> bool {
    is_p(s, t, "]")
}
fn open_bracket(s: &Store, t: u32) -> bool {
    is_p(s, t, "[")
}
fn not_semicolon(s: &Store, t: u32) -> bool {
    !is_p(s, t, ";")
}
fn optional_tok(s: &Store, t: u32) -> bool {
    is_p(s, t, "?.")
}
fn html_content(s: &Store, t: u32) -> bool {
    matches!(
        s.tok(t).kind,
        TokenKind::HTMLText
            | TokenKind::HTMLRCDataText
            | TokenKind::HTMLTagOpen
            | TokenKind::HTMLEndTagOpen
            | TokenKind::HTMLComment
    )
}

impl<'e, 'a> Engine<'e, 'a> {
    pub fn new(
        t: &'e Tree,
        s: &'e Store<'a>,
        o: &'e Opts,
        lines: &'e crate::LineIndex,
        root: u32,
    ) -> Self {
        Engine {
            t,
            s,
            o,
            lines,
            offs: vec![NONE; s.len()],
            infos: Vec::new(),
            ignored: vec![false; s.len()],
            root,
            out: Vec::new(),
        }
    }

    pub fn run(&mut self) {
        self.walk(self.root);
    }

    fn walk(&mut self, n: u32) {
        self.enter(n);
        let t = self.t;
        for c in t.child_iter(n) {
            self.walk(c);
        }
        self.exit(n);
    }

    // ------------------------------------------------------------------
    // Token helpers
    // ------------------------------------------------------------------

    fn r(&self, n: u32) -> R {
        self.t.range(n)
    }
    fn tr(&self, t: u32) -> R {
        self.s.range(t)
    }
    fn first(&self, n: u32) -> u32 {
        if n == NONE {
            NONE
        } else {
            self.s.first_token(self.r(n))
        }
    }
    fn last(&self, n: u32) -> u32 {
        if n == NONE {
            NONE
        } else {
            self.s.last_token(self.r(n))
        }
    }
    fn before_n(&self, n: u32) -> u32 {
        if n == NONE {
            NONE
        } else {
            self.s.before(self.r(n))
        }
    }
    fn after_n(&self, n: u32) -> u32 {
        if n == NONE {
            NONE
        } else {
            self.s.after(self.r(n))
        }
    }
    fn before_t(&self, t: u32) -> u32 {
        if t == NONE {
            NONE
        } else {
            self.s.before(self.tr(t))
        }
    }
    fn after_t(&self, t: u32) -> u32 {
        if t == NONE {
            NONE
        } else {
            self.s.after(self.tr(t))
        }
    }
    fn after_nf(&self, n: u32, f: &dyn Fn(&Store, u32) -> bool) -> u32 {
        if n == NONE {
            NONE
        } else {
            self.s.after_f(self.r(n), false, f)
        }
    }
    fn after_tf(&self, t: u32, f: &dyn Fn(&Store, u32) -> bool) -> u32 {
        if t == NONE {
            NONE
        } else {
            self.s.after_f(self.tr(t), false, f)
        }
    }
    fn before_nf(&self, n: u32, f: &dyn Fn(&Store, u32) -> bool) -> u32 {
        if n == NONE {
            NONE
        } else {
            self.s.before_f(self.r(n), f)
        }
    }
    fn between(&self, a: R, b: R) -> Vec<u32> {
        self.s.between(a, b)
    }
    fn p(&self, t: u32, v: &str) -> bool {
        is_p(self.s, t, v)
    }
    fn text(&self, t: u32) -> &'a str {
        if t == NONE { "" } else { self.s.text(t) }
    }
    fn line_s(&self, pos: u32) -> usize {
        self.lines.line_of(pos)
    }
    fn tline_s(&self, t: u32) -> usize {
        self.line_s(self.s.tok(t).start)
    }
    fn tline_e(&self, t: u32) -> usize {
        self.line_s(self.s.tok(t).end)
    }
    fn col(&self, t: u32) -> i32 {
        let (_, c) = self.lines.line_col(self.s.src, self.s.tok(t).start);
        c as i32 - 1
    }
    fn get(&self, n: u32, f: F) -> u32 {
        if n == NONE { NONE } else { self.t.get(n, f) }
    }
    fn list(&self, n: u32, f: F) -> &'e [u32] {
        let t = self.t;
        if n == NONE { &[] } else { t.list(n, f) }
    }
    fn tok_r(&self, t: u32) -> Option<R> {
        (t != NONE).then(|| self.tr(t))
    }
    fn node_r(&self, n: u32) -> Option<R> {
        (n != NONE).then(|| self.r(n))
    }

    // ------------------------------------------------------------------
    // Offsets
    // ------------------------------------------------------------------

    fn info(&self, t: u32) -> Option<Off> {
        if t == NONE {
            return None;
        }
        let s = self.offs[t as usize];
        (s != NONE).then(|| self.infos[s as usize])
    }

    fn set_offset(&mut self, t: u32, offset: i32, base: u32) {
        if t == NONE || t == base {
            return;
        }
        let slot = self.infos.len() as u32;
        self.infos.push(Off {
            base,
            offset,
            baseline: false,
            expected: None,
        });
        self.offs[t as usize] = slot;
    }
    fn set_offsets(&mut self, ts: &[u32], offset: i32, base: u32) {
        for &t in ts {
            self.set_offset(t, offset, base);
        }
    }
    fn copy_offset(&mut self, t: u32, src: u32) {
        if t == NONE {
            return;
        }
        let Some(d) = self.info(src) else { return };
        self.set_offset(t, d.offset, d.base);
        if d.baseline {
            self.set_baseline(t);
        }
        let slot = self.offs[t as usize];
        if slot != NONE {
            self.infos[slot as usize].expected = d.expected;
        }
    }
    fn set_baseline(&mut self, t: u32) {
        if t == NONE {
            return;
        }
        let slot = self.offs[t as usize];
        if slot != NONE {
            self.infos[slot as usize].baseline = true;
        }
    }

    fn set_preformatted(&mut self, n: u32) {
        let start_tag = self.get(n, F::StartTag);
        let end_tag = self.get(n, F::EndTag);
        let mut end_token = if end_tag != NONE {
            self.first(end_tag)
        } else {
            NONE
        };
        if end_token == NONE {
            end_token = self.after_n(n);
        }
        let st = self.r(start_tag);
        let content = if end_token != NONE {
            self.s
                .between_f(st, self.tr(end_token), true, &html_content)
        } else {
            self.s.tokens_after_f(st, true, &html_content)
        };
        for t in content {
            self.ignored[t as usize] = true;
        }
        if end_token != NONE {
            self.ignored[end_token as usize] = true;
        }
    }

    /// `getFirstAndLastTokens`.
    fn fl(&self, r: R, border: u32) -> (u32, u32) {
        let mut first = self.s.first_token(r);
        let mut last = self.s.last_token(r);
        if first == NONE || last == NONE {
            return (first, last);
        }
        loop {
            let t = self.before_t(first);
            let u = self.after_t(last);
            if t != NONE
                && u != NONE
                && open_paren(self.s, t)
                && close_paren(self.s, u)
                && self.tr(t).0 >= border
            {
                first = t;
                last = u;
            } else {
                break;
            }
        }
        (first, last)
    }
    fn fl_n(&self, n: u32) -> (u32, u32) {
        if n == NONE {
            (NONE, NONE)
        } else {
            self.fl(self.r(n), 0)
        }
    }

    fn is_beginning_of_line(&self, items: &[Option<R>], i: usize) -> bool {
        let Some(n) = items[i] else { return false };
        for p in items[..i].iter().rev() {
            let Some(p) = p else { continue };
            return self.line_s(n.0) != self.line_s(p.1);
        }
        false
    }

    /// `processNodeList`.
    fn pnl(&mut self, nodes: &[u32], left: Option<R>, right: Option<R>, offset: i32, align: bool) {
        let items: Vec<Option<R>> = nodes.iter().map(|&n| self.node_r(n)).collect();
        self.pnl_r(&items, left, right, offset, align);
    }

    /// `processNodeList` over item ranges (`None` = array hole).
    fn pnl_r(
        &mut self,
        items: &[Option<R>],
        left: Option<R>,
        right: Option<R>,
        offset: i32,
        align: bool,
    ) {
        let left_token = left.map_or(NONE, |r| self.s.first_token(r));
        let right_token = right.map_or(NONE, |r| self.s.first_token(r));
        let iter = |s: &Store, t: u32| not_ws(s, t);

        if !items.is_empty() {
            let mut base_token = NONE;
            let mut last_end: Option<u32> = left.map(|r| r.1);
            let mut before_base: Vec<u32> = Vec::new();
            let mut align_tokens: Vec<u32> = Vec::new();
            for r in items.iter().flatten() {
                let (ef, el) = self.fl(*r, last_end.unwrap_or(0));
                if let Some(le) = last_end {
                    let ef_start = if ef == NONE { 0 } else { self.tr(ef).0 };
                    self.collect_between(
                        le,
                        ef_start,
                        base_token == NONE,
                        &mut before_base,
                        &mut align_tokens,
                        &iter,
                    );
                }
                if base_token == NONE {
                    base_token = ef;
                } else {
                    align_tokens.push(ef);
                }
                last_end = if el == NONE {
                    None
                } else {
                    Some(self.tr(el).1)
                };
            }
            if right_token != NONE
                && let Some(le) = last_end
            {
                let rs = self.tr(right_token).0;
                self.collect_between(
                    le,
                    rs,
                    base_token == NONE,
                    &mut before_base,
                    &mut align_tokens,
                    &iter,
                );
            }
            if left_token != NONE {
                self.set_offsets(&before_base, offset, left_token);
            }
            if base_token != NONE {
                if left_token != NONE {
                    self.set_offset(base_token, offset, left_token);
                }
                if (0..items.len()).any(|i| self.is_beginning_of_line(items, i)) {
                    self.set_baseline(base_token);
                }
                if align || left_token == NONE {
                    self.set_offsets(&align_tokens, 0, base_token);
                } else {
                    self.set_offsets(&align_tokens, offset, left_token);
                }
            }
        }
        if right_token != NONE && left_token != NONE {
            self.set_offset(right_token, 0, left_token);
        }
    }

    /// Comma/comment tokens after `from` that end at or before `limit`.
    fn collect_between(
        &self,
        from: u32,
        limit: u32,
        before: bool,
        before_base: &mut Vec<u32>,
        align_tokens: &mut Vec<u32>,
        f: &dyn Fn(&Store, u32) -> bool,
    ) {
        for t in self.s.fwd(from as i64, -1, true) {
            if !f(self.s, t) {
                continue;
            }
            if self.tr(t).1 > limit {
                break;
            }
            if before {
                before_base.push(t);
            } else {
                align_tokens.push(t);
            }
        }
    }

    fn process_maybe_block(&mut self, n: u32, base: u32) {
        let first = self.fl_n(n).0;
        let off = if self.p(first, "{") { 0 } else { 1 };
        self.set_offset(first, off, base);
    }

    fn process_semicolons(&mut self, n: u32) {
        let first = self.first(n);
        let last = self.last(n);
        if first != last && self.p(last, ";") {
            self.set_offset(last, 0, first);
        }
        if first == NONE {
            return;
        }
        let slot = self.offs[first as usize];
        let prev = self.before_t(first);
        if prev != NONE
            && slot != NONE
            && self.p(prev, ";")
            && self.tline_e(prev) == self.tline_s(first)
        {
            self.offs[prev as usize] = slot;
        }
    }

    fn chain_head(&self, mut n: u32) -> u32 {
        let ty = self.t.ty(n);
        loop {
            let p = self.t.parent(n);
            if p == NONE || self.t.ty(p) != ty {
                break;
            }
            if open_paren(self.s, self.before_n(n)) {
                break;
            }
            n = p;
        }
        self.first(n)
    }

    fn is_beginning_of_element(&self, token: u32, belonging: u32) -> bool {
        let ts = self.tr(token).0;
        let mut n = belonging;
        while n != NONE && self.t.parent(n) != NONE {
            let p = self.t.parent(n);
            let pty = self.t.ty(p);
            if pty.is_statement_class() {
                return self.r(p).0 == ts;
            }
            if pty == Ty::VExpressionContainer {
                if self.r(n).0 != ts {
                    return false;
                }
                let prev = self.before_n(belonging);
                return !open_paren(self.s, prev) || prev == NONE;
            }
            if matches!(pty, Ty::CallExpression | Ty::NewExpression) {
                let open = self.after_nf(self.get(p, F::Callee), &not_close_paren);
                let border = if open == NONE { 0 } else { self.tr(open).1 };
                return self.list(p, F::Arguments).iter().any(|&a| {
                    a != NONE && {
                        let f = self.fl(self.r(a), border).0;
                        f != NONE && self.tr(f).0 == ts
                    }
                });
            }
            if pty == Ty::ArrayExpression {
                return self.list(p, F::Elements).iter().any(|&a| {
                    a != NONE && {
                        let f = self.fl_n(a).0;
                        f != NONE && self.tr(f).0 == ts
                    }
                });
            }
            if pty == Ty::SequenceExpression {
                return self.list(p, F::Expressions).iter().any(|&a| {
                    let f = self.fl_n(a).0;
                    f != NONE && self.tr(f).0 == ts
                });
            }
            n = p;
        }
        false
    }

    fn process_top_level(&mut self, n: u32, expected: i32) {
        let token = self.first(n);
        if token == NONE {
            return;
        }
        let slot = self.offs[token as usize];
        if slot == NONE {
            let s = self.infos.len() as u32;
            self.infos.push(Off {
                base: NONE,
                offset: 0,
                baseline: false,
                expected: Some(expected),
            });
            self.offs[token as usize] = s;
        } else {
            self.infos[slot as usize].expected = Some(expected);
        }
    }

    fn ignore(&mut self, n: u32) {
        for t in self.s.tokens(self.r(n)) {
            self.offs[t as usize] = NONE;
            self.ignored[t as usize] = true;
        }
    }

    // ------------------------------------------------------------------
    // Dispatch
    // ------------------------------------------------------------------

    fn exit(&mut self, n: u32) {
        let ignores = &self.o.ignores;
        if ignores.iter().any(|s| s.matches(self.t, n)) {
            self.ignore(n);
        }
        if self.t.ty(n) == Ty::Unknown {
            self.ignore(n);
        }
        if n == self.root {
            self.validate_all(n);
        }
    }

    fn enter(&mut self, n: u32) {
        use Ty::*;
        let ty = self.t.ty(n);
        // ':statement, PropertyDefinition'
        if ty.is_statement_class() || ty == PropertyDefinition {
            self.process_semicolons(n);
        }
        // ':expression'
        if ty.is_expression_class() {
            self.paren(n);
        }
        match ty {
            VAttribute => self.v_attribute(n),
            VElement => self.v_element(n),
            VEndTag => self.v_end_tag(n),
            VExpressionContainer => self.v_container(n),
            VFilter => self.v_filter(n),
            VFilterSequenceExpression => self.v_filter_seq(n),
            VForExpression => self.v_for(n),
            VOnExpression => {
                let b = self.list(n, F::Body);
                self.pnl(b, None, None, 0, true);
            }
            VStartTag => self.v_start_tag(n),
            VText => self.v_text(n),
            ArrayExpression | ArrayPattern => {
                let first = self.first(n);
                let els = self.list(n, F::Elements);
                let lastel = els.last().copied().unwrap_or(NONE);
                let right = if lastel != NONE {
                    self.after_nf(lastel, &close_bracket)
                } else {
                    self.after_tf(first, &close_bracket)
                };
                self.pnl(els, self.tok_r(first), self.tok_r(right), 1, true);
            }
            ArrowFunctionExpression => self.arrow(n),
            AssignmentExpression | AssignmentPattern | BinaryExpression | LogicalExpression => {
                self.binary_like(n)
            }
            AwaitExpression | RestElement | SpreadElement | UnaryExpression | UpdateExpression => {
                let first = self.first(n);
                let next = self.after_t(first);
                self.set_offset(next, 1, first);
            }
            BlockStatement | ClassBody => {
                let b = self.list(n, F::Body);
                let (f, l) = (self.first(n), self.last(n));
                self.pnl(b, self.tok_r(f), self.tok_r(l), 1, true);
            }
            StaticBlock => {
                let first = self.first(n);
                let mut next = self.after_t(first);
                while next != NONE && !self.p(next, "{") {
                    self.set_offset(next, 0, first);
                    next = self.after_t(next);
                }
                self.set_offset(next, 0, first);
                let b = self.list(n, F::Body);
                let l = self.last(n);
                self.pnl(b, self.tok_r(next), self.tok_r(l), 1, true);
            }
            BreakStatement | ContinueStatement | ReturnStatement | ThrowStatement => {
                let has = match ty {
                    ReturnStatement | ThrowStatement => self.get(n, F::Argument) != NONE,
                    _ => self.get(n, F::Label) != NONE,
                };
                if has {
                    let first = self.first(n);
                    let next = self.after_t(first);
                    self.set_offset(next, 1, first);
                }
            }
            CallExpression => self.call(n),
            ImportExpression => {
                let first = self.first(n);
                let right = self.last(n);
                let left = self.after_tf(first, &open_paren);
                self.set_offset(left, 1, first);
                let src = self.get(n, F::Source);
                self.pnl(&[src], self.tok_r(left), self.tok_r(right), 1, true);
            }
            CatchClause => {
                let first = self.first(n);
                let body = self.first(self.get(n, F::Body));
                let param = self.get(n, F::Param);
                if param != NONE {
                    let left = self.after_t(first);
                    let right = self.after_n(param);
                    self.set_offset(left, 1, first);
                    self.pnl(&[param], self.tok_r(left), self.tok_r(right), 1, true);
                }
                self.set_offset(body, 0, first);
            }
            ClassDeclaration | ClassExpression => self.class(n),
            ConditionalExpression => self.conditional(n),
            DoWhileStatement => {
                let do_t = self.first(n);
                let body = self.get(n, F::Body);
                let while_t = self.after_nf(body, &not_close_paren);
                let left = self.after_t(while_t);
                let test = self.after_t(left);
                let last = self.last(n);
                let right = if self.p(last, ";") {
                    self.before_t(last)
                } else {
                    last
                };
                self.process_maybe_block(body, do_t);
                self.set_offset(while_t, 0, do_t);
                self.set_offset(left, 1, while_t);
                self.set_offset(test, 1, left);
                self.set_offset(right, 0, left);
            }
            ExportAllDeclaration => self.export_all(n),
            ExportDefaultDeclaration => {
                let export = self.first(n);
                let default = self
                    .s
                    .fwd(self.r(n).0 as i64, self.r(n).1 as i64, false)
                    .nth(1)
                    .unwrap_or(NONE);
                let decl = self.fl_n(self.get(n, F::Declaration)).0;
                self.set_offsets(&[default, decl], 1, export);
            }
            ExportNamedDeclaration => self.export_named(n),
            ExportSpecifier | ImportSpecifier => {
                let mut tokens = self.s.tokens(self.r(n));
                if tokens.is_empty() {
                    return self.after_enter(n, ty);
                }
                let mut first = tokens.remove(0);
                if self.text(first) == "type" && !tokens.is_empty() {
                    let type_t = first;
                    first = tokens.remove(0);
                    self.set_offset(first, 0, type_t);
                }
                self.set_offsets(&tokens, 1, first);
            }
            ForInStatement | ForOfStatement => self.for_in(n),
            ForStatement => {
                let for_t = self.first(n);
                let left = self.after_t(for_t);
                let body = self.get(n, F::Body);
                let right = self.before_nf(body, &not_open_paren);
                self.set_offset(left, 1, for_t);
                let l = [
                    self.get(n, F::Init),
                    self.get(n, F::Test),
                    self.get(n, F::Update),
                ];
                self.pnl(&l, self.tok_r(left), self.tok_r(right), 1, true);
                self.process_maybe_block(body, for_t);
            }
            FunctionDeclaration | FunctionExpression => self.function(n),
            IfStatement => {
                let if_t = self.first(n);
                let lp = self.after_t(if_t);
                let cons = self.get(n, F::Consequent);
                let rp = self.before_nf(cons, &close_paren);
                self.set_offset(lp, 1, if_t);
                self.set_offset(rp, 0, lp);
                self.process_maybe_block(cons, if_t);
                let alt = self.get(n, F::Alternate);
                if alt != NONE {
                    let else_t = self.after_nf(cons, &not_close_paren);
                    self.set_offset(else_t, 0, if_t);
                    self.process_maybe_block(alt, else_t);
                }
            }
            ImportDeclaration => self.import(n),
            ImportNamespaceSpecifier => {
                let mut tokens = self.s.tokens(self.r(n));
                if !tokens.is_empty() {
                    let first = tokens.remove(0);
                    self.set_offsets(&tokens, 1, first);
                }
            }
            LabeledStatement => {
                let label = self.first(n);
                let colon = self.after_t(label);
                let body = self.after_t(colon);
                self.set_offsets(&[colon, body], 1, label);
            }
            MemberExpression | MetaProperty => self.member(n),
            MethodDefinition | Property | PropertyDefinition => self.property_like(n, F::Key),
            NewExpression => self.new_expr(n),
            ObjectExpression | ObjectPattern => {
                let first = self.first(n);
                let ps = self.list(n, F::Properties);
                let lastp = ps.last().copied().unwrap_or(NONE);
                let right = if lastp != NONE {
                    self.after_nf(lastp, &close_brace)
                } else {
                    self.after_tf(first, &close_brace)
                };
                self.pnl(ps, self.tok_r(first), self.tok_r(right), 1, true);
            }
            SequenceExpression => {
                let es = self.list(n, F::Expressions);
                self.pnl(es, None, None, 0, true);
            }
            SwitchCase => {
                let case_t = self.first(n);
                let test = self.get(n, F::Test);
                if test == NONE {
                    let colon = self.after_t(case_t);
                    self.set_offset(colon, 1, case_t);
                } else {
                    let tt = self.after_t(case_t);
                    let colon = self.after_nf(test, &not_close_paren);
                    self.set_offsets(&[tt, colon], 1, case_t);
                }
                let cons = self.list(n, F::Consequent);
                if cons.len() == 1 && self.t.ty(cons[0]) == BlockStatement {
                    let f = self.first(cons[0]);
                    self.set_offset(f, 0, case_t);
                } else if !cons.is_empty() {
                    let f = self.first(cons[0]);
                    self.set_offset(f, 1, case_t);
                    self.pnl(cons, None, None, 0, true);
                }
            }
            SwitchStatement => {
                let sw = self.first(n);
                let lp = self.after_t(sw);
                let disc = self.after_t(lp);
                let lb = self.after_nf(self.get(n, F::Discriminant), &|s, t| is_p(s, t, "{"));
                let rp = self.before_t(lb);
                let rb = self.last(n);
                self.set_offset(lp, 1, sw);
                self.set_offset(disc, 1, lp);
                self.set_offset(rp, 0, lp);
                self.set_offset(lb, 0, sw);
                let cases = self.list(n, F::Cases);
                let sc = self.o.switch_case;
                self.pnl(cases, self.tok_r(lb), self.tok_r(rb), sc, true);
            }
            TaggedTemplateExpression => {
                let tag = self.get(n, F::Tag);
                let (tf, tl) = self.fl(self.r(tag), self.r(n).0);
                let quasi = self.after_t(tl);
                self.set_offset(quasi, 1, tf);
            }
            TemplateLiteral | TSTemplateLiteralType => {
                let first = self.first(n);
                let qs = self.list(n, F::Quasis);
                let quasi_tokens: Vec<u32> = qs.iter().skip(1).map(|&q| self.first(q)).collect();
                let expr_tokens: Vec<u32> = qs
                    .iter()
                    .take(qs.len().saturating_sub(1))
                    .map(|&q| self.after_n(q))
                    .collect();
                self.set_offsets(&quasi_tokens, 0, first);
                self.set_offsets(&expr_tokens, 1, first);
            }
            TryStatement => {
                let try_t = self.first(n);
                let block = self.first(self.get(n, F::Block));
                self.set_offset(block, 0, try_t);
                let h = self.get(n, F::Handler);
                if h != NONE {
                    let c = self.first(h);
                    self.set_offset(c, 0, try_t);
                }
                let fin = self.get(n, F::Finalizer);
                if fin != NONE {
                    let ft = self.before_n(fin);
                    let fb = self.first(fin);
                    self.set_offsets(&[ft, fb], 0, try_t);
                }
            }
            VariableDeclaration => {
                let ds = self.list(n, F::Declarations);
                let f = self.first(n);
                self.pnl(ds, self.tok_r(f), None, 1, true);
            }
            VariableDeclarator => {
                if self.get(n, F::Init) != NONE {
                    let id = self.first(n);
                    let eq = self.after_n(self.get(n, F::Id));
                    let init = self.after_t(eq);
                    self.set_offsets(&[eq, init], 1, id);
                }
            }
            WhileStatement | WithStatement => {
                let first = self.first(n);
                let lp = self.after_t(first);
                let body = self.get(n, F::Body);
                let rp = self.before_nf(body, &close_paren);
                self.set_offset(lp, 1, first);
                self.set_offset(rp, 0, lp);
                self.process_maybe_block(body, first);
            }
            YieldExpression => {
                if self.get(n, F::Argument) != NONE {
                    let y = self.first(n);
                    let a = self.after_t(y);
                    self.set_offset(a, 1, y);
                    if self.t.has(n, flag::DELEGATE) {
                        let b = self.s.after_skip(self.tr(y), 1);
                        self.set_offset(b, 1, y);
                    }
                }
            }
            Program => {
                let base = self.o.indent_size * self.o.base_indent;
                for &st in self.list(n, F::Body) {
                    self.process_top_level(st, base);
                }
            }
            // ---------------- TypeScript ----------------
            TSTypeAnnotation => {
                let ft = self.s.first_tokens(self.r(n), 2);
                let p = self.t.parent(n);
                let base = self.first(p);
                self.set_offsets(&ft, 1, base);
                if let Some(&c) = ft.first() {
                    let before = self.before_t(c);
                    if before != NONE && self.text(before) == "?" {
                        self.set_offset(before, 1, base);
                    }
                }
            }
            TSAsExpression | TSSatisfiesExpression => {
                let (ef, el) = self.fl_n(self.get(n, F::Expression));
                let op = self.after_t(el);
                let tf = self.fl_n(self.get(n, F::TypeAnnotation)).0;
                self.set_offsets(&[op, tf], 1, ef);
            }
            TSTypeReference | TSInstantiationExpression => {
                let ta = self.get(n, F::TypeArguments);
                if ta != NONE {
                    let first = self.first(n);
                    let f = self.first(ta);
                    self.set_offset(f, 1, first);
                }
            }
            TSTypeParameterInstantiation | TSTypeParameterDeclaration => {
                let ps = self.list(n, F::Params);
                let (f, l) = (self.first(n), self.last(n));
                self.pnl(ps, self.tok_r(f), self.tok_r(l), 1, true);
            }
            TSTypeAliasDeclaration => {
                let type_t = self.first(n);
                let id = self.get(n, F::Id);
                let id_t = self.first(id);
                self.set_offset(id_t, 1, type_t);
                let tp = self.get(n, F::TypeParameters);
                let eq = if tp != NONE {
                    let f = self.first(tp);
                    self.set_offset(f, 1, id_t);
                    self.after_n(tp)
                } else {
                    self.after_n(id)
                };
                let init = self.after_t(eq);
                self.set_offsets(&[eq, init], 1, id_t);
            }
            TSConstructorType | TSFunctionType => {
                let first = self.first(n);
                let mut cur = first;
                if ty == TSConstructorType {
                    cur = self.after_t(cur);
                    self.set_offset(cur, 1, first);
                }
                let tp = self.get(n, F::TypeParameters);
                if tp != NONE {
                    cur = self.after_n(tp);
                    self.set_offset(cur, 1, first);
                }
                let lp = cur;
                let ps = self.list(n, F::Params);
                let rp = match ps.last() {
                    Some(&l) => self.after_nf(l, &close_paren),
                    None => self.after_tf(lp, &close_paren),
                };
                self.pnl(ps, self.tok_r(lp), self.tok_r(rp), 1, true);
                let arrow = self.after_t(rp);
                self.set_offset(arrow, 1, lp);
            }
            TSTypeLiteral => {
                let ms = self.list(n, F::Members);
                let (f, l) = (self.first(n), self.last(n));
                self.pnl(ms, self.tok_r(f), self.tok_r(l), 1, true);
            }
            TSPropertySignature => {
                let first = self.first(n);
                let key = self.get(n, F::Key);
                let (_, kl) = self.fl_n(key);
                let key_last = if self.t.has(n, flag::COMPUTED) {
                    let cb = self.after_t(kl);
                    self.pnl(&[key], self.tok_r(first), self.tok_r(cb), 1, true);
                    cb
                } else {
                    kl
                };
                let ta = self.get(n, F::TypeAnnotation);
                if ta != NONE {
                    let tat = self.first(ta);
                    let mut v = self.between_tok(key_last, tat);
                    v.push(tat);
                    self.set_offsets(&v, 1, first);
                } else if self.t.has(n, flag::OPTIONAL) {
                    let q = self.last(n);
                    self.set_offset(q, 1, first);
                }
            }
            TSIndexSignature => {
                let lb = self.first(n);
                let ps = self.list(n, F::Parameters);
                let rb = match ps.last() {
                    Some(&l) => self.after_nf(l, &close_bracket),
                    None => self.after_tf(lb, &close_bracket),
                };
                self.pnl(ps, self.tok_r(lb), self.tok_r(rb), 1, true);
                let ta = self.get(n, F::TypeAnnotation);
                if ta != NONE {
                    let tat = self.first(ta);
                    let mut v = self.between_tok(rb, tat);
                    v.push(tat);
                    self.set_offsets(&v, 1, lb);
                }
            }
            TSArrayType => {
                let first = self.first(n);
                let l2 = self.s.last_tokens(self.r(n), 2);
                self.set_offsets(&l2, 0, first);
            }
            TSTupleType => {
                let es = self.list(n, F::ElementTypes);
                let (f, l) = (self.first(n), self.last(n));
                self.pnl(es, self.tok_r(f), self.tok_r(l), 1, true);
            }
            TSQualifiedName => {
                let obj = self.first(n);
                let dot = self.before_n(self.get(n, F::Right));
                let prop = self.after_t(dot);
                self.set_offsets(&[dot, prop], 1, obj);
            }
            TSIndexedAccessType => {
                let obj = self.first(n);
                let it = self.get(n, F::IndexType);
                let lb = self.before_nf(it, &open_bracket);
                let rb = self.after_nf(it, &close_bracket);
                self.set_offset(lb, 1, obj);
                self.pnl(&[it], self.tok_r(lb), self.tok_r(rb), 1, true);
            }
            TSUnionType | TSIntersectionType => self.union(n),
            TSMappedType => self.mapped(n),
            TSTypeParameter => self.type_param(n),
            TSConditionalType => self.conditional_type(n),
            TSInterfaceDeclaration => {
                let it = self.first(n);
                let id = self.get(n, F::Id);
                let idt = self.first(id);
                self.set_offset(idt, 1, it);
                let tp = self.get(n, F::TypeParameters);
                if tp != NONE {
                    let f = self.first(tp);
                    self.set_offset(f, 1, idt);
                }
                let ex = self.list(n, F::Extends);
                if !ex.is_empty() {
                    let et = self.before_n(ex[0]);
                    self.set_offset(et, 1, it);
                    self.pnl(ex, self.tok_r(et), None, 1, true);
                }
                let bt = self.first(self.get(n, F::Body));
                self.set_offset(bt, 0, it);
            }
            TSInterfaceBody | TSModuleBlock => {
                let b = self.list(n, F::Body);
                let (f, l) = (self.first(n), self.last(n));
                self.pnl(b, self.tok_r(f), self.tok_r(l), 1, true);
            }
            TSClassImplements | TSInterfaceHeritage => {
                let ta = self.get(n, F::TypeArguments);
                if ta != NONE {
                    let f = self.first(ta);
                    let base = self.first(n);
                    self.set_offset(f, 1, base);
                }
            }
            TSEnumDeclaration => {
                let first = self.first(n);
                let (idf, idl) = self.fl_n(self.get(n, F::Id));
                let prefix = self.between_tok(first, idf);
                self.set_offsets(&prefix, 0, first);
                self.set_offset(idf, 1, first);
                let lb = self.after_t(idl);
                self.set_offset(lb, 0, first);
            }
            TSEnumBody => {
                let ms = self.list(n, F::Members);
                let (f, l) = (self.first(n), self.last(n));
                self.pnl(ms, self.tok_r(f), self.tok_r(l), 1, true);
            }
            TSModuleDeclaration => {
                let first = self.first(n);
                let (idf, _) = self.fl_n(self.get(n, F::Id));
                let prefix = self.between_tok(first, idf);
                self.set_offsets(&prefix, 0, first);
                self.set_offset(idf, 1, first);
                let body = self.get(n, F::Body);
                if body != NONE {
                    let bf = self.first(body);
                    let off = if self.p(bf, "{") { 0 } else { 1 };
                    self.set_offset(bf, off, first);
                }
            }
            TSMethodSignature => {
                let first = self.first(n);
                let key = self.get(n, F::Key);
                let (_, kl) = self.fl_n(key);
                let key_last = if self.t.has(n, flag::COMPUTED) {
                    let cb = self.after_t(kl);
                    self.pnl(&[key], self.tok_r(first), self.tok_r(cb), 1, true);
                    cb
                } else {
                    kl
                };
                let lp = self.after_tf(key_last, &open_paren);
                let mut v = self.between_tok(key_last, lp);
                v.push(lp);
                self.set_offsets(&v, 1, first);
                let ps = self.list(n, F::Params);
                let rp = match ps.last() {
                    Some(&l) => self.after_nf(l, &close_paren),
                    None => self.after_tf(lp, &close_paren),
                };
                self.pnl(ps, self.tok_r(lp), self.tok_r(rp), 1, true);
                let rt = self.get(n, F::ReturnType);
                if rt != NONE {
                    let rtt = self.first(rt);
                    let mut v = self.between_tok(key_last, rtt);
                    v.push(rtt);
                    self.set_offsets(&v, 1, first);
                }
            }
            TSCallSignatureDeclaration | TSConstructSignatureDeclaration => {
                let first = self.first(n);
                let mut cur = first;
                if ty == TSConstructSignatureDeclaration {
                    cur = self.after_t(cur);
                    self.set_offset(cur, 1, first);
                }
                let tp = self.get(n, F::TypeParameters);
                if tp != NONE {
                    cur = self.after_n(tp);
                    self.set_offset(cur, 1, first);
                }
                let lp = cur;
                let ps = self.list(n, F::Params);
                let rp = match ps.last() {
                    Some(&l) => self.after_nf(l, &close_paren),
                    None => self.after_tf(lp, &close_paren),
                };
                self.pnl(ps, self.tok_r(lp), self.tok_r(rp), 1, true);
                let rt = self.get(n, F::ReturnType);
                if rt != NONE {
                    let rtt = self.first(rt);
                    let mut v = self.between_tok(rp, rtt);
                    v.push(rtt);
                    self.set_offsets(&v, 1, first);
                }
            }
            TSDeclareFunction | TSEmptyBodyFunctionExpression => self.declare_function(n),
            TSTypeOperator | TSTypeQuery | TSInferType => {
                let first = self.first(n);
                let next = self.after_t(first);
                self.set_offset(next, 1, first);
            }
            TSTypePredicate => {
                let first = self.first(n);
                let op = self.after_nf(self.get(n, F::ParameterName), &not_close_paren);
                let ta = self.get(n, F::TypeAnnotation);
                let right = if ta != NONE { self.fl_n(ta).0 } else { NONE };
                let base = if first == NONE {
                    NONE
                } else {
                    self.fl(self.tr(first), 0).0
                };
                self.set_offsets(&[op, right], 1, base);
            }
            TSAbstractMethodDefinition
            | TSAbstractPropertyDefinition
            | TSAbstractAccessorProperty
            | TSEnumMember => {
                let key_f = if ty == TSEnumMember { F::Id } else { F::Key };
                self.property_like(n, key_f);
            }
            TSOptionalType | TSNonNullExpression => {
                let (f, l) = (self.first(n), self.last(n));
                self.set_offset(l, 1, f);
            }
            TSTypeAssertion => {
                let first = self.first(n);
                let et = self.fl_n(self.get(n, F::Expression)).0;
                let ta = self.get(n, F::TypeAnnotation);
                let bt = self.before_t(et);
                self.pnl(&[ta], self.tok_r(first), self.tok_r(bt), 1, true);
                self.set_offset(et, 1, first);
            }
            TSImportType => self.import_type(n),
            TSParameterProperty => {
                let first = self.first(n);
                let pt = self.first(self.get(n, F::Parameter));
                let mut v = self.between_tok(first, pt);
                v.push(pt);
                self.set_offsets(&v, 1, first);
            }
            TSImportEqualsDeclaration => {
                let it = self.first(n);
                let (idf, idl) = self.fl_n(self.get(n, F::Id));
                self.set_offset(idf, 1, it);
                let op = self.after_t(idl);
                let mr = self.first(self.get(n, F::ModuleReference));
                self.set_offsets(&[op, mr], 1, idl);
            }
            TSExternalModuleReference => {
                let req = self.first(n);
                let lp = self.after_tf(req, &open_paren);
                let rp = self.last(n);
                self.set_offset(lp, 1, req);
                let e = self.get(n, F::Expression);
                self.pnl(&[e], self.tok_r(lp), self.tok_r(rp), 1, true);
            }
            TSExportAssignment => {
                let et = self.first(n);
                let (ef, _) = self.fl_n(self.get(n, F::Expression));
                let op = self.before_t(ef);
                self.set_offsets(&[op, ef], 1, et);
            }
            TSNamedTupleMember => {
                let label = self.first(n);
                let (ef, _) = self.fl_n(self.get(n, F::ElementType));
                let mut v = self.between_tok(label, ef);
                v.push(ef);
                self.set_offsets(&v, 1, label);
            }
            TSRestType => {
                let first = self.first(n);
                let next = self.after_t(first);
                self.set_offset(next, 1, first);
            }
            TSNamespaceExportDeclaration => {
                let first = self.first(n);
                let idt = self.first(self.get(n, F::Id));
                let mut v = self.between_tok(first, idt);
                v.push(idt);
                self.set_offsets(&v, 1, first);
            }
            Decorator => self.decorator(n),
            AccessorProperty => {
                self.property_like(n, F::Key);
                self.process_semicolons(n);
            }
            ImportAttribute => {
                let first = self.first(n);
                let (kf, kl) = self.fl_n(self.get(n, F::Key));
                let prefix = self.between_tok(first, kf);
                self.set_offsets(&prefix, 0, first);
                self.set_offset(kf, 0, first);
                let init = self.first(self.get(n, F::Value));
                let mut v = self.between_tok(kl, init);
                v.push(init);
                self.set_offsets(&v, 1, kl);
            }
            _ => {}
        }
        self.after_enter(n, ty);
    }

    /// Selectors more specific than the per-type handlers.
    fn after_enter(&mut self, n: u32, ty: Ty) {
        use Ty::*;
        if matches!(
            ty,
            TSTypeAliasDeclaration
                | TSCallSignatureDeclaration
                | TSConstructSignatureDeclaration
                | TSImportEqualsDeclaration
                | TSAbstractMethodDefinition
                | TSAbstractPropertyDefinition
                | TSAbstractAccessorProperty
                | TSEnumMember
                | TSPropertySignature
                | TSIndexSignature
                | TSMethodSignature
        ) {
            self.process_semicolons(n);
        }
        if matches!(ty, ClassDeclaration | ClassExpression) {
            self.ts_class(n);
        }
        if ty.name().starts_with("TS") {
            self.ts_paren(n);
        }
    }

    fn between_tok(&self, a: u32, b: u32) -> Vec<u32> {
        if a == NONE || b == NONE {
            return Vec::new();
        }
        self.between(self.tr(a), self.tr(b))
    }

    // ---- common handlers ----

    fn paren(&mut self, n: u32) {
        let mut left = self.before_n(n);
        let mut right = self.after_n(n);
        let mut first = self.first(n);
        while left != NONE
            && right != NONE
            && open_paren(self.s, left)
            && close_paren(self.s, right)
        {
            self.set_offset(first, 1, left);
            self.set_offset(right, 0, left);
            first = left;
            left = self.before_t(left);
            right = self.after_t(right);
        }
    }

    fn ts_paren(&mut self, n: u32) {
        if !self.t.ty(n).is_type_node() {
            return;
        }
        self.paren(n);
    }

    fn v_attribute(&mut self, n: u32) {
        let key_t = self.first(n);
        let eq = self.after_n(self.get(n, F::Key));
        if eq == NONE || self.tr(eq).1 > self.r(n).1 {
            return;
        }
        self.set_offset(eq, 1, key_t);
        let v = self.after_t(eq);
        if v != NONE && self.tr(v).1 <= self.r(n).1 {
            self.set_offset(v, 1, key_t);
        }
    }

    fn v_element(&mut self, n: u32) {
        if self.t.has(n, flag::PRE) {
            let st = self.first(n);
            let et = self.first(self.get(n, F::EndTag));
            self.set_offset(et, 0, st);
            self.set_preformatted(n);
        } else {
            let p = self.t.parent(n);
            let top = p == NONE || self.t.ty(p) != Ty::VElement;
            let off = if top { self.o.base_indent } else { 1 };
            let kids: Vec<u32> = self
                .list(n, F::Children)
                .iter()
                .copied()
                .filter(|&c| !(self.t.ty(c) == Ty::VText && self.t.has(c, flag::EMPTY_TEXT)))
                .collect();
            let st = self.get(n, F::StartTag);
            let et = self.get(n, F::EndTag);
            self.pnl(&kids, self.node_r(st), self.node_r(et), off, false);
        }
        if n == self.root {
            self.process_top_level(n, 0);
        }
    }

    fn v_end_tag(&mut self, n: u32) {
        let el = self.t.parent(n);
        let open = self.first(self.get(el, F::StartTag));
        let close = self.last(n);
        if close != NONE
            && matches!(
                self.s.tok(close).kind,
                TokenKind::HTMLTagClose | TokenKind::HTMLSelfClosingTagClose
            )
        {
            self.set_offset(close, self.o.close_end, open);
        }
    }

    fn v_container(&mut self, n: u32) {
        let e = self.get(n, F::Expression);
        if e == NONE || self.r(n).0 == self.r(e).0 {
            return;
        }
        let sq = self.first(n);
        let eq = self.last(n);
        let child = self.after_t(sq);
        self.set_offset(child, 1, sq);
        self.set_offset(eq, 0, sq);
    }

    fn v_filter(&mut self, n: u32) {
        let id = self.first(n);
        let last = self.last(n);
        if !close_paren(self.s, last) || last == NONE {
            return;
        }
        let lp = self.after_n(self.get(n, F::Callee));
        self.set_offset(lp, 1, id);
        let args = self.list(n, F::Arguments);
        self.pnl(args, self.tok_r(lp), self.tok_r(last), 1, true);
    }

    fn v_filter_seq(&mut self, n: u32) {
        let fs = self.list(n, F::Filters);
        if fs.is_empty() {
            return;
        }
        let first = self.first(n);
        let mut ts = Vec::new();
        for &f in fs {
            ts.push(self.before_nf(f, &|s, t| is_p(s, t, "|")));
            ts.push(self.first(f));
        }
        self.set_offsets(&ts, 1, first);
    }

    fn v_for(&mut self, n: u32) {
        let first = self.first(n);
        let left = self.list(n, F::Left);
        let last_of_left = left.last().copied().unwrap_or(NONE);
        let lol: R = if last_of_left != NONE {
            self.r(last_of_left)
        } else if first != NONE {
            self.tr(first)
        } else {
            return;
        };
        let in_t = self.s.after_f(lol, false, &not_close_paren);
        let right_t = self.first(self.get(n, F::Right));
        if open_paren(self.s, first) && first != NONE {
            let rp = self.s.after_f(lol, false, &close_paren);
            self.pnl(left, self.tok_r(first), self.tok_r(rp), 1, true);
        }
        self.set_offset(in_t, 1, first);
        self.set_offset(right_t, 1, in_t);
    }

    fn v_start_tag(&mut self, n: u32) {
        let open = self.first(n);
        let close = self.last(n);
        let attrs = self.list(n, F::Attributes);
        let (a, al) = (self.o.attribute, self.o.align_attrs);
        self.pnl(attrs, self.tok_r(open), None, a, al);
        if close == NONE {
            return;
        }
        let off = match self.s.tok(close).kind {
            TokenKind::HTMLSelfClosingTagClose => self.o.close_self,
            TokenKind::HTMLTagClose => self.o.close_start,
            _ => return,
        };
        self.set_offset(close, off, open);
    }

    fn v_text(&mut self, n: u32) {
        let tokens = self.s.tokens_f(self.r(n), false, &not_ws);
        let fi = self.info(self.first(n));
        for t in tokens {
            let slot = self.infos.len() as u32;
            self.infos.push(fi.unwrap_or(Off {
                base: NONE,
                offset: 0,
                baseline: false,
                expected: None,
            }));
            self.offs[t as usize] = slot;
        }
    }

    fn arrow(&mut self, n: u32) {
        let first = self.first(n);
        let second = self.after_t(first);
        let is_async = self.t.has(n, flag::ASYNC);
        let left = if is_async { second } else { first };
        let body = self.get(n, F::Body);
        let arrow = self.before_nf(body, &|s, t| is_p(s, t, "=>"));
        if is_async {
            self.set_offset(second, 1, first);
        }
        if open_paren(self.s, left) && left != NONE {
            let ps = self.list(n, F::Params);
            let rp = match ps.last() {
                Some(&l) => self.after_nf(l, &close_paren),
                None => self.after_tf(left, &close_paren),
            };
            self.pnl(ps, self.tok_r(left), self.tok_r(rp), 1, true);
        }
        self.set_offset(arrow, 1, first);
        self.process_maybe_block(body, first);
    }

    fn binary_like(&mut self, n: u32) {
        let left_t = self.chain_head(n);
        let op = self.after_nf(self.get(n, F::Left), &not_close_paren);
        let right_t = self.after_t(op);
        let prev = self.before_t(left_t);
        let should_indent = prev == NONE
            || left_t == NONE
            || self.tline_e(prev) == self.tline_s(left_t)
            || self.is_beginning_of_element(left_t, n);
        self.set_offsets(&[op, right_t], if should_indent { 1 } else { 0 }, left_t);
    }

    fn call(&mut self, n: u32) {
        let ta = self.get(n, F::TypeArguments);
        let first = self.first(n);
        let right = self.last(n);
        let anchor = if ta != NONE {
            ta
        } else {
            self.get(n, F::Callee)
        };
        let left = self.after_nf(anchor, &open_paren);
        if ta != NONE {
            let f = self.first(ta);
            self.set_offset(f, 1, first);
        }
        let al = self.last(anchor);
        if al != NONE && left != NONE {
            for t in self
                .s
                .between_f(self.tr(al), self.tr(left), false, &optional_tok)
            {
                self.set_offset(t, 1, first);
            }
        }
        self.set_offset(left, 1, first);
        let args = self.list(n, F::Arguments);
        self.pnl(args, self.tok_r(left), self.tok_r(right), 1, true);
    }

    fn class(&mut self, n: u32) {
        let first = self.first(n);
        let body = self.first(self.get(n, F::Body));
        let id = self.get(n, F::Id);
        if id != NONE {
            let f = self.first(id);
            self.set_offset(f, 1, first);
        }
        let sc = self.get(n, F::SuperClass);
        if sc != NONE {
            let ext = self.before_nf(sc, &|s, t| {
                s.tok(t).kind == TokenKind::Keyword && s.text(t) == "extends"
            });
            let sct = self.after_t(ext);
            self.set_offset(ext, 1, first);
            self.set_offset(sct, 1, ext);
        }
        self.set_offset(body, 0, first);
    }

    fn ts_class(&mut self, n: u32) {
        let tp = self.get(n, F::TypeParameters);
        if tp != NONE {
            let id = self.get(n, F::Id);
            let base = self.first(if id != NONE { id } else { n });
            let f = self.first(tp);
            self.set_offset(f, 1, base);
        }
        let sta = self.get(n, F::SuperTypeArguments);
        let sc = self.get(n, F::SuperClass);
        if sta != NONE && sc != NONE {
            let f = self.first(sta);
            let b = self.first(sc);
            self.set_offset(f, 1, b);
        }
        let imps = self.list(n, F::Implements);
        if imps.is_empty() {
            return;
        }
        let ct = self.first(n);
        let it = self.before_n(imps[0]);
        self.set_offset(it, 1, ct);
        self.pnl(imps, self.tok_r(it), None, 1, true);
    }

    fn conditional(&mut self, n: u32) {
        let prev = self.before_n(n);
        let first = self.first(n);
        let test = self.get(n, F::Test);
        let cons = self.get(n, F::Consequent);
        let q = self.after_nf(test, &not_close_paren);
        let ct = self.after_t(q);
        let colon = self.after_nf(cons, &not_close_paren);
        let at = self.after_t(colon);
        let is_flat = prev != NONE
            && self.tline_e(prev) != self.line_s(self.r(n).0)
            && self.line_s(self.r(test).1) == self.line_s(self.r(cons).0);
        if is_flat {
            self.set_offsets(&[q, ct, colon, at], 0, first);
        } else {
            self.set_offsets(&[q, colon], 1, first);
            self.set_offsets(&[ct, at], 1, q);
        }
    }

    fn assertions(&mut self, n: u32, src: u32, kw: u32) {
        let last = self.s.last_token_f(self.r(n), &not_semicolon);
        if last == NONE || src == NONE {
            return;
        }
        let mut at = self.between(self.r(src), self.tr(last));
        if at.is_empty() {
            return;
        }
        let assert_t = at.remove(0);
        self.set_offset(assert_t, 0, kw);
        if at.is_empty() {
            return;
        }
        let open = at.remove(0);
        self.set_offset(open, 1, assert_t);
        let items: Vec<Option<R>> = at.iter().map(|&t| Some(self.tr(t))).collect();
        self.pnl_r(&items, self.tok_r(open), self.tok_r(last), 1, true);
    }

    fn export_all(&mut self, n: u32) {
        let et = self.first(n);
        let src = self.get(n, F::Source);
        let mut tokens = if src != NONE {
            self.between(self.tr(et), self.r(src))
        } else {
            Vec::new()
        };
        tokens.push(self.first(src));
        if self.get(n, F::Exported) != NONE {
            let star = tokens
                .iter()
                .copied()
                .find(|&t| self.p(t, "*"))
                .unwrap_or(NONE);
            let as_t = self.after_t(star);
            let exported = self.after_t(as_t);
            let idx = tokens.iter().position(|&t| t == exported);
            let after: Vec<u32> = match idx {
                Some(i) => tokens[i + 1..].to_vec(),
                None => tokens.clone(),
            };
            self.set_offset(star, 1, et);
            self.set_offset(as_t, 1, star);
            self.set_offset(exported, 1, star);
            self.set_offsets(&after, 1, et);
        } else {
            self.set_offsets(&tokens, 1, et);
        }
        self.assertions(n, src, et);
    }

    fn export_named(&mut self, n: u32) {
        let et = self.first(n);
        if self.get(n, F::Declaration) != NONE {
            let dt = self
                .s
                .fwd(self.r(n).0 as i64, self.r(n).1 as i64, false)
                .nth(1)
                .unwrap_or(NONE);
            self.set_offset(dt, 1, et);
            return;
        }
        let specs = self.list(n, F::Specifiers);
        let first_spec = specs.first().copied().unwrap_or(NONE);
        let left_braces = if first_spec != NONE {
            self.between(self.tr(et), self.r(first_spec))
        } else {
            vec![self.after_t(et)]
        };
        let src = self.get(n, F::Source);
        let rb = if src != NONE {
            self.before_nf(src, &close_brace)
        } else {
            self.s.last_token_f(self.r(n), &close_brace)
        };
        self.set_offsets(&left_braces, 0, et);
        let lb_last = left_braces.last().copied().unwrap_or(NONE);
        self.pnl(specs, self.tok_r(lb_last), self.tok_r(rb), 1, true);
        if src != NONE {
            let mut ts = if rb != NONE {
                self.between(self.tr(rb), self.r(src))
            } else {
                Vec::new()
            };
            ts.push(self.first(src));
            self.set_offsets(&ts, 1, et);
            self.assertions(n, src, et);
        }
    }

    fn for_in(&mut self, n: u32) {
        let for_t = self.first(n);
        let await_t = if self.t.ty(n) == Ty::ForOfStatement && self.t.has(n, flag::AWAIT) {
            self.after_t(for_t)
        } else {
            NONE
        };
        let lp = self.after_t(if await_t != NONE { await_t } else { for_t });
        let left_t = self.after_t(lp);
        let in_t = self.after_tf(left_t, &not_close_paren);
        let right_t = self.after_t(in_t);
        let body = self.get(n, F::Body);
        let rp = self.before_nf(body, &not_open_paren);
        if await_t != NONE {
            self.set_offset(await_t, 0, for_t);
        }
        self.set_offset(lp, 1, for_t);
        self.set_offset(left_t, 1, lp);
        self.set_offset(in_t, 1, left_t);
        self.set_offset(right_t, 1, left_t);
        self.set_offset(rp, 0, lp);
        self.process_maybe_block(body, for_t);
    }

    fn function(&mut self, n: u32) {
        let first = self.first(n);
        let (mut lp, body_base);
        if open_paren(self.s, first) && first != NONE {
            lp = first;
            body_base = self.first(self.t.parent(n));
        } else {
            let mut next = self.after_t(first);
            let mut off = 0;
            let id = self.get(n, F::Id);
            let id_start = if id != NONE { Some(self.r(id).0) } else { None };
            while next != NONE && !open_paren(self.s, next) && self.text(next) != "<" {
                if self.text(next) == "*" || Some(self.tr(next).0) == id_start {
                    off = 1;
                }
                self.set_offset(next, off, first);
                next = self.after_t(next);
            }
            lp = next;
            body_base = first;
        }
        let tp = self.get(n, F::TypeParameters);
        if !open_paren(self.s, lp) && tp != NONE {
            lp = self.after_n(tp);
        }
        let ps = self.list(n, F::Params);
        let rp = match ps.last() {
            Some(&l) => self.after_nf(l, &close_paren),
            None => self.after_tf(lp, &close_paren),
        };
        self.set_offset(lp, 1, body_base);
        self.pnl(ps, self.tok_r(lp), self.tok_r(rp), 1, true);
        let bt = self.first(self.get(n, F::Body));
        self.set_offset(bt, 0, body_base);
    }

    fn declare_function(&mut self, n: u32) {
        let first = self.first(n);
        let (mut lp, body_base);
        if first != NONE && self.s.tok(first).kind == TokenKind::Punctuator {
            lp = first;
            body_base = self.first(self.t.parent(n));
        } else {
            let mut next = self.after_t(first);
            let mut off = 0;
            let id = self.get(n, F::Id);
            let id_start = if id != NONE { Some(self.r(id).0) } else { None };
            while next != NONE && !open_paren(self.s, next) && self.text(next) != "<" {
                if self.text(next) == "*" || Some(self.tr(next).0) == id_start {
                    off = 1;
                }
                self.set_offset(next, off, first);
                next = self.after_t(next);
            }
            lp = next;
            body_base = first;
        }
        let tp = self.get(n, F::TypeParameters);
        if !open_paren(self.s, lp) && tp != NONE {
            lp = self.after_n(tp);
        }
        let ps = self.list(n, F::Params);
        let rp = match ps.last() {
            Some(&l) => self.after_nf(l, &close_paren),
            None => self.after_tf(lp, &close_paren),
        };
        self.set_offset(lp, 1, body_base);
        self.pnl(ps, self.tok_r(lp), self.tok_r(rp), 1, true);
    }

    fn import(&mut self, n: u32) {
        let it = self.first(n);
        let src = self.get(n, F::Source);
        let tokens = if src != NONE {
            self.between(self.tr(it), self.r(src))
        } else {
            Vec::new()
        };
        let from_idx = tokens.iter().rposition(|&t| self.text(t) == "from");
        let (from_t, mut before, after) = match from_idx {
            None => {
                let mut b = tokens.clone();
                b.push(self.first(src));
                (NONE, b, Vec::new())
            }
            Some(i) => {
                let mut a = tokens[i + 1..].to_vec();
                a.push(self.first(src));
                (tokens[i], tokens[..i].to_vec(), a)
            }
        };
        let mut named = Vec::new();
        for &sp in self.list(n, F::Specifiers) {
            if self.t.ty(sp) == Ty::ImportSpecifier {
                named.push(sp);
            } else {
                let mut rm = self.s.tokens(self.r(sp));
                if !rm.is_empty() {
                    rm.remove(0);
                }
                for t in rm {
                    if let Some(i) = before.iter().position(|&b| b == t) {
                        before.remove(i);
                    }
                }
            }
        }
        if let Some(&last_named) = named.last() {
            let lb = self.before_n(named[0]);
            let rb = self.after_nf(last_named, &close_brace);
            self.pnl(&named, self.tok_r(lb), self.tok_r(rb), 1, true);
            let mut rm = self.between_tok(lb, rb);
            rm.push(rb);
            for t in rm {
                if let Some(i) = before.iter().position(|&b| b == t) {
                    before.remove(i);
                }
            }
        }
        let all_braces = before.iter().all(|&t| self.p(t, "{") || self.p(t, "}"));
        self.set_offsets(&before, if all_braces { 0 } else { 1 }, it);
        if from_t != NONE {
            self.set_offset(from_t, 1, it);
            self.set_offsets(&after, 0, from_t);
        }
        self.assertions(n, src, it);
    }

    fn member(&mut self, n: u32) {
        let obj = self.first(n);
        if self.t.ty(n) == Ty::MemberExpression && self.t.has(n, flag::COMPUTED) {
            let prop = self.get(n, F::Property);
            let lb = self.before_nf(prop, &open_bracket);
            let pt = self.after_t(lb);
            let rb = self.after_nf(prop, &close_bracket);
            let ol = self.last(self.get(n, F::Object));
            if ol != NONE && lb != NONE {
                for t in self
                    .s
                    .between_f(self.tr(ol), self.tr(lb), false, &optional_tok)
                {
                    self.set_offset(t, 1, obj);
                }
            }
            self.set_offset(lb, 1, obj);
            self.set_offset(pt, 1, lb);
            self.set_offset(rb, 0, lb);
        } else {
            // MetaProperty: `property` is the last token.
            let prop_t = if self.t.ty(n) == Ty::MetaProperty {
                self.last(n)
            } else {
                self.first(self.get(n, F::Property))
            };
            let dot = self.before_t(prop_t);
            let pt = self.after_t(dot);
            self.set_offsets(&[dot, pt], 1, obj);
        }
    }

    /// `MethodDefinition, Property, PropertyDefinition` and the TS abstract / enum member variant.
    fn property_like(&mut self, n: u32, key_f: F) {
        let first = self.first(n);
        let key = self.get(n, key_f);
        let (kf, kl) = self.fl_n(key);
        let mut prefix = self.between_tok(first, kf);
        let computed = self.t.has(n, flag::COMPUTED);
        if computed {
            prefix.pop();
        }
        self.set_offsets(&prefix, 0, first);
        let last_key;
        if computed {
            let lb = self.before_t(kf);
            let rb = self.after_t(kl);
            last_key = rb;
            self.set_offset(lb, 0, first);
            self.pnl(&[key], self.tok_r(lb), self.tok_r(rb), 1, true);
        } else {
            self.set_offset(kf, 0, first);
            last_key = kl;
        }
        let vf = if key_f == F::Id {
            F::Initializer
        } else {
            F::Value
        };
        let value = self.get(n, vf);
        if value == NONE {
            return;
        }
        let init = self.first(value);
        let mut v = self.between_tok(last_key, init);
        v.push(init);
        self.set_offsets(&v, 1, last_key);
    }

    fn new_expr(&mut self, n: u32) {
        let ta = self.get(n, F::TypeArguments);
        let nt = self.first(n);
        let callee_t = self.after_t(nt);
        let right = self.last(n);
        let left = if close_paren(self.s, right) && right != NONE {
            let anchor = if ta != NONE {
                ta
            } else {
                self.get(n, F::Callee)
            };
            self.s
                .first_between_f(self.r(anchor), self.tr(right), &open_paren)
        } else {
            NONE
        };
        if ta != NONE {
            let f = self.first(ta);
            self.set_offset(f, 1, callee_t);
        }
        self.set_offset(callee_t, 1, nt);
        if left == NONE {
            return;
        }
        self.set_offset(left, 1, callee_t);
        let args = self.list(n, F::Arguments);
        self.pnl(args, self.tok_r(left), self.tok_r(right), 1, true);
    }

    // ---- TypeScript handlers ----

    fn ts_is_beginning_of_element(&self, token: u32, belonging: u32) -> bool {
        let ts = self.tr(token).0;
        let mut n = belonging;
        while n != NONE && self.t.parent(n) != NONE {
            let p = self.t.parent(n);
            match self.t.ty(p) {
                Ty::TSTypeParameterInstantiation => {
                    let ps = self.list(p, F::Params);
                    return ps.len() >= 2
                        && ps.iter().any(|&x| {
                            let f = self.fl_n(x).0;
                            f != NONE && self.tr(f).0 == ts
                        });
                }
                Ty::TSTupleType => {
                    return self.list(p, F::ElementTypes).iter().any(|&x| {
                        x != NONE && {
                            let f = self.fl_n(x).0;
                            f != NONE && self.tr(f).0 == ts
                        }
                    });
                }
                _ => {}
            }
            n = p;
        }
        false
    }

    fn union(&mut self, n: u32) {
        let first = self.first(n);
        if first == NONE {
            return;
        }
        let prev = self.before_t(first);
        let should_indent = prev == NONE
            || self.tline_e(prev) == self.tline_s(first)
            || self.ts_is_beginning_of_element(first, n);
        let off = if should_indent { 1 } else { 0 };
        let mut list: Vec<(u32, u32)> = self
            .list(n, F::Types)
            .iter()
            .map(|&t| self.fl_n(t))
            .collect();
        if list.is_empty() {
            return;
        }
        let head = list.remove(0);
        let last = if head.0 == first {
            head.1
        } else {
            list.insert(0, head);
            first
        };
        // `lastToken` is never advanced upstream.
        for (tf, _) in list {
            let b = self.between_tok(last, tf);
            self.set_offsets(&b, off, first);
            self.set_offset(tf, off, first);
        }
    }

    fn mapped(&mut self, n: u32) {
        let lbrace = self.first(n);
        let key = self.get(n, F::Key);
        let lbracket = self.before_n(key);
        let name_type = self.get(n, F::NameType);
        let constraint = self.get(n, F::Constraint);
        let anchor = if name_type != NONE {
            name_type
        } else if constraint != NONE {
            constraint
        } else {
            key
        };
        let rbracket = self.after_n(anchor);
        let mut v = self.between_tok(lbrace, lbracket);
        v.push(lbracket);
        self.set_offsets(&v, 1, lbrace);
        self.pnl(
            &[key, name_type],
            self.tok_r(lbracket),
            self.tok_r(rbracket),
            1,
            true,
        );
        if constraint != NONE {
            let mut v = self.between(self.r(key), self.r(constraint));
            v.push(self.first(constraint));
            let kt = self.first(key);
            self.set_offsets(&v, 1, kt);
        }
        let rbrace = self.last(n);
        let ta = self.get(n, F::TypeAnnotation);
        if ta != NONE {
            let tat = self.first(ta);
            let mut v = self.between_tok(rbracket, tat);
            v.push(tat);
            self.set_offsets(&v, 1, lbrace);
        } else {
            let v = self.between_tok(rbracket, rbrace);
            self.set_offsets(&v, 1, lbrace);
        }
        self.set_offset(rbrace, 0, lbrace);
    }

    fn type_param(&mut self, n: u32) {
        let mut tokens = self.s.tokens(self.r(n));
        if tokens.is_empty() {
            return;
        }
        let first = tokens.remove(0);
        let mut after = tokens;
        for f in [F::Constraint, F::Default] {
            let c = self.get(n, f);
            if c == NONE {
                continue;
            }
            let mut rm = self.s.tokens(self.r(c));
            if !rm.is_empty() {
                rm.remove(0);
            }
            for t in rm {
                if let Some(i) = after.iter().position(|&a| a == t) {
                    after.remove(i);
                }
            }
        }
        if after.is_empty() {
            return;
        }
        let second = after.remove(0);
        self.set_offset(second, 1, first);
        if self.text(second) == "extends" {
            let mut prev = NONE;
            let mut it = after.into_iter();
            let mut tok = it.next();
            while let Some(t) = tok {
                if self.text(t) == "=" {
                    break;
                }
                self.set_offset(t, 1, second);
                prev = t;
                tok = it.next();
            }
            while let Some(t) = tok {
                self.set_offset(t, 1, if prev != NONE { prev } else { second });
                tok = it.next();
            }
        } else {
            self.set_offsets(&after, 1, first);
        }
    }

    fn conditional_type(&mut self, n: u32) {
        let check = self.first(n);
        let ext = self.after_n(self.get(n, F::CheckType));
        let ext_type = self.first(self.get(n, F::ExtendsType));
        self.set_offset(ext, 1, check);
        self.set_offset(ext_type, 1, ext);
        let q = self.after_nf(self.get(n, F::ExtendsType), &not_close_paren);
        let cons = self.after_t(q);
        let colon = self.after_nf(self.get(n, F::TrueType), &not_close_paren);
        let alt = self.after_t(colon);
        let mut base = n;
        let mut p = self.t.parent(base);
        while p != NONE
            && self.t.ty(p) == Ty::TSConditionalType
            && self.get(p, F::FalseType) == base
        {
            base = p;
            p = self.t.parent(base);
        }
        let bt = self.first(base);
        self.set_offsets(&[q, colon], 1, bt);
        self.set_offset(cons, 1, q);
        self.set_offset(alt, 1, colon);
    }

    fn import_type(&mut self, n: u32) {
        let first = self.first(n);
        let lp = self.after_tf(first, &open_paren);
        self.set_offset(lp, 1, first);
        let mut args = vec![self.get(n, F::Argument)];
        let opts = self.get(n, F::Options);
        if opts != NONE {
            args.push(opts);
        }
        let rp = self.after_nf(*args.last().unwrap_or(&NONE), &close_paren);
        self.pnl(&args, self.tok_r(lp), self.tok_r(rp), 1, true);
        let q = self.get(n, F::Qualifier);
        if q != NONE {
            let dot = self.before_n(q);
            let pt = self.after_t(dot);
            self.set_offsets(&[dot, pt], 1, first);
        }
        let ta = self.get(n, F::TypeArguments);
        if ta != NONE {
            let f = self.first(ta);
            self.set_offset(f, 1, first);
        }
    }

    fn decorator(&mut self, n: u32) {
        let ft = self.s.first_tokens(self.r(n), 2);
        let at = ft.first().copied().unwrap_or(NONE);
        let second = ft.get(1).copied().unwrap_or(NONE);
        self.set_offset(second, 0, at);
        let parent = self.t.parent(n);
        if parent == NONE {
            return;
        }
        let decos = self.list(parent, F::Decorators);
        if decos.is_empty() {
            return;
        }
        if decos[0] == n {
            if self.r(parent).0 == self.r(n).0 {
                let sp = self.after_n(*decos.last().unwrap_or(&NONE));
                self.set_offset(sp, 0, at);
            } else {
                let gp = self.t.parent(parent);
                let target = if gp != NONE
                    && matches!(
                        self.t.ty(gp),
                        Ty::ExportDefaultDeclaration | Ty::ExportNamedDeclaration
                    )
                    && self.r(n).0 < self.r(gp).0
                {
                    gp
                } else {
                    parent
                };
                let sp = self.first(target);
                self.copy_offset(at, sp);
            }
        } else {
            let f = self.first(decos[0]);
            self.set_offset(at, 0, f);
        }
    }

    // ------------------------------------------------------------------
    // Validation
    // ------------------------------------------------------------------

    fn is_comment(&self, t: u32) -> bool {
        self.s.tok(t).comment
    }

    fn expected_indents(&self, tokens: &[u32]) -> Option<(i32, i32)> {
        let mut v: Vec<i32> = Vec::new();
        for (i, &t) in tokens.iter().enumerate() {
            let Some(info) = self.info(t) else { continue };
            match info.expected {
                None => {
                    if let Some(b) = self.info(info.base)
                        && let Some(be) = b.expected
                        && (i == 0 || !b.baseline)
                    {
                        v.push(be + info.offset * self.o.indent_size);
                        if b.baseline {
                            break;
                        }
                    }
                }
                Some(e) => v.push(e),
            }
        }
        if v.is_empty() {
            return None;
        }
        let min = *v.iter().min().unwrap_or(&v[0]);
        Some((v[0], min))
    }

    fn indent_text(&self, t: u32) -> &'a str {
        let start = self.s.tok(t).start as usize;
        let src = self.s.src;
        let before = &src[..start];
        let i = before
            .char_indices()
            .rev()
            .find(|&(_, c)| matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}'))
            .map_or(0, |(i, c)| i + c.len_utf8());
        &src[i..start]
    }

    fn validate_core(&mut self, t: u32, expected: i32, optional: Option<&[i32]>) {
        let indent = self.indent_text(t);
        if !js_trim(indent).is_empty() {
            return;
        }
        let line = self.tline_s(t) as u32 + 1;
        let actual = self.col(t);
        let ic = self.o.indent_char;
        for (i, ch) in indent.chars().enumerate() {
            if ch != ic {
                let msg = format!(
                    "Expected {} character, but found {} character.",
                    serde_json::to_string(&ic.to_string()).unwrap_or_default(),
                    serde_json::to_string(&ch.to_string()).unwrap_or_default()
                );
                let fix = self.define_fix(t, indent, expected);
                self.out.push(Report {
                    start: self.s.tok(t).start - indent.len() as u32,
                    end: self.s.tok(t).start,
                    loc: [line, i as u32 + 1, line, i as u32 + 2],
                    message: msg,
                    fix,
                });
                return;
            }
        }
        if actual != expected && !optional.is_some_and(|o| o.contains(&actual)) {
            let unit = if ic == '\t' { "tab" } else { "space" };
            let msg = format!(
                "Expected indentation of {expected} {unit}{} but found {actual} {unit}{}.",
                if expected == 1 { "" } else { "s" },
                if actual == 1 { "" } else { "s" },
            );
            let fix = self.define_fix(t, indent, expected);
            self.out.push(Report {
                start: self.s.tok(t).start - indent.len() as u32,
                end: self.s.tok(t).start,
                loc: [line, 1, line, actual as u32 + 1],
                message: msg,
                fix,
            });
        }
    }

    fn define_fix(&self, t: u32, indent: &str, expected: i32) -> (u32, u32, String) {
        let tok = self.s.tok(t);
        let start = tok.start - indent.len() as u32;
        let ind: String =
            std::iter::repeat_n(self.o.indent_char, expected.max(0) as usize).collect();
        if tok.kind == TokenKind::BlockComment && self.tline_s(t) != self.tline_e(t) {
            let text = self.s.text(t);
            let mut lines = split_lines_keep(text);
            if !lines.is_empty() {
                let first = lines.remove(0);
                if lines.iter().all(|l| block_prefix_len(l).is_some()) {
                    let mut out = String::new();
                    out.push_str(&ind);
                    out.push_str(first);
                    for l in lines {
                        let p = block_prefix_len(l).unwrap_or(0);
                        out.push_str(&ind);
                        out.push_str(" *");
                        out.push_str(&l[p..]);
                    }
                    return (start, tok.end, out);
                }
            }
        }
        (start, tok.start, ind)
    }

    fn comment_expected(&self, next: u32, next_exp: i32, last_exp: Option<i32>) -> Vec<i32> {
        if let Some(le) = last_exp
            && self.is_closing(next)
        {
            let indent = if next_exp == le {
                next_exp + self.o.indent_size
            } else {
                le
            };
            return vec![indent, next_exp];
        }
        vec![next_exp]
    }

    fn is_closing(&self, t: u32) -> bool {
        match self.s.tok(t).kind {
            TokenKind::HTMLEndTagOpen | TokenKind::VExpressionEnd => true,
            TokenKind::Punctuator => matches!(self.s.text(t), ")" | "}" | "]"),
            _ => false,
        }
    }

    fn validate(&mut self, tokens: &[u32], comments: &[u32], last_token: u32) {
        let first = tokens[0];
        let actual = self.col(first);
        let Some((expected, base_exp)) = self.expected_indents(tokens) else {
            return;
        };
        let mut baseline: Vec<u32> = Vec::new();
        for &t in tokens {
            let slot = self.offs[t as usize];
            if slot == NONE {
                continue;
            }
            let info = self.infos[slot as usize];
            if info.baseline {
                let e = if self.o.indent_char == ' ' {
                    (self.col(t) + base_exp - actual).max(0)
                } else {
                    base_exp + if t == first { 0 } else { 1 }
                };
                self.infos[slot as usize].expected = Some(e);
                baseline.push(t);
            } else if info.base != NONE && baseline.contains(&info.base) {
                let be = self.info(info.base).and_then(|b| b.expected);
                self.infos[slot as usize].expected = be;
                baseline.push(t);
            } else {
                self.infos[slot as usize].expected = Some(base_exp);
            }
        }
        if self.ignored[first as usize] {
            return;
        }
        let last_exp = self.info(last_token).and_then(|i| i.expected);
        let opt = self.comment_expected(first, expected, last_exp);
        for &c in comments {
            let ce = self.expected_indents(&[c]).map_or(opt[0], |e| e.0);
            self.validate_core(c, ce, Some(&opt));
        }
        self.validate_core(first, expected, None);
    }

    fn validate_all(&mut self, root: u32) {
        let toks = self.s.tokens_f(self.r(root), true, &not_ws);
        let mut comments: Vec<u32> = Vec::new();
        let mut same: Vec<u32> = Vec::new();
        let mut beside_multiline = false;
        let mut last_validated = NONE;
        for t in toks {
            let line = self.tline_s(t);
            if same.is_empty() || self.tline_s(same[0]) == line {
                same.push(t);
            } else if same.iter().all(|&x| self.is_comment(x)) {
                comments.push(same[0]);
                beside_multiline = self.tline_e(*same.last().unwrap_or(&t)) == line;
                same = vec![t];
            } else {
                if !beside_multiline {
                    let s = std::mem::take(&mut same);
                    self.validate(&s, &comments, last_validated);
                    last_validated = s[0];
                    same = s;
                }
                beside_multiline = self.tline_e(*same.last().unwrap_or(&t)) == line;
                same = vec![t];
                comments.clear();
            }
        }
        if same.iter().any(|&x| !self.is_comment(x)) {
            let s = std::mem::take(&mut same);
            self.validate(&s, &comments, last_validated);
        }
    }
}

/// `text.match(/[^\n\r  ]+(?:$|\r\n|[\n\r  ])/gu)`.
fn split_lines_keep(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let b = text.as_bytes();
    let mut i = 0;
    let is_lt = |c: char| matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}');
    while i < text.len() {
        let rest = &text[i..];
        let c = rest.chars().next().unwrap_or('\n');
        if is_lt(c) {
            i += c.len_utf8();
            continue;
        }
        let len = rest.find(is_lt).unwrap_or(rest.len());
        let mut end = i + len;
        if end < text.len() {
            if b[end] == b'\r' && b.get(end + 1) == Some(&b'\n') {
                end += 2;
            } else {
                end += text[end..].chars().next().map_or(1, char::len_utf8);
            }
        }
        out.push(&text[i..end]);
        i = end;
    }
    out
}

/// Length of a `/^\s*\*/` prefix.
fn block_prefix_len(l: &str) -> Option<usize> {
    let ws = l.len() - l.trim_start_matches(is_js_ws).len();
    l[ws..].starts_with('*').then_some(ws + 1)
}

impl Selector {
    pub fn matches(&self, t: &Tree, n: u32) -> bool {
        let Some((last, rest)) = self.parts.split_last() else {
            return false;
        };
        if !last.1.iter().all(|s| s.matches(t, n)) {
            return false;
        }
        let mut cur = n;
        let mut combinator = last.0;
        for part in rest.iter().rev() {
            match combinator {
                super::Comb::Child => {
                    cur = t.parent(cur);
                    if cur == NONE || !part.1.iter().all(|s| s.matches(t, cur)) {
                        return false;
                    }
                }
                super::Comb::Descendant => loop {
                    cur = t.parent(cur);
                    if cur == NONE {
                        return false;
                    }
                    if part.1.iter().all(|s| s.matches(t, cur)) {
                        break;
                    }
                },
            }
            combinator = part.0;
        }
        true
    }
}

impl super::Simple {
    fn matches(&self, t: &Tree, n: u32) -> bool {
        match self {
            super::Simple::Any => true,
            super::Simple::Type(name) => t.ty(n).name() == name,
            super::Simple::Attr(name, value) => match (name.as_str(), value.as_deref()) {
                ("computed", Some(v)) => t.has(n, flag::COMPUTED) == (v == "true"),
                ("computed", None) => t.has(n, flag::COMPUTED),
                ("type", Some(v)) => t.ty(n).name() == v,
                _ => false,
            },
            super::Simple::Field(f) => t.field_of(n) == Some(*f),
            super::Simple::Never => false,
        }
    }
}
