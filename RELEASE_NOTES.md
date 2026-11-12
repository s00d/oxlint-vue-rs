**0.1.4** — safer autofix. Every fix and suggestion was run on every upstream test input (all option variants) and
on 9334 real `.vue` files, then compiled with Vue before and after (`tests/fixes.rs`, `scripts/fix-check.mjs`).
On the real-project corpora: fixes that produced invalid code 499 → 0, layout fixes that changed rendered output
19 → 0, non-idempotent fixes 114 → 0, lost text 36 → 0.

- `--fix` keeps CRLF line endings; no fix breaks an unquoted attribute value, deletes a comment or writes the
  attribute's own quote into its value;
- `html-self-closing`, `padding-line-between-tags`, `no-useless-mustaches`: no fixes that change rendered
  whitespace (U+00A0, `<pre>`, text next to tags);
- `this-in-template`, `no-useless-v-bind`, `no-deprecated-v-bind-sync`, `component-name-in-template-casing`,
  `component-options-name-casing`, `v-on-handler-style`, `attributes-order`, `no-extra-parens`: no fixes that do
  not compile or change behaviour;
- `require-macro-variable-name`'s suggestion renames the uses too;
- fixes that undid each other forever are gone; `--fix` runs up to 50 passes (was 10) and stops on a cycle.

Most of these bugs are in eslint-plugin-vue too — see the
[changelog](https://github.com/s00d/oxlint-vue-rs/blob/main/CHANGELOG.md) and
[docs/compatibility.md](https://github.com/s00d/oxlint-vue-rs/blob/main/docs/compatibility.md).
