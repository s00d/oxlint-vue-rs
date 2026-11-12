//! Port of eslint-plugin-vue `lib/utils/casing.ts`: case checks and converters.
//!
//! Regex semantics follow the JS originals (`\w` / `\B` are ASCII-only there).

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Symbols that make a name "not a case" (without ` `, `$`, `-` and `_`).
fn has_symbols(s: &str) -> bool {
    s.chars()
        .any(|c| "!\"#%&'()*+,./:;<=>?@[\\]^`{|}".contains(c))
}

fn has_upper(s: &str) -> bool {
    s.chars().any(|c| c.is_ascii_uppercase())
}

/// Insert `sep` before every ASCII uppercase letter preceded by a word char (JS `/\B([A-Z])/g`).
fn split_upper(s: &str, sep: char) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    let mut prev: Option<char> = None;
    for c in s.chars() {
        if c.is_ascii_uppercase() && prev.is_some_and(is_word) {
            out.push(sep);
        }
        out.push(c);
        prev = Some(c);
    }
    out
}

/// Uppercase the first character.
pub fn capitalize(s: &str) -> String {
    let mut it = s.chars();
    match it.next() {
        Some(c) => c.to_uppercase().chain(it).collect(),
        None => String::new(),
    }
}

pub fn kebab_case(s: &str) -> String {
    split_upper(&s.replace('_', "-"), '-').to_lowercase()
}

pub fn is_kebab_case(s: &str) -> bool {
    !has_upper(s)
        && !has_symbols(s)
        && !s.starts_with('-')
        && !s.contains('_')
        && !s.contains("--")
        && !s.chars().any(char::is_whitespace)
}

pub fn snake_case(s: &str) -> String {
    split_upper(s, '_').replace('-', "_").to_lowercase()
}

pub fn is_snake_case(s: &str) -> bool {
    !has_upper(s)
        && !has_symbols(s)
        && !s.contains('-')
        && !s.contains("__")
        && !s.chars().any(char::is_whitespace)
}

pub fn camel_case(s: &str) -> String {
    if is_pascal_case(s) {
        let mut it = s.chars();
        return match it.next() {
            Some(c) => c.to_lowercase().chain(it).collect(),
            None => String::new(),
        };
    }
    // `str.replace(/[-_](\w)/gu, (_, c) => c.toUpperCase())`
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if (c == '-' || c == '_') && chars.get(i + 1).is_some_and(|n| is_word(*n)) {
            out.push(chars[i + 1].to_ascii_uppercase());
            i += 2;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

pub fn is_camel_case(s: &str) -> bool {
    !has_symbols(s)
        && !s.starts_with(|c: char| c.is_ascii_uppercase())
        && !s.contains(['-', '_'])
        && !s.chars().any(char::is_whitespace)
}

pub fn pascal_case(s: &str) -> String {
    capitalize(&camel_case(s))
}

pub fn is_pascal_case(s: &str) -> bool {
    !has_symbols(s)
        && !s.starts_with(|c: char| c.is_ascii_lowercase())
        && !s.contains(['-', '_'])
        && !s.chars().any(char::is_whitespace)
}

/// Checker by case name (`camelCase`, `kebab-case`, `PascalCase`, `snake_case`); unknown = PascalCase.
pub fn checker(name: &str) -> fn(&str) -> bool {
    match name {
        "kebab-case" => is_kebab_case,
        "snake_case" => is_snake_case,
        "camelCase" => is_camel_case,
        _ => is_pascal_case,
    }
}

/// Converter by case name; unknown = PascalCase.
pub fn converter(name: &str) -> fn(&str) -> String {
    match name {
        "kebab-case" => kebab_case,
        "snake_case" => snake_case,
        "camelCase" => camel_case,
        _ => pascal_case,
    }
}

/// Converter that returns the input unchanged when the result is still not in the wanted case.
pub fn exact_convert(name: &str, s: &str) -> String {
    let r = converter(name)(s);
    if checker(name)(&r) { r } else { s.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cases() {
        assert_eq!(kebab_case("FooBar_baz"), "foo-bar-baz");
        assert_eq!(camel_case("foo-bar_baz"), "fooBarBaz");
        assert_eq!(pascal_case("foo-bar"), "FooBar");
        assert_eq!(camel_case("FooBar"), "fooBar");
        assert_eq!(snake_case("fooBar-baz"), "foo_bar_baz");
        assert!(is_kebab_case("foo-bar") && !is_kebab_case("-foo"));
        assert!(is_pascal_case("FooBar") && !is_pascal_case("fooBar"));
    }
}
