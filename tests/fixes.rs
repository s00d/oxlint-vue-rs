//! Fix verification. `every_fix_on_upstream_inputs` runs the `--fix` loop of every rule on its own
//! upstream inputs (with their options), every suggestion alone, and all fixable rules together on
//! every SFC input, and checks what `tests/fixcheck` checks: the output parses, the loop converges
//! without cycling, nothing is lost. The heavy corpus run (Vue compiler render comparison, vue-tsc)
//! is `scripts/fix-check.mjs`.
//!
//! The other tests are regressions found by that harness. Expectations follow Vue / JS semantics, not
//! eslint-plugin-vue, where the two differ (docs/compatibility.md §5).

mod fixcheck;

use std::collections::BTreeMap;
use std::path::Path;

use oxlint_vue_rs::{FileOptions, apply_fixes, lint_source, rules};
use serde_json::{Value, json};

/// (rule, problem tag): known and accepted, with the reason.
const ACCEPTED: &[(&str, &str, &str)] = &[
    // eslint-plugin-vue moves one property per report: a long list needs more than 10 passes.
    ("order-in-components", "no-converge", "one move per pass"),
    // Removing a block / an element's children is the purpose of these fixes.
    ("no-empty-component-block", "blocks", "removes blocks"),
    (
        "no-empty-component-block",
        "blocks:script-count",
        "removes blocks",
    ),
    ("no-child-content", "comments", "removes the children"),
    (
        "prefer-define-options",
        "blocks:script-count",
        "moves the options, drops the emptied block",
    ),
    // Upstream's expected output (quote-props invalid0) puts `"` in an unquoted attribute value:
    // a spec parse error that Vue and browsers accept.
    ("quote-props", "parse", "upstream expected output"),
    // Unclosed tags at the end of a broken template (`<div>` + EOF): the inserted end tags land before
    // a trailing comment.
    ("html-end-tags", "parse", "broken input"),
    ("html-end-tags", "no-converge", "broken input"),
    // `v-slot=/**/` (an attribute value that is only a comment): vue-eslint-parser's token store skips
    // the comment, so eslint-plugin-vue deletes it too.
    ("no-multi-spaces", "parse", "comment-only attribute value"),
    (
        "html-closing-bracket-spacing",
        "parse",
        "comment-only attribute value",
    ),
    // Two `defineEmits()` in one `<script setup>` (invalid program): the moves undo each other.
    ("define-macros-order", "cycle", "duplicate macros"),
];

fn file_options(case: &Value) -> (FileOptions, &'static str) {
    let lo = &case["languageOptions"];
    let filename = case["filename"].as_str().unwrap_or("test.js");
    let parser = lo["parser"].as_str().unwrap_or("espree");
    let sp = &lo["scriptParser"];
    let ts = parser == "ts"
        || sp.as_str() == Some("ts")
        || sp.get("js").and_then(Value::as_str) == Some("ts")
        || (parser != "vue" && sp.get("ts").is_some() && filename.ends_with(".ts"));
    let sfc = parser == "vue"
        && (filename.ends_with(".vue")
            || case["code"]
                .as_str()
                .unwrap_or("")
                .trim_start()
                .starts_with('<'));
    let path = if sfc {
        "t.vue"
    } else if filename.ends_with(".ts") {
        "t.ts"
    } else {
        "t.js"
    };
    let fo = FileOptions {
        sfc: Some(sfc),
        ts,
        jsx: lo["ecmaFeatures"]["jsx"].as_bool() == Some(true),
        settings: case["settings"].clone(),
        template_directives: true,
        ..FileOptions::default()
    };
    (fo, path)
}

fn accepted(rule: &str, problem: &str) -> bool {
    ACCEPTED
        .iter()
        .any(|(r, p, _)| *r == rule && (problem == *p || problem.starts_with(&format!("{p}:"))))
}

