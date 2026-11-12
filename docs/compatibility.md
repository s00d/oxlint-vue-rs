# Compatibility report

Reference: **eslint-plugin-vue 10.12.1** (256 rules), **oxlint 1.93.0** (46 native `vue/*` rules, script-only),
vue-eslint-parser 10.4.2. oxlint-vue-rs implements the other **210** rules — every rule oxlint lacks, including
the deprecated and Vue 2-only ones. Per-rule detail: [rules.md](rules.md).

## 1. Upstream test suite

Every RuleTester case of eslint-plugin-vue for these 210 rules was replayed through real ESLint and recorded
(`tests/upstream/*.json`): diagnostics with full locations, single-pass fix output, suggestions. The harness
(`tests/upstream.rs`) requires exact equality of line, column, end line, end column, message, fix output and
suggestion outputs.

| | |
|---|---:|
| cases | 6878 |
| pass | **6839** |
| skipped | 39 |
| failing | 0 |

Skips: 29 type-aware cases (they need a TypeScript program — `languageOptions.project`) and 10 listed with reasons
in `tests/upstream-skips.json`: 4 where oxc 0.158 cannot parse a type argument list on its own line after
`implements X` (`script-indent`), `v-bind()` in `<style>` for `no-parsing-error`, the `vueFeatures.filter: false`
parser option, an upstream capture that parsed TS template expressions as JS, two cases that depend on the capture
machine's paths, and one spurious ESLint parse error. Dropped from the data set: 3 `no-parsing-error` cases with lone
UTF-16 surrogates (not representable in UTF-8) and the 5 `jsx-uses-vars` cases, which exercise ESLint's core
`no-unused-vars`.

vue-eslint-parser port: 266/280 of its own AST fixtures match (tree, every token, errors, references, variables);
14 need parser options we do not support (pug, custom template tokenizers, babel, ecmaVersion 5, `filter: false`,
`interpolationAsNonHTML: false`).

## 2. Real projects

All 204 non-deprecated rules at default options (`-D all`), on 9 projects / **4121 `.vue` files**: element-plus,
vue-vben-admin, nuxt/ui, vue-element-admin (Vue 2), vitesse, and four private applications (two Nuxt 3, two
Vue 3 + Vite). eslint-plugin-vue ran with vue-eslint-parser +
typescript-eslint and `noInlineConfig`; we ran `--no-inline-config --hidden`. Problems are matched by
file + line + column + rule + message.

Second round, 11 public projects / **5213 `.vue` files**: cypress (app, frontend-shared), directus (app),
gitea (web_src), gitlab-foss (Vue 2, `app/assets/javascripts`), halo (ui), hoppscotch, primevue, slidev,
soybean-admin, vitepress, vue-pure-admin. Same setup; problems starting at one position are compared as a
multiset.

