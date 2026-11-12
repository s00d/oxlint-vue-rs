# TODO

## Integration with oxlint (track upstream)

- [ ] **oxc language plugins** — move to the official path once it lands.
  - RFC: https://github.com/oxc-project/oxc/discussions/21936, umbrella issue https://github.com/oxc-project/oxc/issues/23207,
    phases #23208–#23213, Q4 2026 plan #28275.
  - Shape (proposed): `languagePlugins` config key, `defineLanguagePlugin` returning a native AST + `visitorKeys`
    (+ optional TS transform with mappings to the original file).
  - Plan: expose our Rust SFC/template parser (`src/parser`) as the language plugin's `parse`, and our rules as
    regular rules over that AST; drop the wrapper CLI path.
- [ ] **PR #28720** (`sourceCode.actualText`, `context.report({ actualRange })`, template-only SFCs passed to JS
  plugins): https://github.com/oxc-project/oxc/pull/28720. When merged, the JS shim plugin calls the Rust core
  (NAPI) and reports into `<template>` directly inside oxlint, LSP included.
- [ ] Custom parsers for JS plugins (closed PR #24262, "likely to come back in the new year").
- [ ] Rust upstream helpers worth adopting: #29123 (parse a JS expression at an offset), #29235 (template nodes in
  the script's scope analysis), `oxc-htmlish-parser` crate (official template parser, ranges only today).

## Parity gaps (known)

- [ ] `comment-directive`: a block `<!-- eslint-disable -->` in the template should stop at `</template>`;
  a bare `eslint-enable` should also re-enable rules that were disabled by name.
- [ ] `no-parsing-error`: `v-bind()` expressions inside `<style>` are not parsed (upstream case invalid80).
- [ ] `block-order`: selector engine subset (no combinators, `:has`, `:nth-*`; such selectors never match).
- [ ] `script-indent`: oxc 0.158 fatal parse error on type arguments on a new line in `implements` (2 upstream cases); `<script setup generic>` type prefix and decorator-before-export ranges not ported.
- [ ] `script-indent`/`html-indent` `ignores` option: selector subset (type, `*`, `[attr=v]`, `.field`, descendant/child).
- [ ] tpl_ext (stylistic extension rules) skips every TS type node in templates; ESLint skips only TS-only fields (typeAnnotation, typeArguments, typeParameters, returnType) and still visits e.g. literal types after `as`. Align.
- [ ] no-restricted-syntax: esquery subset; `VDocumentFragment` selectors never match.
