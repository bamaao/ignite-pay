use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Json;
use serde::{Deserialize, Serialize};
use solana_client::rpc_client::RpcClient;
use solana_sdk::signature::{Keypair, Signer};
use solana_sdk::transaction::Transaction;
use std::sync::Arc;

// ── Config ──────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct RelayerConfig {
    #[serde(default)]
    keypair_b58: String,
    #[serde(default = "default_rpc_url")]
    rpc_url: String,
    #[serde(default = "default_listen_addr")]
    listen_addr: String,
    #[serde(default = "default_rate_limit")]
    #[allow(dead_code)]
    rate_limit: u32,
}

#[derive(Debug, Deserialize)]
struct Config {
    #[serde(default)]
    relayer: RelayerConfig,
}

fn default_rpc_url() -> String {
    "https://api.devnet.solana.com".to_string()
}
fn default_listen_addr() -> String {
    "0.0.0.0:3030".to_string()
}
fn default_rate_limit() -> u32 {
    60
}

impl Default for RelayerConfig {
    fn default() -> Self {
        Self {
            keypair_b58: String::new(),
            rpc_url: default_rpc_url(),
            listen_addr: default_listen_addr(),
            rate_limit: default_rate_limit(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            relayer: RelayerConfig::default(),
        }
    }
}

// ── Request / Response types ────────────────────────────────────────────

#[derive(Deserialize)]
struct SponsorRequest {
    /// Base58-encoded partially-signed transaction.
    transaction: String,
}

/// Unused directly (response is built via serde_json::json!) but kept for documentation.
#[allow(dead_code)]
#[derive(Serialize)]
struct SponsorResponse {
    /// Transaction signature (base58).
    signature: String,
}

#[derive(Serialize)]
struct InfoResponse {
    /// Relayer fee-payer public key (base58).
    pubkey: String,
}

// ── App state ───────────────────────────────────────────────────────────

struct AppState {
    keypair: Keypair,
    rpc_client: RpcClient,
}

// ── Handlers ────────────────────────────────────────────────────────────

async fn get_info(State(state): State<Arc<AppState>>) -> Json<InfoResponse> {
    Json(InfoResponse {
        pubkey: state.keypair.pubkey().to_string(),
    })
}

async fn post_sponsor(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SponsorRequest>,
) -> impl IntoResponse {
    // Decode the base58 partially-signed transaction
    let tx_bytes = match bs58::decode(&req.transaction).into_vec() {
        Ok(bytes) => bytes,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("Invalid base58: {e}") })),
            );
        }
    };

    let mut tx: Transaction = match bincode::deserialize(&tx_bytes) {
        Ok(tx) => tx,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("Invalid transaction: {e}") })),
            );
        }
    };

    // Verify fee_payer matches relayer pubkey
    let fee_payer = match tx.message.account_keys.first() {
        Some(pk) => pk,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "Transaction has no account keys" })),
            );
        }
    };

    if *fee_payer != state.keypair.pubkey() {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "error": format!(
                    "Fee payer mismatch: expected {}, got {}",
                    state.keypair.pubkey(),
                    fee_payer
                )
            })),
        );
    }

    // Sign with relayer keypair as fee payer
    let recent_blockhash = match state.rpc_client.get_latest_blockhash() {
        Ok(bh) => bh,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Failed to get blockhash: {e}") })),
            );
        }
    };

    tx.partial_sign(&[&state.keypair], recent_blockhash);

    // Broadcast
    let sig = match state.rpc_client.send_and_confirm_transaction(&tx) {
        Ok(sig) => sig,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Transaction failed: {e}") })),
            );
        }
    };

    tracing::info!("Sponsored transaction: {}", sig);

    (
        StatusCode::OK,
        Json(serde_json::json!({ "signature": sig.to_string() })),
    )
}

// ── Main ────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    // Load config
    let config: Config = match std::fs::read_to_string("config.toml") {
        Ok(content) => toml::from_str(&content).unwrap_or_default(),
        Err(_) => {
            tracing::warn!("No config.toml found, using defaults");
            Config::default()
        }
    };

    // Load or generate keypair
    let keypair = if config.relayer.keypair_b58.is_empty() {
        let kp = Keypair::new();
        tracing::warn!(
            "Generated new relayer keypair. Pubkey: {}",
            kp.pubkey()
        );
        tracing::warn!(
            "To persist, add to config.toml: keypair_b58 = \"{}\"",
            bs58::encode(kp.to_bytes()).into_string()
        );
        kp
    } else {
        let bytes = bs58::decode(&config.relayer.keypair_b58).into_vec()?;
        Keypair::try_from(bytes.as_slice())?
    };

    tracing::info!("Relayer fee payer: {}", keypair.pubkey());

    let state = Arc::new(AppState {
        keypair,
        rpc_client: RpcClient::new(&config.relayer.rpc_url),
    });

    let app = axum::Router::new()
        .route("/info", get(get_info))
        .route("/sponsor", post(post_sponsor))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&config.relayer.listen_addr).await?;
    tracing::info!("Relayer listening on {}", config.relayer.listen_addr);
    axum::serve(listener, app).await?;

    Ok(())
}
