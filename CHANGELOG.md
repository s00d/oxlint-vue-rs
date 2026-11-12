# Changelog

## 0.1.3 — 2026-11-12

- Fix: a bare `#="…"` / `:="…"` / `@="…"` in a template is a directive (`v-slot` / `v-bind` / `v-on` without an
  argument), as Vue compiles it: slot variables are no longer reported as undefined (`no-undef-properties`).
  Deliberate difference from vue-eslint-parser — see docs/compatibility.md.
- Fix: `no-duplicate-class-names` ignores class-name fragments glued to an interpolation or a concatenation
  (`` `hover:text-${c}-600` ``), and its fix inside a template literal next to `${…}` no longer corrupts the code
  (eslint-plugin-vue writes `` `${x} b} ``).
- Fix: `no-undef-properties` no longer reports type names used in template expressions
  (`$event.target as HTMLInputElement`, `(el: Element) => …`) — they are erased by the compiler.
- `no-undef-properties` and `no-unused-properties` report nothing in a component with `extends` or non-empty
  `mixins` (their members were reported as undefined, e.g. every PrimeVue component; props they read as unused).
- Fix: `no-unused-refs` reports nothing for a component whose `setup()` returns a spread / non-literal object or
  whose `getCurrentInstance()` is stored or passed on (`instance.refs.x` reads a ref).
- Fix: props of a type alias used in several members of a union (`(Base & A) | (Base & B)`) were reported once per
  member (`require-prop-comment` and other prop rules); now once, as eslint-plugin-vue.
- Fix: `prefer-define-options`'s fix no longer deletes blank lines above the removed `export default`.

## 0.1.2 — 2026-11-12

- Fix: in TS templates, a union type in a cast (`x as string | undefined`) is no longer parsed as a Vue 2 filter
  (false `no-deprecated-filter`). Deliberate difference from vue-eslint-parser — see docs/compatibility.md.
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
