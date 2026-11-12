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

| | |
|---|---:|
| matching problems | **151 548** |
| missing (ESLint reports, we don't) | **0** |
| message differences | **0** |
| extra (we report, ESLint doesn't) | **0** |
| reports in files ESLint failed to parse | 99 |

The 99 are in 10 files where ESLint stopped with a parse error (`lang="tsx"` blocks and JSX in a plain
`<script>`; the reference config had no JSX enabled for them); we parse and lint those files.

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

## 5. Known gaps

See [TODO.md](../TODO.md) "Parity gaps". In short: type-aware cases (no TypeScript type checker), a few selector
engines are subsets (`block-order`, `no-restricted-syntax`, indent `ignores`), parser options not supported
(pug, custom tokenizers, `vueFeatures.filter: false`), and `comment-directive`'s template block ranges.
