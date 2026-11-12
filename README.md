# oxlint-vue-rs

[![npm version](https://img.shields.io/npm/v/oxlint-vue-rs/latest?style=for-the-badge)](https://www.npmjs.com/package/oxlint-vue-rs)
[![Crates.io](https://img.shields.io/crates/v/oxlint-vue-rs?style=for-the-badge)](https://crates.io/crates/oxlint-vue-rs)
[![CI](https://img.shields.io/github/actions/workflow/status/s00d/oxlint-vue-rs/ci.yml?branch=main&style=for-the-badge&label=CI)](https://github.com/s00d/oxlint-vue-rs/actions/workflows/ci.yml)
[![GitHub release](https://img.shields.io/github/v/release/s00d/oxlint-vue-rs?style=for-the-badge)](https://github.com/s00d/oxlint-vue-rs/releases)
[![eslint-plugin-vue](https://img.shields.io/badge/eslint--plugin--vue-10.12.1-4fc08d?style=for-the-badge)](docs/rules.md)
[![Rules](https://img.shields.io/badge/rules-210%20%2B%2046%20native-4fc08d?style=for-the-badge)](docs/rules.md)
[![MSRV](https://img.shields.io/badge/MSRV-1.97-blue?style=for-the-badge)](https://github.com/s00d/oxlint-vue-rs/blob/main/Cargo.toml)
[![Platform](https://img.shields.io/badge/platform-Linux%20%7C%20macOS%20%7C%20Windows-blue?style=for-the-badge)](https://github.com/s00d/oxlint-vue-rs/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT-green?style=for-the-badge)](https://github.com/s00d/oxlint-vue-rs/blob/main/LICENSE)
[![GitHub issues](https://img.shields.io/github/issues/s00d/oxlint-vue-rs?style=for-the-badge)](https://github.com/s00d/oxlint-vue-rs/issues)
[![GitHub stars](https://img.shields.io/github/stars/s00d/oxlint-vue-rs?style=for-the-badge)](https://github.com/s00d/oxlint-vue-rs/stargazers)
[![Donate](https://img.shields.io/badge/Donate-Donationalerts-ff4081?style=for-the-badge)](https://www.donationalerts.com/r/s00d88)

All of [eslint-plugin-vue](https://eslint.vuejs.org/) for projects that use [oxlint](https://oxc.rs): the
**210 rules oxlint does not have** — `<template>` rules included — written in Rust on the oxc toolchain.

- **Same behaviour as eslint-plugin-vue 10.12.1:** rule names, options, messages, locations, fixes and
  suggestions. 6839/6878 of eslint-plugin-vue's own test cases pass, and on 9334 real `.vue` files (20 projects)
  the output is identical except for a few deliberate fixes of upstream false positives —
  [compatibility report](docs/compatibility.md).
- **One command for everything:** `oxlint-vue-rs` runs oxlint and the Vue rules on the same `.oxlintrc.json` and
  prints one report.
- **Fast:** all 210 rules over those 4121 files in 2.3 s (eslint-plugin-vue: 45 s).

| | oxlint | oxlint-vue-rs |
|---|---|---|
| `vue/*` rules for `<script>` | 46, native (`vue/<rule>`) | — |
| every other eslint-plugin-vue rule, `<template>` included | — | 210 (`vue-rs/<rule>`) |
| config | `.oxlintrc.json` | the same file |

## Install

```bash
npm i -D oxlint oxlint-vue-rs     # pnpm add -D / yarn add -D work the same
```

The npm package holds the oxlint plugin, the presets and a small launcher. The native binary comes from one
platform package, `@oxlint-vue-rs/<os>-<cpu>[-gnu|-musl]`, which npm picks for your machine (macOS arm64/x64,
Linux x64/arm64 glibc and musl, Windows x64/arm64). Nothing is compiled on install.

Without Node: `cargo install oxlint-vue-rs` (Rust 1.97+), or a binary from the
[releases](https://github.com/s00d/oxlint-vue-rs/releases).

## Quick start

`.oxlintrc.json` — a preset that reproduces eslint-plugin-vue's `flat/recommended` (oxlint's native `vue/*` rules
plus ours):

```json
{ "extends": ["./node_modules/oxlint-vue-rs/configs/recommended.json"] }
```

`package.json`:

```json
{ "scripts": { "lint": "oxlint-vue-rs", "lint:fix": "oxlint-vue-rs --fix" } }
```

```bash
npm run lint
```

## How it fits with oxlint

oxlint (1.93) hands JS plugins only the `<script>` part of a `.vue` file, so template rules cannot run inside
oxlint yet. oxlint-vue-rs ships two pieces:

1. an oxlint **JS plugin** (alias `vue-rs`) that registers every rule name, so the rules can be configured in
   `.oxlintrc.json` and oxlint and its editor extension accept the config;
2. the **`oxlint-vue-rs` binary**, which reads the same config, lints `.vue` files and — when oxlint is installed —
   also runs oxlint on the same paths and prints one merged report (`--no-oxlint` runs only the Vue rules).

When oxlint lets plugins see the whole SFC (oxc PR #28720, or "language plugins"), the plugin will call the same
Rust core directly — see [TODO.md](TODO.md) and [docs/architecture.md](docs/architecture.md).

## Configure

```jsonc
// .oxlintrc.json
{
  "plugins": ["vue"],                       // oxlint's native vue/* rules
  "jsPlugins": [{ "name": "vue-rs", "specifier": "oxlint-vue-rs" }],
  "rules": {
    "vue/no-dupe-keys": "error",            // native: oxlint runs it
    "vue-rs/html-indent": ["warn", 2],      // ours: oxlint-vue-rs runs it
    "vue-rs/valid-v-for": "error",
    "vue-rs/attributes-order": ["warn", { "alphabetical": true }]
  },
  "overrides": [{ "files": ["**/legacy/**"], "rules": { "vue-rs/html-indent": "off" } }]
}
```

- `vue` is reserved by oxlint, so our rules use the plugin's alias (`vue-rs` by default). Options are exactly
  eslint-plugin-vue's. A `vue/<rule>` key for one of our rules (a leftover of an ESLint config) is ignored with a
  warning naming the key to use; oxlint itself rejects it.
- **Presets** (`configs/<name>.json`): `essential`, `strongly-recommended`, `recommended`, `vue2-essential`,
  `vue2-strongly-recommended`, `vue2-recommended` — eslint-plugin-vue's configs, complete: `plugins: ["vue"]`, the
  native `vue/*` rules at upstream severity and our `vue-rs/*` rules (generated by `scripts/gen-presets.mjs`).
- **Config keys read:** `rules`, `overrides` (`files` globs), `ignorePatterns`, `settings`, `globals`, `extends`
  (file paths), `jsPlugins` (to find the alias). Nested `.oxlintrc.json` files apply per directory, as in oxlint.
  Only JSON configs are read; with only an `oxlint.config.ts` present, oxlint-vue-rs warns and exits 2.
- **Disable comments:** `// eslint-disable…` / `// oxlint-disable…` in scripts, `<!-- eslint-disable… -->` in
  templates.

### Without the JS plugin (faster plain `oxlint`)

With the JS plugin loaded, plain `oxlint` starts its JS plugin pipeline (~2× slower on large repos). If you also
run plain `oxlint` (editor, pre-commit), put our rules in `settings["vue-rs"].rules` and drop `jsPlugins` —
oxlint ignores `settings`, oxlint-vue-rs reads it like `rules`. Presets: `configs/settings/<name>.json`.
Trade-off: oxlint no longer checks those rule names.

```jsonc
{
  "plugins": ["vue"],
  "settings": { "vue-rs": { "rules": { "html-indent": ["warn", 2], "valid-v-for": "error" } } }
}
```

### Yarn PnP

The binary works, but oxlint cannot load the JS plugin there and `./node_modules/…` preset paths do not exist: use
`nodeLinker: node-modules` in `.yarnrc.yml`, or the setup without the JS plugin.

## CLI

```bash
oxlint-vue-rs                    # current directory: oxlint (if installed) + Vue rules, one report
oxlint-vue-rs src --no-oxlint    # only the Vue rules
oxlint-vue-rs --fix              # apply fixes (repeats until stable, max 10 passes)
oxlint-vue-rs -D all src         # every rule as an error, ignoring the config's rule list
oxlint-vue-rs -f json            # json (oxlint-like) | unix | github | default
oxlint-vue-rs --rules            # list rules
```

| Option | |
|---|---|
| `-c, --config <file>` | config file (default: nearest `.oxlintrc.json`) |
| `-A/-W/-D <rule>` | allow / warn / deny a rule (`vue-rs/x`, `vue/x` or `x`; `all`); other names go to oxlint |
| `--fix`, `--fix-suggestions` | apply fixes; also the first suggestion of each problem |
| `-f, --format` | `default`, `json`, `unix`, `github` |
| `--quiet`, `--deny-warnings`, `--max-warnings <n>` | as in oxlint |
| `--ignore-pattern <glob>`, `--no-ignore`, `--hidden` | file selection (`.gitignore` honoured); the first two also go to oxlint |
| `--no-inline-config` | ignore disable comments |
| `--threads <n>` | worker threads (default: all cores) |
| `--no-oxlint` / `--with-oxlint` | skip oxlint / require it (default: run it when found); `-- <args>` go to oxlint |
| `--disable-nested-config`, `--print-config` | one config for all files; print the resolved rules |

Exit code: 0 clean, 1 problems (or warnings with `--deny-warnings` / over `--max-warnings`, or a file `--fix`
could not write), 2 usage or config error. `OXLINT_VUE_RS_BINARY=/path/to/binary` overrides the npm binary.

## Documentation

| | |
|---|---|
| [docs/rules.md](docs/rules.md) | every eslint-plugin-vue rule: native in oxlint or here, upstream tests and real-project results per rule |
| [docs/compatibility.md](docs/compatibility.md) | parity with eslint-plugin-vue: upstream suite, 9 real projects, benchmark, robustness |
| [docs/architecture.md](docs/architecture.md) | why a Rust core + oxlint plugin, how it is built |
| [docs/mutation.md](docs/mutation.md) | mutation testing |
| [docs/releasing.md](docs/releasing.md) | crates.io, GitHub release, npm |
| [TODO.md](TODO.md) | oxlint integration roadmap, known parity gaps |
| [CHANGELOG.md](CHANGELOG.md) | changes |

## Development

```bash
cargo test                                   # upstream cases, parser fixtures, robustness, CLI, unit tests
UPSTREAM_RULE=html-indent UPSTREAM_VERBOSE=1 cargo test --test upstream
cargo clippy --all-targets -- -D warnings && cargo fmt --check
scripts/bench.sh <corpora> <node-ref-dir> <eslint-config>     # benchmark
```

| Path | |
|---|---|
| `src/parser/` | vue-eslint-parser port (HTML tokenizer, template AST, oxc expressions, tokens, errors) |
| `src/rules/` | one file per rule |
| `src/utils/` | shared helpers, ESTree-shaped tree and ESLint token store for the stylistic/indent rules |
| `src/config.rs`, `src/directives.rs`, `src/main.rs` | `.oxlintrc.json`, disable comments, CLI |
| `npm/`, `scripts/npm/` | oxlint plugin, presets, launcher; packaging and publishing |
| `tests/upstream/` | eslint-plugin-vue's RuleTester cases with real ESLint results (MIT, license file there) |
| `tests/parser-fixtures/` | vue-eslint-parser AST fixtures (MIT) |

## License

MIT. Test data derived from eslint-plugin-vue and vue-eslint-parser (both MIT) keeps their license files.
