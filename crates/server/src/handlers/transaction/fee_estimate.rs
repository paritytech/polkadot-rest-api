use crate::state::AppState;
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Deserialize)]
pub struct FeeEstimateRequest {
    pub tx: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeeEstimateResponse {
    pub weight: Weight,
    pub class: String,
    pub partial_fee: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Weight {
    pub ref_time: String,
    pub proof_size: String,
}

#[derive(Debug, Serialize)]
pub struct At {
    pub hash: String,
}

#[derive(Debug, Serialize)]
pub struct FeeEstimateFailure {
    pub code: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<At>,
    pub error: String,
    pub transaction: String,
    pub cause: String,
    pub stack: String,
}

#[derive(Debug, Error)]
pub enum FeeEstimateError {
    #[error("Missing field `tx` on request body.")]
    MissingTx,

    #[error("Unable to fetch fee info")]
    FetchFailed {
        at_hash: Option<String>,
        transaction: String,
        cause: String,
        stack: String,
    },

    #[error("Relay chain not configured")]
    RelayChainNotConfigured { transaction: String },
}

impl IntoResponse for FeeEstimateError {
    fn into_response(self) -> axum::response::Response {
        let (status, code, at_hash, error_msg, transaction, cause, stack) = match self {
            FeeEstimateError::MissingTx => {
                let cause = "Missing field `tx` on request body.".to_string();
                (
                    StatusCode::BAD_REQUEST,
                    400,
                    None,
                    "Unable to fetch fee info",
                    String::new(),
                    cause.clone(),
                    format!("Error: {}\n    at fee_estimate", cause),
                )
            }
            FeeEstimateError::FetchFailed {
                at_hash,
                transaction,
                cause,
                stack,
            } => (
                StatusCode::BAD_REQUEST,
                400,
                at_hash,
                "Unable to fetch fee info",
                transaction,
                cause,
                stack,
            ),
            FeeEstimateError::RelayChainNotConfigured { transaction } => {
                let cause = "Relay chain not configured".to_string();
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    503,
                    None,
                    "Unable to fetch fee info",
                    transaction,
                    cause.clone(),
                    format!("Error: {}\n    at fee_estimate", cause),
                )
            }
        };

        let body = Json(FeeEstimateFailure {
            code,
            at: at_hash.map(|hash| At { hash }),
            error: error_msg.to_string(),
            transaction,
            cause,
            stack,
        });
        (status, body).into_response()
    }
}

pub async fn fee_estimate(
    State(state): State<AppState>,
    Json(body): Json<FeeEstimateRequest>,
) -> Result<Json<FeeEstimateResponse>, FeeEstimateError> {
    fee_estimate_internal(&state.client, body).await
}

pub async fn fee_estimate_rc(
    State(state): State<AppState>,
    Json(body): Json<FeeEstimateRequest>,
) -> Result<Json<FeeEstimateResponse>, FeeEstimateError> {
    let tx = body.tx.as_deref().unwrap_or_default();
    let relay_client = state.get_relay_chain_client().ok_or_else(|| {
        FeeEstimateError::RelayChainNotConfigured {
            transaction: tx.to_string(),
        }
    })?;

    fee_estimate_internal(relay_client, body).await
}

async fn fee_estimate_internal(
    client: &subxt::OnlineClient<subxt::SubstrateConfig>,
    body: FeeEstimateRequest,
) -> Result<Json<FeeEstimateResponse>, FeeEstimateError> {
    let tx = body.tx.as_ref().ok_or(FeeEstimateError::MissingTx)?;
    if tx.is_empty() {
        return Err(FeeEstimateError::MissingTx);
    }

    // Get finalized block
    let client_at = client.at_current_block().await.map_err(|e| {
        let cause = e.to_string();
        FeeEstimateError::FetchFailed {
            at_hash: None,
            transaction: tx.to_string(),
            cause: cause.clone(),
            stack: format!("Error: {}\n    at fee_estimate", cause),
        }
    })?;

    let block_hash = format!("{:?}", client_at.block_ref().hash());

    // Decode transaction bytes
    let tx_bytes = hex::decode(tx.strip_prefix("0x").unwrap_or(tx)).map_err(|e| {
        FeeEstimateError::FetchFailed {
            at_hash: Some(block_hash.clone()),
            transaction: tx.to_string(),
            cause: format!("Invalid hex encoding: {}", e),
            stack: format!("Error: Invalid hex encoding: {}\n    at fee_estimate", e),
        }
    })?;

    // Call TransactionPaymentApi.query_info(extrinsic, len)
    let len = tx_bytes.len() as u32;
    let extrinsic = subxt::dynamic::Value::from_bytes(tx_bytes);
    let length = subxt::dynamic::Value::u128(len as u128);

    let method = subxt::dynamic::runtime_api_call::<_, scale_value::Value<()>>(
        "TransactionPaymentApi",
        "query_info",
        (extrinsic, length),
    );
    let result = client_at.runtime_apis().call(method).await.map_err(|e| {
        let cause = e.to_string();
        FeeEstimateError::FetchFailed {
            at_hash: Some(block_hash.clone()),
            transaction: tx.to_string(),
            cause: cause.clone(),
            stack: format!("Error: {}\n    at fee_estimate", cause),
        }
    })?;

    parse_fee_estimate_response(result, tx, &block_hash)
}

fn parse_fee_estimate_response(
    result: scale_value::Value<()>,
    tx: &str,
    block_hash: &str,
) -> Result<Json<FeeEstimateResponse>, FeeEstimateError> {
    use scale_value::{Composite, ValueDef};

    // Helper to get value by key from a Composite
    fn get_named_value<'a>(
        composite: &'a Composite<()>,
        key: &str,
    ) -> Option<&'a scale_value::Value<()>> {
        match composite {
            Composite::Named(vals) => vals.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            Composite::Unnamed(_) => None,
        }
    }

    // Extract primitive as string
    fn extract_u128(value: &scale_value::Value<()>) -> Option<String> {
        if let ValueDef::Primitive(scale_value::Primitive::U128(n)) = &value.value {
            Some(n.to_string())
        } else {
            None
        }
    }

    // The result is RuntimeDispatchInfo { weight, class, partialFee }
    if let ValueDef::Composite(composite) = &result.value {
        // Extract weight
        let weight = if let Some(weight_val) = get_named_value(composite, "weight") {
            if let ValueDef::Composite(weight_composite) = &weight_val.value {
                let ref_time = get_named_value(weight_composite, "ref_time")
                    .and_then(extract_u128)
                    .unwrap_or_else(|| "0".to_string());
                let proof_size = get_named_value(weight_composite, "proof_size")
                    .and_then(extract_u128)
                    .unwrap_or_else(|| "0".to_string());
                Weight {
                    ref_time,
                    proof_size,
                }
            } else {
                Weight {
                    ref_time: "0".to_string(),
                    proof_size: "0".to_string(),
                }
            }
        } else {
            Weight {
                ref_time: "0".to_string(),
                proof_size: "0".to_string(),
            }
        };

        // Extract class (it's a variant: Normal, Operational, or Mandatory)
        let class = if let Some(class_val) = get_named_value(composite, "class") {
            if let ValueDef::Variant(variant) = &class_val.value {
                variant.name.clone()
            } else {
                "Normal".to_string()
            }
        } else {
            "Normal".to_string()
        };

        // Extract partialFee
        let partial_fee = get_named_value(composite, "partial_fee")
            .and_then(extract_u128)
            .unwrap_or_else(|| "0".to_string());

        return Ok(Json(FeeEstimateResponse {
            weight,
            class,
            partial_fee,
        }));
    }

    Err(FeeEstimateError::FetchFailed {
        at_hash: Some(block_hash.to_string()),
        transaction: tx.to_string(),
        cause: "Unexpected response format".to_string(),
        stack: "Error: Unexpected response format\n    at fee_estimate".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fee_estimate_response_serialization() {
        let response = FeeEstimateResponse {
            weight: Weight {
                ref_time: "1000000".to_string(),
                proof_size: "2000".to_string(),
            },
            class: "Normal".to_string(),
            partial_fee: "123456789".to_string(),
        };
        let json = serde_json::to_value(&response).unwrap();
        assert_eq!(json["weight"]["refTime"], "1000000");
        assert_eq!(json["weight"]["proofSize"], "2000");
        assert_eq!(json["class"], "Normal");
        assert_eq!(json["partialFee"], "123456789");
    }

    #[test]
    fn test_fee_estimate_failure_with_at_serialization() {
        let error = FeeEstimateFailure {
            code: 400,
            at: Some(At {
                hash: "0x1234567890abcdef".to_string(),
            }),
            error: "Unable to fetch fee info".to_string(),
            transaction: "0x1234".to_string(),
            cause: "Invalid transaction".to_string(),
            stack: "Error: Invalid transaction\n    at fee_estimate".to_string(),
        };
        let json = serde_json::to_value(&error).unwrap();
        assert_eq!(json["code"], 400);
        assert_eq!(json["at"]["hash"], "0x1234567890abcdef");
        assert_eq!(json["error"], "Unable to fetch fee info");
        assert_eq!(json["transaction"], "0x1234");
    }

    #[test]
    fn test_fee_estimate_failure_without_at_serialization() {
        let error = FeeEstimateFailure {
            code: 400,
            at: None,
            error: "Unable to fetch fee info".to_string(),
            transaction: "".to_string(),
            cause: "Missing field `tx`".to_string(),
            stack: "Error: Missing field `tx`\n    at fee_estimate".to_string(),
        };
        let json = serde_json::to_value(&error).unwrap();
        assert_eq!(json["code"], 400);
        assert!(json.get("at").is_none());
        assert_eq!(json["error"], "Unable to fetch fee info");
    }
}
