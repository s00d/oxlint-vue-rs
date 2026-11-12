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

---

**0.1.3** — fewer false positives, found on 11 large Vue projects (gitlab-foss, directus, hoppscotch, halo,
primevue, cypress, vue-pure-admin, slidev, soybean-admin, vitepress, gitea; 5213 `.vue` files):

- bare `#="…"` / `:="…"` / `@="…"` are directives (`v-slot` / `v-bind` / `v-on`), as Vue compiles them;
- `no-duplicate-class-names`: no reports on class fragments next to `${…}`, and its fix no longer corrupts template literals;
- `no-undef-properties`: type names in template expressions (`as HTMLInputElement`) are not properties;
- `no-undef-properties` / `no-unused-properties`: nothing reported in components with `extends` / `mixins`;
- `no-unused-refs`: nothing reported when `setup()` spreads its return or the instance escapes;
- union-typed props reported once, not once per union member;
- `prefer-define-options`: its fix keeps the blank lines above `export default`.

Several of these are deliberate differences from eslint-plugin-vue — see
[docs/compatibility.md](https://github.com/s00d/oxlint-vue-rs/blob/main/docs/compatibility.md).

---

**0.1.2**

- Fix: with `<script lang="ts">`, a union type in a template cast (`attrs.style as string | undefined`) is no
  longer parsed as a Vue 2 filter (false `no-deprecated-filter`). Real filters are still reported. This is a
  deliberate difference from vue-eslint-parser / eslint-plugin-vue.
- `npm run publish:npm` / release workflow: the 30 s pause applies only before a package name's first publish.
- README restructured (badges, quick start, documentation index).

---

**0.1.1** — npm packaging: the prebuilt binaries now ship as scoped packages `@oxlint-vue-rs/<platform>`
(installed automatically by `npm i -D oxlint oxlint-vue-rs`). No rule changes since 0.1.0.

---

First release of **oxlint-vue-rs**: every eslint-plugin-vue 10.12.1 rule that oxlint 1.93 does not implement
natively — 210 rules, `<template>` rules included — written in Rust on the oxc toolchain.

- Same rule names, options, messages, locations, fixes and suggestions as eslint-plugin-vue: 6839/6878 of its own
  test cases pass, and on 4121 real `.vue` files the output is identical (151 548 problems, 0 missing, 0 extra).
- One command: `oxlint-vue-rs` runs oxlint (when installed) and the Vue rules on the same `.oxlintrc.json` and prints
  one report. Rules go in `.oxlintrc.json` as `vue-rs/<rule>`; presets for Vue 3 and Vue 2 included.
- Fast: all 210 rules on 4121 files in 2.3 s (eslint-plugin-vue: 45 s).

Install:

```bash
npm i -D oxlint oxlint-vue-rs   # prebuilt binaries for macOS, Linux (glibc/musl), Windows — x64 and arm64
cargo install oxlint-vue-rs     # or from crates.io
```

Assets: native binaries per platform (`oxlint-vue-rs-<target>`) and the npm package tarballs.
See the [README](https://github.com/s00d/oxlint-vue-rs#readme), the
[rule matrix](https://github.com/s00d/oxlint-vue-rs/blob/main/docs/rules.md) and the
[compatibility report](https://github.com/s00d/oxlint-vue-rs/blob/main/docs/compatibility.md).
