// The vue-rs rules inside oxlint (oxc PR #28720) vs the `oxlint-vue-rs` CLI on the same fixture project:
// same problems (positions, messages), same `--fix` output. oxlint with that PR cannot be built in CI, so
// these run only when the paths are given:
//
//   OXLINT_PR=<oxlint built from the PR: its bin or apps/oxlint/dist/cli.js> \
//   OXLINT_RELEASED=<released oxlint bin (fallback test)> \
//   node --test tests/oxlint-plugin/oxlint-plugin.test.mjs
//
// The CLI binary and the Node addon come from $CARGO_TARGET_DIR/release (default target/; `cargo build --release --workspace`);
// OXLINT_VUE_RS_BINARY / OXLINT_VUE_RS_ADDON override them.
import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'
import { pkg, target } from '../../npm/platform.js'

const here = path.dirname(fileURLToPath(import.meta.url))
const root = path.join(here, '../..')
const release = path.join(process.env.CARGO_TARGET_DIR || path.join(root, 'target'), 'release')
const cli =
  process.env.OXLINT_VUE_RS_BINARY ||
  path.join(release, process.platform === 'win32' ? 'oxlint-vue-rs.exe' : 'oxlint-vue-rs')
const addon =
  process.env.OXLINT_VUE_RS_ADDON ||
  ['liboxlint_vue_rs_napi.so', 'liboxlint_vue_rs_napi.dylib', 'oxlint_vue_rs_napi.dll']
    .map((f) => path.join(release, f))
    .find((f) => fs.existsSync(f))
const { OXLINT_PR, OXLINT_RELEASED } = process.env

/** A copy of the fixture with the npm package and the platform package (addon only) installed. */
function project() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'oxlint-vue-rs-e2e-'))
  fs.cpSync(path.join(here, 'fixture'), dir, { recursive: true })
  const nm = path.join(dir, 'node_modules')
  fs.cpSync(path.join(root, 'npm'), path.join(nm, 'oxlint-vue-rs'), { recursive: true })
  const plat = path.join(nm, pkg)
  fs.mkdirSync(plat, { recursive: true })
  fs.writeFileSync(path.join(plat, 'package.json'), JSON.stringify({ name: pkg, version: '0.0.0' }))
  fs.copyFileSync(addon, path.join(plat, 'oxlint-vue-rs.node'))
  return dir
}

function exec(bin, args, cwd, env = {}) {
  const [cmd, pre] = bin.endsWith('.js') ? [process.execPath, [bin]] : [bin, []]
  const r = spawnSync(cmd, [...pre, ...args], { cwd, encoding: 'utf8', env: { ...process.env, ...env } })
  if (r.error) throw r.error
  return r
}

/**
 * vue-rs problems from `--format json` output (oxlint's or ours), sorted. Positions are the byte range
 * and the line: oxlint counts columns in UTF-8 bytes, the CLI in UTF-16 units like ESLint.
 */
function problems(stdout) {
  return JSON.parse(stdout)
    .diagnostics.map((d) => {
      const rule = d.code.match(/^vue-rs[/(]([^)]+)\)?$/)?.[1]
      const { offset, length, line } = d.labels[0].span
      return rule && { file: d.filename, rule, severity: d.severity, message: d.message, offset, length, line }
    })
    .filter(Boolean)
    .sort((a, b) => a.file.localeCompare(b.file) || a.offset - b.offset || a.rule.localeCompare(b.rule))
}

const oxlint = (dir, ...args) => exec(OXLINT_PR, ['--format', 'json', ...args], dir)
const ours = (dir, ...args) => exec(cli, ['--no-oxlint', '-f', 'json', ...args], dir)

function vueFiles(dir) {
  return fs
    .readdirSync(dir, { recursive: true })
    .filter((f) => f.endsWith('.vue') && !f.startsWith('node_modules'))
    .sort()
    .map((f) => [f, fs.readFileSync(path.join(dir, f), 'utf8')])
}

