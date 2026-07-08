# Release Process

Steps to prepare a new release of `polkadot-rest-api`.

## Before you start

Four pins are maintained by hand — Dependabot doesn't bump them. Check each; if stale, bump it in
its **own PR before the release**.

**1. CI nightly toolchain** (`ci.yml`) — a newer nightly can add clippy lints.
```bash
grep -n "toolchain: nightly-" .github/workflows/ci.yml   # pinned
rustup check                                             # latest
```
If stale, bump the date and verify: `cargo +nightly-YYYY-MM-DD fmt --all -- --check` and `... clippy --workspace --all-features -- -D warnings`. Example: [#359](https://github.com/paritytech/polkadot-rest-api/pull/359).

**2. `dtolnay/rust-toolchain` action** (`ci.yml`, `# master`) — no release tag, so Dependabot skips it.
```bash
pinned=$(grep -m1 -oE 'rust-toolchain@[0-9a-f]{40}' .github/workflows/ci.yml | cut -d@ -f2)
git ls-remote https://github.com/dtolnay/rust-toolchain master | grep -q "$pinned" && echo "UP TO DATE" || echo "BEHIND"
```
If `BEHIND`, replace the SHA in **all** `ci.yml` occurrences (keep `# master`).

**3. `ubuntu:22.04` container digest** (`benchmark.yml`) — a container image, not an action.
```bash
grep -n "ubuntu:22.04@sha256" .github/workflows/benchmark.yml    # pinned
docker buildx imagetools inspect ubuntu:22.04 | grep -i digest   # latest (needs Docker)
```
If different, update the `@sha256:...` in `benchmark.yml`.

**4. Build Rust version** (`Dockerfile`, `rust:X.Y.Z-...`) — no `docker` Dependabot ecosystem set up.
```bash
grep -n "FROM.*rust:" Dockerfile   # pinned build Rust
rustup check                       # latest stable
```
If behind, bump the version in the `Dockerfile` `FROM` line.

## 1. Bump workspace version

Update the `version` field in the root `Cargo.toml`:

```toml
# Cargo.toml
[workspace.package]
version = "0.X.X"
```

## 2. Update `polkadot-rest-api-config` dependency version

Update the `polkadot-rest-api-config` version in both crates that depend on it:

- `crates/server/Cargo.toml`
- `crates/integration_tests/Cargo.toml`

```toml
polkadot-rest-api-config = { path = "../config", version = "0.X.X" }
```

Then run `cargo check` to update `Cargo.lock`.

## 3. Update the changelog

Add a new entry to `CHANGELOG.md` for the release version, following the existing format (Features, Fixes, Performance, Refactors, CI, Other).

## 4. Update docs

### Version strings

Update the hardcoded version in these files:

1. **`crates/server/src/openapi.rs`**: update the `version` in the `#[openapi]` attribute.
2. **`docs/index.html`**: update the version in three places: `#api-version`, `#version-display`, and `#version-display-gs`.

### Regenerate the OpenAPI spec and rebuild

The `openapi.json` is generated dynamically by the API from utoipa annotations on handlers, so you need a running server to fetch it:

```bash
# 1. Start the API server locally
SAS_SUBSTRATE_URL=wss://rpc.polkadot.io cargo run --release --bin polkadot-rest-api

# 2. In another terminal, fetch the latest spec
cd docs
yarn update-spec   # Runs: curl -s http://localhost:8080/api-docs/openapi.json > openapi.json

# 3. Rebuild the docs bundle with the updated spec
yarn build         # Regenerates docs/dist/index.html and docs/dist/bundle.js

# 4. Rebuild the API binary to embed the updated docs
cd ..
cargo build --release --package polkadot-rest-api
```

The built `dist/` folder is embedded into the API binary at compile time using `include_dir`, so the documentation is served directly by the API at `/docs/`.

## 5. Create the release PR

Commit all changes with the message `chore: release v0.X.X` and open a PR against `main`.

```bash
git add -A
git commit -m "chore: release v0.X.X"
```

After the PR merges to `main`, tag the release:

```bash
git checkout main && git pull
git tag v0.X.X
git push origin v0.X.X
```

## 6. Publish to crates.io

Publish `polkadot-rest-api-config` **first**, then `polkadot-rest-api`. You must be a crate **owner** of both crates (see the [Appendix](#appendix-cratesio-onboarding) for ownership + token setup). Log in, then dry-run each package before publishing:

```bash
cargo login   # paste the token

cargo publish -p polkadot-rest-api-config --dry-run
cargo publish -p polkadot-rest-api-config

cargo publish -p polkadot-rest-api --dry-run
cargo publish -p polkadot-rest-api
```

## 7. Publish the Docker image

Create a release on GitHub, selecting the corresponding version tag and including a release summary, then publish the release. The CI will handle Docker image publishing automatically.

Verify the tag appears at https://hub.docker.com/r/paritytech/polkadot-rest-api

## 8. Update the public instances

All public instances of `polkadot-rest-api` need to be updated to latest version, so create an issue in the `devops-cloud-infra` repository (example issue #3886).

## 9. Final check

- crates: [config](https://crates.io/crates/polkadot-rest-api-config) - [main](https://crates.io/crates/polkadot-rest-api)
- [GitHub release](https://github.com/paritytech/polkadot-rest-api/releases)
- [Docker tags](https://hub.docker.com/r/paritytech/polkadot-rest-api)
- All public instances up to date; any external partner waiting on a fix is informed.

## Appendix: crates.io onboarding

`crates.io` identity is your **GitHub username**. To publish you must be an owner of **both** crates.

- Ownership often comes via the `paritytech/core-devs` team; otherwise an existing owner runs `cargo owner --add <github-username>` per crate.
- A **verified email** is required before accepting invites or publishing:
  1. Save your email at https://crates.io/settings/profile and click the confirmation link.
  2. Accept both invites at https://crates.io/me/pending-invites.

**API token (least privilege)** at https://crates.io/settings/tokens:

- Scope: **only `publish-update`** (both crates already exist). Leave `publish-new`, `yank`, `change-owners` unchecked.
- Restrict to crates matching `polkadot-rest-api*`.
- Optional ~90-day expiry; give it an identifiable name (e.g. `rest-api-release`).
