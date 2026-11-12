// CAPTURE MODE: records every RuleTester case with the real ESLint result.
import { createRequire } from 'node:module'
import fs from 'node:fs'
import path from 'node:path'
import { getESLint } from 'eslint-compat-utils/eslint'
import { getLinter } from 'eslint-compat-utils/linter'
import { Linter as LinterRaw, ESLint as ESLintRaw } from 'eslint'

export const ESLint = getESLint()
export const Linter = getLinter()
export const FlatESLint: typeof ESLintRaw | null = ESLintRaw

const OUT = process.env.CAPTURE_DIR || '/tmp/epv-capture'
fs.mkdirSync(OUT, { recursive: true })
const req = createRequire(import.meta.url)

function parserName(p: any): any {
  if (p == null) return null
  if (typeof p === 'string') {
    if (p.includes('typescript-eslint')) return 'ts'
    if (p.includes('espree')) return 'espree'
    if (p.includes('vue-eslint-parser')) return 'vue'
    if (p.includes('babel')) return 'babel'
    return p
  }
  if (p.meta?.name) {
    const n = p.meta.name
    if (n.includes('typescript-eslint')) return 'ts'
    if (n.includes('vue-eslint-parser')) return 'vue'
    if (n.includes('espree')) return 'espree'
    return n
  }
  if (typeof p.parseForESLint === 'function' || typeof p.parse === 'function') {
    if (p.version && p.parseForESLint && p.clearCaches) return 'ts'
    if (p.AST || p.parseForESLint?.toString().includes('vue')) return 'vue'
    return 'espree'
  }
  if (typeof p === 'object') {
    const o: any = {}
    for (const k of Object.keys(p)) o[k] = parserName(p[k])
    return o
  }
  return String(p)
}

function merge(a: any, b: any): any {
  if (b === undefined) return a
  if (a && b && typeof a === 'object' && typeof b === 'object' && !Array.isArray(a) && !Array.isArray(b) && !b.parseForESLint && !b.parse) {
    const o: any = { ...a }
    for (const k of Object.keys(b)) o[k] = merge(a[k], b[k])
    return o
  }
  return b
}

function applyFixes(code: string, messages: any[]): string | null {
  const fixes = messages.filter((m) => m.fix).map((m) => m.fix)
  if (!fixes.length) return null
  fixes.sort((a, b) => a.range[0] - b.range[0] || a.range[1] - b.range[1])
  let out = ''
  let last = -Infinity
  let pos = 0
  for (const f of fixes) {
    if (last >= f.range[0]) continue
    out += code.slice(pos, f.range[0]) + f.text
    pos = f.range[1]
    last = f.range[1]
  }
  return out + code.slice(pos)
}

function jsonable(v: any): any {
  try {
    return JSON.parse(JSON.stringify(v, (_k, x) => (x instanceof RegExp ? { $regexp: x.source, flags: x.flags } : typeof x === 'function' ? '[function]' : x)))
  } catch {
    return '[unserializable]'
  }
}

function lo(base: any, c: any) {
  const l = merge(base?.languageOptions || {}, c.languageOptions || {})
  return {
    parser: parserName(l.parser),
    scriptParser: parserName(l.parserOptions?.parser),
    ecmaVersion: l.ecmaVersion ?? l.parserOptions?.ecmaVersion ?? null,
    sourceType: l.sourceType ?? l.parserOptions?.sourceType ?? null,
    ecmaFeatures: jsonable(l.parserOptions?.ecmaFeatures ?? null),
    vueFeatures: jsonable(l.parserOptions?.vueFeatures ?? null),
    templateTokenizer: jsonable(l.parserOptions?.templateTokenizer ?? null),
    project: l.parserOptions?.project ? true : l.parserOptions?.projectService ? true : false,
    globals: jsonable(l.globals ?? null),
    full: l
  }
}

export class RuleTester {
  base: any
  constructor(base: any = {}) {
    this.base = base
  }
  run(name: string, rule: any, tests: { valid: any[]; invalid: any[] }) {
    const base = this.base
    // eslint-disable-next-line no-undef
    describe(name, () => {
      // eslint-disable-next-line no-undef
      it('capture', () => {
        const linter = new LinterRaw({ configType: 'flat' } as any)
        const records: any[] = []
        const all = [
          ...tests.valid.map((c: any, i: number) => ['valid', i, typeof c === 'string' ? { code: c } : c]),
          ...tests.invalid.map((c: any, i: number) => ['invalid', i, c])
        ] as const
        for (const [kind, idx, c] of all) {
          const l = lo(base, c)
          const filename = c.filename ?? 'test.js'
          const ruleId = `vue/${name}`
          const config: any = [
            {
              files: ['**/*', '**/*.*', '*'],
              plugins: { vue: { rules: { [name]: rule } } },
              languageOptions: l.full,
              settings: merge(base.settings || {}, c.settings || {}),
              rules: { [ruleId]: ['error', ...(c.options || [])] }
            }
          ]
          let messages: any[] = []
          let fatal: any = null
          let crash: string | null = null
          try {
            messages = linter.verify(c.code, config, { filename: path.join(process.cwd(), filename) === filename ? filename : filename } as any)
          } catch (e: any) {
            crash = String(e?.message || e)
          }
          const mine = messages.filter((m) => m.ruleId === ruleId)
          const other = messages.filter((m) => m.ruleId !== ruleId)
          if (other.length) fatal = other.map((m) => ({ ruleId: m.ruleId, message: m.message, line: m.line, column: m.column, fatal: !!m.fatal }))
          const output = applyFixes(c.code, mine)
          records.push({
            rule: name,
            kind,
            idx,
            name: c.name ?? null,
            code: c.code,
            filename,
            options: jsonable(c.options ?? []),
            settings: jsonable(merge(base.settings || {}, c.settings || {})),
            languageOptions: { ...l, full: undefined },
            only: !!c.only,
            expected: { errors: jsonable(c.errors ?? null), output: c.output === undefined ? undefined : c.output },
            actual: {
              messages: mine.map((m) => ({
                messageId: m.messageId ?? null,
                message: m.message,
                line: m.line,
                column: m.column,
                endLine: m.endLine ?? null,
                endColumn: m.endColumn ?? null,
                fix: m.fix ? { range: m.fix.range, text: m.fix.text } : null,
                suggestions: (m.suggestions || []).map((s: any) => ({
                  messageId: s.messageId ?? null,
                  desc: s.desc,
                  output: applyFixes(c.code, [s])
                }))
              })),
              output,
              other: fatal,
              crash
            }
          })
        }
        fs.appendFileSync(path.join(OUT, `${name.replace(/\//g, "__")}.${process.pid}.jsonl`), records.map((r) => JSON.stringify(r)).join('\n') + '\n')
      })
    })
  }
  static describe = undefined
}
void req
