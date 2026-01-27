use crate::state::AppState;
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde::{Deserialize, Serialize};
use subxt_rpcs::rpc_params;
use thiserror::Error;

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

/// Error response when transaction fails to parse.
#[derive(Debug, Serialize)]
pub struct TransactionFailedToParse {
    pub code: u16,
    pub error: String,
    pub transaction: String,
    pub cause: String,
    pub stack: String,
}

/// Error response when transaction fails to submit.
#[derive(Debug, Serialize)]
pub struct TransactionFailedToSubmit {
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

    #[error("Relay chain not configured")]
    RelayChainNotConfigured { transaction: String },
}

impl IntoResponse for SubmitError {
    fn into_response(self) -> axum::response::Response {
        match self {
            SubmitError::MissingTx => {
                let cause = "Missing field `tx` on request body.".to_string();
                let body = Json(TransactionFailedToParse {
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
                let body = Json(TransactionFailedToParse {
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
                let body = Json(TransactionFailedToSubmit {
                    code: 400,
                    error: "Failed to submit transaction.".to_string(),
                    transaction,
                    cause,
                    stack,
                });
                (StatusCode::BAD_REQUEST, body).into_response()
            }
            SubmitError::RelayChainNotConfigured { transaction } => {
                let cause = "Relay chain not configured".to_string();
                let body = Json(TransactionFailedToSubmit {
                    code: 503,
                    error: "Failed to submit transaction.".to_string(),
                    transaction,
                    cause: cause.clone(),
                    stack: format!("Error: {}\n    at submit_rc", cause),
                });
                (StatusCode::SERVICE_UNAVAILABLE, body).into_response()
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

/// Submit a signed extrinsic to the transaction pool.
pub async fn submit(
    State(state): State<AppState>,
    Json(body): Json<SubmitRequest>,
) -> Result<Json<SubmitResponse>, SubmitError> {
    // Validate tx field
    let tx = body.tx.as_ref().ok_or(SubmitError::MissingTx)?;
    if tx.is_empty() {
        return Err(SubmitError::MissingTx);
    }

    // Submit via author_submitExtrinsic RPC
    // The node will validate and parse the transaction
    let hash: String = state
        .rpc_client
        .request("author_submitExtrinsic", rpc_params![tx])
        .await
        .map_err(|e| {
            let (cause, stack) = extract_cause_and_stack(&e);

            // Determine if this is a parse error or submit error based on the error content
            if is_parse_error(&e) {
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
            }
        })?;

    Ok(Json(SubmitResponse { hash }))
}

/// Submit a signed extrinsic to the relay chain's transaction pool.
pub async fn submit_rc(
    State(state): State<AppState>,
    Json(body): Json<SubmitRequest>,
) -> Result<Json<SubmitResponse>, SubmitError> {
    // Validate tx field
    let tx = body.tx.as_ref().ok_or(SubmitError::MissingTx)?;
    if tx.is_empty() {
        return Err(SubmitError::MissingTx);
    }

    // Get relay chain RPC client
    let relay_rpc =
        state
            .get_relay_chain_rpc_client()
            .ok_or_else(|| SubmitError::RelayChainNotConfigured {
                transaction: tx.clone(),
            })?;

    // Submit via relay chain's author_submitExtrinsic RPC
    let hash: String = relay_rpc
        .request("author_submitExtrinsic", rpc_params![tx])
        .await
        .map_err(|e| {
            let (cause, stack) = extract_cause_and_stack(&e);

            if is_parse_error(&e) {
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
            }
        })?;

    Ok(Json(SubmitResponse { hash }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_error_response_serialization() {
        let error = TransactionFailedToParse {
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
        let error = TransactionFailedToSubmit {
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
