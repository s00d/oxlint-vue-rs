//! Port of ESLint's `TokenStore` (`lib/languages/js/source-code/token-store`): tokens and comments
//! kept apart, an offset → index map and forward/backward cursors with the exact index semantics
//! (`utils.getFirstIndex` / `getLastIndex`), so node ranges that start or end between tokens
//! behave like in ESLint.

use crate::ast::TokenKind;

pub const NONE: u32 = u32::MAX;

#[derive(Debug, Clone, Copy)]
pub struct Tok {
    pub start: u32,
    pub end: u32,
    pub kind: TokenKind,
    pub comment: bool,
}

/// Token/comment ids are indices into [`Store::all`].
pub struct Store<'a> {
    pub src: &'a str,
    pub all: Vec<Tok>,
    tokens: Vec<u32>,
    comments: Vec<u32>,
    /// `indexMap`: sorted `(offset, token index)`.
    imap: Vec<(u32, u32)>,
}

pub type Filter<'f> = &'f dyn Fn(&Store, u32) -> bool;

impl<'a> Store<'a> {
    /// `items` must be sorted by start; tokens and comments must not overlap.
    pub fn new(src: &'a str, mut all: Vec<Tok>) -> Self {
        all.sort_by_key(|t| (t.start, t.end));
        let mut tokens = Vec::new();
        let mut comments = Vec::new();
        for (i, t) in all.iter().enumerate() {
            if t.comment {
                comments.push(i as u32);
            } else {
                tokens.push(i as u32);
            }
        }
        // createIndexMap: writes in position order, last write wins.
        let mut imap: Vec<(u32, u32)> = Vec::with_capacity(all.len() * 2);
        let mut ti = 0usize;
        for t in &all {
            if t.comment {
                imap.push((t.start, ti as u32));
                imap.push((t.end.wrapping_sub(1), ti as u32));
            } else {
                imap.push((t.start, ti as u32));
                imap.push((t.end.wrapping_sub(1), ti as u32));
                ti += 1;
            }
        }
        // Stable sort keeps write order among equal keys; keep the last one.
        imap.sort_by_key(|e| e.0);
        let mut dedup: Vec<(u32, u32)> = Vec::with_capacity(imap.len());
        for e in imap {
            match dedup.last_mut() {
                Some(l) if l.0 == e.0 => *l = e,
                _ => dedup.push(e),
            }
        }
        Store {
            src,
            all,
            tokens,
            comments,
            imap: dedup,
        }
    }