| | first 9 projects | 11 new projects |
|---|---:|---:|
| matching problems | **151 480** | **420 416** |
| missing (ESLint reports, we don't) | 73 | 3 573 |
| message differences | **0** | **0** |
| extra (we report, ESLint doesn't) | **0** | 17 |
| reports in files ESLint failed to parse | 98 | 676 |

Every missing and extra problem is one of the deliberate differences in section 5 (each lists its counts):
eslint-plugin-vue / vue-eslint-parser false positives we do not reproduce, plus `no-ref-as-operand` checks
from upstream's main branch. The unparsed files are `lang="tsx"` blocks and JSX in a plain `<script>` (the
reference config had no JSX for them); we parse and lint those files. With each project's own rule selection and
options (gitea, directus, gitlab, primevue, vue-pure-admin) the results match the same way.

Found and fixed during these runs: global `ref()`/`computed()` were tracked even when not declared (ESLint only
follows declared `globals`) — 139 false positives of `no-ref-object-reactivity-loss` on Nuxt auto-imports;
TS type nodes visited by template extension rules; a panic in `no-spaces-around-equal-signs-in-attribute` on
`:foo` shorthand; template comments outside `<template>` for the comment rules.

## 3. Performance

Best of 5 wall-clock runs, Apple M1 Max (10 cores), neutral configs, same files (`scripts/bench.sh`):

| corpus | files | oxlint-vue-rs | + oxlint (`--with-oxlint`) | oxlint-vue 0.3.2 (TS) | eslint-plugin-vue |
|---|---:|---:|---:|---:|---:|
| element-plus | 1022 | 0.39 s | 0.54 s | 14.11 s | 8.13 s |
| private-app-a | 95 | 0.07 s | 0.12 s | 2.45 s | 1.96 s |
| private-nuxt-b | 414 | 0.53 s | 1.57 s | 9.72 s | 3.76 s |
| private-nuxt-a | 912 | 0.56 s | 1.27 s | 15.14 s (fails) | 11.72 s |
| private-app-b | 16 | 0.03 s | 0.09 s | 1.50 s | 0.89 s |
| ui | 821 | 0.44 s | 0.61 s | 11.68 s (fails) | 9.92 s |
| vitesse | 10 | 0.02 s | 0.07 s | 0.38 s | 0.84 s |
| vue-element-admin | 131 | 0.08 s | 0.14 s | 3.33 s | 1.62 s |
| vue-vben-admin | 700 | 0.22 s | 0.32 s | 10.24 s | 6.39 s |
| **total** | **4121** | **2.34 s** | **4.72 s** | **68.5 s** | **45.2 s** |

oxlint-vue-rs runs all 210 rules; eslint-plugin-vue the same 204 non-deprecated rules (single-threaded, as
ESLint is); oxlint-vue its 25 template rules plus oxlint's defaults. Single-threaded, oxlint-vue-rs lints the
912-file private-nuxt-a corpus with all rules in about 1.5 s.

## 4. Robustness

- `tests/robustness.rs`: every rule over all ~5900 distinct upstream inputs and parser fixtures, whole and cut
  at 7 points, as `.vue` (JS and TS) and `.js` — no panics.
- `--fix` on copies of the corpora converges (a re-run reports nothing fixable).
- Mutation testing (`cargo mutants`), see [mutation.md](mutation.md).

## 5. Deliberate differences from eslint-plugin-vue

- **TS union types in template casts are not Vue 2 filters.** With `<script lang="ts">`, an expression like
  `attrs.style as string | undefined` is a TS cast with a union type. vue-eslint-parser still splits it at `|` as a
  Vue 2 filter, so eslint-plugin-vue reports `no-deprecated-filter` there (and other rules see a filter sequence).
  oxlint-vue-rs keeps the expression whole when every `|` lies inside a TS type (`as`, `satisfies`, type
  arguments); a real filter such as `{{ msg | upper }}` is still reported. On the corpora this removes exactly the
  3 such false positives (on-nuxt-3) and changes nothing else.
- **A bare `#` / `:` / `@` / `.` attribute is a directive.** Vue compiles `#="{ open }"` as the default slot's
  scope and `:="obj"` / `@="handlers"` as `v-bind` / `v-on` without an argument (checked with
  @vue/compiler-dom 3.5). vue-eslint-parser keeps them as plain attributes, so the slot variables look undefined
  (`no-undef-properties`) and the bound objects unused. We parse them as directives. On cypress this removes 6
  false `no-undef-properties` and adds what any `v-slot="{ … }"` gets: `v-slot-style` (4), `object-curly-spacing`
  (8), `attributes-order` (2).
- **`no-duplicate-class-names`: fragments are not class names.** In `` `text-${c}-500 hover:text-${c}-600` `` or
  `'p-' + size`, the text glued to an interpolation or a concatenated operand is part of a longer class
  (`-600` is not a class). eslint-plugin-vue compares those fragments (2 false positives on hoppscotch); we
  skip them, and give no fix when a duplicate is spelled like a fragment of the same string. Its fix of a template
  element next to `${…}` also treats `}` / `${` as quotes and corrupts the literal (`` `${x} b b` `` →
  `` `${x} b} ``); ours keeps the delimiters.
- **Type names in template expressions are not component properties.** `($event.target as HTMLInputElement)`,
  `(el: Element) => …`, `el as HTMLElement | null`: the compiler erases these types, nothing is read from the
  instance. eslint-plugin-vue counts them as property references (except `as const` and `<script setup generic>`
  names), so `no-undef-properties` reports DOM types as undefined. We skip every type-position name. This removes
  45 false `no-undef-properties` (old corpora: on-nuxt-3 15, ui 13, vue-vben-admin 3, nuxt-i18n-next 1; new:
  directus 7, vitepress 3, hoppscotch 2, slidev 1) and nothing else.
- **`no-unused-refs` does not guess when the refs are reachable elsewhere.** `setup()` returning
  `{ ...api }` (or a non-literal) exposes bindings we cannot list, and a stored or passed-on
  `getCurrentInstance()` exposes `instance.refs` (the Composition API's `this.$refs`): any template ref may be read
  there. eslint-plugin-vue reports every ref then; we report none for that component. Removes 37 false positives
  (element-plus `select`/`select-v2` spread returns 23, `table` 13; vue-pure-admin `Amap` reading
  `instance.refs.mapview` 1). `getCurrentInstance().<other property>` keeps the check.
- **`no-undef-properties` / `no-unused-properties` report nothing in a component with `extends` or non-empty `mixins`.** Vue merges
  those options into the component, and they are usually imported, so any property may be defined there.
  eslint-plugin-vue reports their members: all 2134 reports on primevue (`ptmi`, `cx`, … from
  `extends: BaseComponent`) and 1400 of 2817 on gitlab (mixins). Cost: 1 lost true positive on vue-element-admin
  (`onlyOneChild`, assigned in `data()` but never declared, in a component with an unrelated mixin).
  Likewise `no-unused-properties` skips such components, since the merged options may read any property
  (removes 18: gitlab 11 — several already silenced inline "used by the mixin" —, primevue 6; vue-element-admin 1, a true positive lost: its mixin does not read `autoResize`).
- **`no-ref-as-operand` checks right operands of `&&` / `||` in a test** (`if (a && hasNext)` with a `ref`
  `hasNext`): ported from eslint-plugin-vue's main branch (#3148, after 10.12.1). All 3 such reports on the new
  corpora (halo, vue-pure-admin) are real bugs: the ref object is always truthy.
- **Fixes are checked against Vue, not against ESLint's output.** `tests/fixes.rs` and `scripts/fix-check.mjs`
  run every fix and suggestion on the upstream inputs and the corpora and compile the result with
  @vue/compiler-sfc; where eslint-plugin-vue's fix breaks the code or changes what renders, ours is narrower:
  - CRLF files keep CRLF (ESLint inserts `\n`); no fix writes whitespace or `>` into an unquoted attribute value
    (any rule).
  - `html-self-closing`: Vue's whitespace is ASCII only, so `<div>\u00a0</div>` is not empty (eslint-plugin-vue
    uses JS `trim()` and deletes the text); in `<pre>` / `<textarea>` only an empty element is.
  - `padding-line-between-tags` reports nothing inside `<pre>`; `no-extra-parens` does not report the
    parentheses of a template sequence expression (`{{ (a, b) }}`), which Vue needs.
  - No fix where the result would not compile: `no-useless-v-bind` next to a same-name static attribute or with
    a dynamic argument; `no-deprecated-v-bind-sync` on a plain element or with a non-assignable value;
    `component-options-name-casing` for names that do not convert to an identifier (it wrote `Fio.fio: X`).
  - `require-macro-variable-name`'s suggestion renames uses in the script and the template (eslint-plugin-vue
    renames only the declaration) and is not offered when the new name is taken.
  - `require-name-property` (suggestion), `no-empty-component-block`, `prefer-separate-static-class` and
    `padding-lines-in-component-definition` keep comments; the last one no longer counts comment lines as blank
    lines (eslint-plugin-vue reports two comment lines between single-line properties and deletes the first).
  - Vue drops whitespace between tags only when it holds a line break, and drops whitespace-only text at the
    start / end of an element: `padding-line-between-tags` does not fix a gap with text or a same-line space
    (`</b> <i>`, `a<br />b`), `no-useless-mustaches` does not fix the first / last mustache next to such
    whitespace (6 upstream cases skipped for this).
  - `--fix` runs up to 50 passes (ESLint: 10) and stops on a cycle.

## 6. Known gaps

See [TODO.md](../TODO.md) "Parity gaps". In short: type-aware cases (no TypeScript type checker), a few selector
engines are subsets (`block-order`, `no-restricted-syntax`, indent `ignores`), parser options not supported
(pug, custom tokenizers, `vueFeatures.filter: false`), and `comment-directive`'s template block ranges.
