# Changelog

## 0.1.0 — unreleased

First release.

- All 210 eslint-plugin-vue 10.12.1 rules that oxlint 1.93 does not implement natively (template and script),
  with eslint-plugin-vue's options, messages, locations, fixes and suggestions.
- Port of vue-eslint-parser (HTML tokenizer, template AST, oxc expression ASTs, references, tokens, parse errors).
- `oxlint-vue-rs` CLI reading `.oxlintrc.json` (`rules`, `overrides`, `ignorePatterns`, `settings`, `globals`,
  `extends`), formats default/json/unix/github, `--fix`, `--with-oxlint`, disable comments.
- oxlint JS plugin shim (`vue-rs/*` rule names) and presets for Vue 3 / Vue 2.
- npm distribution with prebuilt binaries for macOS, Linux (glibc, musl) and Windows (x64, arm64).
