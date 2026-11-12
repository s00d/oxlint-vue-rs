# Changelog

## 0.1.4 — 2026-11-12

Fixes checked by a new harness (`tests/fixes.rs`, `scripts/fix-check.mjs`): every fix and suggestion on every
upstream test input and on the real-project corpora, compiled with Vue before and after.

- Fix: `--fix` keeps CRLF line endings in files that use them (rules wrote `\n`, mixing endings).
- Fix: no fix writes whitespace or `>` into an unquoted attribute value (`:class=a+b` → `a + b` broke the tag;
  `space-infix-ops`, `key-spacing`, …).
- Fix: `html-self-closing` no longer treats U+00A0 / U+3000 text, or whitespace in `<pre>` / `<textarea>`, as empty
  (the fix deleted it).
- Fix: `padding-line-between-tags` does not add or remove blank lines inside `<pre>` (rendered text).
- Fix: `no-extra-parens` keeps the parentheses of a template sequence (`{{ (a, b) }}`); without them the output
  does not compile.
- Fix: `no-useless-v-bind` gives no fix next to a static attribute of the same name (`class="a" :class="'b'"`) or
  for a dynamic name (`:[k]="'v'"`), and quotes an unquoted value when needed.
- Fix: `no-deprecated-v-bind-sync` fixes to `v-model:x` only where that compiles (a component, an assignable
  value).
- Fix: `component-options-name-casing` no longer writes an invalid key (`'fio.fio'` → `Fio.fio`); no fix when the
  name cannot be converted. `component-name-in-template-casing`: no fix that leaves the name unchanged.
- Fix: `require-macro-variable-name`'s suggestion renames the uses too, and is not offered when the name is taken.
- Fix: `require-name-property`'s suggestion and `no-empty-component-block`'s fix keep comments.
- Fix: `padding-line-between-tags` no longer fixes a gap with text or a same-line space (`</b> <i>`, `a<br />b`):
  the inserted line break removed or added a rendered space. Still reported.
- Fix: `no-useless-mustaches` gives no fix for the first / last mustache of an element next to whitespace-only
  text (`<b>\n  {{ 'a' }}\n</b>` renders `a`, the fixed text ` a `).
- Fix: `padding-lines-in-component-definition` no longer treats comment lines as blank lines; its fix deleted all
  but the last line of a comment between two properties.
- Fix: `prefer-separate-static-class` keeps the comments inside a `:class` list.
- Fix: no fix deletes a comment or joins a `//` comment with the code after it (e.g. `mustache-interpolation-spacing`
  `never` wrote `// comment}}`), and none writes the attribute's own quote into a quoted value (`dot-notation`
  `allowKeywords: false` wrote `v-if="$slots["default"]"`) or glues text to an unquoted value (`foo=a />` →
  `foo=a/>`).
- Fix: `this-in-template` `always` no longer prefixes Vue's allowed globals (`Math`, `console`, …) or TS type names,
  and writes `{ a: this.a }` for a shorthand property (eslint-plugin-vue wrote `{ this.a }`).
- Fix: `component-name-in-template-casing` gives no fix that turns a component into an HTML element (`<Address>` →
  `<address>`).
- Fix: `v-on-handler-style` (`inline`) unwraps only function bodies of expression statements (several of them
  only with a `;`): Vue compiles `a()\nb()` as one expression and does not scope declarations in an inline
  handler (`const x = f(); g(x)` reads `_ctx.x`).
- Fix: fixes that undid each other forever: `define-macros-order` with a macro called twice,
  `padding-lines-in-component-definition` with a `// @vue/component` object inside an option, spacing rules in an
  unparsable `<style>` `v-bind()`.
- Fix: `attributes-order` gives no fix for attributes written without whitespace between them (swapping merged
  them into one).
- `--fix` repeats until the file is stable, up to 50 passes (was 10, like ESLint), and stops on a cycle: on the
  real-project corpora ~3% of files needed 11–30 passes with every rule on (reordering rules move one item per
  pass), and were left half-fixed.

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
