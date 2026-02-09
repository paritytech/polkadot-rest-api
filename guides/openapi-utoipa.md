# OpenAPI Documentation with utoipa

This project uses [utoipa](https://github.com/juhaku/utoipa) v5 to generate an OpenAPI 3.0 spec from handler annotations. The spec is served as interactive Swagger UI at `/docs` and raw JSON at `/api-docs/openapi.json`.

## How it works

1. Each handler function has a `#[utoipa::path(...)]` attribute describing its HTTP method, path, parameters, and responses.
2. The macro generates a hidden `__path_<fn_name>` struct in the same module as the handler.
3. `crates/server/src/openapi.rs` aggregates all handlers via `#[derive(OpenApi)]` with a `paths(...)` list.
4. `crates/server/src/app.rs` serves the generated spec at `/api-docs/openapi.json` and a static Swagger UI page at `/docs`.

## Adding a new endpoint

### 1. Annotate the handler function

Add `#[utoipa::path(...)]` directly above your `pub async fn`. Pick the template that matches your handler type.

**GET with path + query params:**

```rust
#[utoipa::path(
    get,
    path = "/v1/my-category/{myId}",
    tag = "my-category",
    summary = "Short title for Swagger UI sidebar",
    description = "Longer description shown when the endpoint is expanded.",
    params(
        ("myId" = String, Path, description = "The resource identifier"),
        ("at" = Option<String>, Query, description = "Block identifier (number or hash)"),
        ("flag" = Option<bool>, Query, description = "Some boolean toggle"),
    ),
    responses(
        (status = 200, description = "Success", body = Object),
        (status = 400, description = "Invalid request"),
        (status = 500, description = "Internal server error"),
    )
)]
pub async fn my_handler(...) -> ... { ... }
```

**POST with JSON body:**

```rust
#[utoipa::path(
    post,
    path = "/v1/transaction/my-action",
    tag = "transaction",
    summary = "Do something",
    description = "Does something with the submitted data.",
    request_body(content = Object, description = "JSON body with 'tx' field"),
    responses(
        (status = 200, description = "Success", body = Object),
        (status = 400, description = "Invalid input"),
    )
)]
pub async fn my_action(...) -> ... { ... }
```

**GET with no params:**

```rust
#[utoipa::path(
    get,
    path = "/v1/health",
    tag = "health",
    summary = "Health check",
    description = "Returns service health status.",
    responses(
        (status = 200, description = "Healthy", body = Object),
    )
)]
pub async fn get_health(...) -> ... { ... }
```

> **Note on `body = Object`:** Most handlers return dynamic `serde_json::Value` data, so use `body = Object`. Only use a concrete type (e.g., `body = HealthResponse`) if the response struct derives `utoipa::ToSchema`.

### 2. Ensure the handler's module is `pub mod`

The `#[utoipa::path]` macro generates a hidden struct `__path_<fn_name>` in the same module as the handler. This struct must be accessible from `openapi.rs`. If the module is declared as `mod my_handler;` (private), change it to `pub mod my_handler;` in the parent `mod.rs`.

```rust
// crates/server/src/handlers/my_category/mod.rs

pub mod my_handler;  // must be `pub mod`, NOT `mod`

pub use my_handler::my_handler;  // re-export is fine but doesn't help utoipa
```

### 3. Register the handler in `openapi.rs`

Add the handler's **full submodule path** to the `paths(...)` list in `crates/server/src/openapi.rs`.

```rust
paths(
    // ... existing paths ...

    // Use the FULL module path, not the re-exported name
    crate::handlers::my_category::my_handler_file::my_handler,
)
```

**Critical:** Always use the path to the actual module file, not the re-exported path. `pub use` re-exports the function but NOT the `__path_*` struct that utoipa needs.

```rust
// WRONG - utoipa can't find __path_my_handler at this path
crate::handlers::my_category::my_handler,

// CORRECT - utoipa finds __path_my_handler in the submodule
crate::handlers::my_category::my_handler_file::my_handler,
```

### 4. Pick the right tag

Use an existing tag from `openapi.rs` so the endpoint groups properly in Swagger UI:

| Tag | Use for |
|-----|---------|
| `health` | Health check endpoints |
| `node` | Node version, network, tx pool |
| `version` | API version |
| `blocks` | Block queries |
| `accounts` | Account balance, staking, proxy, vesting |
| `pallets` | Pallet storage, consts, errors, events, dispatchables |
| `runtime` | Runtime spec, metadata, code |
| `transaction` | Submit, dry-run, fee-estimate, material |
| `coretime` | Coretime info, leases, regions, renewals, reservations |
| `paras` | Parachain inclusion |
| `ahm` | Asset Hub Migration |
| `capabilities` | API capabilities |
| `rc` | All relay chain (`/rc/...`) endpoints |

If you need a new tag, add it to the `tags(...)` block in `openapi.rs`.

### 5. Build and verify

```bash
cargo build --package server
cargo test --package server
```

If you get `could not find __path_<fn_name>`, check:
1. Is the module `pub mod` in its parent `mod.rs`?
2. Does the path in `openapi.rs` point to the **actual submodule**, not the re-exported name?

**The sync test will catch you if you forget.** A test in `openapi.rs` (`openapi_paths_match_registered_routes`) automatically compares the route registry against the OpenAPI spec. It runs as part of `cargo test --package server` and will fail with a message like:

```
Routes registered but MISSING from OpenAPI spec:
  - GET /v1/my-category/{}
```

This means a route exists in `routes/*.rs` but has no matching `#[utoipa::path]` annotation (or isn't registered in `openapi.rs`). The reverse case — a path in the spec with no registered route — is also caught.

## Checklist

- [ ] `#[utoipa::path(...)]` added above the handler function
- [ ] Module is `pub mod` (not private `mod`) in parent `mod.rs`
- [ ] Full submodule path added to `paths(...)` in `openapi.rs`
- [ ] `path =` matches the actual route registered in `routes/*.rs`
- [ ] `tag =` uses an existing tag (or you added a new one)
- [ ] `cargo build --package server` compiles
- [ ] Swagger UI at `/docs` shows the new endpoint

## Architecture notes

### Why not utoipa-swagger-ui?

`utoipa-swagger-ui` v9 targets axum 0.8. This project uses axum 0.7. Instead, the Swagger UI is served as a static HTML page (`crates/server/src/swagger_ui.html`) that loads Swagger UI from the unpkg CDN (`unpkg.com/swagger-ui-dist@5`). The OpenAPI JSON is served via a plain Axum route.

### Why full submodule paths?

`#[utoipa::path]` generates `__path_<fn_name>` in the handler's module. A `pub use handler::fn_name` in `mod.rs` re-exports the function but **not** the generated struct. The `#[derive(OpenApi)]` macro needs the struct, so paths in `openapi.rs` must point to the actual submodule where the struct lives.

### Key files

| File | Purpose |
|------|---------|
| `crates/server/src/openapi.rs` | Central `#[derive(OpenApi)]` — registers all paths and tags |
| `crates/server/src/app.rs` | Mounts `/api-docs/openapi.json` and `/docs` routes |
| `crates/server/src/swagger_ui.html` | Static Swagger UI page (CDN-based) |
| `crates/server/Cargo.toml` | `utoipa = { version = "5", features = ["axum_extras"] }` |