    pub fn tok(&self, id: u32) -> Tok {
        self.all[id as usize]
    }
    pub fn text(&self, id: u32) -> &'a str {
        let t = self.all[id as usize];
        self.src.get(t.start as usize..t.end as usize).unwrap_or("")
    }
    pub fn len(&self) -> usize {
        self.all.len()
    }
    pub fn is_empty(&self) -> bool {
        self.all.is_empty()
    }

    fn map_get(&self, k: i64) -> Option<u32> {
        if k < 0 || k > u32::MAX as i64 {
            return None;
        }
        let k = k as u32;
        self.imap
            .binary_search_by_key(&k, |e| e.0)
            .ok()
            .map(|i| self.imap[i].1)
    }

    fn tstart(&self, i: usize) -> u32 {
        self.all[self.tokens[i] as usize].start
    }
    fn tend(&self, i: usize) -> u32 {
        self.all[self.tokens[i] as usize].end
    }

    fn first_index(&self, start: i64) -> i64 {
        let n = self.tokens.len() as i64;
        if start == -1 {
            return 0;
        }
        if let Some(i) = self.map_get(start) {
            return i as i64;
        }
        if let Some(i) = self.map_get(start - 1) {
            if i as i64 >= n {
                return n;
            }
            return if self.tstart(i as usize) as i64 >= start {
                i as i64
            } else {
                i as i64 + 1
            };
        }
        if start == 0 { 0 } else { n }
    }

    fn last_index(&self, end: i64) -> i64 {
        let n = self.tokens.len() as i64;
        if end == -1 {
            return n - 1;
        }
        if let Some(i) = self.map_get(end) {
            return i as i64 - 1;
        }
        if let Some(i) = self.map_get(end - 1) {
            if i as i64 >= n {
                return n - 1;
            }
            return if self.tend(i as usize) as i64 > end {
                i as i64 - 1
            } else {
                i as i64
            };
        }
        if end == 0 { -1 } else { n - 1 }
    }

    /// `search(comments, location)`: first comment with start >= location.
    fn comment_search(&self, loc: i64) -> usize {
        self.comments
            .partition_point(|&c| (self.all[c as usize].start as i64) < loc)
    }

    /// Forward cursor over `[start, end]` (`-1` = unbounded).
    pub fn fwd(&self, start: i64, end: i64, comments: bool) -> Fwd<'_, 'a> {
        if comments {
            Fwd {
                s: self,
                ti: self.first_index(start),
                tend: i64::MAX,
                ci: self.comment_search(start) as i64,
                border: end,
                with_comments: true,
            }
        } else {
            Fwd {
                s: self,
                ti: self.first_index(start),
                tend: self.last_index(end),
                ci: 0,
                border: -1,
                with_comments: false,
            }
        }
    }

    /// Backward cursor over `[start, end]` (`-1` = unbounded).
    pub fn bwd(&self, start: i64, end: i64, comments: bool) -> Bwd<'_, 'a> {
        if comments {
            Bwd {
                s: self,
                ti: self.last_index(end),
                tend: i64::MIN,
                ci: self.comment_search(end) as i64 - 1,
                border: start,
                with_comments: true,
            }
        } else {
            Bwd {
                s: self,
                ti: self.last_index(end),
                tend: self.first_index(start),
                ci: -1,
                border: -1,
                with_comments: false,
            }
        }
    }

    // ---- ESLint API (ranges are `(start, end)`; `NONE` results for "null") ----

    pub fn first_token(&self, r: (u32, u32)) -> u32 {
        self.fwd(r.0 as i64, r.1 as i64, false)
            .next()
            .unwrap_or(NONE)
    }
    pub fn first_token_f(&self, r: (u32, u32), comments: bool, f: Filter) -> u32 {
        self.fwd(r.0 as i64, r.1 as i64, comments)
            .find(|&t| f(self, t))
            .unwrap_or(NONE)
    }
    pub fn first_tokens(&self, r: (u32, u32), count: usize) -> Vec<u32> {
        self.fwd(r.0 as i64, r.1 as i64, false)
            .take(count)
            .collect()
    }
    pub fn last_token(&self, r: (u32, u32)) -> u32 {
        self.bwd(r.0 as i64, r.1 as i64, false)
            .next()
            .unwrap_or(NONE)
    }
    pub fn last_token_f(&self, r: (u32, u32), f: Filter) -> u32 {
        self.bwd(r.0 as i64, r.1 as i64, false)
            .find(|&t| f(self, t))
            .unwrap_or(NONE)
    }
    pub fn last_tokens(&self, r: (u32, u32), count: usize) -> Vec<u32> {
        let mut v: Vec<u32> = self
            .bwd(r.0 as i64, r.1 as i64, false)
            .take(count)
            .collect();
        v.reverse();
        v
    }
    pub fn before(&self, r: (u32, u32)) -> u32 {
        self.bwd(-1, r.0 as i64, false).next().unwrap_or(NONE)
    }
    pub fn before_f(&self, r: (u32, u32), f: Filter) -> u32 {
        self.bwd(-1, r.0 as i64, false)
            .find(|&t| f(self, t))
            .unwrap_or(NONE)
    }
    pub fn after(&self, r: (u32, u32)) -> u32 {
        self.fwd(r.1 as i64, -1, false).next().unwrap_or(NONE)
    }
    pub fn after_skip(&self, r: (u32, u32), skip: usize) -> u32 {
        self.fwd(r.1 as i64, -1, false).nth(skip).unwrap_or(NONE)
    }
    pub fn after_f(&self, r: (u32, u32), comments: bool, f: Filter) -> u32 {
        self.fwd(r.1 as i64, -1, comments)
            .find(|&t| f(self, t))
            .unwrap_or(NONE)
    }
    /// `getTokensBetween(left, right)`.
    pub fn between(&self, l: (u32, u32), r: (u32, u32)) -> Vec<u32> {
        self.fwd(l.1 as i64, r.0 as i64, false).collect()
    }
    pub fn between_f(&self, l: (u32, u32), r: (u32, u32), comments: bool, f: Filter) -> Vec<u32> {
        self.fwd(l.1 as i64, r.0 as i64, comments)
            .filter(|&t| f(self, t))
            .collect()
    }
    pub fn first_between_f(&self, l: (u32, u32), r: (u32, u32), f: Filter) -> u32 {
        self.fwd(l.1 as i64, r.0 as i64, false)
            .find(|&t| f(self, t))
            .unwrap_or(NONE)
    }
    /// `getTokens(node)`.
    pub fn tokens(&self, r: (u32, u32)) -> Vec<u32> {
        self.fwd(r.0 as i64, r.1 as i64, false).collect()
    }
    pub fn tokens_f(&self, r: (u32, u32), comments: bool, f: Filter) -> Vec<u32> {
        self.fwd(r.0 as i64, r.1 as i64, comments)
            .filter(|&t| f(self, t))
            .collect()
    }
    pub fn tokens_after_f(&self, r: (u32, u32), comments: bool, f: Filter) -> Vec<u32> {
        self.fwd(r.1 as i64, -1, comments)
            .filter(|&t| f(self, t))
            .collect()
    }
    pub fn range(&self, id: u32) -> (u32, u32) {
        let t = self.all[id as usize];
        (t.start, t.end)
    }
}

