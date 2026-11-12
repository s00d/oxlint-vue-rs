//! JavaScript regular expressions (ECMAScript semantics via the `regress` engine) and the port of
//! eslint-plugin-vue `lib/utils/regexp.ts` (option strings written as `/pattern/flags`).
//!
//! Rule options come from ESLint configs as JS regexps, so they must behave like `new RegExp(p, flags)`:
//! lookbehind, backreferences, `\p{…}` only with `u`, ASCII `\w`/`\d`, etc.

use std::borrow::Cow;

/// A compiled JS `RegExp` (flags `i`, `m`, `s`, `u`, `v`; `g`/`y`/`d` have no effect on these helpers).
#[derive(Debug, Clone)]
pub struct JsRegex(regress::Regex);

impl JsRegex {
    /// `new RegExp(pattern)`.
    pub fn new(pattern: &str) -> Result<Self, String> {
        Self::with_flags(pattern, "")
    }
    /// `new RegExp(pattern, flags)`.
    pub fn with_flags(pattern: &str, flags: &str) -> Result<Self, String> {
        let flags: String = flags
            .chars()
            .filter(|c| matches!(c, 'i' | 'm' | 's' | 'u' | 'v'))
            .collect();
        regress::Regex::with_flags(pattern, flags.as_str())
            .map(Self)
            .map_err(|e| e.to_string())
    }
    /// `re.test(s)`.
    pub fn is_match(&self, s: &str) -> bool {
        self.0.find(s).is_some()
    }
    /// Text of capture group `i` of the first match (`s.match(re)?.[i]`).
    pub fn capture(&self, s: &str, i: usize) -> Option<String> {
        let m = self.0.find(s)?;
        m.group(i).map(|r| s[r].to_string())
    }
    /// `s.replace(/…/g, with)` with a literal replacement.
    pub fn replace_all<'s>(&self, s: &'s str, with: &str) -> Cow<'s, str> {
        let mut out = String::new();
        let mut last = 0;
        let mut any = false;
        for m in self.0.find_iter(s) {
            any = true;
            let r = m.range();
            out.push_str(&s[last..r.start]);
            out.push_str(with);
            last = r.end;
        }
        if !any {
            return Cow::Borrowed(s);
        }
        out.push_str(&s[last..]);
        Cow::Owned(out)
    }
}

/// Escape regexp syntax characters (`escape` of eslint-plugin-vue `utils/regexp.ts`: `/[\\^$.*+?()[\]{}|]/gu`).
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if "\\^$.*+?()[]{}|".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// `/^\/(.+)\/(.*)$/u` — returns (pattern, flags).
fn split(s: &str) -> Option<(&str, &str)> {
    let rest = s.strip_prefix('/')?;
    let last = rest.rfind('/')?;
    if last == 0 {
        return None;
    }
    Some((&rest[..last], &rest[last + 1..]))
}

/// Whether the string is written as a regexp (`/…/flags`).
pub fn is_regexp(s: &str) -> bool {
    split(s).is_some()
}

/// `toRegExp(s, { remove: 'g' })`: `/…/flags` becomes that regexp, anything else an exact,
/// anchored literal match. `None` when the pattern does not compile.
pub fn to_regexp(s: &str) -> Option<JsRegex> {
    match split(s) {
        Some((pat, flags)) => JsRegex::with_flags(pat, flags).ok(),
        None => JsRegex::new(&format!("^{}$", escape(s))).ok(),
    }
}

/// `toRegExpGroupMatcher(patterns)`: true when any pattern matches any of the given strings.
pub struct GroupMatcher(Vec<JsRegex>);

impl GroupMatcher {
    pub fn new<'s>(patterns: impl IntoIterator<Item = &'s str>) -> Self {
        Self(patterns.into_iter().filter_map(to_regexp).collect())
    }
    pub fn is_match(&self, s: &str) -> bool {
        self.0.iter().any(|r| r.is_match(s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn regexps() {
        assert!(to_regexp("/^foo/i").unwrap().is_match("FOObar"));
        assert!(to_regexp("a.b").unwrap().is_match("a.b"));
        assert!(!to_regexp("a.b").unwrap().is_match("axb"));
        assert!(!is_regexp("//"));
        // JS semantics: lookbehind, backreferences, `\p` needs `u`, `\w` is ASCII.
        assert!(to_regexp("/(?<=a)b/").unwrap().is_match("ab"));
        assert!(to_regexp("/(x)\\1/").unwrap().is_match("xx"));
        assert!(!to_regexp("/\\p{Lu}/").unwrap().is_match("É"));
        assert!(to_regexp("/\\p{Lu}/u").unwrap().is_match("É"));
        assert!(!to_regexp("/^\\w$/").unwrap().is_match("é"));
        assert_eq!(JsRegex::new("a+").unwrap().replace_all("baaca", ""), "bc");
        assert_eq!(
            JsRegex::new("(\\d+)")
                .unwrap()
                .capture("ab12c", 1)
                .as_deref(),
            Some("12")
        );
    }
}
