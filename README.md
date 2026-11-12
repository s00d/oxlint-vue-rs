# oxlint-vue-rs

All of [eslint-plugin-vue](https://eslint.vuejs.org/) for projects that use [oxlint](https://oxc.rs): the
**210 rules oxlint does not have** — `<template>` rules included — written in Rust on the oxc toolchain.

- **Same behaviour as eslint-plugin-vue 10.12.1.** Same rule names, options, messages, locations, fixes and
  suggestions; 6841/6878 of eslint-plugin-vue's own test cases pass (the rest need a TypeScript type checker or
  parser options we don't support), and on 9 real projects (4121 `.vue` files) the output is identical to
  eslint-plugin-vue: 151 548 matching problems, 0 missing, 0 extra. See [docs/compatibility.md](docs/compatibility.md).
- **Fast.** All 210 rules over those 4121 files take 2.3 s (eslint-plugin-vue: 45 s).
- **Same config as oxlint.** Rules go in your `.oxlintrc.json`; oxlint keeps its 46 native `vue/*` rules.

## How it fits with oxlint

oxlint (1.93) hands JS plugins only the `<script>` part of a `.vue` file, so template rules cannot run inside
oxlint yet. oxlint-vue-rs therefore ships two pieces:

1. an oxlint **JS plugin** (`oxlint-vue-rs`, alias `vue-rs`) that registers every rule name, so the rules can be
   configured in `.oxlintrc.json` and oxlint (and its editor extension) accepts the config;
2. the **`oxlint-vue-rs` binary**, which reads the same config and lints `.vue` files — and, when oxlint is
   installed, also runs oxlint on the same paths and prints one merged report. So `oxlint-vue-rs` replaces the
   `oxlint` command in your scripts (`--no-oxlint` to run only the Vue rules).

When oxlint lets plugins see the whole SFC (oxc PR #28720, or "language plugins"), the plugin will call the same
Rust core directly — see [TODO.md](TODO.md) and [docs/architecture.md](docs/architecture.md).

## Install

```bash
npm i -D oxlint oxlint-vue-rs     # pnpm add -D / yarn add -D work the same
npx oxlint-vue-rs src               # runs oxlint + the Vue rules, one report
```

The npm package carries the oxlint plugin, the presets and a small launcher; the native binary comes from a
platform package (`oxlint-vue-rs-<os>-<cpu>[-gnu|-musl]`, installed automatically through
`optionalDependencies`: macOS arm64/x64, Linux x64/arm64 glibc and musl, Windows x64/arm64). Nothing is compiled on
install. `OXLINT_VUE_RS_BINARY=/path/to/binary` overrides the binary.

From source: `cargo install --path .` (Rust 1.97+).

Yarn PnP: the binary works, but oxlint cannot load the JS plugin (`Cannot find module 'oxlint-vue-rs'`) and the
`./node_modules/...` preset paths do not exist. Use `nodeLinker: node-modules` in `.yarnrc.yml`, or the setup
without the JS plugin (below).

### Releasing

The `release` GitHub workflow (manual) builds all platform binaries, assembles the packages with
`scripts/npm/pack.mjs` and, when run with `publish: true`, publishes them to npm and creates the GitHub release.
A re-run skips versions already on npm and updates an existing release, so a failed run can simply be repeated.
npm auth: with the `NPM_TOKEN` secret set (a granular automation token with publish rights) it is used; with
the secret unset, npm uses [trusted publishing](https://docs.npmjs.com/trusted-publishers) (OIDC, npm ≥ 11.5,
installed by the workflow), which needs each package linked to this repo and workflow on npmjs.com. Trusted
publishing cannot create a package, so the first release (0.1.0) needs the token. Not published yet.

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

`vue` is reserved by oxlint, so our rules use the alias you give the plugin (`vue-rs` by default). Options are
exactly eslint-plugin-vue's. A `vue/<rule>` key for one of our rules (the usual leftover of an ESLint config) is
ignored with a warning naming the key to use; oxlint itself rejects it.

Presets reproduce eslint-plugin-vue's configs completely: `plugins: ["vue"]`, oxlint's native `vue/*` rules at
upstream severity (the other native `vue/*` rules off, as upstream), and our `vue-rs/*` rules. They are in
`npm/configs/`: `essential`, `strongly-recommended`, `recommended`, and `vue2-*` (generated from
eslint-plugin-vue by `scripts/gen-presets.mjs`):

```jsonc
{ "extends": ["./node_modules/oxlint-vue-rs/configs/recommended.json"] }
```

### Setup without the JS plugin (faster plain `oxlint`)

The JS plugin does nothing at lint time, but with it plain `oxlint` starts its JS plugin pipeline: on
element-plus (2120 files) `oxlint` takes ~0.2 s with the recommended preset vs ~0.09 s without the plugin
(~2×; median of 12 runs, oxlint 1.93). If you run plain `oxlint` (editor, pre-commit) and care about that, put
our rules in `settings["vue-rs"].rules` instead and drop `jsPlugins`. oxlint ignores `settings`; oxlint-vue-rs
reads rules from there as from `rules`:

```jsonc
{
  "plugins": ["vue"],
  "settings": { "vue-rs": { "rules": { "vue-rs/html-indent": ["warn", 2], "valid-v-for": "error" } } }
}
```

Presets for it: `configs/settings/<name>.json`. Trade-off: oxlint (and its editor extension) no longer checks
those rule names, so a typo is only reported by oxlint-vue-rs (as `unknown rule`).

Supported config keys: `rules`, `overrides` (`files` globs), `ignorePatterns`, `settings` (incl.
`settings["vue-rs"].rules`), `globals`, `extends` (file paths), `jsPlugins` (to find the alias). Only JSON configs
are read (`.oxlintrc.json`, `.oxlintrc.jsonc`, `oxlintrc.json`); oxlint's JS/TS configs (`oxlint.config.ts` …)
are not. When one is found and no JSON config is, oxlint-vue-rs warns and exits 2 (nothing would be checked),
unless `-W`/`-D` enable rules on the command line; use a `.oxlintrc.json` or `-c <file.json>`. Inline `// eslint-disable…` / `// oxlint-disable…` comments work
in scripts, and `<!-- eslint-disable… -->` in templates.

## Run

```bash
oxlint-vue-rs src                 # oxlint (if installed) + Vue rules, same .oxlintrc.json, merged output
oxlint-vue-rs --no-oxlint src     # only the Vue rules
oxlint-vue-rs --fix src           # apply fixes (repeats until stable, max 10 passes)
oxlint-vue-rs -D all src          # every rule as an error, ignoring the config's rule list
oxlint-vue-rs -f json src         # json (oxlint-like) | unix | github | default
oxlint-vue-rs --rules             # list rules
```

| Option | |
|---|---|
| `-c, --config <file>` | config file (default: nearest `.oxlintrc.json`) |
| `-A/-W/-D <rule>` | allow / warn / deny a rule (`vue-rs/x`, `vue/x` or `x`; `all`); other names go to oxlint |
| `--fix`, `--fix-suggestions` | apply fixes; also the first suggestion of each problem |
| `-f, --format` | `default`, `json`, `unix`, `github` |
| `--quiet`, `--deny-warnings`, `--max-warnings <n>` | as in oxlint |
| `--ignore-pattern <glob>`, `--no-ignore`, `--hidden` | file selection (`.gitignore` is honoured); the first two also go to oxlint |
| `--no-inline-config` | ignore disable comments |
| `--threads <n>` | worker threads (default: all cores) |
| `--no-oxlint` / `--with-oxlint` | skip oxlint / require it (default: run it when found); `-- <args>` go to oxlint |
| `--print-config` | resolved rules |

Exit code: 0 clean, 1 problems (or warnings with `--deny-warnings` / over `--max-warnings`, or a file `--fix`
could not write, e.g. read-only), 2 usage/config error (including an `oxlint.config.*` with no JSON config).

## Rules

[docs/rules.md](docs/rules.md) lists every eslint-plugin-vue rule: native in oxlint or implemented here, with
upstream test results and real-project results per rule. Known differences: [TODO.md](TODO.md) "Parity gaps".

## Develop

```bash
cargo test                                   # upstream cases, parser fixtures, robustness, unit tests
UPSTREAM_RULE=html-indent UPSTREAM_VERBOSE=1 cargo test --test upstream
cargo clippy --all-targets -- -D warnings && cargo fmt --check
scripts/bench.sh <corpora> <node-ref-dir> <eslint-config>     # benchmark
node scripts/compare-corpus.mjs eslint.json ours.json --implemented list.txt
```

Layout: `src/parser/` (vue-eslint-parser port), `src/ast.rs` (template AST), `src/rules/` (one file per rule),
`src/utils/` (shared helpers, ESTree-shaped tree and ESLint token store for the stylistic/indent rules),
`src/config.rs`, `src/directives.rs`, `src/main.rs` (CLI), `npm/` (oxlint plugin + presets),
`tests/upstream/` (eslint-plugin-vue's cases with real ESLint results; MIT, see the license file there),
`tests/parser-fixtures/` (vue-eslint-parser fixtures, MIT).

## License

MIT. Test data derived from eslint-plugin-vue and vue-eslint-parser (both MIT) keeps their license files.
