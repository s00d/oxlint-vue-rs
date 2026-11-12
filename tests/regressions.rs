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
