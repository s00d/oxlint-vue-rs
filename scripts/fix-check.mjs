#!/usr/bin/env node
// Fix-verification harness: runs every fixable rule (and every suggestion) over many inputs and checks
// each fix for validity, behaviour preservation, convergence and lost text.
//
// usage:
//   node scripts/fix-check.mjs upstream --ref <node-dir> --out <dir> [--variants] [--rules a,b] [--bin <fix_check>]
//   node scripts/fix-check.mjs corpus <dir>... --ref <node-dir> --out <dir> [--variants] [--rules a,b] [--sample N]
//
// <node-dir> holds node_modules with eslint-plugin-vue (rule metadata and option schemas),
// @vue/compiler-sfc and @babel/parser. Inputs are only read, never written.
//
// Pipeline: build units (inputs) + configs (rule × option variant × mode) → `examples/fix_check.rs`
// (fix loop, our parser, oxc script ASTs, lost text; tests/fixcheck/mod.rs) → this script checks every
// changed output with @vue/compiler-sfc (parse, compileTemplate, compileScript) and @babel/parser, and
// compares the template render code before/after (Babel AST without locations: exact, and with text
// whitespace collapsed). Writes <out>/findings.jsonl (one line per problem) and <out>/summary.txt.
//
// Modes: `fix` = --fix loop of one rule; `suggest` = --fix-suggestions loop; `each` = every suggestion
// applied alone; `all` = every fixable rule at default options together (+ suggestions).
import fs from 'node:fs'
import path from 'node:path'
import { createRequire } from 'node:module'
import { execFileSync, spawn } from 'node:child_process'
import readline from 'node:readline'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const argv = process.argv.slice(2)
const flag = (n) => argv.includes(n)
const opt = (n) => {
  const i = argv.indexOf(n)
  return i >= 0 ? argv[i + 1] : undefined
}
const refDir = path.resolve(opt('--ref') ?? '.')
const req = createRequire(path.join(refDir, 'package.json'))
const outDir = path.resolve(opt('--out') ?? 'fix-check-out')

// ---------------------------------------------------------------------------------------------
// Worker: analyze one shard of records (spawned by the main process).
if (argv[0] === '--shard') {
  await shard(Number(argv[1]), Number(argv[2]))
  process.exit(0)
}

const mode = argv[0]
const onlyRules = opt('--rules')?.split(',')
fs.mkdirSync(outDir, { recursive: true })

// --- rules ---------------------------------------------------------------------------------------
const plugin = req('eslint-plugin-vue')
const inventory = JSON.parse(fs.readFileSync(path.join(ROOT, 'tests/rules-inventory.json'), 'utf8'))
const ours = new Set(inventory.missing)
const fixable = []
const suggesting = []
for (const [name, r] of Object.entries(plugin.rules)) {
  if (!ours.has(name) || (onlyRules && !onlyRules.includes(name))) continue
  if (r.meta.fixable) fixable.push(name)
  if (r.meta.hasSuggestions) suggesting.push(name)
}

