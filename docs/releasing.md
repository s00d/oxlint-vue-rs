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
binaries and Node addons (`napi/`, shipped as `oxlint-vue-rs.node` in each platform package and attached to the
release as `oxlint-vue-rs-<target>.node`), assembles and publint-checks the npm packages, and creates the GitHub release `X.Y.Z` with the binaries
and the npm tarballs (`oxlint-vue-rs-X.Y.Z.tgz` + one per platform package `@oxlint-vue-rs/<platform>`, packed as
`oxlint-vue-rs-<platform>-X.Y.Z.tgz`).

## 4. npm (by hand, from the release tarballs)

```bash
npm login            # once
npm run publish:npm  # version from Cargo.toml; `-- X.Y.Z --dry-run` to check first; `--delay 60` for new names
```

`scripts/npm/publish.mjs` downloads the release's tarballs with `gh`, publishes the 8 platform packages first and
the main package last (it depends on them through `optionalDependencies`), and skips versions already on npm, so a
re-run finishes a partial publish. It waits 30 s (`--delay`) only before publishing a package name that does not exist on
npm yet — many new, similarly named packages in a row trip npm's spam detection; updates are not delayed.

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
