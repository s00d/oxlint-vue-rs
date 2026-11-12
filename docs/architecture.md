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

When oxlint lets plugins see the whole SFC (oxc PR #28720) or ships language plugins (RFC #21936), the shim
calls the same core through NAPI and the CLI step disappears — see [TODO.md](../TODO.md).

Alternatives considered: virtual-file preprocessing in TS (oxlint-vue: same-length `.ts` file, oxlint runs on it
plus a JS template walk) — limited to rules expressible as JS on the virtual file and pays Node startup + JS
template walking; building on vize_patina (168 rules, different semantics and parser) — would not give exact
eslint-plugin-vue parity, which is this project's acceptance criterion.

## Parity testing

- `tests/upstream/*.json`: every eslint-plugin-vue RuleTester case for our 210 rules, with the output of the real
  rule through ESLint (locations, messages, fix output, suggestions). `tests/upstream.rs` replays all of them.
- `tests/parser_fixtures.rs`: vue-eslint-parser's AST fixtures (tree, tokens, errors).
- Real corpora (`scripts/compare-corpus.mjs`): eslint-plugin-vue vs us on thousands of `.vue` files.
