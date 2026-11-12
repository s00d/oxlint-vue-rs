//! Regression cases from the differential audit against real ESLint (eslint-plugin-vue 10.12.1).
//! Each expectation (report start `line:col`, fix output) was produced by ESLint itself, except the
//! deliberate differences (docs/compatibility.md §5), which say so and are justified by Vue semantics.

use std::path::Path;

use oxlint_vue_rs::{FileOptions, LineIndex, Severity, apply_fixes, configure, lint_source};
use serde_json::{Value, json};

/// Lints `code` (`t.vue`) with one rule; returns sorted `line:col` starts and the fix output.
fn run(rule: &str, opts: Value, globals: Value, code: &str) -> (Vec<String>, Option<String>) {
    let fo = FileOptions {
        globals,
        ..FileOptions::default()
    };
    let opts: Vec<Value> = opts.as_array().cloned().unwrap_or_default();
    let cfg = configure(rule, Severity::Error, &opts, &fo.settings)
        .unwrap()
        .unwrap();
    let rules = [cfg];
    let diags = lint_source(code, Path::new("t.vue"), &fo, &rules);
    let lines = LineIndex::for_source(code, true);
    let mut at: Vec<String> = diags
        .iter()
        .map(|d| {
            let [l, c, ..] = d.location(&lines, code);
            format!("{l}:{c}")
        })
        .collect();
    at.sort();
    (at, apply_fixes(code, &diags))
}

fn reports(rule: &str, opts: Value, globals: Value, code: &str) -> Vec<String> {
    run(rule, opts, globals, code).0
}

#[test]
fn no_parsing_error_only_upstream_codes() {
    let r = |c: &str, o: Value| reports("no-parsing-error", o, Value::Null, c);
    assert!(r("<template><div>{{ </div></template>", json!([])).is_empty());
    assert!(r("<template><div>{{ a }</div></template>", json!([])).is_empty());
    assert_eq!(
        r("<template><div></span></div></template>", json!([])),
        ["1:16"]
    );
    let on = json!([{ "non-void-html-element-start-tag-with-trailing-solidus": true }]);
    assert_eq!(r("<template><div/></template>", on), ["1:11"]);
}

#[test]
fn undeclared_globals_are_not_vue_apis() {
    let g = json!([{ "groups": ["props", "setup"] }]);
    let to_refs = "<script setup>\nconst props = defineProps(['a', 'b'])\nconst { a } = toRefs(props)\n</script>";
    assert!(reports("no-unused-properties", g.clone(), Value::Null, to_refs).is_empty());
    let declared = json!({ "toRefs": "readonly" });
    assert_eq!(
        reports("no-unused-properties", g.clone(), declared, to_refs),
        ["2:33"]
    );
    let opt = "<script>\nexport default { props: ['a','b'], setup(props){ const {a} = toRefs(props) } }\n</script>";
    assert!(reports("no-unused-properties", g, Value::Null, opt).is_empty());

    let undef = "<template>{{ a }} {{ b }}</template>\n<script>\nexport default { setup(){ const state = reactive({a:1}); return {...toRefs(state)} } }\n</script>";
    assert_eq!(
        reports("no-undef-properties", json!([]), Value::Null, undef),
        ["1:14", "1:22"]
    );
    let declared = json!({ "reactive": "readonly", "toRefs": "readonly" });
    assert_eq!(
        reports("no-undef-properties", json!([]), declared, undef),
        ["1:22"]
    );

    let paren = "<script setup>\nconst count = (ref)(0)\nconst x = count.value\n</script>";
    let declared = json!({ "ref": "readonly" });
    assert_eq!(
        reports("no-ref-object-reactivity-loss", json!([]), declared, paren),
        ["3:11"]
    );
}

#[test]
fn script_indent_checks_leading_comments() {
    let (at, fixed) = run(
        "script-indent",
        json!([]),
        Value::Null,
        "<script>\n  // line\n    /* b */\nconst a = 1\n  // trailing\n</script>",
    );
    assert_eq!(at, ["2:1", "3:1"]);
    assert_eq!(
        fixed.as_deref(),
        Some("<script>\n// line\n/* b */\nconst a = 1\n  // trailing\n</script>")
    );
    let r = |c: &str| reports("script-indent", json!([]), Value::Null, c);
    assert_eq!(r("<script>\n  /* a */ const a = 1\n</script>"), ["2:1"]);
    // Two blocks: the range starts at the `<script>` text or the first statement, whichever is first.
    assert_eq!(
        r(
            "<script>\n  // line\nconst a = 1\n</script>\n<script setup>\n  // s\nconst b = 2\n</script>"
        ),
        ["2:1", "6:1"]
    );
    assert_eq!(
        r(
            "<script setup>\n  // s\nconst b = 2\n</script>\n<script>\n   // line\nexport default {}\n</script>"
        ),
        ["6:1"]
    );
}

