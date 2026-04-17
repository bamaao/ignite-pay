use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;

use crate::state::RegistryState;

/// Query parameters for fee listing.
#[derive(Debug, Deserialize)]
pub struct ListFeesQuery {
    /// Filter by operation type (e.g., "register", "update_vc", "rotate_key").
    #[serde(default)]
    pub operation: Option<String>,
    /// Only return fees recorded after this Unix timestamp (milliseconds).
    #[serde(default)]
    pub since: Option<i64>,
    /// Maximum number of records to return. Defaults to 100.
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    100
}

/// `GET /v1/fees` — List fee records.
pub async fn list_fees(
    State(state): State<RegistryState>,
    axum::extract::Query(query): axum::extract::Query<ListFeesQuery>,
) -> impl IntoResponse {
    let prefix = match &query.operation {
        Some(op) => format!("fee:{}:", op),
        None => "fee:".to_string(),
    };

    let store = crate::storage::sled_store::MerchantStore::new((*state.db).clone());
    let mut fees = store.list_fees(&prefix, query.limit);

    // Filter by `since` if provided
    if let Some(since) = query.since {
        fees.retain(|f| {
            f.get("timestamp")
                .and_then(|t| t.as_i64())
                .map_or(false, |ts| ts >= since)
        });
    }

    (StatusCode::OK, axum::Json(serde_json::json!({
        "fees": fees,
    })))
}
