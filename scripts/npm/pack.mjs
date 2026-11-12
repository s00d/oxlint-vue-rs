#!/usr/bin/env node
// Assemble npm packages into dist-npm/:
//   node scripts/npm/pack.mjs platform <target-triple> <path-to-binary>   → dist-npm/platforms/<os-cpu[-libc]>/ (@oxlint-vue-rs/<os-cpu[-libc]>)
//   node scripts/npm/pack.mjs main                                       → dist-npm/oxlint-vue-rs/
// Versions come from Cargo.toml so every package matches the crate. The shim's rule list is
// npm/rules.json as committed. Each package directory is recreated from scratch.
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('../..', import.meta.url))
const out = path.join(root, 'dist-npm')
const version = fs.readFileSync(path.join(root, 'Cargo.toml'), 'utf8').match(/^version\s*=\s*"([^"]+)"/m)[1]
const platforms = JSON.parse(fs.readFileSync(path.join(root, 'npm/platforms.json'), 'utf8'))
const [mode, target, binary] = process.argv.slice(2)

const common = {
  version,
  license: 'MIT',
  repository: { type: 'git', url: 'git+https://github.com/s00d/oxlint-vue-rs.git' },
  homepage: 'https://github.com/s00d/oxlint-vue-rs#readme',
}

if (mode === 'platform') {
  const p = platforms.find((x) => x.target === target)
  if (!p) throw new Error(`unknown target ${target}`)
  const dir = path.join(out, 'platforms', p.pkg.split('/').pop())
  fs.rmSync(dir, { recursive: true, force: true })
  fs.mkdirSync(dir, { recursive: true })
  const exe = p.os === 'win32' ? 'oxlint-vue-rs.exe' : 'oxlint-vue-rs'
  fs.copyFileSync(binary, path.join(dir, exe))
  fs.chmodSync(path.join(dir, exe), 0o755)
  const pkg = {
    name: p.pkg,
    ...common,
    description: `oxlint-vue-rs native binary for ${p.os}-${p.cpu}${p.libc ? ` (${p.libc})` : ''}`,
    os: [p.os],
    cpu: [p.cpu],
    ...(p.libc ? { libc: [p.libc] } : {}),
    type: 'commonjs',
    files: [exe, 'LICENSE'],
    preferUnplugged: true,
  }
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify(pkg, null, 2) + '\n')
  fs.copyFileSync(path.join(root, 'LICENSE'), path.join(dir, 'LICENSE'))
  fs.writeFileSync(path.join(dir, 'README.md'), `Native binary of [oxlint-vue-rs](https://github.com/s00d/oxlint-vue-rs) for ${p.os}-${p.cpu}. Install \`oxlint-vue-rs\` instead.\n`)
  console.log(dir)
} else if (mode === 'main') {
  const dir = path.join(out, 'oxlint-vue-rs')
  fs.rmSync(dir, { recursive: true, force: true })
  fs.cpSync(path.join(root, 'npm'), dir, { recursive: true, filter: (s) => !s.endsWith('platforms.json') })
  fs.copyFileSync(path.join(root, 'README.md'), path.join(dir, 'README.md'))
  fs.copyFileSync(path.join(root, 'LICENSE'), path.join(dir, 'LICENSE'))
  const pkg = JSON.parse(fs.readFileSync(path.join(dir, 'package.json'), 'utf8'))
  pkg.version = version
  pkg.optionalDependencies = Object.fromEntries(platforms.map((p) => [p.pkg, version]))
  Object.assign(pkg, { repository: common.repository, homepage: common.homepage })
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify(pkg, null, 2) + '\n')
  console.log(dir)
} else {
  console.error('usage: pack.mjs platform <target> <binary> | pack.mjs main')
  process.exit(2)
}