// --- option variants from the rule's JSON schema + the upstream test cases ------------------------
function resolve(s, root) {
  while (s && s.$ref) {
    const parts = s.$ref.replace(/^#\/?/, '').split('/').filter(Boolean)
    s = parts.reduce((o, k) => o?.[k], root)
  }
  return s
}
function values(s, root, depth = 0) {
  s = resolve(s, root)
  if (!s || depth > 3) return []
  if (s.enum) return s.enum
  if (s.const !== undefined) return [s.const]
  const alts = s.anyOf ?? s.oneOf
  if (alts) return alts.flatMap((a) => values(a, root, depth + 1))
  const t = Array.isArray(s.type) ? s.type : [s.type]
  const out = []
  if (t.includes('boolean')) out.push(true, false)
  if (t.includes('integer') || t.includes('number')) {
    const min = s.minimum ?? 0
    for (const n of [0, 1, 2, 4]) if (n >= min) out.push(n)
  }
  if (t.includes('object') && s.properties) {
    for (const [k, p] of Object.entries(s.properties)) {
      for (const v of values(p, root, depth + 1)) out.push({ [k]: v })
    }
    if (!out.length) out.push({})
  }
  return out
}
function schemaVariants(name) {
  const schema = plugin.rules[name].meta.schema
  if (!schema) return []
  const items = Array.isArray(schema) ? schema : schema.type === 'array' ? (schema.items ?? []) : [schema]
  const list = Array.isArray(items) ? items : [items]
  const root = Array.isArray(schema) ? { items: schema } : schema
  const out = []
  list.forEach((s, i) => {
    const fill = list.slice(0, i).map((p) => values(p, root)[0])
    if (fill.some((v) => v === undefined)) return
    for (const v of values(s, root)) out.push([...fill, v])
  })
  return out
}
const upstream = {}
for (const f of fs.readdirSync(path.join(ROOT, 'tests/upstream'))) {
  if (f.endsWith('.json')) upstream[f.slice(0, -5)] = JSON.parse(fs.readFileSync(path.join(ROOT, 'tests/upstream', f), 'utf8'))
}
function variants(name) {
  const seen = new Map([['[]', []]])
  if (flag('--variants')) {
    for (const v of schemaVariants(name)) seen.set(JSON.stringify(v), v)
    for (const c of upstream[name] ?? []) {
      if (c.options?.length) seen.set(JSON.stringify(c.options), c.options)
    }
  }
  return [...seen.values()].slice(0, 60)
}

const configs = []
for (const name of fixable) for (const o of variants(name)) configs.push({ rules: [[name, o]], mode: 'fix' })
for (const name of suggesting) {
  for (const o of variants(name)) {
    configs.push({ rules: [[name, o]], mode: 'each' })
    configs.push({ rules: [[name, o]], mode: 'suggest' })
  }
}
if (!onlyRules || flag('--all')) {
  configs.push({ rules: fixable.map((n) => [n, []]), mode: 'fix', all: true })
  configs.push({ rules: [...new Set([...fixable, ...suggesting])].map((n) => [n, []]), mode: 'suggest', all: true })
}

// --- units ---------------------------------------------------------------------------------------
const units = []
if (mode === 'upstream') {
  const seen = new Set()
  for (const cases of Object.values(upstream)) {
    for (const c of cases) {
      const lo = c.languageOptions ?? {}
      const filename = c.filename ?? 'test.js'
      const parser = lo.parser ?? 'espree'
      const sp = lo.scriptParser
      const ts =
        parser === 'ts' ||
        sp === 'ts' ||
        sp?.js === 'ts' ||
        (parser !== 'vue' && sp?.ts !== undefined && filename.endsWith('.ts'))
      const sfc = parser === 'vue' && (filename.endsWith('.vue') || c.code.trimStart().startsWith('<'))
      const u = {
        path: sfc ? 'test.vue' : filename.replace(/\.vue$/, '.js'),
        code: c.code,
        sfc,
        ts,
        jsx: lo.ecmaFeatures?.jsx === true,
        settings: c.settings ?? {},
      }
      const k = JSON.stringify(u)
      if (seen.has(k)) continue
      seen.add(k)
      units.push({ id: `${c.rule}:${c.kind}${c.idx}`, ...u })
    }
  }
} else if (mode === 'corpus') {
  const dirs = argv.slice(1).filter((a, i, all) => !a.startsWith('--') && !['--ref', '--out', '--rules', '--jobs', '--sample', '--bin'].includes(all[i - 1]))
  const walk = (d) => {
    for (const e of fs.readdirSync(d, { withFileTypes: true })) {
      const p = path.join(d, e.name)
      if (e.isDirectory()) {
        if (e.name !== 'node_modules' && e.name !== '.git') walk(p)
      } else if (e.name.endsWith('.vue')) {
        // ESLint lints the text without a BOM; the CLI does the same and puts it back.
        units.push({ id: p, path: p, code: fs.readFileSync(p, 'utf8').replace(/^﻿/, '') })
      }
    }
  }
  for (const d of dirs) walk(path.resolve(d))
  // --sample N: every k-th file (deterministic), for the heavy option-variant runs.
  const n = Number(opt('--sample') ?? 0)
  if (n && units.length > n) {
    const k = units.length / n
    const picked = Array.from({ length: n }, (_, i) => units[Math.floor(i * k)])
    units.length = 0
    units.push(...picked)
  }
} else {
  console.error('usage: fix-check.mjs upstream|corpus <dir>... --ref <node-dir> --out <dir>')
  process.exit(2)
}

fs.writeFileSync(path.join(outDir, 'units.jsonl'), units.map((u) => JSON.stringify(u)).join('\n') + '\n')
fs.writeFileSync(path.join(outDir, 'configs.json'), JSON.stringify(configs))
console.log(`${units.length} units, ${configs.length} configs (${fixable.length} fixable rules, ${suggesting.length} with suggestions)`)

// --- Rust side -----------------------------------------------------------------------------------
// --bin <path>: another build of examples/fix_check (e.g. an older checkout, for before/after numbers).
const bin = opt('--bin') ? path.resolve(opt('--bin')) : path.join(ROOT, 'target/release/examples/fix_check')
if (!flag('--no-build') && !opt('--bin')) execFileSync('cargo', ['build', '--release', '--example', 'fix_check'], { cwd: ROOT, stdio: 'inherit' })
execFileSync(bin, [path.join(outDir, 'units.jsonl'), path.join(outDir, 'configs.json'), path.join(outDir, 'records.jsonl')], {
  stdio: ['ignore', 'inherit', 'inherit'],
})

// --- Node side, sharded --------------------------------------------------------------------------
const K = Number(opt('--jobs') ?? 8)
await Promise.all(
  Array.from({ length: K }, (_, k) =>
    new Promise((res, rej) => {
      const p = spawn(process.execPath, [fileURLToPath(import.meta.url), '--shard', String(k), String(K), '--ref', refDir, '--out', outDir], {
        stdio: 'inherit',
      })
      p.on('exit', (c) => (c === 0 ? res() : rej(new Error(`shard ${k} exit ${c}`))))
    }),
  ),
)

// --- summary -------------------------------------------------------------------------------------
const stats = new Map() // config label → {changed, problems: Map}
const label = (c) => (c.all ? `ALL(${c.mode})` : `${c.rules[0][0]} ${JSON.stringify(c.rules[0][1])} ${c.mode}`)
const findings = fs.createWriteStream(path.join(outDir, 'findings.jsonl'))
for (let k = 0; k < K; k++) {
  const f = path.join(outDir, `shard-${k}.jsonl`)
  for (const line of fs.readFileSync(f, 'utf8').split('\n')) {
    if (!line) continue
    const r = JSON.parse(line)
    const c = configs[r.c]
    const key = label(c)
    const s = stats.get(key) ?? { rule: c.all ? 'ALL' : c.rules[0][0], changed: 0, problems: new Map() }
    stats.set(key, s)
    s.changed++
    for (const p of r.problems) {
      const tag = p.split(':')[0]
      s.problems.set(tag, (s.problems.get(tag) ?? 0) + 1)
    }
    if (r.problems.length) findings.write(JSON.stringify({ config: key, unit: units[r.u].id, ...r }) + '\n')
  }
  fs.rmSync(f)
}
findings.end()
const perRule = new Map()
for (const s of stats.values()) {
  const t = perRule.get(s.rule) ?? { changed: 0, problems: new Map() }
  perRule.set(s.rule, t)
  t.changed += s.changed
  for (const [k, v] of s.problems) t.problems.set(k, (t.problems.get(k) ?? 0) + v)
}
const lines = [`${units.length} units, ${configs.length} configs`, '']
let total = 0
const totals = new Map()
for (const [rule, t] of [...perRule].sort()) {
  total += t.changed
  for (const [k, v] of t.problems) totals.set(k, (totals.get(k) ?? 0) + v)
  const ps = [...t.problems].map(([k, v]) => `${k}=${v}`).join(' ')
  lines.push(`${rule.padEnd(48)} ${String(t.changed).padStart(7)}  ${ps}`)
}
lines.push('', `checked outputs: ${total}`, `problems: ${[...totals].map(([k, v]) => `${k}=${v}`).join(' ')}`)
fs.writeFileSync(path.join(outDir, 'summary.txt'), lines.join('\n') + '\n')
console.log(lines.slice(-2).join('\n'))

// ---------------------------------------------------------------------------------------------
async function shard(k, K) {
  const sfc = req('@vue/compiler-sfc')
  const babel = req('@babel/parser')
  const units = fs.readFileSync(path.join(outDir, 'units.jsonl'), 'utf8').split('\n').filter(Boolean).map((l) => JSON.parse(l))
  const configs = JSON.parse(fs.readFileSync(path.join(outDir, 'configs.json'), 'utf8'))
  const out = fs.createWriteStream(path.join(outDir, `shard-${k}.jsonl`))
  const cache = new Map() // unit → analysis of the original
  const msg = (e) => String(e?.message ?? e).split('\n')[0].replace(/\(\d+:\d+\)|\b(line|column|at) \d+/g, '')

  // Render code is compared as a Babel AST without locations, modulo what cannot change behaviour:
  // string quote style, `this` vs `_ctx` (both the component proxy), `a['b']` vs `a.b`, quoted vs bare
  // and shorthand keys, and the order of an object literal's distinct static keys (prop order).
  // Merged values (`mergeProps(…)`, `[handler1, handler2]`, `normalizeStyle([…])`) keep their order.
  const STRIP = new Set(['loc', 'start', 'end', 'extra', 'comments', 'leadingComments', 'trailingComments', 'innerComments', 'range', 'tokens', 'shorthand'])
  const IDENT = /^[A-Za-z_$][\w$]*$/
  const keyName = (p) => (p.computed || !p.key ? null : p.key.type === 'Identifier' ? p.key.name : p.key.type === 'StringLiteral' || p.key.type === 'NumericLiteral' ? String(p.key.value) : null)
  function canon(n, ws) {
    if (Array.isArray(n)) return n.map((x) => canon(x, ws))
    if (!n || typeof n !== 'object') return ws && typeof n === 'string' ? n.replace(/\s+/g, ' ').replace(/> | </g, (m) => m.trim()).trim() : n
    if (n.type === 'TemplateLiteral' && !n.expressions.length) return canon({ type: 'StringLiteral', value: n.quasis[0].value.cooked }, ws)
    if (n.type === 'ThisExpression') return { type: 'Identifier', name: '_ctx' }
    if (n.type === 'MemberExpression' && n.computed) {
      const p = canon(n.property, false)
      if (p.type === 'StringLiteral' && IDENT.test(p.value)) return canon({ ...n, computed: false, property: { type: 'Identifier', name: p.value } }, ws)
    }
    const out = {}
    for (const [k, v] of Object.entries(n)) {
      if (STRIP.has(k)) continue
      out[k] = canon(v, ws)
    }
    if ((n.type === 'ObjectProperty' || n.type === 'ObjectMethod') && keyName(n) != null) {
      out.key = { type: 'Key', name: keyName(n) }
      // Vue 2 scope parameters compiled by Vue 3 as a plain attribute string.
      if ((out.key.name === 'slot-scope' || out.key.name === 'scope') && n.value?.type === 'StringLiteral') out.value = { type: 'StringLiteral', value: n.value.value.replace(/[\s'"]+/g, '') }
    }
    // Lists of names (dynamic prop keys, class arrays) are sets.
    if (n.type === 'ArrayExpression' && n.elements.length > 1 && n.elements.every((e) => e?.type === 'StringLiteral'))
      out.elements = out.elements.slice().sort((a, b) => (a.value < b.value ? -1 : 1))
    if (n.type === 'ObjectExpression') {
      const names = n.properties.map(keyName)
      if (names.every((x) => x != null) && new Set(names).size === names.length) {
        out.properties = out.properties.map((p, i) => [names[i], p]).sort((a, b) => (a[0] < b[0] ? -1 : 1)).map((x) => x[1])
      }
    }
    return out
  }
  const norm = (code, ts, ws) => JSON.stringify(canon(babel.parse(code, { sourceType: 'module', plugins: ts ? ['typescript'] : [] }).program, ws))

  function analyze(code, file, unit) {
    const a = { errors: [] }
    if (!unit.sfc && unit.sfc !== undefined) {
      // plain script input
      try {
        babel.parse(code, { sourceType: 'module', errorRecovery: false, allowReturnOutsideFunction: true, plugins: [...(unit.ts ? ['typescript'] : []), ...(unit.jsx ? ['jsx'] : [])] })
      } catch (e) {
        a.errors.push('babel: ' + msg(e))
      }
      return a
    }
    let d
    try {
      const r = sfc.parse(code, { filename: file, ignoreEmpty: false })
      d = r.descriptor
      for (const e of r.errors) a.errors.push('sfc: ' + msg(e))
    } catch (e) {
      a.errors.push('sfc-throw: ' + msg(e))
      return a
    }
    const lang = (d.scriptSetup ?? d.script)?.lang
    const ts = unit.ts || lang === 'ts' || lang === 'tsx'
    a.ts = ts
    // Template rules format the expressions of `v-bind(…)` in <style>: mask them.
    const maskVBind = (css) => css.replace(/v-bind\((?:[^()]|\([^()]*\))*\)/g, 'v-bind()')
    a.blocks = JSON.stringify([...d.styles, ...d.customBlocks].map((b) => [b.type, maskVBind(b.content.trim())]).sort())
    if (d.template && (!d.template.lang || d.template.lang === 'html')) {
      try {
        const r = sfc.compileTemplate({ source: d.template.content, filename: file, id: 'x', compilerOptions: { isTS: ts } })
        for (const e of r.errors) a.errors.push('tpl: ' + msg(e))
        if (!r.errors.length) {
          try {
            a.render = norm(r.code, ts, false)
            a.renderWs = norm(r.code, ts, true)
          } catch (e) {
            a.errors.push('render-babel: ' + msg(e))
          }
        }
      } catch (e) {
        a.errors.push('tpl-throw: ' + msg(e))
      }
    }
    for (const b of [d.script, d.scriptSetup]) {
      if (!b) continue
      const bl = b.lang ?? ''
      try {
        babel.parse(b.content, {
          sourceType: 'module',
          allowReturnOutsideFunction: true,
          plugins: [...(ts || bl.startsWith('ts') ? ['typescript'] : []), ...(bl.endsWith('x') || unit.jsx ? ['jsx'] : [])],
        })
      } catch (e) {
        a.errors.push('babel: ' + msg(e))
      }
    }
    if (d.script || d.scriptSetup) {
      try {
        sfc.compileScript(d, { id: 'x', fs: { fileExists: () => false, readFile: () => '' } })
      } catch (e) {
        const m = msg(e)
        if (!/resolve|import source|Unresolvable|failed to/i.test(m)) a.errors.push('script: ' + m)
      }
    }
    return a
  }

  const rl = readline.createInterface({ input: fs.createReadStream(path.join(outDir, 'records.jsonl')) })
  for await (const line of rl) {
    if (!line) continue
    const r = JSON.parse(line)
    if (r.u % K !== k) continue
    const u = units[r.u]
    const before = u.code
    const b = Buffer.from(before)
    const after = Buffer.concat([b.subarray(0, r.p), Buffer.from(r.m), b.subarray(b.length - r.s)]).toString()
    const file = u.sfc === false ? u.path : 'x.vue'
    if (!cache.has(r.u)) cache.set(r.u, analyze(before, file, u))
    const A = cache.get(r.u)
    const B = analyze(after, file, u)
    const old = new Set(A.errors)
    for (const e of B.errors) if (!old.has(e)) r.problems.push('invalid:' + e)
    if (A.render && B.render && A.render !== B.render) r.problems.push(A.renderWs === B.renderWs ? 'render-ws' : 'render')
    if (A.blocks !== undefined && B.blocks !== undefined && A.blocks !== B.blocks) r.problems.push('sfc-blocks')
    if (r.script.length) r.problems.push('script-ast')
    out.write(JSON.stringify({ u: r.u, c: r.c, desc: r.desc, passes: r.passes, problems: r.problems, p: r.p, s: r.s, m: r.m }) + '\n')
  }
  out.end()
  await new Promise((res) => out.on('finish', res))
}
