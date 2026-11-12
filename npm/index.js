// oxlint JS plugin shim for oxlint-vue-rs.
//
// oxlint (<= 1.93) gives JS plugins only the `<script>` text of a .vue file, so template rules cannot
// run inside oxlint yet. This shim registers every rule name so `vue-rs/<rule>` entries validate in
// .oxlintrc.json; the `oxlint-vue-rs` binary (same config) does the linting. Once oxlint exposes the
// whole SFC to plugins (oxc PR #28720 / language plugins), these rules will call the Rust core here.
import { createRequire } from 'node:module'

const require = createRequire(import.meta.url)
const list = require('./rules.json')

const rules = Object.fromEntries(
  list.map((r) => [
    r.name,
    {
      meta: {
        type: r.type,
        fixable: r.fixable ?? undefined,
        hasSuggestions: r.hasSuggestions || undefined,
        deprecated: r.deprecated || undefined,
        docs: { url: `https://eslint.vuejs.org/rules/${r.name}.html` },
        schema: false,
      },
      create() {
        return {}
      },
    },
  ]),
)

export default { meta: { name: 'vue-rs' }, rules }