#[test]
fn every_fix_on_upstream_inputs() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut failures: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut checked = 0usize;
    let mut inputs: Vec<(String, FileOptions, &str)> = Vec::new();
    let mut fixed_rules: Vec<&str> = Vec::new();
    for def in rules::ALL {
        let Ok(text) =
            std::fs::read_to_string(root.join(format!("tests/upstream/{}.json", def.name)))
        else {
            continue;
        };
        let cases: Vec<Value> = serde_json::from_str(&text).unwrap();
        for c in &cases {
            let code = c["code"].as_str().unwrap();
            let (fo, path) = file_options(c);
            let options = c["options"].as_array().cloned().unwrap_or_default();
            let Some(rule) = fixcheck::rule(def.name, &options, &fo.settings) else {
                continue;
            };
            let rules = [rule];
            let id = format!("{}#{}{}", def.name, c["kind"].as_str().unwrap(), c["idx"]);
            let mut outcomes = Vec::new();
            for suggest in [false, true] {
                if let Some(o) = fixcheck::check(code, Path::new(path), &fo, &rules, suggest) {
                    outcomes.push(o);
                }
            }
            outcomes.extend(
                fixcheck::check_suggestions(code, Path::new(path), &fo, &rules)
                    .into_iter()
                    .map(|(_, o)| o),
            );
            if !outcomes.is_empty() && !fixed_rules.contains(&def.name) {
                fixed_rules.push(def.name);
            }
            for o in outcomes {
                checked += 1;
                for p in o.problems.iter().filter(|p| !accepted(def.name, p)) {
                    failures.entry(id.clone()).or_default().push(p.clone());
                }
            }
            if fo.settings.is_null() || fo.settings == json!({}) {
                inputs.push((code.to_string(), fo, path));
            }
        }
    }
    // Every fixable rule on every input (default options), then all of them together (`-D all --fix`:
    // overlapping edits, rules undoing each other). Threads: ~600k fix loops.
    let fixable: Vec<&str> = rules::ALL
        .iter()
        .map(|d| d.name)
        .filter(|n| fixed_rules.contains(n))
        .collect();
    inputs.sort_by(|a, b| (&a.0, a.2).cmp(&(&b.0, b.2)));
    inputs.dedup_by(|a, b| a.0 == b.0 && a.2 == b.2 && a.1.ts == b.1.ts);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results = std::sync::Mutex::new((0usize, BTreeMap::<String, Vec<String>>::new()));
    std::thread::scope(|sc| {
        for _ in 0..std::thread::available_parallelism().map_or(4, |n| n.get()) {
            sc.spawn(|| {
                let single: Vec<_> = fixable
                    .iter()
                    .map(|n| (*n, fixcheck::rule(n, &[], &Value::Null).unwrap()))
                    .collect();
                let all: Vec<_> = fixable
                    .iter()
                    .filter_map(|n| fixcheck::rule(n, &[], &Value::Null))
                    .collect();
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some((code, fo, path)) = inputs.get(i) else {
                        break;
                    };
                    let mut local = Vec::new();
                    for (name, r) in &single {
                        let rs = std::slice::from_ref(r);
                        if let Some(o) = fixcheck::check(code, Path::new(path), fo, rs, false) {
                            local.push((format!("{name} on {code:?}"), Some(*name), o));
                        }
                    }
                    if fo.sfc == Some(true)
                        && let Some(o) = fixcheck::check(code, Path::new(path), fo, &all, false)
                    {
                        local.push((format!("ALL on {code:?}"), None, o));
                    }
                    let mut g = results.lock().unwrap();
                    for (k, name, o) in local {
                        g.0 += 1;
                        for p in o.problems.iter().filter(|p| match name {
                            Some(n) => !accepted(n, p),
                            // With every rule on, accepted problems of any rule may show up.
                            None => !ACCEPTED.iter().any(|(r, _, _)| accepted(r, p)),
                        }) {
                            g.1.entry(k.clone()).or_default().push(p.clone());
                        }
                    }
                }
            });
        }
    });
    let (n, more) = results.into_inner().unwrap();
    checked += n;
    failures.extend(more);
    eprintln!("{checked} fix outputs checked");
    assert!(checked > 15_000, "only {checked} fixes checked");
    let report: Vec<String> = failures
        .iter()
        .take(40)
        .map(|(k, v)| format!("{k}: {}", v.join(", ")))
        .collect();
    assert!(
        failures.is_empty(),
        "{} inputs with fix problems:\n{}",
        failures.len(),
        report.join("\n")
    );
}

