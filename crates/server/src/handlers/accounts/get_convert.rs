use super::types::{AccountsError, AccountConvertQueryParams, AccountConvertResponse};
use axum::{
    Json,
    extract::{Path, Query},
    response::{IntoResponse, Response},
};
use sp_core::crypto::{AccountId32, Ss58AddressFormat, Ss58Codec};

// ================================================================================================
// Main Handler
// ================================================================================================

/// Handler for GET /accounts/{accountId}/convert
///
/// Converts an AccountId or Public Key (hex) to an SS58 address.
///
/// Path Parameters:
/// - `accountId`: The AccountId or Public Key as hex string (with or without 0x prefix)
///
/// Query Parameters:
/// - `scheme` (optional): Cryptographic scheme - "ed25519", "sr25519", or "ecdsa" (default: "sr25519")
/// - `prefix` (optional): SS58 prefix number (default: 42)
/// - `publicKey` (optional): If true, treat the input as a public key (default: false)
pub async fn get_convert(
    Path(account_id): Path<String>,
    Query(params): Query<AccountConvertQueryParams>,
) -> Result<Response, AccountsError> {
    // Validate scheme
    let scheme = params.scheme.to_lowercase();
    if scheme != "ed25519" && scheme != "sr25519" && scheme != "ecdsa" {
        return Err(AccountsError::InvalidScheme);
    }

    // Validate that account_id is valid hex
    let account_id_clean = account_id.trim_start_matches("0x");
    if !is_valid_hex(account_id_clean) {
        return Err(AccountsError::InvalidHexAccountId);
    }

    // Get the network name for this prefix
    let network = get_network_name(params.prefix)
        .ok_or(AccountsError::InvalidPrefix)?;

    // Decode the hex to bytes
    let account_bytes = hex::decode(account_id_clean)
        .map_err(|_| AccountsError::InvalidHexAccountId)?;

    // For ecdsa with public key > 32 bytes, we need to hash it first
    let final_bytes = if params.public_key && scheme == "ecdsa" && account_bytes.len() > 32 {
        // Hash with blake2_256
        sp_core::blake2_256(&account_bytes).to_vec()
    } else {
        account_bytes
    };

    // Convert to AccountId32 (requires exactly 32 bytes)
    if final_bytes.len() != 32 {
        return Err(AccountsError::EncodingFailed(format!(
            "Expected 32 bytes, got {}",
            final_bytes.len()
        )));
    }

    let mut account_id_bytes = [0u8; 32];
    account_id_bytes.copy_from_slice(&final_bytes);

    let account_id32 = AccountId32::new(account_id_bytes);

    // Encode to SS58
    let ss58_format = Ss58AddressFormat::custom(params.prefix);
    let address = account_id32.to_ss58check_with_version(ss58_format);

    let response = AccountConvertResponse {
        ss58_prefix: params.prefix,
        network,
        address,
        account_id: format!("0x{}", account_id_clean),
        scheme: scheme.to_string(),
        public_key: params.public_key,
    };

    Ok(Json(response).into_response())
}

// ================================================================================================
// Helper Functions
// ================================================================================================

/// Check if a string is valid hexadecimal
fn is_valid_hex(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// Get the network name for a given SS58 prefix
fn get_network_name(prefix: u16) -> Option<String> {
    // Common SS58 prefixes and their network names
    // Based on https://github.com/paritytech/ss58-registry
    match prefix {
        0 => Some("polkadot".to_string()),
        1 => Some("bareEd25519".to_string()),
        2 => Some("kusama".to_string()),
        3 => Some("bareSr25519".to_string()),
        4 => Some("katalchain".to_string()),
        5 => Some("plasm".to_string()),
        6 => Some("bifrost".to_string()),
        7 => Some("edgeware".to_string()),
        8 => Some("karura".to_string()),
        9 => Some("reynolds".to_string()),
        10 => Some("acala".to_string()),
        11 => Some("laminar".to_string()),
        12 => Some("polymesh".to_string()),
        13 => Some("substraTEE".to_string()),
        14 => Some("totem".to_string()),
        15 => Some("synesthesia".to_string()),
        16 => Some("kulupu".to_string()),
        17 => Some("dark".to_string()),
        18 => Some("darwinia".to_string()),
        19 => Some("geek".to_string()),
        20 => Some("stafi".to_string()),
        21 => Some("dock-testnet".to_string()),
        22 => Some("dock-mainnet".to_string()),
        23 => Some("shift".to_string()),
        24 => Some("zero".to_string()),
        25 => Some("zero-alphaville".to_string()),
        26 => Some("jupiter".to_string()),
        27 => Some("kabocha".to_string()),
        28 => Some("subsocial".to_string()),
        29 => Some("cord".to_string()),
        30 => Some("phala".to_string()),
        31 => Some("litentry".to_string()),
        32 => Some("robonomics".to_string()),
        33 => Some("datahighway".to_string()),
        34 => Some("ares".to_string()),
        35 => Some("vln".to_string()),
        36 => Some("centrifuge".to_string()),
        37 => Some("nodle".to_string()),
        38 => Some("kilt".to_string()),
        39 => Some("mathchain".to_string()),
        40 => Some("mathchain-testnet".to_string()),
        41 => Some("poli".to_string()),
        42 => Some("substrate".to_string()),
        43 => Some("bareSecp256k1".to_string()),
        44 => Some("chainx".to_string()),
        45 => Some("uniarts".to_string()),
        46 => Some("reserved46".to_string()),
        47 => Some("reserved47".to_string()),
        48 => Some("neatcoin".to_string()),
        63 => Some("hydradx".to_string()),
        65 => Some("aventus".to_string()),
        66 => Some("crust".to_string()),
        67 => Some("equilibrium".to_string()),
        68 => Some("sora".to_string()),
        69 => Some("sora-kusama".to_string()),
        73 => Some("zeitgeist".to_string()),
        77 => Some("manta".to_string()),
        78 => Some("calamari".to_string()),
        88 => Some("polkadex".to_string()),
        98 => Some("polkasmith".to_string()),
        99 => Some("polkafoundry".to_string()),
        101 => Some("origintrail-parachain".to_string()),
        105 => Some("heiko".to_string()),
        110 => Some("parallel".to_string()),
        128 => Some("clover".to_string()),
        131 => Some("litmus".to_string()),
        136 => Some("altair".to_string()),
        172 => Some("parallel-heiko".to_string()),
        252 => Some("social-network".to_string()),
        255 => Some("quartz".to_string()),
        1284 => Some("moonbeam".to_string()),
        1285 => Some("moonriver".to_string()),
        1328 => Some("ajuna".to_string()),
        2007 => Some("kapex".to_string()),
        2032 => Some("interlay".to_string()),
        2092 => Some("kintsugi".to_string()),
        7391 => Some("unique".to_string()),
        10041 => Some("basilisk".to_string()),
        11330 => Some("cess-testnet".to_string()),
        11331 => Some("cess".to_string()),
        _ => {
            // For unknown prefixes, return a generic name
            // This differs from sidecar which returns an error
            // But it's more flexible for custom networks
            Some(format!("unknown-{}", prefix))
        }
    }
}
