// oxlint JS plugin for oxlint-vue-rs.
//
// With an oxlint that gives JS plugins the whole file (oxc PR #28720: `sourceCode.actualText`,
// `context.report({ actualRange })`, template-only SFCs passed to plugins), the rules run here: every
// enabled `vue-rs/*` rule registers itself for the file in its `before` hook, the first `Program` visit
// runs the Rust core (Node addon from the platform package) once for all of them on `actualText`, and
// each rule reports its own problems. (`Program` because oxlint never runs a rule with an empty visitor.)
//
// Released oxlint (<= 1.93) shows plugins only the `<script>` text: then every `before` hook returns
// false and the plugin only registers the rule names, so `vue-rs/<rule>` entries validate in
// .oxlintrc.json; the `oxlint-vue-rs` binary (same config) does the linting. That binary sets
// OXLINT_VUE_RS_CLI when it runs oxlint itself, so the rules are not reported twice.
import { createRequire } from 'node:module'
import { addon, resolve } from './platform.js'

const require = createRequire(import.meta.url)
const list = require('./rules.json')

let core // undefined = not loaded yet, null = unavailable
function loadCore() {
  if (core !== undefined) return core
  try {
    // process.dlopen loads the native module whatever the file extension (an OXLINT_VUE_RS_ADDON override may
    // point at a cargo build output such as liboxlint_vue_rs_napi.dylib, which `require` would parse as JS).
    const mod = { exports: {} }
    process.dlopen(mod, process.env.OXLINT_VUE_RS_ADDON || resolve(addon))
    core = mod.exports
  } catch (e) {
    core = null
    console.error(`oxlint-vue-rs: Node addon not available, vue-rs rules are skipped (${e.message})`)
  }
  return core
}

// The file being linted: rules registered by `before` hooks, problems by rule once the core ran.
let file = null
// Last core run, reused when oxlint lints the same file again for its next `<script>` block.
let last = { key: null, byRule: null }

function before(context, name) {
  if (process.env.OXLINT_VUE_RS_CLI) return false
  if (!('actualText' in context.sourceCode) || !context.filename.endsWith('.vue') || !loadCore()) return false
  if (file === null || file.byRule !== null) file = { rules: [], byRule: null }
  file.rules.push([name, context.options ?? []])
  return true
}

function run(context) {
  const args = [
    context.sourceCode.actualText,
    context.filename,
    JSON.stringify(file.rules),
    JSON.stringify(context.settings ?? {}),
    JSON.stringify(context.languageOptions.globals ?? {}),
    context.id.slice(0, context.id.indexOf('/')),
  ]
  const key = args.join('\0')
  if (last.key !== key) {
    const byRule = new Map()
    for (const d of JSON.parse(core.lint(...args))) {
      if (!byRule.has(d.rule)) byRule.set(d.rule, [])
      byRule.get(d.rule).push(d)
    }
    last = { key, byRule }
  }
  return last.byRule
}

function report(context, name) {
  file.byRule ??= run(context)
  for (const d of file.byRule.get(name) ?? []) {
    context.report({
      message: d.message,
      actualRange: d.range,
      fix: d.fix ? () => d.fix : undefined,
      suggest: d.suggestions.length
        ? d.suggestions.map((s) => ({ desc: s.desc, fix: () => s.fix }))
        : undefined,
    })
  }
}

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
      createOnce(context) {
        return { before: () => before(context, r.name), Program: () => report(context, r.name) }
      },
    },
  ]),
)

export default { meta: { name: 'vue-rs' }, rules }
