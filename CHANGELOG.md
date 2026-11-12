# Changelog

## Unreleased

- `npm run publish:npm` / release workflow: the 30 s pause applies only before a package name's first publish.
- README restructured (badges, quick start, documentation index).

## 0.1.1 — 2026-11-12

- npm: the prebuilt binaries ship as scoped packages `@oxlint-vue-rs/<os>-<cpu>[-gnu|-musl]` (the unscoped
  `oxlint-vue-rs-<platform>` names of 0.1.0 tripped npm's spam detection and were never fully published).
  Install is unchanged: `npm i -D oxlint oxlint-vue-rs`.
- `npm run publish:npm` publishes a release's packages from this repo (skips what npm already accepted).

## 0.1.0 — 2026-11-12

First release.

- All 210 eslint-plugin-vue 10.12.1 rules that oxlint 1.93 does not implement natively (template and script),
  with eslint-plugin-vue's options, messages, locations, fixes and suggestions.
- Port of vue-eslint-parser (HTML tokenizer, template AST, oxc expression ASTs, references, tokens, parse errors).
- `oxlint-vue-rs` CLI reading `.oxlintrc.json` like oxlint (`rules`, `overrides`, gitignore-style `ignorePatterns`,
  `settings`, `globals`, `extends`; nearest config per file), formats default/json/unix/github, atomic `--fix`,
  `--with-oxlint`, disable comments; a script syntax error is reported as a fatal `Parsing error` like ESLint.
- oxlint JS plugin shim (`vue-rs/*` rule names) and presets for Vue 3 / Vue 2 that reproduce eslint-plugin-vue's
  configs completely (`plugins: ["vue"]` + native `vue/*` rules + `vue-rs/*`), plus `configs/settings/*` variants
  with our rules in `settings["vue-rs"].rules` (no JS plugin, faster plain `oxlint`).
- Warnings for `oxlint.config.*` without a JSON config (exit 2 when nothing is enabled) and for `vue/<rule>` keys
  naming our rules; `-A/-W/-D` for oxlint rules, `--ignore-pattern` and `--no-ignore` are passed to oxlint.
- npm distribution with prebuilt binaries for macOS, Linux (glibc, musl) and Windows (x64, arm64).