/// The `--fix` loop's output for one rule on `t.vue` (None = nothing fixed).
fn fix(rule: &str, opts: Value, code: &str) -> Option<String> {
    let opts: Vec<Value> = opts.as_array().cloned().unwrap_or_default();
    let rules = [fixcheck::rule(rule, &opts, &Value::Null).unwrap()];
    fixcheck::fix_loop(
        code,
        Path::new("t.vue"),
        &FileOptions::default(),
        &rules,
        false,
    )
    .map(|(_, after, ..)| after)
}

/// Each suggestion's output.
fn suggestions(rule: &str, opts: Value, code: &str) -> Vec<String> {
    let opts: Vec<Value> = opts.as_array().cloned().unwrap_or_default();
    let rules = [fixcheck::rule(rule, &opts, &Value::Null).unwrap()];
    lint_source(code, Path::new("t.vue"), &FileOptions::default(), &rules)
        .iter()
        .flat_map(|d| &d.suggestions)
        .map(|s| {
            let mut out = code.to_string();
            out.replace_range(
                s.fix.span.start as usize..s.fix.span.end as usize,
                &s.fix.text,
            );
            out
        })
        .collect()
}

#[test]
fn crlf_files_keep_crlf() {
    let code = "<template>\r\n  <div a=\"1\" b=\"2\"></div>\r\n</template>\r\n<script>\r\nexport default {}\r\n</script>\r\n";
    let out = fix("max-attributes-per-line", json!([]), code).unwrap();
    assert!(!out.replace("\r\n", "").contains('\n'), "{out:?}");
    let out = fix("padding-line-between-blocks", json!([]), code).unwrap();
    assert!(out.contains("</template>\r\n\r\n<script>"), "{out:?}");
    // LF files stay LF; a mixed file is left as the rules write it.
    let lf = code.replace("\r\n", "\n");
    assert!(
        !fix("max-attributes-per-line", json!([]), &lf)
            .unwrap()
            .contains('\r')
    );
}

#[test]
fn unquoted_attribute_values_stay_one_value() {
    // `:class=foo+"bar"` → `:class=foo + "bar"` would end the value at the space.
    for (rule, code) in [
        (
            "space-infix-ops",
            "<template><div :class=foo+'bar'></div></template>",
        ),
        (
            "key-spacing",
            "<template><div :style={height:1}></div></template>",
        ),
    ] {
        let opts: Vec<Value> = Vec::new();
        let rules = [fixcheck::rule(rule, &opts, &Value::Null).unwrap()];
        let ds = lint_source(code, Path::new("t.vue"), &FileOptions::default(), &rules);
        assert!(!ds.is_empty(), "{rule} still reports");
        assert_eq!(apply_fixes(code, &ds), None, "{rule}");
    }
}

#[test]
fn self_closing_keeps_non_html_whitespace_and_pre_content() {
    // U+00A0 / U+3000 are text for Vue (its whitespace is ASCII only); JS `trim()` drops them.
    for code in [
        "<template><div>\u{a0}</div></template>",
        "<template><div>\u{3000}</div></template>",
        "<template><pre> </pre></template>",
        "<template><textarea>\n</textarea></template>",
    ] {
        assert_eq!(fix("html-self-closing", json!([]), code), None, "{code:?}");
    }
    assert_eq!(
        fix(
            "html-self-closing",
            json!([]),
            "<template><div> \n</div></template>"
        )
        .as_deref(),
        Some("<template><div /></template>")
    );
}

#[test]
fn padding_between_tags_leaves_pre_alone() {
    let code = "<template>\n  <pre><span>a</span>\n<span>b</span></pre>\n  <div></div>\n  <p></p>\n</template>";
    assert_eq!(
        fix("padding-line-between-tags", json!([]), code).as_deref(),
        Some(
            "<template>\n  <pre><span>a</span>\n<span>b</span></pre>\n\n  <div></div>\n\n  <p></p>\n</template>"
        )
    );
}

