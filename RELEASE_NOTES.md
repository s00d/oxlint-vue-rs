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