#[test]
fn indent_ignores_use_full_esquery() {
    let tpl = "<template>\n<div>\n<span>\n  y\n</span>\n  <b>\nx</b>\n</div>\n</template>";
    let hi = |sel: &str| {
        reports(
            "html-indent",
            json!([2, { "ignores": [sel] }]),
            Value::Null,
            tpl,
        )
    };
    assert_eq!(hi("VElement[name=span]"), ["2:1", "6:1", "7:1", "8:1"]);
    assert!(hi("VElement[name=template] VElement").is_empty());
    assert!(hi("VElement:not([name=span]) > VElement").is_empty());
    assert_eq!(
        hi("VElement:has(VText) ~ VElement, VElement[rawName=B]"),
        ["2:1", "3:1", "4:1", "5:1", "8:1"]
    );
    let attrs = "<template>\n<div\n:a=\"1\"\n   :b=\"2\"\n/>\n</template>";
    assert_eq!(
        reports(
            "html-indent",
            json!([2, { "ignores": ["VAttribute[directive=true][key.argument.name=b]"] }]),
            Value::Null,
            attrs
        ),
        ["2:1", "3:1", "5:1"]
    );

    let si = |sel: &[&str], code: &str| {
        reports(
            "script-indent",
            json!([2, { "ignores": sel }]),
            Value::Null,
            code,
        )
    };
    let gql = "<script>\nconst q = gql(\n      \"a\",\n   \"b\"\n)\nfoo(\n      1\n)\n</script>";
    assert_eq!(si(&["CallExpression[callee.name=\"gql\"] *"], gql), ["7:1"]);
    let calls = "<script>\nfoo(\n      1,\n   a?.b(\n  2)\n)\n</script>";
    let sels = [
        "CallExpression[optional=true] > *",
        "CallExpression[arguments.0.value=1] > Literal",
    ];
    assert!(si(&sels, calls).is_empty());
    let ops = "<script>\nconst a = 1 +\n      2\nlet b = c ||\n         d\n</script>";
    let sels = [
        "BinaryExpression[operator=\"+\"] > *",
        "VariableDeclaration[kind=let] *",
    ];
    assert!(si(&sels, ops).is_empty());
}

#[test]
fn restricted_syntax_estree_parity() {
    let t = "<template>\n  <div :a=\"foo(1)\" :b=\"{ x: 1, y, m() {} }\" :c=\"a.b?.c\" :d=\"f?.(2)\" :e=\"a?.[0]\" />\n</template>\n<script>\nfoo(2)\n</script>";
    let r =
        |code: &str, sel: &str| reports("no-restricted-syntax", json!([sel]), Value::Null, code);
    assert_eq!(r(t, "Program"), ["1:1"]);
    assert_eq!(r(t, "Program:exit"), ["1:1"]);
    assert_eq!(r(t, "VDocumentFragment > VElement"), ["1:1", "4:1"]);
    assert_eq!(r(t, "Property[shorthand=true]"), ["2:32"]);
    assert_eq!(r(t, "Property[method=false]"), ["2:26", "2:32"]);
    assert_eq!(r(t, "MemberExpression[optional=true]"), ["2:49", "2:73"]);
    assert_eq!(r(t, "CallExpression[optional=true]"), ["2:61"]);
    assert_eq!(r(t, "CallExpression[arguments.0.value=1]"), ["2:12"]);
    assert_eq!(r(t, "Identifier[name.0=f]"), ["2:12", "2:61"]);
    assert_eq!(
        r(t, ":first-child"),
        ["1:1", "1:11", "2:16", "2:26", "2:65", "2:8", "4:9"]
    );

    let t2 = "<template><p :a=\"a /* c */ + b\" @click=\"a = /* x */ b\" :c=\"- /* c */ a\" :d=\"x++\" /></template>";
    assert_eq!(r(t2, "BinaryExpression[operator=\"+\"]"), ["1:18"]);
    assert_eq!(r(t2, "AssignmentExpression[operator=\"=\"]"), ["1:41"]);
    assert_eq!(r(t2, "UnaryExpression[operator=\"-\"]"), ["1:60"]);
    assert_eq!(r(t2, "UpdateExpression[prefix=false]"), ["1:77"]);

    let t3 = "<template><p :a=\"true\" :b=\"null\" :c=\"/re/gi\" :d=\"1n\" :e=\"'s'\" :f=\"0x1Fn\" :g=\"2\" :h=\"'10'\" :i=\"false\" :j=\"''\" :k=\"1_000\" /></template>";
    assert_eq!(r(t3, "Literal[regex.flags=gi]"), ["1:38"]);
    assert_eq!(r(t3, "Literal[bigint=\"31\"]"), ["1:67"]);
    assert_eq!(
        r(t3, "Literal[value>0]"),
        ["1:114", "1:18", "1:50", "1:67", "1:78", "1:85"]
    );
    assert_eq!(
        r(t3, "Literal[value<=1]"),
        ["1:106", "1:18", "1:28", "1:50", "1:95"]
    );
    assert_eq!(r(t3, "Literal[value=/^\\d+$/]"), ["1:85"]);
    assert_eq!(
        r(t3, "Literal[value!=/^\\d+$/]"),
        ["1:106", "1:18", "1:28", "1:38", "1:58", "1:95"]
    );
    assert_eq!(r(t3, "Literal[value=type(bigint)]"), ["1:50", "1:67"]);
    assert_eq!(
        r(t3, "Literal[value>\"1\"]"),
        ["1:114", "1:58", "1:67", "1:78", "1:85"]
    );
    assert_eq!(r(t3, "Literal[value=1.0]"), ["1:50"]);
    assert!(r(t3, "Literal[value=\"1.0\"]").is_empty());
}

