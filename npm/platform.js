// Files of the platform package npm installed for this machine
// (optionalDependencies `@oxlint-vue-rs/<os>-<cpu>[-<libc>]`): the CLI binary and the Node addon.
import { createRequire } from 'node:module'

const require = createRequire(import.meta.url)
const { platform, arch } = process

function isMusl() {
  if (platform !== 'linux') return false
  try {
    return !process.report.getReport().header.glibcVersionRuntime
  } catch {
    return false
  }
}

export const target = `${platform}-${arch}${platform === 'linux' ? (isMusl() ? '-musl' : '-gnu') : ''}`
export const pkg = `@oxlint-vue-rs/${target}`
export const exe = platform === 'win32' ? 'oxlint-vue-rs.exe' : 'oxlint-vue-rs'
export const addon = 'oxlint-vue-rs.node'

/** Absolute path of `file` in the platform package; throws when the package is not installed. */
export function resolve(file) {
  return require.resolve(`${pkg}/${file}`)
}
