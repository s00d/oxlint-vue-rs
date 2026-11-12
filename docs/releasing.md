# Releasing

Version lives in `Cargo.toml`; the npm packages take it from there (`scripts/npm/pack.mjs`). Tags have no `v`
prefix (`0.1.0`).

## 1. Version, changelog, tag

```bash
# bump `version` in Cargo.toml, add the CHANGELOG entry, update RELEASE_NOTES.md
cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check
git commit -am "release: X.Y.Z" && git push
git tag X.Y.Z && git push origin X.Y.Z
```

## 2. crates.io

```bash
cargo publish --dry-run
cargo publish
```

## 3. Binaries + GitHub release (Actions)

Actions → **release** → Run workflow, `github_release: true`, `npm_publish: false`. It tests, builds the 8 platform
binaries, assembles and publint-checks the npm packages, and creates the GitHub release `X.Y.Z` with the binaries
and the npm tarballs (`oxlint-vue-rs-X.Y.Z.tgz` + one `oxlint-vue-rs-<platform>-X.Y.Z.tgz` per platform).

## 4. npm (by hand, from the release tarballs)

The main package depends on the platform packages through `optionalDependencies`, so publish the 8 platform
packages first and the main package last.

```bash
mkdir -p /tmp/ovr-npm && cd /tmp/ovr-npm
gh release download X.Y.Z -R s00d/oxlint-vue-rs -p '*.tgz'
npm login                       # once; an account with 2FA will ask for a one-time password on publish
for f in oxlint-vue-rs-*-X.Y.Z.tgz; do
  case "$f" in oxlint-vue-rs-X.Y.Z.tgz) continue ;; esac
  npm publish "$f" --access public
done
npm publish oxlint-vue-rs-X.Y.Z.tgz --access public
```

Check: `npm view oxlint-vue-rs@X.Y.Z optionalDependencies`, then in an empty directory
`npm i -D oxlint oxlint-vue-rs && npx oxlint-vue-rs --version`.

Notes:

- `--provenance` only works from CI; manual publishes are without provenance.
- Later releases can publish from CI instead: set the repository secret `NPM_TOKEN` (an npm automation token), or
  configure npm trusted publishing for each package (npmjs.com → package → Settings → Trusted publisher → GitHub
  Actions, workflow `release.yml`), then run the workflow with `npm_publish: true`. Versions already on npm are
  skipped, so a re-run after a partial publish completes it.
- A wrongly published version can be deprecated (`npm deprecate oxlint-vue-rs@X.Y.Z "…"`); unpublishing is only
  possible within 72 hours and the version number can never be reused.