/// ESLint `getLocFromIndex` breaks lines on CR and U+2028/9 too; parser node locs only on `\n`.
#[test]
fn eslint_loc_line_terminators() {
    let r = |rule: &str, code: &str| reports(rule, json!([]), Value::Null, code);
    let ls = "<template>\n  <div title=\"a\u{2028}b\">\n    <!--a\u{2028}b-->\n  </div>\n</template>\na\u{2028}b stray\n";
    assert_eq!(r("html-comment-content-spacing", ls), ["4:9", "5:2"]);
    assert_eq!(r("no-irregular-whitespace", ls), ["2:16", "4:10"]);
    assert_eq!(r("no-content-outside-block", ls), ["8:1"]);
    let btn = "<template>a\u{2028}b<div/>a\u{2028}b</template>\n<script>a\u{2028}b\nconst a = 1\na\u{2028}b</script>";
    assert_eq!(r("block-tag-newline", btn), ["1:11", "1:23", "2:9", "4:4"]);
}

/// max-len walks ESLint `sourceCode.lines` (CR-only lines are separate lines) but takes block
/// line ranges from parser locations, so here only ESLint line 1 is "in the template".
#[test]
fn max_len_iterates_eslint_lines() {
    let code = "<template>\r  <div title=\"xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\"></div>\r</template>\n";
    let at = run(
        "max-len",
        json!([{ "code": 40, "template": 40 }]),
        Value::Null,
        code,
    )
    .0;
    assert!(at.is_empty());
}

// Final audit #4: a `//` comment right before `</script>` is not trailing (ESLint: 2:31).
#[test]
fn max_len_line_comment_before_script_end_tag() {
    let code = "<script>\nexport default {} // inline comment xxxxxxxx</script>\n";
    for opt in ["ignoreTrailingComments", "ignoreComments"] {
        assert_eq!(
            reports(
                "max-len",
                json!([{ "code": 30, opt: true }]),
                Value::Null,
                code
            ),
            vec!["2:31"],
            "{opt}"
        );
    }
}

// Final audit #2: CRLF is one line break for `skipBlankLines` (SourceCode.splitLines).
#[test]
fn max_lines_per_block_crlf_blank_lines() {
    let code = "<template>\r\n  <div/>\r\n  <div/>\r\n</template>\r\n";
    assert_eq!(
        reports(
            "max-lines-per-block",
            json!([{ "template": 1, "skipBlankLines": true }]),
            Value::Null,
            code
        ),
        vec!["1:1"]
    );
}

// A TS union inside a template cast is not a Vue 2 filter (vue-eslint-parser splits it; we don't).
#[test]
fn ts_union_in_template_is_not_a_filter() {
    let ts = "<script setup lang=\"ts\">\nconst attrs = useAttrs()\n</script>\n<template>\n  <input :style=\"attrs.style as string | undefined\">\n  <i :a=\"x satisfies A | B\" :b=\"f<A | B>(y)\" />\n  <p>{{ msg | upper }}</p>\n</template>\n";
    assert_eq!(
        reports("no-deprecated-filter", json!([]), Value::Null, ts),
        vec!["7:9"]
    );
    // Plain JS: `|` is still a filter.
    let js = "<template>\n  <p>{{ msg | upper }}</p>\n</template>\n";
    assert_eq!(
        reports("no-deprecated-filter", json!([]), Value::Null, js),
        vec!["2:9"]
    );
}

