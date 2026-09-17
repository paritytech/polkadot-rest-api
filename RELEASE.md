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

Run these strictly in order. `polkadot-rest-api` depends on `polkadot-rest-api-config` by version, so
its dry run fails with `failed to select a version for the requirement polkadot-rest-api-config`
until the config crate is actually on the index. That failure is expected if you jump ahead, not a
problem with the package.

## 7. Publish the GitHub release

The Docker image is already built by this point. `Build and Deploy` (`deploy.yml`) triggers on the
`v*` tag push from step 5, not on the GitHub release, and publishes three tags:

| tag | example | who uses it |
| --- | --- | --- |
| `vX.Y.Z` | `v0.3.0` | humans, and anyone pinning a release |
| `latest` | | only ever points at a released version |
| `YYYYMMDD-HHMMSS-<sha8>` | `20260917-161956-e6f087bb` | **Kargo**, see step 8 |

Check all three appear at https://hub.docker.com/r/paritytech/polkadot-rest-api before continuing. If
they are missing, the tag push did not run the workflow and step 8 has nothing to deploy.

Then create the release on GitHub against the version tag, with a summary taken from the changelog
entry. This is release notes only; nothing is triggered by it.

## 8. Promote to the public instances

Deployment is driven by [Kargo](https://kargo.teleport.parity.io/), not by a change in
`devops-cloud-infra`. The image tag is written by a Kargo promotion, so there is no version pinned in
that repo to raise a PR or an issue against. Access is via Teleport with GitHub SSO; promote rights
come from a GitHub group named after the app, so if the UI will not let you promote, that is what to
request (example: `paritytech/devops-cloud-infra#4218`).

The Kargo project is `polkadot-rest-api-kargo`. Its Warehouse polls Docker Hub every 5 minutes and
only matches the `YYYYMMDD-HHMMSS-<sha8>` tag from step 7. The `vX.Y.Z` tag is invisible to it, so
identify your freight by the date stamped tag.

Promote **westend first, then production**, and note the pairing is per chain rather than per
environment:

```
westend-hub-rest-api     ->  kusama-hub-rest-api,    polkadot-hub-rest-api
westend-relay-rest-api   ->  kusama-relay-rest-api,  polkadot-relay-rest-api
```

The westend stages take freight directly from the Warehouse. The kusama and polkadot stages cannot:
they only accept freight that has already passed the matching westend stage, so `polkadot-relay`
sources from `westend-relay`, never from `westend-hub`. Promoting only one westend stage leaves half
the production instances behind on the old version.

Then verify, rather than assuming the promotion landed:

```bash
for h in polkadot-hub kusama-hub westend-hub polkadot-relay kusama-relay westend-relay; do
  printf "%-16s %s\n" "$h" "$(curl -s https://$h-rest-api.parity.io/v1/version)"
done
```

All six should report the version you just released. A `Bad Gateway` usually means that pod is still
restarting; retry before treating it as a failure.

## 9. Final check

- crates: [config](https://crates.io/crates/polkadot-rest-api-config) - [main](https://crates.io/crates/polkadot-rest-api)
- [GitHub release](https://github.com/paritytech/polkadot-rest-api/releases)
- [Docker tags](https://hub.docker.com/r/paritytech/polkadot-rest-api)
- All six public instances report the new version (see the loop in step 8); any external partner
  waiting on a fix is informed.

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
