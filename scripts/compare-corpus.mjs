#!/usr/bin/env node
// Compare oxlint-vue-rs against eslint-plugin-vue on a corpus of .vue files.
//
// usage: node scripts/compare-corpus.mjs <eslint-json> <oxlint-vue-rs-json> [--rules a,b] [--show N]
//   <eslint-json>:  `eslint --format json` output (rules named vue/<rule>)
//   --json FILE writes per-rule counts. Reports in files ESLint failed to parse are counted as
//   `unparsedByEslint`, not `extra`.
//   <ours-json>:    `oxlint-vue-rs -D all --format json` output (codes <prefix>/<rule>)
// Only rules present in our output's implemented set (--rules, or every rule we reported at least once
// plus every rule listed in --implemented FILE) are compared. Prints per-rule match / missing / extra.
import fs from 'node:fs'

const [eslintFile, oursFile, ...rest] = process.argv.slice(2)
const opt = (name) => {
  const i = rest.indexOf(name)
  return i >= 0 ? rest[i + 1] : undefined
}
const show = Number(opt('--show') ?? 5)
let rules = opt('--rules')?.split(',')
const implementedFile = opt('--implemented')
if (implementedFile) rules = fs.readFileSync(implementedFile, 'utf8').split(/\s+/).filter(Boolean)

const key = (f, l, c, r) => `${f}\u0000${l}\u0000${c}\u0000${r}`
const eslint = JSON.parse(fs.readFileSync(eslintFile, 'utf8'))
const ours = JSON.parse(fs.readFileSync(oursFile, 'utf8'))

const want = new Map()
const fatal = []
const fatalFiles = new Set()
for (const f of eslint) {
  for (const m of f.messages) {
    if (!m.ruleId) {
      // Only real parse failures (`fatal`), not e.g. noInlineConfig notices.
      if (m.fatal) {
        fatal.push(`${f.filePath}:${m.line}:${m.column} ${m.message}`)
        fatalFiles.add(f.filePath)
      }
      continue
    }
    const r = m.ruleId.replace(/^vue\//, '')
    want.set(key(f.filePath, m.line, m.column, r), m.message)
  }
}
const got = new Map()
for (const d of ours.diagnostics) {
  const r = d.code.split('/').slice(1).join('/')
  const s = d.labels[0].span
  got.set(key(d.filename, s.line, s.column, r), d.message)
}
const ruleSet = new Set(rules ?? [...got.keys()].map((k) => k.split('\u0000')[3]))
const stats = {}
const ex = {}
const bump = (r, k, sample) => {
  stats[r] ??= { match: 0, msgDiff: 0, missing: 0, extra: 0, unparsedByEslint: 0 }
  stats[r][k]++
  if (sample) ((ex[r] ??= {})[k] ??= []).push(sample)
}
for (const [k, msg] of want) {
  const r = k.split('\u0000')[3]
  if (!ruleSet.has(r)) continue
  if (!got.has(k)) bump(r, 'missing', `${k.split('\u0000').slice(0, 3).join(':')} ${msg}`)
  else if (got.get(k) !== msg) bump(r, 'msgDiff', `${k.split('\u0000').slice(0, 3).join(':')}\n      eslint: ${msg}\n      ours:   ${got.get(k)}`)
  else bump(r, 'match')
}
for (const [k, msg] of got) {
  const r = k.split('\u0000')[3]
  if (!ruleSet.has(r)) continue
  if (want.has(k)) continue
  // ESLint could not parse the file: it reports nothing there, so this is not comparable.
  if (fatalFiles.has(k.split('\u0000')[0])) bump(r, 'unparsedByEslint')
  else bump(r, 'extra', `${k.split('\u0000').slice(0, 3).join(':')} ${msg}`)
}
let total = { match: 0, msgDiff: 0, missing: 0, extra: 0, unparsedByEslint: 0 }
for (const r of Object.keys(stats).sort()) {
  const s = stats[r]
  for (const k of Object.keys(total)) total[k] += s[k]
  const bad = s.missing + s.extra + s.msgDiff
  console.log(`${bad ? 'DIFF' : 'ok  '} ${r.padEnd(45)} match ${s.match}  msgDiff ${s.msgDiff}  missing ${s.missing}  extra ${s.extra}`)
  if (bad)
    for (const k of ['missing', 'extra', 'msgDiff'])
      for (const e of (ex[r]?.[k] ?? []).slice(0, show)) console.log(`    ${k}: ${e}`)
}
console.log(`TOTAL match ${total.match}  msgDiff ${total.msgDiff}  missing ${total.missing}  extra ${total.extra}  (in files ESLint could not parse: ${total.unparsedByEslint}; eslint fatal parse errors: ${fatal.length})`)
const jsonOut = opt('--json')
if (jsonOut) fs.writeFileSync(jsonOut, JSON.stringify(stats, null, 1))
