#!/usr/bin/env node
// Launcher: runs the prebuilt native binary from the platform package npm installed for this machine
// (optionalDependencies `@oxlint-vue-rs/<os>-<cpu>[-<libc>]`). OXLINT_VUE_RS_BINARY overrides the path.
import { spawnSync } from 'node:child_process'
import { existsSync } from 'node:fs'
import { exe, pkg, resolve, target } from '../platform.js'

function binaryPath() {
  if (process.env.OXLINT_VUE_RS_BINARY) return process.env.OXLINT_VUE_RS_BINARY
  try {
    return resolve(exe)
  } catch {
    console.error(
      `oxlint-vue-rs: no prebuilt binary for ${target} (package "${pkg}" is not installed).\n` +
        'Reinstall without --no-optional / --omit=optional, or build it: cargo install oxlint-vue-rs,\n' +
        'then set OXLINT_VUE_RS_BINARY to the binary path.',
    )
    process.exit(2)
  }
}

const bin = binaryPath()
if (!existsSync(bin)) {
  console.error(`oxlint-vue-rs: binary not found at ${bin}`)
  process.exit(2)
}
const r = spawnSync(bin, process.argv.slice(2), { stdio: 'inherit' })
if (r.error) {
  console.error(`oxlint-vue-rs: cannot run ${bin}: ${r.error.message}`)
  process.exit(2)
}
process.exit(r.status ?? (r.signal ? 1 : 0))
