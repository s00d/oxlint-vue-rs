#!/usr/bin/env node
// Generates npm/configs/*.json from eslint-plugin-vue's own flat configs, so each preset is the whole
// upstream config: `plugins: ["vue"]` + oxlint's native `vue/*` rules + our `vue-rs/*` rules, same
// severities and options. Native `vue/*` rules the upstream config does not enable are set to "off":
// `plugins: ["vue"]` alone would turn on oxlint's whole vue correctness category.
//
//   node scripts/gen-presets.mjs [dir]   # dir: where eslint-plugin-vue resolves from (default: cwd)
//
// npm/configs/<name>.json           rules in `rules`, shim in `jsPlugins` (oxlint validates the names)
// npm/configs/settings/<name>.json  our rules in `settings["vue-rs"].rules`, no `jsPlugins` (plain oxlint
//                                   skips its JS plugin pipeline; oxlint-vue-rs reads them from settings)
import fs from 'node:fs'
import path from 'node:path'
import { createRequire } from 'node:module'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('..', import.meta.url))
const from = path.resolve(process.argv[2] ?? '.')
const vue = createRequire(path.join(from, 'noop.js'))('eslint-plugin-vue')
const inv = JSON.parse(fs.readFileSync(path.join(root, 'tests/rules-inventory.json'), 'utf8'))
const native = new Set(inv.native)
const ours = new Set(JSON.parse(fs.readFileSync(path.join(root, 'npm/rules.json'), 'utf8')).map((r) => r.name))

const presets = {
  essential: 'flat/essential',
  'strongly-recommended': 'flat/strongly-recommended',
  recommended: 'flat/recommended',
  'vue2-essential': 'flat/vue2-essential',
  'vue2-strongly-recommended': 'flat/vue2-strongly-recommended',
  'vue2-recommended': 'flat/vue2-recommended',
}
const sorted = (o) => Object.fromEntries(Object.entries(o).sort(([a], [b]) => a.localeCompare(b)))
const write = (rel, v) => {
  const p = path.join(root, 'npm/configs', rel)
  fs.mkdirSync(path.dirname(p), { recursive: true })
  fs.writeFileSync(p, JSON.stringify(v, null, 2) + '\n')
}

fs.rmSync(path.join(root, 'npm/configs/settings'), { recursive: true, force: true })
for (const [name, key] of Object.entries(presets)) {
  const flat = vue.configs[key]
  if (!flat) throw new Error(`eslint-plugin-vue has no config ${key}`)
  const nat = {}
  const rs = {}
  for (const [id, v] of flat.flatMap((c) => Object.entries(c.rules ?? {}))) {
    const rule = id.replace(/^vue\//, '')
    if (native.has(rule)) nat[`vue/${rule}`] = v
    else if (ours.has(rule)) rs[`vue-rs/${rule}`] = v
    else throw new Error(`${key}: ${id} is neither an oxlint native rule nor ours`)
  }
  for (const rule of native) nat[`vue/${rule}`] ??= 'off'
  write(`${name}.json`, {
    plugins: ['vue'],
    jsPlugins: [{ name: 'vue-rs', specifier: 'oxlint-vue-rs' }],
    rules: { ...sorted(nat), ...sorted(rs) },
  })
  write(`settings/${name}.json`, {
    plugins: ['vue'],
    rules: sorted(nat),
    settings: { 'vue-rs': { rules: sorted(rs) } },
  })
}
console.log(`eslint-plugin-vue ${vue.meta?.version ?? '?'}: ${Object.keys(presets).length} presets written`)
