# Releasing

A release is a version-bump PR followed by a `vX.Y.Z` tag. Pushing the tag starts two workflows
that run independently: **Release** (crates.io, CLI binaries, GitHub Release) and **CI & PyPI
Release** (wheels, sdist, PyPI). Both first require the CI checks of the tagged commit to be
green, so a red commit cannot be published to either registry.

## 1. Prepare the version bump

```bash
git switch -c release/v1.2.0 origin/main
python3 scripts/release/prepare.py 1.2.0 --dry-run   # preview the diff
python3 scripts/release/prepare.py 1.2.0
```

The script bumps `[workspace.package]` and the internal `rust-motion-*` dependency versions, any
crate that pins its own version, `pyproject.toml`, `crates/ts/package.json`, the docs and the
Homebrew formula's version, refreshes `Cargo.lock`, and drafts a `CHANGELOG.md` entry from the
conventional commits since the last tag. It warns about any other file that still mentions the
old version. It never commits, tags or pushes.

Then edit the CHANGELOG draft, review `git diff`, commit as `chore(release): prepare v1.2.0`,
and open a PR. If a crate was added or is now publishable, update `.github/release-crates.txt`
(the Release workflow fails if it disagrees with the workspace).

## 2. Dry run (optional, recommended)

After the PR is merged, or from the release branch, run both workflows without publishing:

- Actions → **Release** → Run workflow → leave `dry_run` checked.
- Actions → **CI & PyPI Release** → Run workflow → leave `dry_run` checked.

A dry run checks versions and the crates.io file lists, builds the CLI binaries for every
platform, builds and smoke-tests the Python wheels and the sdist, and publishes nothing.

## 3. Tag

Tag the merge commit on `main` once its CI run has finished (the release waits for it either way):

```bash
git switch main && git pull
git tag -a v1.2.0 -m "Rust Motion v1.2.0"
git push origin v1.2.0
```

After the release exists, update `sha256` in `packaging/homebrew/rust-motion.rb`.

## If a release fails part-way

Fix the cause, then run the failed workflow from Actions with `tag` set to the existing tag and
`dry_run` unchecked. Already-published crates are skipped and PyPI uploads use `skip-existing`,
so retrying is safe. Re-pointing a tag is only appropriate when nothing was published.

## Toolchain

CI, releases and local builds use the Rust version in `rust-toolchain.toml`. A weekly workflow
(`Toolchain canary`) checks the newest stable Rust and opens an issue when it starts failing
clippy. To upgrade: fix the new lints, then bump `channel` in `rust-toolchain.toml`.

## Required checks and secrets

- Branch protection on `main` requires: `Code Quality (Fmt & Clippy)`, `Test (ubuntu-latest)`,
  `Test (macos-latest)`, `Test (windows-latest)`, `TypeScript SDK & Remotion Bridge`, and
  `WebAssembly (WASM Engine & Browser Canvas)`. The test job names come from the matrix `label`
  in `ci.yml`; renaming a check there means updating both the rule and the `require-ci` action.
- `CRATES_IO_TOKEN`: crates.io publishing. crates.io trusted publishing issues tokens that expire
  after about 30 minutes, which is shorter than publishing every crate takes, so a token stays.
- PyPI: the publish step uses OIDC when `PYPI_API_TOKEN` is empty. To switch, register a trusted
  publisher on PyPI for project `rust-motion` (repository `sjkim1127/rust-motion`, workflow
  `pypi.yml`), then delete the `PYPI_API_TOKEN` secret.
