//! Disable-directive behaviour end to end (`lint_source`), covering the ranges cargo-mutants found untested.

use std::path::Path;

use oxlint_vue_rs::{FileOptions, Severity, configure, lint_source};
use serde_json::Value;

fn lines(src: &str, path: &str, rule: &str, template: bool) -> Vec<u32> {
    let r = configure(rule, Severity::Error, &[], &Value::Null)
        .unwrap()
        .unwrap();
    let opts = FileOptions {
        template_directives: template,
        prefixes: vec!["vue-rs".into()],
        ..FileOptions::default()
    };
    let li = oxlint_vue_rs::LineIndex::new(src);
    lint_source(src, Path::new(path), &opts, &[r])
        .iter()
        .map(|d| li.line_col(src, d.span.start).0)
        .collect()
}

#[test]
fn script_block_line_and_next_line() {
    let src = "<script>
// eslint-disable vue/sort-keys
export const a = { b: 1, a: 2 }
// eslint-enable vue/sort-keys
export const c = { b: 1, a: 2 }
/* eslint-disable-next-line vue/sort-keys */
export const d = { b: 1, a: 2 }
export const e = { b: 1, a: 2 } // eslint-disable-line vue-rs/sort-keys
export const f = { b: 1, a: 2 } // eslint-disable-line other/sort-keys
// eslint-disable-next-line vue/html-indent
export const g = { b: 1, a: 2 }
/* eslint-disable */
export const h = { b: 1, a: 2 }
</script>
";
    // Lines 5 (re-enabled), 9 (other plugin), 11 (other rule) report; 3, 7, 8, 13 are silenced.
    assert_eq!(lines(src, "t.vue", "sort-keys", false), vec![5, 9, 11]);
}

#[test]
fn enable_of_another_rule_keeps_block() {
    let src = "<script>
/* eslint-disable vue/sort-keys */
/* eslint-enable vue/html-indent */
export const a = { b: 1, a: 2 }
</script>
";
    assert_eq!(lines(src, "t.vue", "sort-keys", false), Vec::<u32>::new());
}

#[test]
fn template_directives_only_when_enabled() {
    let src = "<template>
  <!-- eslint-disable-next-line vue/mustache-interpolation-spacing -->
  <div>{{a}}</div>
  <div>{{b}}</div>
  <div>{{c}}</div><!-- eslint-disable-line vue/mustache-interpolation-spacing -->
</template>
";
    // Each `{{x}}` reports twice (after `{{` and before `}}`).
    assert_eq!(
        lines(src, "t.vue", "mustache-interpolation-spacing", true),
        vec![4, 4]
    );
    assert_eq!(
        lines(src, "t.vue", "mustache-interpolation-spacing", false),
        vec![3, 3, 4, 4, 5, 5]
    );
}
