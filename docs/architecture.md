# Architecture

## Why a Rust core + oxlint shim (decision, 2026-11-12)

Measured on oxlint 1.93.0 (latest at the time):

- JS plugins (`jsPlugins`) receive only the `<script>` text of a `.vue` file: `sourceCode.text` is the script
  content, offsets are script-relative, a report outside it throws `RangeError`, and template-only SFCs are not
  linted at all. So template rules cannot run inside oxlint today, in JS or otherwise.
- oxlint rejects unknown `vue/<rule>` names in `.oxlintrc.json` and reserves the `vue` JS plugin name, so our
  rules live under an alias (`vue-rs/<rule>` by default).
- oxlint has 46 native `vue/*` rules (script-only). eslint-plugin-vue 10.12.1 has 256. This project implements
  the other 210.

Therefore:

1. **Rust core** (`src/`): a port of vue-eslint-parser (HTML tokenizer → template AST with oxc expression ASTs,
   references, variables, tokens, parse errors) and the rules, over oxc 0.158 (parser, semantic). Spans are
   absolute byte offsets into the `.vue` file; the script is parsed from a same-length masked copy of the file,
   template expressions are parsed one by one and their spans shifted.
2. **JS shim plugin** (`npm/index.js`): registers every rule name under `vue-rs`, so the same `.oxlintrc.json`
   validates in oxlint (and in its language server). Presets in `npm/configs/*.json`.
3. **CLI** (`oxlint-vue-rs`): reads that config, lints `.vue` files, and with `--with-oxlint` runs oxlint on the
   same paths and merges the output, so one command covers both.

## Inside oxlint (oxc PR #28720)

With PR #28720 a JS plugin gets `sourceCode.actualText` (the whole file, BOM stripped), `sourceCode.actualOffset`
and `context.report({ actualRange })` (UTF-16 offsets of `actualText`; fixes and suggestions of such a report use
the same coordinates), and a `.vue` file without `<script>` is passed as one empty script section. A file with two
`<script>` blocks is linted once per block; oxlint drops identical `actualRange` reports.

4. **Node addon** (`napi/`, napi-rs, cdylib, not on crates.io): `lint(source, filename, rules, settings, globals,
   prefix)` runs `lint_source` once for all rules and returns problems with UTF-16 ranges, fixes and suggestions
   as JSON. Disable comments (template and script) are applied there, as in the CLI.
5. **Plugin** (`npm/index.js`): every rule is a `createOnce` rule. Its `before` hook returns `false` unless
   `actualText` exists, the file is `.vue` and the addon loads, so released oxlint never runs it; otherwise it
   registers `[rule, context.options]` for the file. The first `Program` visit (oxlint does not run a rule with an
   empty visitor) calls the addon for all registered rules; each rule then reports its own problems. The result
   of the last call is reused when oxlint lints the same file again for its second `<script>` block.
6. **CLI** sets `OXLINT_VUE_RS_CLI=1` for the oxlint it runs, so the plugin stays quiet there and the CLI's own
   pass is the only one (rather than skipping our pass: `--fix-suggestions`, `-A/-W/-D` for our rules and the
   multi-pass `--fix` keep working).

The addon ships next to the CLI binary in each `@oxlint-vue-rs/<platform>` package (`oxlint-vue-rs.node`,
resolved by `npm/platform.js`, shared with the launcher). When oxlint ships language plugins (RFC #21936), the
parser can move there — see [TODO.md](../TODO.md).

Alternatives considered: virtual-file preprocessing in TS (oxlint-vue: same-length `.ts` file, oxlint runs on it
plus a JS template walk) — limited to rules expressible as JS on the virtual file and pays Node startup + JS
template walking; building on vize_patina (168 rules, different semantics and parser) — would not give exact
eslint-plugin-vue parity, which is this project's acceptance criterion.

## Parity testing

- `tests/upstream/*.json`: every eslint-plugin-vue RuleTester case for our 210 rules, with the output of the real
  rule through ESLint (locations, messages, fix output, suggestions). `tests/upstream.rs` replays all of them.
- `tests/parser_fixtures.rs`: vue-eslint-parser's AST fixtures (tree, tokens, errors).
- Real corpora (`scripts/compare-corpus.mjs`): eslint-plugin-vue vs us on thousands of `.vue` files.
