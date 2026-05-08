// Copyright (c) 2026 zouyc zouyccq@gmail.com.
// All rights reserved.
//
// Licensed under the Business Source License 1.1 (BSL 1.1).
// You may not use this file except in compliance with the License.
//
// Change Date: 2031-01-01
// On the Change Date, or the fourth anniversary of the first publicly available
// distribution of the code under the BSL, whichever comes first, the code
// automatically becomes available under the Apache License 2.0.

use axum::extract::{Path, Query, State};
use axum::Json;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::error::RegistryError;
use crate::models::{ListHubsQuery, RegisterHubRequest, UpdateHubRequest, UpdateMetricsRequest};
use crate::state::AppState;

pub async fn health() -> &'static str {
    "OK"
}

pub async fn register_hub(
    State(state): State<AppState>,
    Json(req): Json<RegisterHubRequest>,
) -> Result<Json<Value>, RegistryError> {
    if req.hub_did.is_empty() || req.endpoint_url.is_empty() || req.name.is_empty() {
        return Err(RegistryError::BadRequest(
            "hub_did, endpoint_url, and name are required".to_string(),
        ));
    }

    let hub = crate::repository::register_hub(&state.pool, &req).await?;
    tracing::info!("Registered hub: {} ({})", hub.name, hub.hub_id);

    Ok(Json(serde_json::to_value(&hub).unwrap_or_default()))
}

pub async fn list_hubs(
    State(state): State<AppState>,
    Query(query): Query<ListHubsQuery>,
) -> Result<Json<Value>, RegistryError> {
    let hubs = crate::repository::list_hubs(&state.pool, &query).await?;
    Ok(Json(json!({ "hubs": hubs })))
}

pub async fn get_hub(
    State(state): State<AppState>,
    Path(hub_id): Path<Uuid>,
) -> Result<Json<Value>, RegistryError> {
    let hub = crate::repository::get_hub(&state.pool, hub_id)
        .await?
        .ok_or_else(|| RegistryError::NotFound(format!("Hub {} not found", hub_id)))?;

    Ok(Json(serde_json::to_value(&hub).unwrap_or_default()))
}

pub async fn update_hub(
    State(state): State<AppState>,
    Path(hub_id): Path<Uuid>,
    Json(req): Json<UpdateHubRequest>,
) -> Result<Json<Value>, RegistryError> {
    let hub = crate::repository::update_hub(&state.pool, hub_id, &req).await?;
    tracing::info!("Updated hub: {}", hub_id);

    Ok(Json(serde_json::to_value(&hub).unwrap_or_default()))
}

pub async fn deregister_hub(
    State(state): State<AppState>,
    Path(hub_id): Path<Uuid>,
) -> Result<Json<Value>, RegistryError> {
    crate::repository::deregister_hub(&state.pool, hub_id).await?;
    tracing::info!("Deregistered hub: {}", hub_id);

    Ok(Json(json!({ "hub_id": hub_id.to_string(), "status": "inactive" })))
}

pub async fn get_hub_metrics(
    State(state): State<AppState>,
    Path(hub_id): Path<Uuid>,
) -> Result<Json<Value>, RegistryError> {
    let hub = crate::repository::get_hub(&state.pool, hub_id)
        .await?
        .ok_or_else(|| RegistryError::NotFound(format!("Hub {} not found", hub_id)))?;

    Ok(Json(json!({
        "hub_id": hub.hub_id.to_string(),
        "online_rate": hub.online_rate,
        "success_rate": hub.success_rate,
        "avg_latency_ms": hub.avg_latency_ms,
        "active_channels": hub.active_channels,
        "available_liquidity": hub.available_liquidity,
        "fee_rate_bps": hub.fee_rate_bps,
        "updated_at": hub.updated_at.to_rfc3339(),
    })))
}

pub async fn update_metrics(
    State(state): State<AppState>,
    Path(hub_id): Path<Uuid>,
    Json(req): Json<UpdateMetricsRequest>,
) -> Result<Json<Value>, RegistryError> {
    let hub = crate::repository::update_metrics(&state.pool, hub_id, &req).await?;
    tracing::info!("Updated metrics for hub: {}", hub_id);

    Ok(Json(json!({
        "hub_id": hub.hub_id.to_string(),
        "online_rate": hub.online_rate,
        "success_rate": hub.success_rate,
        "avg_latency_ms": hub.avg_latency_ms,
        "active_channels": hub.active_channels,
        "updated": true,
    })))
}
