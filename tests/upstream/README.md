# Upstream cases

Every RuleTester case of eslint-plugin-vue **10.12.1** (`tests/lib/rules/**`) for the rules oxlint 1.93.0 does
not implement natively, with the result of running the real rule through ESLint 10 (`actual`): messages with
full locations, single-pass fix output, suggestions. `expected` keeps what the upstream test declared.

Regenerate: clone eslint-plugin-vue, `pnpm install --ignore-scripts`, replace `tests/eslint-compat.ts` with
`scripts/capture/eslint-compat.capture.ts`, add `dist/index.js` = `export { default } from '../lib/plugin.ts'`,
run `CAPTURE_DIR=… vitest run tests/lib/rules`, then split per rule (see `scripts/capture/README.md`).

Dropped: 3 `no-parsing-error` cases whose code holds lone UTF-16 surrogates (not representable in UTF-8).
Type-aware cases (`languageOptions.project`) are skipped by the harness (no type information).

License: MIT, see `LICENSE.eslint-plugin-vue`.