const skipPr = !OXLINT_PR && 'set OXLINT_PR to an oxlint built from oxc PR #28720'

test('oxlint runs the vue-rs rules exactly like the CLI', { skip: skipPr }, () => {
  const dir = project()
  const a = oxlint(dir)
  const b = ours(dir)
  assert.equal(a.stderr, '')
  const got = problems(a.stdout)
  assert.deepEqual(got, problems(b.stdout))
  const rules = (file) => got.filter((p) => p.file === file).map((p) => p.rule)
  // template-only SFC, options, both script blocks, TS, non-ASCII text before a report (UTF-16 ranges)
  assert.ok(rules('template-only.vue').includes('html-indent'))
  assert.ok(rules('template-only.vue').includes('eqeqeq'))
  assert.ok(got.some((p) => p.file === 'template-only.vue' && p.line === 5 && p.rule === 'mustache-interpolation-spacing'))
  assert.ok(rules('script-template.vue').includes('order-in-components'))
  assert.ok(rules('two-scripts.vue').includes('padding-line-between-blocks'))
  assert.ok(rules('typed.vue').includes('require-explicit-emits'))
  // overrides: legacy/** turns html-indent and eqeqeq off
  const legacy = rules(path.join('legacy', 'old.vue'))
  assert.ok(legacy.includes('require-v-for-key') && !legacy.includes('html-indent') && !legacy.includes('eqeqeq'))
  // disable comments: template (next-line, block) and script (eslint-/oxlint-disable-next-line)
  const disabled = got.filter((p) => p.file === 'disabled.vue')
  assert.deepEqual(
    disabled.map((p) => `${p.line}:${p.rule}`),
    ['7:html-self-closing', '8:html-indent', '10:padding-line-between-blocks', '14:order-in-components'],
  )
  // one report per problem in a file with two <script> blocks (oxlint lints each block)
  const two = problems(a.stdout).filter((p) => p.file === 'two-scripts.vue')
  assert.equal(new Set(two.map((p) => JSON.stringify(p))).size, two.length)
})

test('oxlint --fix gives the same files as the CLI --fix', { skip: skipPr }, () => {
  const [a, b] = [project(), project()]
  // The CLI repeats fixing until stable (max 10 passes), like ESLint; so does this loop over oxlint.
  for (let i = 0, prev = ''; i < 10; i++) {
    oxlint(a, '--fix')
    const now = JSON.stringify(vueFiles(a))
    if (now === prev) break
    prev = now
  }
  ours(b, '--fix')
  assert.deepEqual(vueFiles(a), vueFiles(b))
  assert.notDeepEqual(vueFiles(a), vueFiles(path.join(here, 'fixture')))
})

test('the CLI does not report the rules twice when its oxlint runs them too', {
  skip: skipPr || (process.platform === 'win32' && 'needs a shell wrapper for oxlint'),
}, () => {
  const dir = project()
  const bin = path.join(dir, 'node_modules/.bin')
  fs.mkdirSync(bin, { recursive: true })
  const run = OXLINT_PR.endsWith('.js') ? `"${process.execPath}" "${OXLINT_PR}"` : `"${OXLINT_PR}"`
  fs.writeFileSync(path.join(bin, 'oxlint'), `#!/bin/sh\nexec ${run} "$@"\n`, { mode: 0o755 })
  const merged = exec(cli, ['-f', 'json', '--with-oxlint'], dir)
  assert.equal(merged.stderr, '')
  assert.deepEqual(problems(merged.stdout), problems(ours(dir).stdout))
})

test('released oxlint accepts the config and stays quiet', {
  skip: !OXLINT_RELEASED && 'set OXLINT_RELEASED to a released oxlint (e.g. 1.93)',
}, () => {
  const dir = project()
  const r = exec(OXLINT_RELEASED, ['--format', 'json'], dir)
  assert.equal(r.stderr, '')
  assert.equal(r.status, 0, r.stdout)
  assert.deepEqual(JSON.parse(r.stdout).diagnostics, [])
})
