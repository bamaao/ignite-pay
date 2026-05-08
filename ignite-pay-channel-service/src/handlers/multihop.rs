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
    extract::{Path, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use solana_sdk::pubkey::Pubkey;

use crate::error::ChannelServiceError;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct CreatePaymentRequest {
    pub hash_lock: String,
    pub preimage: String,
    pub hops: Vec<HopMetadata>,
    pub challenge_duration: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct HopMetadata {
    pub owner: String,
    pub beneficiary: String,
    pub amount: u64,
    pub leaf_index: usize,
    pub channel_id: String,
}

#[derive(Debug, Deserialize)]
pub struct ResolveHopRequest {
    // no additional fields needed
}

#[derive(Debug, Deserialize)]
pub struct RelayHopRequest {
    pub payment_id: String,
    pub hop_index: usize,
    pub preimage: Option<String>,
}

fn decode_channel_id(hex_str: &str) -> Result<[u8; 32], ChannelServiceError> {
    let bytes = hex::decode(hex_str)
        .map_err(|_| ChannelServiceError::BadRequest("invalid channel_id hex".into()))?;
    bytes.try_into()
        .map_err(|_| ChannelServiceError::BadRequest("channel_id must be 32 bytes".into()))
}

pub async fn create_payment(
    State(state): State<AppState>,
    Json(req): Json<CreatePaymentRequest>,
) -> Result<Json<Value>, ChannelServiceError> {
    let multihop_mgr = state.multihop_manager.as_ref()
        .ok_or_else(|| ChannelServiceError::BadRequest("multi-hop only available on User role".into()))?;

    let hash_lock: [u8; 32] = hex::decode(&req.hash_lock)
        .map_err(|_| ChannelServiceError::BadRequest("invalid hash_lock hex".into()))?
        .try_into()
        .map_err(|_| ChannelServiceError::BadRequest("hash_lock must be 32 bytes".into()))?;
    let preimage: [u8; 32] = hex::decode(&req.preimage)
        .map_err(|_| ChannelServiceError::BadRequest("invalid preimage hex".into()))?
        .try_into()
        .map_err(|_| ChannelServiceError::BadRequest("preimage must be 32 bytes".into()))?;

    let hops_metadata: Result<Vec<_>, ChannelServiceError> = req.hops.iter().map(|h| {
        let owner: Pubkey = h.owner.parse()
            .map_err(|_| ChannelServiceError::BadRequest("invalid owner".into()))?;
        let beneficiary: Pubkey = h.beneficiary.parse()
            .map_err(|_| ChannelServiceError::BadRequest("invalid beneficiary".into()))?;
        let channel_id = decode_channel_id(&h.channel_id)?;
        Ok((owner, beneficiary, h.amount, h.leaf_index, channel_id))
    }).collect();
    let hops_metadata = hops_metadata?;

    let current_slot = state.rpc_client.get_slot()
        .map_err(|e| ChannelServiceError::SolanaRpc(e.to_string()))?;
    let challenge_duration = req.challenge_duration.unwrap_or(state.config.channel.default_challenge_duration);

    let mgr = multihop_mgr.lock().await;
    let payment = mgr.create_payment(
        hash_lock,
        preimage,
        hops_metadata,
        current_slot,
        challenge_duration,
    ).map_err(ChannelServiceError::StateChannel)?;

    Ok(Json(json!({
        "payment_id": hex::encode(payment.payment_id),
        "hash_lock": hex::encode(payment.hash_lock),
        "status": format!("{:?}", payment.status),
        "hop_count": payment.hops.len(),
        "created_slot": payment.created_slot,
        "hops": payment.hops.iter().enumerate().map(|(i, h)| json!({
            "index": i,
            "channel_id": hex::encode(h.channel_id),
            "amount": h.amount,
            "timelock_slot": h.timelock_slot,
            "resolved": h.resolved,
        })).collect::<Vec<_>>(),
    })))
}

pub async fn resolve_hop(
    State(state): State<AppState>,
    Path(payment_id_hex): Path<String>,
) -> Result<Json<Value>, ChannelServiceError> {
    let multihop_mgr = state.multihop_manager.as_ref()
        .ok_or_else(|| ChannelServiceError::BadRequest("multi-hop only available on User/Hub role".into()))?;

    let payment_id: [u8; 32] = hex::decode(&payment_id_hex)
        .map_err(|_| ChannelServiceError::BadRequest("invalid payment_id hex".into()))?
        .try_into()
        .map_err(|_| ChannelServiceError::BadRequest("payment_id must be 32 bytes".into()))?;

    let payment = {
        let mgr = multihop_mgr.lock().await;
        mgr.load_payment(&payment_id)
            .map_err(ChannelServiceError::StateChannel)?
    };

    // Find the first unresolved hop
    let hop_index = payment.hops.iter().position(|h| !h.resolved)
        .ok_or_else(|| ChannelServiceError::BadRequest("all hops already resolved".into()))?;

    let mgr = multihop_mgr.lock().await;
    let updated = mgr.resolve_hop(&payment_id, hop_index)
        .map_err(ChannelServiceError::StateChannel)?;

    Ok(Json(json!({
        "payment_id": payment_id_hex,
        "resolved_hop": hop_index,
        "status": format!("{:?}", updated.status),
        "resolved_hops": updated.hops.iter().filter(|h| h.resolved).count(),
        "total_hops": updated.hops.len(),
    })))
}

pub async fn relay_hop(
    State(state): State<AppState>,
    Json(req): Json<RelayHopRequest>,
) -> Result<Json<Value>, ChannelServiceError> {
    let multihop_mgr = state.multihop_manager.as_ref()
        .ok_or_else(|| ChannelServiceError::BadRequest("multi-hop relay only available on Hub role".into()))?;

    let payment_id: [u8; 32] = hex::decode(&req.payment_id)
        .map_err(|_| ChannelServiceError::BadRequest("invalid payment_id hex".into()))?
        .try_into()
        .map_err(|_| ChannelServiceError::BadRequest("payment_id must be 32 bytes".into()))?;

    // If preimage provided, reveal it
    if let Some(preimage_hex) = req.preimage {
        let preimage: [u8; 32] = hex::decode(&preimage_hex)
            .map_err(|_| ChannelServiceError::BadRequest("invalid preimage hex".into()))?
            .try_into()
            .map_err(|_| ChannelServiceError::BadRequest("preimage must be 32 bytes".into()))?;

        let mgr = multihop_mgr.lock().await;
        let updated = mgr.reveal_preimage(&payment_id, &preimage)
            .map_err(ChannelServiceError::StateChannel)?;

        return Ok(Json(json!({
            "payment_id": hex::encode(updated.payment_id),
            "status": format!("{:?}", updated.status),
            "action": "preimage_revealed",
        })));
    }

    // Otherwise, resolve the specified hop
    let mgr = multihop_mgr.lock().await;
    let updated = mgr.resolve_hop(&payment_id, req.hop_index)
        .map_err(ChannelServiceError::StateChannel)?;

    Ok(Json(json!({
        "payment_id": hex::encode(updated.payment_id),
        "resolved_hop": req.hop_index,
        "status": format!("{:?}", updated.status),
    })))
}

pub async fn get_payment(
    State(state): State<AppState>,
    Path(payment_id_hex): Path<String>,
) -> Result<Json<Value>, ChannelServiceError> {
    let multihop_mgr = state.multihop_manager.as_ref()
        .ok_or_else(|| ChannelServiceError::BadRequest("multi-hop only available on Hub role".into()))?;

    let payment_id: [u8; 32] = hex::decode(&payment_id_hex)
        .map_err(|_| ChannelServiceError::BadRequest("invalid payment_id hex".into()))?
        .try_into()
        .map_err(|_| ChannelServiceError::BadRequest("payment_id must be 32 bytes".into()))?;

    let mgr = multihop_mgr.lock().await;
    let payment = mgr.load_payment(&payment_id)
        .map_err(ChannelServiceError::StateChannel)?;

    Ok(Json(json!({
        "payment_id": hex::encode(payment.payment_id),
        "hash_lock": hex::encode(payment.hash_lock),
        "status": format!("{:?}", payment.status),
        "created_slot": payment.created_slot,
        "hops": payment.hops.iter().enumerate().map(|(i, h)| json!({
            "index": i,
            "channel_id": hex::encode(h.channel_id),
            "amount": h.amount,
            "timelock_slot": h.timelock_slot,
            "resolved": h.resolved,
        })).collect::<Vec<_>>(),
    })))
}
