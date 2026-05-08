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

use ignite_pay_state_channel::signing::sign_leaf_update;
use ignite_pay_state_channel::types::{LeafUpdate, UTXOLeaf};

use crate::error::ChannelServiceError;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct PayRequest {
    pub leaf_index: u32,
    pub new_owner: String,
    pub amount: u64,
}

#[derive(Debug, Deserialize)]
pub struct BatchRequest {
    pub updates: Vec<PayRequest>,
}

#[derive(Debug, Deserialize)]
pub struct SplitRequest {
    pub leaves: Vec<SplitLeaf>,
}

#[derive(Debug, Deserialize)]
pub struct SplitLeaf {
    pub owner: String,
    pub amount: u64,
    #[serde(default)]
    pub leaf_type: Option<String>,
    #[serde(default)]
    pub hash_lock: Option<String>,
    #[serde(default)]
    pub timelock_slot: Option<u64>,
    #[serde(default)]
    pub beneficiary: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AcceptPaymentRequest {
    pub update: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct AcceptBatchRequest {
    pub updates: Vec<serde_json::Value>,
}

fn decode_channel_id(hex_str: &str) -> Result<[u8; 32], ChannelServiceError> {
    let bytes = hex::decode(hex_str)
        .map_err(|_| ChannelServiceError::BadRequest("invalid channel_id hex".into()))?;
    bytes.try_into()
        .map_err(|_| ChannelServiceError::BadRequest("channel_id must be 32 bytes".into()))
}

fn parse_pubkey(s: &str) -> Result<solana_sdk::pubkey::Pubkey, ChannelServiceError> {
    s.parse()
        .map_err(|_| ChannelServiceError::BadRequest(format!("invalid pubkey: {}", s)))
}

pub async fn pay(
    State(state): State<AppState>,
    Path(channel_id_hex): Path<String>,
    Json(req): Json<PayRequest>,
) -> Result<Json<Value>, ChannelServiceError> {
    let channel_id = decode_channel_id(&channel_id_hex)?;
    let new_owner = parse_pubkey(&req.new_owner)?;

    let ed_kp = state.ed_keypair();
    let our_pubkey = solana_sdk::pubkey::Pubkey::new_from_array(ed_kp.public.to_bytes());

    let mgr = state.channel_manager.lock().await;
    let mut channel_state = mgr.load_state(&channel_id)
        .map_err(ChannelServiceError::StateChannel)?;

    let prev_leaf = channel_state.tree.get_leaf(req.leaf_index as usize)
        .ok_or_else(|| ChannelServiceError::BadRequest("leaf_index out of bounds".into()))?
        .clone();

    let new_leaf = UTXOLeaf::standard(new_owner, req.amount);
    let new_sequence = channel_state.metadata.sequence + 1;

    let update = sign_leaf_update(
        &channel_id,
        new_sequence,
        req.leaf_index,
        &prev_leaf,
        new_leaf,
        &ed_kp,
    );

    mgr.apply_leaf_update(&mut channel_state, &update, &our_pubkey)
        .map_err(ChannelServiceError::StateChannel)?;

    Ok(Json(json!({
        "channel_id": channel_id_hex,
        "sequence": channel_state.metadata.sequence,
        "leaf_index": req.leaf_index,
        "new_root": hex::encode(channel_state.metadata.current_root),
    })))
}

pub async fn batch_update(
    State(state): State<AppState>,
    Path(channel_id_hex): Path<String>,
    Json(req): Json<BatchRequest>,
) -> Result<Json<Value>, ChannelServiceError> {
    let channel_id = decode_channel_id(&channel_id_hex)?;

    let ed_kp = state.ed_keypair();
    let our_pubkey = solana_sdk::pubkey::Pubkey::new_from_array(ed_kp.public.to_bytes());

    let mgr = state.channel_manager.lock().await;
    let mut channel_state = mgr.load_state(&channel_id)
        .map_err(ChannelServiceError::StateChannel)?;

    let base_sequence = channel_state.metadata.sequence + 1;

    let mut updates = Vec::with_capacity(req.updates.len());
    for (i, pay_req) in req.updates.iter().enumerate() {
        let prev_leaf = channel_state.tree.get_leaf(pay_req.leaf_index as usize)
            .ok_or_else(|| ChannelServiceError::BadRequest(format!("leaf_index {} out of bounds", pay_req.leaf_index)))?
            .clone();

        let new_owner = parse_pubkey(&pay_req.new_owner)?;
        let new_leaf = UTXOLeaf::standard(new_owner, pay_req.amount);

        let update = sign_leaf_update(
            &channel_id,
            base_sequence + i as u64,
            pay_req.leaf_index,
            &prev_leaf,
            new_leaf,
            &ed_kp,
        );
        updates.push(update);
    }

    let applied_count = match mgr.apply_leaf_update_batch_with_info(&mut channel_state, &updates, &our_pubkey) {
        Ok(()) => updates.len(),
        Err(info) => {
            return Err(ChannelServiceError::StateChannel(info.error));
        }
    };

    Ok(Json(json!({
        "channel_id": channel_id_hex,
        "applied": applied_count,
        "new_sequence": channel_state.metadata.sequence,
        "new_root": hex::encode(channel_state.metadata.current_root),
    })))
}

pub async fn split(
    State(state): State<AppState>,
    Path(channel_id_hex): Path<String>,
    Json(req): Json<SplitRequest>,
) -> Result<Json<Value>, ChannelServiceError> {
    let channel_id = decode_channel_id(&channel_id_hex)?;

    let leaves: Vec<UTXOLeaf> = req.leaves.iter().map(|l| {
        let owner = parse_pubkey(&l.owner)?;
        match l.leaf_type.as_deref() {
            Some("htlc") => {
                let hash_lock = l.hash_lock.as_ref()
                    .map(|h| hex::decode(h))
                    .transpose()
                    .map_err(|_| ChannelServiceError::BadRequest("invalid hash_lock".into()))?
                    .map(|b| <[u8; 32]>::try_from(b.as_slice()))
                    .transpose()
                    .map_err(|_| ChannelServiceError::BadRequest("hash_lock must be 32 bytes".into()))?;
                let beneficiary = l.beneficiary.as_ref()
                    .map(|b| parse_pubkey(b))
                    .transpose()?;
                Ok(UTXOLeaf::htlc(
                    owner,
                    l.amount,
                    hash_lock.unwrap_or([0u8; 32]),
                    l.timelock_slot.unwrap_or(0),
                    beneficiary.unwrap_or_default(),
                ))
            }
            _ => Ok(UTXOLeaf::standard(owner, l.amount)),
        }
    }).collect::<Result<Vec<_>, ChannelServiceError>>()?;

    let ed_kp = state.ed_keypair();

    let mgr = state.channel_manager.lock().await;
    let mut channel_state = mgr.load_state(&channel_id)
        .map_err(ChannelServiceError::StateChannel)?;

    let signed_state = mgr.construct_split_tree(
        &mut channel_state,
        leaves,
        &ed_kp,
        &ed_kp,
    ).map_err(ChannelServiceError::StateChannel)?;

    let tree = &channel_state.tree;
    let leaves_json: Vec<Value> = tree.leaves().iter().enumerate().map(|(i, leaf)| {
        json!({
            "index": i,
            "owner": leaf.owner.to_string(),
            "amount": leaf.amount,
            "leaf_type": format!("{:?}", leaf.leaf_type),
        })
    }).collect();

    Ok(Json(json!({
        "channel_id": channel_id_hex,
        "new_root": hex::encode(signed_state.root),
        "sequence": signed_state.sequence,
        "leaves": leaves_json,
    })))
}

pub async fn accept_payment(
    State(state): State<AppState>,
    Path(channel_id_hex): Path<String>,
    Json(req): Json<AcceptPaymentRequest>,
) -> Result<Json<Value>, ChannelServiceError> {
    let channel_id = decode_channel_id(&channel_id_hex)?;

    let update: LeafUpdate = serde_json::from_value(req.update)
        .map_err(|e| ChannelServiceError::BadRequest(format!("invalid leaf update: {}", e)))?;

    if update.channel_id != channel_id {
        return Err(ChannelServiceError::BadRequest("channel_id mismatch".into()));
    }

    let our_pubkey = state.pubkey();

    let mgr = state.channel_manager.lock().await;
    let mut channel_state = mgr.load_state(&channel_id)
        .map_err(ChannelServiceError::StateChannel)?;

    mgr.apply_leaf_update(&mut channel_state, &update, &our_pubkey)
        .map_err(ChannelServiceError::StateChannel)?;

    Ok(Json(json!({
        "channel_id": channel_id_hex,
        "sequence": channel_state.metadata.sequence,
        "new_root": hex::encode(channel_state.metadata.current_root),
    })))
}

pub async fn accept_batch(
    State(state): State<AppState>,
    Path(channel_id_hex): Path<String>,
    Json(req): Json<AcceptBatchRequest>,
) -> Result<Json<Value>, ChannelServiceError> {
    let channel_id = decode_channel_id(&channel_id_hex)?;

    let updates: Vec<LeafUpdate> = req.updates.iter()
        .map(|v| serde_json::from_value(v.clone()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| ChannelServiceError::BadRequest(format!("invalid batch updates: {}", e)))?;

    let our_pubkey = state.pubkey();

    let mgr = state.channel_manager.lock().await;
    let mut channel_state = mgr.load_state(&channel_id)
        .map_err(ChannelServiceError::StateChannel)?;

    let applied_count = match mgr.apply_leaf_update_batch_with_info(&mut channel_state, &updates, &our_pubkey) {
        Ok(()) => updates.len(),
        Err(info) => {
            return Err(ChannelServiceError::StateChannel(info.error));
        }
    };

    Ok(Json(json!({
        "channel_id": channel_id_hex,
        "applied": applied_count,
        "new_sequence": channel_state.metadata.sequence,
        "new_root": hex::encode(channel_state.metadata.current_root),
    })))
}