#[test]
fn extra_parens_keep_template_sequences() {
    // `{{ a, b }}` is not a single expression: Vue would put `a, b` in an argument list.
    assert_eq!(
        fix(
            "no-extra-parens",
            json!([]),
            "<template>{{ (a, b) }}</template>"
        ),
        None
    );
    assert_eq!(
        fix(
            "no-extra-parens",
            json!([]),
            "<template>{{ ((a, b)) }}</template>"
        )
        .as_deref(),
        Some("<template>{{ (a, b) }}</template>")
    );
}

#[test]
fn useless_v_bind_fix_keeps_one_valid_attribute() {
    let f = |c: &str| fix("no-useless-v-bind", json!([]), c);
    // Static `class` next to `:class` is merged by Vue; two `class` attributes are an error.
    assert_eq!(
        f("<template><div class=\"a\" :class=\"'b'\" /></template>"),
        None
    );
    // A dynamic name has no static spelling.
    assert_eq!(f("<template><div :[k]=\"'v'\" /></template>"), None);
    assert_eq!(
        f("<template><div :title=\"'a'\" /></template>").as_deref(),
        Some("<template><div title=\"a\" /></template>")
    );
}

#[test]
fn v_bind_sync_fix_only_where_v_model_compiles() {
    let f = |c: &str| fix("no-deprecated-v-bind-sync", json!([]), c);
    // Vue 3: `v-model:x` on a plain element (also `<tr :is>`) or with a non-assignable value is a
    // compile error.
    assert_eq!(f("<template><div :foo.sync=\"bar\" /></template>"), None);
    assert_eq!(
        f("<template><tr :is=\"row\" :foo.sync=\"bar\" /></template>"),
        None
    );
    assert_eq!(
        f("<template><MyComp :foo.sync=\"bar()\" /></template>"),
        None
    );
    assert_eq!(
        f("<template><MyComp :foo.sync=\"a.b\" /></template>").as_deref(),
        Some("<template><MyComp v-model:foo=\"a.b\" /></template>")
    );
}

#[test]
fn component_options_name_fix_stays_valid() {
    let code = "<script>\nexport default { components: { 'fio.fio': A, 'foo-bar': B } }\n</script>";
    // `Fio.fio` is no identifier and still not PascalCase: no fix. `'foo-bar'` → `FooBar`.
    assert_eq!(
        fix("component-options-name-casing", json!([]), code).as_deref(),
        Some("<script>\nexport default { components: { 'fio.fio': A, FooBar: B } }\n</script>")
    );
}

#[test]
fn macro_variable_rename_follows_uses() {
    let code = "<template>{{ p.a }}</template>\n<script setup>\nconst p = defineProps(['a'])\nconsole.log(p.a)\n</script>";
    assert_eq!(
        suggestions("require-macro-variable-name", json!([]), code),
        [
            "<template>{{ props.a }}</template>\n<script setup>\nconst props = defineProps(['a'])\nconsole.log(props.a)\n</script>"
        ]
    );
    // The name is taken: renaming would declare it twice.
    let taken = "<script setup>\nconst slots = defineSlots()\nconst s = useSlots()\n</script>";
    assert!(suggestions("require-macro-variable-name", json!([]), taken).is_empty());
    // `{ p }` would change the key.
    let shorthand =
        "<script setup>\nconst p = defineProps(['a'])\nexport const x = { p }\n</script>";
    assert!(suggestions("require-macro-variable-name", json!([]), shorthand).is_empty());
}

#[test]
fn name_property_suggestion_keeps_comments() {
    let code = "<script>\nexport default {\n  // todo\n}\n</script>";
    let out = suggestions("require-name-property", json!([]), code);
    assert_eq!(
        out,
        ["<script>\nexport default {\n  name: 't',\n  // todo\n}\n</script>"]
    );
}

#[test]
fn empty_block_fix_keeps_comments() {
    let code = "<template><!-- later --></template>\n<script>\nexport default {}\n</script>\n";
    let opts: Vec<Value> = Vec::new();
    let rules = [fixcheck::rule("no-empty-component-block", &opts, &Value::Null).unwrap()];
    let ds = lint_source(code, Path::new("t.vue"), &FileOptions::default(), &rules);
    assert_eq!(ds.len(), 1);
    assert_eq!(apply_fixes(code, &ds), None);
}
