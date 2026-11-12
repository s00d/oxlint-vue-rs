#!/usr/bin/env node
// One-off cleanup: remove the unscoped platform packages of 0.1.0 (`oxlint-vue-rs-<platform>`), replaced by
// `@oxlint-vue-rs/<platform>` in 0.1.1. Runs `npm unpublish <name> --force` for each existing one, with npm's own
// interactive auth, waiting between calls. Allowed by npm within 72 hours of publishing and with no dependents.
//
//   npm run unpublish:legacy               # all legacy names that still exist
//   npm run unpublish:legacy -- --dry-run  # only list them
import { spawnSync } from 'node:child_process'

const names = ['darwin-arm64', 'darwin-x64', 'linux-x64-gnu', 'linux-arm64-gnu', 'linux-x64-musl', 'linux-arm64-musl', 'win32-x64', 'win32-arm64'].map(
  (p) => `oxlint-vue-rs-${p}`,
)
const dryRun = process.argv.includes('--dry-run')
const sleep = (s) => Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, s * 1000)
const exists = (name) => spawnSync('npm', ['view', name, 'name'], { stdio: ['ignore', 'pipe', 'ignore'], encoding: 'utf8' }).stdout.trim() === name

const todo = names.filter(exists)
if (!todo.length) {
  console.log('nothing to remove')
  process.exit(0)
}
console.log(`to remove: ${todo.join(', ')}`)
if (dryRun) process.exit(0)
todo.forEach((name, i) => {
  if (i > 0) sleep(10)
  console.log(`unpublish ${name}`)
  const r = spawnSync('npm', ['unpublish', name, '--force'], { stdio: 'inherit' })
  if (r.status !== 0) {
    console.error(`failed: ${name} (if npm says it is too late or has dependents: npm deprecate ${name} "use @oxlint-vue-rs/*")`)
    process.exit(r.status ?? 1)
  }
})
console.log('done')
