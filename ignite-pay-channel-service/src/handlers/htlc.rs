use axum::{
    extract::{Path, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use ed25519_dalek::Signer;
use ignite_pay_state_channel::signing::sign_leaf_update;
use ignite_pay_state_channel::types::UTXOLeaf;
use solana_sdk::pubkey::Pubkey;

use crate::error::ChannelServiceError;
use crate::state::AppState;

fn decode_channel_id(hex_str: &str) -> Result<[u8; 32], ChannelServiceError> {
    let bytes = hex::decode(hex_str)
        .map_err(|_| ChannelServiceError::BadRequest("invalid channel_id hex".into()))?;
    bytes.try_into()
        .map_err(|_| ChannelServiceError::BadRequest("channel_id must be 32 bytes".into()))
}

fn parse_pubkey(s: &str) -> Result<Pubkey, ChannelServiceError> {
    s.parse()
        .map_err(|_| ChannelServiceError::BadRequest(format!("invalid pubkey: {}", s)))
}

#[derive(Debug, Deserialize)]
pub struct CreateHtlcRequest {
    pub amount: u64,
    pub leaf_index: Option<u32>,
    pub beneficiary: String,
    pub duration_slots: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct ResolveHtlcRequest {
    pub leaf_index: u32,
    pub preimage: String,
}

#[derive(Debug, Deserialize)]
pub struct RefundHtlcRequest {
    pub leaf_index: u32,
}

pub async fn create_htlc(
    State(state): State<AppState>,
    Path(channel_id_hex): Path<String>,
    Json(req): Json<CreateHtlcRequest>,
) -> Result<Json<Value>, ChannelServiceError> {
    let channel_id = decode_channel_id(&channel_id_hex)?;
    let beneficiary = parse_pubkey(&req.beneficiary)?;

    let current_slot = state.rpc_client.get_slot()
        .map_err(|e| ChannelServiceError::SolanaRpc(e.to_string()))?;

    let ed_kp = state.ed_keypair();
    let our_pubkey = Pubkey::new_from_array(ed_kp.public.to_bytes());

    let mgr = state.channel_manager.lock().await;
    let mut channel_state = mgr.load_state(&channel_id)
        .map_err(ChannelServiceError::StateChannel)?;

    let leaf_index = req.leaf_index.map(|i| i as usize).unwrap_or_else(|| {
        *channel_state.tree.available_slots().first()
            .expect("no available slots")
    });

    let prev_leaf = channel_state.tree.get_leaf(leaf_index)
        .ok_or_else(|| ChannelServiceError::BadRequest("leaf_index out of bounds".into()))?
        .clone();

    let duration = req.duration_slots.unwrap_or(state.config.channel.default_challenge_duration);
    let timelock_slot = current_slot + duration;

    // Generate HTLC: preimage + hash_lock
    let mut htlc_mgr = ignite_pay_state_channel::htlc::HtlcManager::new();
    let (hash_lock, preimage) = htlc_mgr.create_htlc(
        req.amount,
        leaf_index,
        our_pubkey,
        beneficiary,
        current_slot,
        duration,
    );

    let new_leaf = UTXOLeaf::htlc(
        our_pubkey,
        req.amount,
        hash_lock,
        timelock_slot,
        beneficiary,
    );

    let new_sequence = channel_state.metadata.sequence + 1;
    let update = sign_leaf_update(
        &channel_id,
        new_sequence,
        leaf_index as u32,
        &prev_leaf,
        new_leaf,
        &ed_kp,
    );

    mgr.apply_leaf_update(&mut channel_state, &update, &our_pubkey)
        .map_err(ChannelServiceError::StateChannel)?;

    Ok(Json(json!({
        "channel_id": channel_id_hex,
        "leaf_index": leaf_index,
        "hash_lock": hex::encode(hash_lock),
        "preimage": hex::encode(preimage),
        "timelock_slot": timelock_slot,
        "amount": req.amount,
        "sequence": channel_state.metadata.sequence,
    })))
}

pub async fn resolve_htlc(
    State(state): State<AppState>,
    Path(channel_id_hex): Path<String>,
    Json(req): Json<ResolveHtlcRequest>,
) -> Result<Json<Value>, ChannelServiceError> {
    let channel_id = decode_channel_id(&channel_id_hex)?;
    let preimage: [u8; 32] = hex::decode(&req.preimage)
        .map_err(|_| ChannelServiceError::BadRequest("invalid preimage hex".into()))?
        .try_into()
        .map_err(|_| ChannelServiceError::BadRequest("preimage must be 32 bytes".into()))?;

    let current_slot = state.rpc_client.get_slot()
        .map_err(|e| ChannelServiceError::SolanaRpc(e.to_string()))?;

    let ed_kp = state.ed_keypair();
    let our_pubkey = Pubkey::new_from_array(ed_kp.public.to_bytes());

    // Sign the claim message for HTLC verify
    let mut htlc_msg = Vec::with_capacity(32 + 8 + 32);
    htlc_msg.extend_from_slice(&channel_id);
    htlc_msg.extend_from_slice(&current_slot.to_le_bytes());

    let mgr = state.channel_manager.lock().await;
    let mut channel_state = mgr.load_state(&channel_id)
        .map_err(ChannelServiceError::StateChannel)?;

    htlc_msg.extend_from_slice(&channel_state.metadata.current_root);
    let claimer_sig: ed25519_dalek::Signature = ed_kp.sign(&htlc_msg);

    mgr.claim_htlc_verify(
        &mut channel_state,
        req.leaf_index,
        &preimage,
        &our_pubkey,
        current_slot,
        &claimer_sig.to_bytes(),
    ).map_err(ChannelServiceError::StateChannel)?;

    Ok(Json(json!({
        "channel_id": channel_id_hex,
        "leaf_index": req.leaf_index,
        "resolved": true,
        "sequence": channel_state.metadata.sequence,
    })))
}

pub async fn refund_htlc(
    State(state): State<AppState>,
    Path(channel_id_hex): Path<String>,
    Json(req): Json<RefundHtlcRequest>,
) -> Result<Json<Value>, ChannelServiceError> {
    let channel_id = decode_channel_id(&channel_id_hex)?;

    let current_slot = state.rpc_client.get_slot()
        .map_err(|e| ChannelServiceError::SolanaRpc(e.to_string()))?;

    let ed_kp = state.ed_keypair();
    let our_pubkey = Pubkey::new_from_array(ed_kp.public.to_bytes());

    // Sign the refund message
    let mut refund_msg = Vec::with_capacity(32 + 8 + 32);
    refund_msg.extend_from_slice(&channel_id);
    refund_msg.extend_from_slice(&current_slot.to_le_bytes());

    let mgr = state.channel_manager.lock().await;
    let mut channel_state = mgr.load_state(&channel_id)
        .map_err(ChannelServiceError::StateChannel)?;

    refund_msg.extend_from_slice(&channel_state.metadata.current_root);
    let claimer_sig: ed25519_dalek::Signature = ed_kp.sign(&refund_msg);

    mgr.claim_htlc_refund(
        &mut channel_state,
        req.leaf_index,
        &our_pubkey,
        current_slot,
        &claimer_sig.to_bytes(),
    ).map_err(ChannelServiceError::StateChannel)?;

    Ok(Json(json!({
        "channel_id": channel_id_hex,
        "leaf_index": req.leaf_index,
        "refunded": true,
        "sequence": channel_state.metadata.sequence,
    })))
}
