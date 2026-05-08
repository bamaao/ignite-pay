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

use axum::{
    extract::State,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use ignite_pay_state_channel::hub::{HubLeaf, HubMetrics};
use ignite_pay_state_channel::routing::RouteRequest;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::hash::hash;

use crate::error::ChannelServiceError;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct RegisterHubRequest {
    pub active_pubkey: String,
    pub endpoint: String,
    pub collateral: u64,
    pub platform_vc_hash: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateMetricsRequest {
    pub hub_did_hash: String,
    pub online_rate: u16,
    pub success_rate: u16,
    pub avg_latency_ms: u32,
    pub total_routed: u64,
    pub total_transactions: u64,
    pub active_channels: u32,
    pub available_liquidity: u64,
    pub fee_rate_bps: u16,
}

#[derive(Debug, Deserialize)]
pub struct FindRoutesRequest {
    pub from_did_hash: String,
    pub to_did_hash: String,
    pub amount: u64,
    pub token_mint: String,
    pub max_hops: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct AddEdgeRequest {
    pub from_did_hash: String,
    pub to_did_hash: String,
}

fn decode_32bytes(hex_str: &str) -> Result<[u8; 32], ChannelServiceError> {
    let bytes = hex::decode(hex_str)
        .map_err(|_| ChannelServiceError::BadRequest("invalid hex".into()))?;
    bytes.try_into()
        .map_err(|_| ChannelServiceError::BadRequest("must be 32 bytes".into()))
}

pub async fn register_hub(
    State(state): State<AppState>,
    Json(req): Json<RegisterHubRequest>,
) -> Result<Json<Value>, ChannelServiceError> {
    let hub_manager = state.hub_manager.as_ref()
        .ok_or_else(|| ChannelServiceError::BadRequest("hub routes only available on Hub role".into()))?;

    let active_pubkey: Pubkey = req.active_pubkey.parse()
        .map_err(|_| ChannelServiceError::BadRequest("invalid active_pubkey".into()))?;
    let endpoint_hash = hash(req.endpoint.as_bytes()).to_bytes();
    let platform_vc_hash = decode_32bytes(&req.platform_vc_hash)?;

    // Derive hub_did_hash from active_pubkey
    let hub_did_hash = hash(active_pubkey.as_ref()).to_bytes();

    let current_slot = state.rpc_client.get_slot()
        .map_err(|e| ChannelServiceError::SolanaRpc(e.to_string()))?;

    let hub = HubLeaf {
        hub_did_hash,
        active_pubkey,
        endpoint_hash,
        collateral: req.collateral,
        platform_vc_hash,
        metrics_hash: [0u8; 32],
        slot_updated: current_slot,
    };

    let mgr = hub_manager.lock().await;
    mgr.register_hub(hub)
        .map_err(ChannelServiceError::StateChannel)?;

    Ok(Json(json!({
        "hub_did_hash": hex::encode(hub_did_hash),
        "registered": true,
    })))
}

pub async fn hub_info(
    State(state): State<AppState>,
) -> Result<Json<Value>, ChannelServiceError> {
    let hub_manager = state.hub_manager.as_ref()
        .ok_or_else(|| ChannelServiceError::BadRequest("hub routes only available on Hub role".into()))?;

    let our_pubkey = state.pubkey();
    let hub_did_hash = hash(our_pubkey.as_ref()).to_bytes();

    let mgr = hub_manager.lock().await;
    match mgr.get_hub(hub_did_hash)
        .map_err(ChannelServiceError::StateChannel)?
    {
        Some(hub) => Ok(Json(json!({
            "hub_did_hash": hex::encode(hub.hub_did_hash),
            "active_pubkey": hub.active_pubkey.to_string(),
            "collateral": hub.collateral,
            "slot_updated": hub.slot_updated,
        }))),
        None => Ok(Json(json!({ "registered": false }))),
    }
}

pub async fn update_metrics(
    State(state): State<AppState>,
    Json(req): Json<UpdateMetricsRequest>,
) -> Result<Json<Value>, ChannelServiceError> {
    let hub_manager = state.hub_manager.as_ref()
        .ok_or_else(|| ChannelServiceError::BadRequest("hub routes only available on Hub role".into()))?;

    let hub_did_hash = decode_32bytes(&req.hub_did_hash)?;

    let metrics = HubMetrics {
        online_rate: req.online_rate,
        success_rate: req.success_rate,
        avg_latency_ms: req.avg_latency_ms,
        total_routed: req.total_routed,
        total_transactions: req.total_transactions,
        active_channels: req.active_channels,
        available_liquidity: req.available_liquidity,
        fee_rate_bps: req.fee_rate_bps,
    };

    let metrics_hash = ignite_pay_state_channel::hub::HubManager::compute_metrics_hash(&metrics);

    let mgr = hub_manager.lock().await;
    mgr.update_metrics(hub_did_hash, metrics)
        .map_err(ChannelServiceError::StateChannel)?;

    Ok(Json(json!({
        "hub_did_hash": hex::encode(hub_did_hash),
        "metrics_hash": hex::encode(metrics_hash),
        "updated": true,
    })))
}

pub async fn list_hubs(
    State(state): State<AppState>,
) -> Result<Json<Value>, ChannelServiceError> {
    let hub_manager = state.hub_manager.as_ref()
        .ok_or_else(|| ChannelServiceError::BadRequest("hub routes only available on Hub role".into()))?;

    let mgr = hub_manager.lock().await;
    let hubs = mgr.list_hubs()
        .map_err(ChannelServiceError::StateChannel)?;

    Ok(Json(json!({
        "hubs": hubs.iter().map(|h| hex::encode(h)).collect::<Vec<_>>(),
    })))
}

pub async fn find_routes(
    State(_state): State<AppState>,
) -> Result<Json<Value>, ChannelServiceError> {
    // User role: uses a cached/default route service
    Ok(Json(json!({ "routes": [], "message": "specify parameters via hub endpoint" })))
}

pub async fn find_routes_hub(
    State(state): State<AppState>,
    Json(req): Json<FindRoutesRequest>,
) -> Result<Json<Value>, ChannelServiceError> {
    let route_service = state.route_service.as_ref()
        .ok_or_else(|| ChannelServiceError::BadRequest("route discovery only available on Hub role".into()))?;

    let from_did_hash = decode_32bytes(&req.from_did_hash)?;
    let to_did_hash = decode_32bytes(&req.to_did_hash)?;
    let token_mint: Pubkey = req.token_mint.parse()
        .map_err(|_| ChannelServiceError::BadRequest("invalid token_mint".into()))?;

    let route_req = RouteRequest {
        from_did_hash,
        to_did_hash,
        amount: req.amount,
        token_mint,
        max_hops: req.max_hops.unwrap_or(3),
    };

    let svc = route_service.lock().await;
    let routes = svc.discover_routes(&route_req)
        .map_err(ChannelServiceError::StateChannel)?;

    let routes_json: Vec<Value> = routes.iter().map(|r| {
        json!({
            "hops": r.hops.iter().map(|h| json!({
                "hub_pubkey": h.hub_pubkey.to_string(),
                "hub_did_hash": hex::encode(h.hub_did_hash),
                "fee": h.fee,
                "latency_ms": h.latency_ms,
                "liquidity": h.liquidity,
            })).collect::<Vec<_>>(),
            "total_fee": r.total_fee,
            "max_latency_ms": r.max_latency_ms,
            "score": r.score,
            "sufficient_liquidity": r.sufficient_liquidity,
        })
    }).collect();

    Ok(Json(json!({ "routes": routes_json })))
}

pub async fn add_edge(
    State(state): State<AppState>,
    Json(req): Json<AddEdgeRequest>,
) -> Result<Json<Value>, ChannelServiceError> {
    let route_service = state.route_service.as_ref()
        .ok_or_else(|| ChannelServiceError::BadRequest("route management only available on Hub role".into()))?;

    let from = decode_32bytes(&req.from_did_hash)?;
    let to = decode_32bytes(&req.to_did_hash)?;

    let mut svc = route_service.lock().await;
    svc.add_channel_edge(from, to);

    Ok(Json(json!({ "added": true })))
}

pub async fn refresh_graph(
    State(state): State<AppState>,
) -> Result<Json<Value>, ChannelServiceError> {
    let route_service = state.route_service.as_ref()
        .ok_or_else(|| ChannelServiceError::BadRequest("route management only available on Hub role".into()))?;

    let mut svc = route_service.lock().await;
    svc.refresh_graph()
        .map_err(ChannelServiceError::StateChannel)?;

    Ok(Json(json!({ "refreshed": true })))
}
