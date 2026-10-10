// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::state::{AppState, RelayChainError};
use crate::utils::{ChainHasher, chain_hash_hex};
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use subxt_rpcs::rpc_params;
use thiserror::Error;

/// A submit taking longer than this is logged at warn rather than info.
///
/// A stalled RPC connection otherwise looks exactly like normal operation until the
/// caller times out, with nothing in our logs to distinguish the two.
const SLOW_SUBMIT: Duration = Duration::from_secs(5);

/// Request body for transaction submission.
#[derive(Debug, Deserialize)]
pub struct SubmitRequest {
    /// Hex-encoded signed extrinsic with 0x prefix.
    pub tx: Option<String>,
}

/// Response for successful transaction submission.
#[derive(Debug, Serialize)]
pub struct SubmitResponse {
    /// Transaction hash with 0x prefix.
    pub hash: String,
}

/// Error response when transaction fails to parse or parse.
#[derive(Debug, Serialize)]
pub struct TransactionError {
    pub code: u16,
    pub error: String,
    pub transaction: String,
    pub cause: String,
    pub stack: String,
}

/// Errors that can occur during transaction submission.
#[derive(Debug, Error)]
pub enum SubmitError {
    #[error("Missing field `tx` on request body.")]
    MissingTx,

    #[error("Failed to parse transaction.")]
    ParseFailed {
        transaction: String,
        cause: String,
        stack: String,
    },

    #[error("Failed to submit transaction.")]
    SubmitFailed {
        transaction: String,
        cause: String,
        stack: String,
    },

    #[error("Relay chain error")]
    RelayChain {
        source: RelayChainError,
        transaction: String,
    },
}

impl IntoResponse for SubmitError {
    fn into_response(self) -> axum::response::Response {
        match self {
            SubmitError::MissingTx => {
                let cause = "Missing field `tx` on request body.".to_string();
                let body = Json(TransactionError {
                    code: 400,
                    error: "Failed to parse transaction.".to_string(),
                    transaction: String::new(),
                    cause: cause.clone(),
                    stack: format!("Error: {}\n    at submit_transaction", cause),
                });
                (StatusCode::BAD_REQUEST, body).into_response()
            }
            SubmitError::ParseFailed {
                transaction,
                cause,
                stack,
            } => {
                let body = Json(TransactionError {
                    code: 400,
                    error: "Failed to parse transaction.".to_string(),
                    transaction,
                    cause,
                    stack,
                });
                (StatusCode::BAD_REQUEST, body).into_response()
            }
            SubmitError::SubmitFailed {
                transaction,
                cause,
                stack,
            } => {
                let body = Json(TransactionError {
                    code: 400,
                    error: "Failed to submit transaction.".to_string(),
                    transaction,
                    cause,
                    stack,
                });
                (StatusCode::BAD_REQUEST, body).into_response()
            }
            SubmitError::RelayChain {
                source,
                transaction,
            } => {
                let status = match source {
                    RelayChainError::NotConfigured => StatusCode::BAD_REQUEST,
                    RelayChainError::ConnectionFailed(_) => StatusCode::SERVICE_UNAVAILABLE,
                };
                let cause = source.to_string();
                let body = Json(TransactionError {
                    code: status.as_u16(),
                    error: "Failed to submit transaction.".to_string(),
                    transaction,
                    cause: cause.clone(),
                    stack: format!("Error: {}\n    at submit", cause),
                });
                (status, body).into_response()
            }
        }
    }
}

/// Extract cause and stack from an RPC error.
/// Mimics sidecar's extractCauseAndStack behavior.
fn extract_cause_and_stack(err: &subxt_rpcs::Error) -> (String, String) {
    let error_string = err.to_string();

    // The cause is the error message
    let cause = error_string.clone();

    // Build a stack trace - include the error and context
    let stack = format!("Error: {}\n    at submit_transaction", error_string);

    (cause, stack)
}

/// Check if an RPC error indicates a parsing/decoding failure.
fn is_parse_error(err: &subxt_rpcs::Error) -> bool {
    let error_str = err.to_string().to_lowercase();
    error_str.contains("decode")
        || error_str.contains("parse")
        || error_str.contains("invalid")
        || error_str.contains("extrinsic")
        || error_str.contains("bad signature")
        || error_str.contains("unable to decode")
}