// Vue compiles a bare `#` / `:` / `@` as `v-slot` / `v-bind` / `v-on` without an argument (checked with
// @vue/compiler-dom 3.5); vue-eslint-parser keeps them as plain attributes.
#[test]
fn bare_shorthand_is_a_directive() {
    let code = "<template>\n  <Popover #=\"{ open, close }\">\n    <div :class=\"{ a: open }\" :=\"attrs\" @=\"handlers\" />\n  </Popover>\n</template>\n<script>\nexport default { props: ['attrs', 'handlers'] }\n</script>\n";
    // `open` is the slot's scope variable, not an undefined component property.
    assert!(reports("no-undef-properties", json!([]), Value::Null, code).is_empty());
    // `close` is declared by the slot scope and never used.
    assert_eq!(
        reports("no-unused-vars", json!([]), Value::Null, code),
        ["2:23"]
    );
    // `attrs` / `handlers` are used by `:=` / `@=`.
    let g = json!([{ "groups": ["props"] }]);
    assert!(reports("no-unused-properties", g, Value::Null, code).is_empty());
}

// A class-name fragment glued to an interpolation or a concatenated operand is not a class name.
#[test]
fn class_fragments_are_not_duplicates() {
    let r = |c: &str| run("no-duplicate-class-names", json!([]), Value::Null, c);
    // `hover:text-X-600` and `focus-visible:text-X-600` differ: no `-600` class exists.
    let tpl = "<template><a :class=\"`text-${c}-500 hover:text-${c}-600 focus:text-${c}-600`\" /></template>";
    assert!(r(tpl).0.is_empty());
    assert!(
        r("<template><a :class=\"`btn-${a} btn-${b}`\" /></template>")
            .0
            .is_empty()
    );
    assert!(
        r("<template><a :class=\"'p-' + s + ' m-' + s\" /></template>")
            .0
            .is_empty()
    );
    assert!(
        r("<template><a class=\"x\" :class=\"`${a}x`\" /></template>")
            .0
            .is_empty()
    );
    // Whole names next to an interpolation are still checked.
    assert_eq!(
        r("<template><a :class=\"`a ${x} a`\" /></template>").0,
        ["1:22"]
    );
    assert_eq!(
        r("<template><a :class=\"'foo ' + 'foo'\" /></template>").0,
        ["1:22"]
    );
    // `${x}y y y`: one `y` is duplicated, but the raw-text fix would drop the fragment too — no fix.
    let (at, fix) = r("<template><a :class=\"`${x}y y y`\" /></template>");
    assert_eq!(at, ["1:26"]);
    assert_eq!(fix, None);
}

// The fix of a template element keeps its `}` / `${` delimiters (upstream treats them as quotes and
// writes `` `${x} b} ``).
#[test]
fn duplicate_class_fix_keeps_template_delimiters() {
    let (_, fix) = run(
        "no-duplicate-class-names",
        json!([]),
        Value::Null,
        "<template><a :class=\"`${x} b b`\" /><a :class=\"`b b ${x}`\" /></template>",
    );
    assert_eq!(
        fix.as_deref(),
        Some("<template><a :class=\"`${x} b`\" /><a :class=\"`b ${x}`\" /></template>")
    );
}

// ESLint-verified: the removed `export default` starts at the end of the previous line, one line
// terminator only — blank lines above it stay.
#[test]
fn prefer_define_options_keeps_blank_lines_above() {
    let code = "<script>\nimport { defineComponent } from 'vue'\n\nexport default defineComponent({ inheritAttrs: true })\n</script>\n<script setup>\nconst a = 1\n</script>\n";
    let (_, fix) = run("prefer-define-options", json!([]), Value::Null, code);
    assert_eq!(
        fix.as_deref(),
        Some(
            "<script>\nimport { defineComponent } from 'vue'\n\n</script>\n<script setup>\ndefineOptions(defineComponent({ inheritAttrs: true }))\n\nconst a = 1\n</script>\n"
        )
    );
}

// A name in a type position of a template expression is erased by the compiler: it is not a component
// property (upstream reports DOM types such as `HTMLInputElement` as undefined).
#[test]
fn template_type_names_are_not_properties() {
    let code = "<script setup lang=\"ts\">\nconst onInput = (v: string) => v\n</script>\n<template>\n  <input @input=\"onInput(($event.target as HTMLInputElement).value)\" :ref=\"(el: Element) => el\" />\n  <i>{{ missing }}</i>\n</template>\n";
    assert_eq!(
        reports("no-undef-properties", json!([]), Value::Null, code),
        ["6:9"]
    );
}
