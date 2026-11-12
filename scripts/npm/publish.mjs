#!/usr/bin/env node
// Publish the npm packages of a GitHub release (built by the `release` workflow) from this machine.
//
//   npm run publish:npm                    # version from Cargo.toml
//   npm run publish:npm -- 0.1.0 --dry-run # check without publishing
//
// Downloads the release's *.tgz with `gh`, publishes the platform packages first and the main package last
// (it depends on them through optionalDependencies), and skips versions already on npm, so a re-run finishes
// a partial publish. Requires `gh` (authenticated) and `npm login`.
import { execFileSync, spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..')
const repo = 's00d/oxlint-vue-rs'
const args = process.argv.slice(2)
const dryRun = args.includes('--dry-run')
const version =
  args.find((a) => !a.startsWith('--')) ??
  fs.readFileSync(path.join(root, 'Cargo.toml'), 'utf8').match(/^version\s*=\s*"([^"]+)"/m)[1]
const platforms = JSON.parse(fs.readFileSync(path.join(root, 'npm/platforms.json'), 'utf8')).map((p) => p.pkg)

const run = (cmd, argv, opts = {}) => execFileSync(cmd, argv, { stdio: ['ignore', 'pipe', 'inherit'], encoding: 'utf8', ...opts })
const published = (name) => {
  try {
    return run('npm', ['view', `${name}@${version}`, 'version'], { stdio: ['ignore', 'pipe', 'ignore'] }).trim() === version
  } catch {
    return false
  }
}

const dir = fs.mkdtempSync(path.join(os.tmpdir(), `oxlint-vue-rs-npm-${version}-`))
try {
  console.log(`release ${version}: downloading tarballs from github.com/${repo} …`)
  run('gh', ['release', 'download', version, '-R', repo, '-p', '*.tgz', '-D', dir], { stdio: 'inherit' })
  const order = [...platforms, 'oxlint-vue-rs']
  for (const name of order) {
    const file = path.join(dir, `${name}-${version}.tgz`)
    if (!fs.existsSync(file)) throw new Error(`release ${version} has no ${path.basename(file)}`)
  }
  for (const name of order) {
    if (published(name)) {
      console.log(`skip     ${name}@${version} (already on npm)`)
      continue
    }
    const argv = ['publish', path.join(dir, `${name}-${version}.tgz`), '--access', 'public']
    if (dryRun) argv.push('--dry-run')
    console.log(`${dryRun ? 'dry-run ' : 'publish '} ${name}@${version}`)
    // stdin/stdout stay interactive for npm's own auth; stderr is captured to recognise known failures.
    const r = spawnSync('npm', argv, { stdio: ['inherit', 'inherit', 'pipe'], encoding: 'utf8' })
    process.stderr.write(r.stderr ?? '')
    if (r.status === 0) continue
    const err = r.stderr ?? ''
    if (/previously published|cannot publish over|You cannot publish over/i.test(err)) {
      // Accepted earlier but not visible yet (npm moderation / replication): nothing to do.
      console.log(`skip     ${name}@${version} (already published, maybe still in npm moderation)`)
      continue
    }
    const left = order.slice(order.indexOf(name))
    if (/spam detection/i.test(err)) {
      console.error(
        `\nnpm's spam detection blocked ${name}. Packages accepted so far may still be in moderation.\n` +
          `Wait for them to appear (npm view <name>), then run the same command again; it skips what is published.\n` +
          `If the block persists, ask npm support to allow the names (https://www.npmjs.com/support).\n` +
          `Not published yet: ${left.join(', ')}`,
      )
    }
    process.exit(r.status ?? 1)
  }
  if (!dryRun) console.log(`done: npm i -D oxlint oxlint-vue-rs@${version}`)
} finally {
  fs.rmSync(dir, { recursive: true, force: true })
}