#[utoipa::path(
    post,
    path = "/v1/transaction",
    tag = "transaction",
    summary = "Submit transaction",
    description = "Submit a signed extrinsic to the transaction pool.",
    request_body(content = Object, description = "Signed extrinsic with 'tx' field containing hex-encoded transaction"),
    responses(
        (status = 200, description = "Transaction hash", body = Object),
        (status = 400, description = "Invalid transaction"),
        (status = 503, description = "Service unavailable"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn submit(
    State(state): State<AppState>,
    Json(body): Json<SubmitRequest>,
) -> Result<Json<SubmitResponse>, SubmitError> {
    submit_internal(&state.rpc_client, &state.hasher, body).await
}

#[utoipa::path(
    post,
    path = "/v1/rc/transaction",
    tag = "rc",
    summary = "Submit transaction (relay chain)",
    description = "Submit a signed extrinsic to the relay chain transaction pool. Only available on parachains.",
    request_body(content = Object, description = "Signed extrinsic with 'tx' field containing hex-encoded transaction"),
    responses(
        (status = 200, description = "Transaction hash", body = Object),
        (status = 400, description = "Invalid transaction"),
        (status = 503, description = "Relay chain not configured"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn submit_rc(
    State(state): State<AppState>,
    Json(body): Json<SubmitRequest>,
) -> Result<Json<SubmitResponse>, SubmitError> {
    let tx_str = body.tx.as_deref().unwrap_or_default();
    let rpc_client =
        state
            .get_relay_chain_rpc_client()
            .await
            .map_err(|e| SubmitError::RelayChain {
                source: e,
                transaction: tx_str.to_string(),
            })?;

    // The relay chain is a different runtime, so it gets its own hasher. A relay chain we
    // can submit to but cannot read a hasher from is not worth failing the submission over,
    // so fall back to ours and let the node's hash settle any disagreement.
    let hasher = match state.get_relay_hasher().await {
        Ok(h) => h,
        Err(e) => {
            tracing::debug!(
                error = %e,
                "Could not resolve the relay chain hash function; logging with this chain's"
            );
            state.hasher
        }
    };

    submit_internal(&rpc_client, &hasher, body).await
}

/// Byte length and hash of a hex encoded extrinsic, using the chain's own hash function.
///
/// Invalid hex yields a length of 0 and an empty hash: this is only used for logging, and
/// the node is the authority on whether the payload is well formed.
fn describe_transaction(tx: &str, hasher: &ChainHasher) -> (usize, String) {
    match hex::decode(tx.strip_prefix("0x").unwrap_or(tx)) {
        Ok(bytes) => (bytes.len(), chain_hash_hex(hasher, &bytes)),
        Err(_) => (0, String::new()),
    }
}

async fn submit_internal(
    rpc_client: &std::sync::Arc<subxt_rpcs::RpcClient>,
    hasher: &ChainHasher,
    body: SubmitRequest,
) -> Result<Json<SubmitResponse>, SubmitError> {
    let tx = body.tx.as_ref().ok_or(SubmitError::MissingTx)?;
    if tx.is_empty() {
        return Err(SubmitError::MissingTx);
    }

    // Identify the transaction before it leaves us, so a submit that never comes back is
    // still attributable. The payload itself is never logged.
    //
    // At info, not debug: if the RPC connection stalls, this is the only line that ever
    // fires, because the outcome below is never reached. A sent line with no matching
    // outcome is exactly the signature of a stuck transaction, and it has to be visible
    // at the default level for that to be worth anything.
    let (tx_len, expected_hash) = describe_transaction(tx, hasher);
    tracing::info!(
        tx_hash = %expected_hash,
        tx_len,
        "Submitting extrinsic"
    );
    tracing::trace!(
        tx_hash = %expected_hash,
        tx = %tx,
        "Submitting extrinsic payload"
    );

    let started = Instant::now();
    let result: Result<String, _> = rpc_client
        .request("author_submitExtrinsic", rpc_params![tx])
        .await;
    let elapsed = started.elapsed();
    let elapsed_ms = elapsed.as_millis();

    let hash = match result {
        Ok(hash) => hash,
        Err(e) => {
            let (cause, stack) = extract_cause_and_stack(&e);
            let parse_error = is_parse_error(&e);

            tracing::warn!(
                tx_hash = %expected_hash,
                tx_len,
                elapsed_ms,
                kind = if parse_error { "parse" } else { "submit" },
                cause = %cause,
                "Extrinsic rejected"
            );

            return Err(if parse_error {
                SubmitError::ParseFailed {
                    transaction: tx.clone(),
                    cause,
                    stack,
                }
            } else {
                SubmitError::SubmitFailed {
                    transaction: tx.clone(),
                    cause,
                    stack,
                }
            });
        }
    };

    // The node echoes back the hash it computed. A mismatch means we and it disagree on
    // what was submitted, which is worth seeing rather than silently returning the node's.
    if hash != expected_hash {
        tracing::warn!(
            tx_hash = %expected_hash,
            node_hash = %hash,
            "Node returned a different extrinsic hash than we computed"
        );
    }

    if elapsed >= SLOW_SUBMIT {
        tracing::warn!(
            tx_hash = %hash,
            tx_len,
            elapsed_ms,
            "Extrinsic accepted, but the submit was slow"
        );
    } else {
        tracing::info!(
            tx_hash = %hash,
            tx_len,
            elapsed_ms,
            "Extrinsic accepted"
        );
    }

    Ok(Json(SubmitResponse { hash }))
}

#[cfg(test)]
mod describe_tests {
    use super::*;

    /// Asset Hub's hasher, read from metadata the same way production reads it. It resolves
    /// to BlakeTwo256, which is what the expected hashes below were taken from on chain.
    fn hasher() -> ChainHasher {
        crate::test_fixtures::test_chain_hasher()
    }

    /// The hash we log must be the one the node computes, which is blake2 over the whole
    /// length prefixed extrinsic. Real Asset Hub extrinsic, hash confirmed on chain.
    #[test]
    fn describes_a_real_extrinsic() {
        let tx = "0x55028400dc0c5e6f6c8265265f0bb9bbfa5c46a2471c05152066ed925e450361ad229913003132c42127d205558668f484efeafe4edcc8b49197e1f3f408774c411abc541d167c4f365bc62462bc2a46ca5db6284bf55bb48713171f15903e55cf7c415d01580529130000000a03001d46ccb04c50f93bde3457fd8e1dcee7da4ae2b8719f6d1df89f25afd75557c90f00506d96f51401";
        let (len, hash) = describe_transaction(tx, &hasher());
        assert_eq!(len, 151);
        assert_eq!(
            hash,
            "0x5c6a339dc73bca212310398c854689cfe024fced9a3b7a1c959c6daa27fff331"
        );
    }

    /// Works without the 0x prefix too, since callers are not required to send one.
    #[test]
    fn accepts_an_unprefixed_payload() {
        let (a, ha) = describe_transaction("0x0400", &hasher());
        let (b, hb) = describe_transaction("0400", &hasher());
        assert_eq!((a, &ha), (b, &hb));
    }

    /// Logging must never be the thing that fails a request, so bad hex degrades rather
    /// than panicking. The node is the authority on whether a payload is valid.
    #[test]
    fn invalid_hex_degrades_instead_of_panicking() {
        let (len, hash) = describe_transaction("0xnothex", &hasher());
        assert_eq!(len, 0);
        assert!(hash.is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_error_response_serialization() {
        let error = TransactionError {
            code: 400,
            error: "Failed to parse transaction.".to_string(),
            transaction: "0x1234".to_string(),
            cause: "Unable to decode extrinsic".to_string(),
            stack: "Error: Unable to decode extrinsic\n    at submit_transaction".to_string(),
        };
        let json = serde_json::to_value(&error).unwrap();
        assert_eq!(json["code"], 400);
        assert_eq!(json["error"], "Failed to parse transaction.");
        assert_eq!(json["transaction"], "0x1234");
        assert_eq!(json["cause"], "Unable to decode extrinsic");
        assert!(json["stack"].as_str().unwrap().contains("Error:"));
    }

    #[test]
    fn test_submit_error_response_serialization() {
        let error = TransactionError {
            code: 400,
            error: "Failed to submit transaction.".to_string(),
            transaction: "0x1234".to_string(),
            cause: "Transaction pool is full".to_string(),
            stack: "Error: Transaction pool is full\n    at submit_transaction".to_string(),
        };
        let json = serde_json::to_value(&error).unwrap();
        assert_eq!(json["code"], 400);
        assert_eq!(json["error"], "Failed to submit transaction.");
        assert_eq!(json["transaction"], "0x1234");
        assert_eq!(json["cause"], "Transaction pool is full");
        assert!(json["stack"].as_str().unwrap().contains("Error:"));
    }
}
