# Release Process

Steps to prepare a new release of `polkadot-rest-api`.

## 1. Bump workspace version

Update the `version` field in the root `Cargo.toml`:

```toml
# Cargo.toml
[workspace.package]
version = "0.1.0-beta.X"
```

## 2. Update `polkadot-rest-api-config` dependency version

Update the `polkadot-rest-api-config` version in both crates that depend on it:

- `crates/server/Cargo.toml`
- `crates/integration_tests/Cargo.toml`

```toml
polkadot-rest-api-config = { path = "../config", version = "0.1.0-beta.X" }
```

Then run `cargo check` to update `Cargo.lock`.

## 3. Update the changelog

Add a new entry to `CHANGELOG.md` for the release version, following the existing format (Features, Fixes, Performance, Refactors, CI, Other).

## 4. Update docs

### Version strings

Update the hardcoded version in these files:

1. **`crates/server/src/openapi.rs`** — update the `version` in the `#[openapi]` attribute.
2. **`docs/index.html`** — update the version in three places: `#api-version`, `#version-display`, and `#version-display-gs`.

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

## 5. Create a PR

Commit all changes with the message `chore: release v0.1.0-beta.X` and open a PR against `main`.

After merging, tag the release:

```bash
git tag v0.1.0-beta.X
git push origin v0.1.0-beta.X
```

## 6. Publish to crates.io

1. Create an API token at [crates.io](https://crates.io).
2. Log in:
   ```bash
   cargo login
   ```
3. Publish `polkadot-rest-api-config` first, then `polkadot-rest-api`. For each package, dry-run before publishing:
   ```bash
   cargo publish -p polkadot-rest-api-config --dry-run
   cargo publish -p polkadot-rest-api-config

   cargo publish -p polkadot-rest-api --dry-run
   cargo publish -p polkadot-rest-api
   ```

## 7. Publish the Docker image

Create a release on GitHub, selecting the corresponding version tag and including a release summary, then publish the release. The CI will handle Docker image publishing automatically.