pub struct Fwd<'s, 'a> {
    s: &'s Store<'a>,
    ti: i64,
    tend: i64,
    ci: i64,
    border: i64,
    with_comments: bool,
}

impl Iterator for Fwd<'_, '_> {
    type Item = u32;
    fn next(&mut self) -> Option<u32> {
        let s = self.s;
        if !self.with_comments {
            if self.ti <= self.tend && self.ti >= 0 && (self.ti as usize) < s.tokens.len() {
                let t = s.tokens[self.ti as usize];
                self.ti += 1;
                return Some(t);
            }
            return None;
        }
        let tok = (self.ti >= 0 && (self.ti as usize) < s.tokens.len())
            .then(|| s.tokens[self.ti as usize]);
        let com = (self.ci >= 0 && (self.ci as usize) < s.comments.len())
            .then(|| s.comments[self.ci as usize]);
        let cur = match (tok, com) {
            (Some(t), c) if c.is_none_or(|c| s.all[t as usize].start < s.all[c as usize].start) => {
                self.ti += 1;
                t
            }
            (_, Some(c)) => {
                self.ci += 1;
                c
            }
            _ => return None,
        };
        if self.border == -1 || s.all[cur as usize].end as i64 <= self.border {
            Some(cur)
        } else {
            None
        }
    }
}

pub struct Bwd<'s, 'a> {
    s: &'s Store<'a>,
    ti: i64,
    tend: i64,
    ci: i64,
    border: i64,
    with_comments: bool,
}

impl Iterator for Bwd<'_, '_> {
    type Item = u32;
    fn next(&mut self) -> Option<u32> {
        let s = self.s;
        if !self.with_comments {
            if self.ti >= self.tend && self.ti >= 0 && (self.ti as usize) < s.tokens.len() {
                let t = s.tokens[self.ti as usize];
                self.ti -= 1;
                return Some(t);
            }
            return None;
        }
        let tok = (self.ti >= 0 && (self.ti as usize) < s.tokens.len())
            .then(|| s.tokens[self.ti as usize]);
        let com = (self.ci >= 0 && (self.ci as usize) < s.comments.len())
            .then(|| s.comments[self.ci as usize]);
        let cur = match (tok, com) {
            (Some(t), c) if c.is_none_or(|c| s.all[t as usize].end > s.all[c as usize].end) => {
                self.ti -= 1;
                t
            }
            (_, Some(c)) => {
                self.ci -= 1;
                c
            }
            _ => return None,
        };
        if self.border == -1 || s.all[cur as usize].start as i64 >= self.border {
            Some(cur)
        } else {
            None
        }
    }
}
