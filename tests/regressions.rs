//! Regression cases from the differential audit against real ESLint (eslint-plugin-vue 10.12.1).
//! Each expectation (report start `line:col`, fix output) was produced by ESLint itself.

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
    let lines = LineIndex::new(code);
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
