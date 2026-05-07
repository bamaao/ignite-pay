use axum::{
    extract::{Path, State},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use ignite_pay_solana::channel::{build_fund_channel_ix, build_open_channel_ix, build_open_channel_ed25519_ix, derive_channel_pda};
use solana_sdk::pubkey::Pubkey;

use crate::error::ChannelServiceError;
use crate::state::AppState;

// ── Request types ──

#[derive(Debug, Deserialize)]
pub struct OpenChannelRequest {
    pub user_pubkey: String,
    pub provider_pubkey: String,
    pub token_mint: String,
    pub deposit_amount: u64,
    #[serde(default)]
    pub tree_depth: Option<u32>,
    #[serde(default)]
    pub open_slot: Option<u64>,
    pub vault_a: String,
    pub vault_b: String,
    #[serde(default)]
    pub challenge_duration: Option<u64>,
    #[serde(default)]
    pub min_challenge_delay: Option<u64>,
    #[serde(default)]
    pub auto_close_offset: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct ChannelResponse {
    pub channel_id: String,
    pub sequence: u64,
    pub status: String,
    pub current_root: String,
    pub total_deposited: u64,
    pub deposit_a: u64,
    pub deposit_b: u64,
}

#[derive(Debug, Deserialize)]
pub struct FundChannelRequest {
    pub source_vault: String,
    pub deposit_b: u64,
}

// ── Handlers ──

pub async fn open_channel(
    State(state): State<AppState>,
    Json(req): Json<OpenChannelRequest>,
) -> Result<Json<Value>, ChannelServiceError> {
    let user_pubkey: Pubkey = req.user_pubkey.parse()
        .map_err(|_| ChannelServiceError::BadRequest("invalid user_pubkey".into()))?;
    let provider_pubkey: Pubkey = req.provider_pubkey.parse()
        .map_err(|_| ChannelServiceError::BadRequest("invalid provider_pubkey".into()))?;
    let token_mint: Pubkey = req.token_mint.parse()
        .map_err(|_| ChannelServiceError::BadRequest("invalid token_mint".into()))?;
    let vault_a: Pubkey = req.vault_a.parse()
        .map_err(|_| ChannelServiceError::BadRequest("invalid vault_a".into()))?;
    let vault_b: Pubkey = req.vault_b.parse()
        .map_err(|_| ChannelServiceError::BadRequest("invalid vault_b".into()))?;

    let tree_depth = req.tree_depth.unwrap_or(state.config.channel.default_tree_depth);
    let challenge_duration = req.challenge_duration.unwrap_or(state.config.channel.default_challenge_duration);
    let min_challenge_delay = req.min_challenge_delay.unwrap_or(state.config.channel.default_min_challenge_delay);

    let open_slot = req.open_slot.unwrap_or_else(|| {
        state.rpc_client.get_slot().unwrap_or(0)
    });

    let auto_close_slot = req.auto_close_offset.map(|offset| open_slot + offset);

    let mgr = state.channel_manager.lock().await;
    let channel_state = mgr.open_channel(
        &user_pubkey,
        &provider_pubkey,
        &token_mint,
        req.deposit_amount,
        tree_depth,
        open_slot,
        &vault_a,
        &vault_b,
        challenge_duration,
        min_challenge_delay,
        auto_close_slot,
    ).map_err(ChannelServiceError::StateChannel)?;

    let channel_id = channel_state.metadata.channel_id;

    // Initialize compliance if configured
    drop(mgr);
    state.init_compliance_for_channel(channel_id).await?;

    // Build on-chain instruction for the user to sign and send
    let (channel_pda, _) = derive_channel_pda(&channel_id, &state.program_id);
    let our_pubkey = state.pubkey();

    // Sign the on-chain message: channel_id || deposit_a || tree_depth || initial_root
    let ed_kp = state.ed_keypair();
    let mut on_chain_msg = Vec::with_capacity(32 + 8 + 4 + 32);
    on_chain_msg.extend_from_slice(&channel_id);
    on_chain_msg.extend_from_slice(&req.deposit_amount.to_le_bytes());
    on_chain_msg.extend_from_slice(&tree_depth.to_le_bytes());
    on_chain_msg.extend_from_slice(&channel_state.metadata.current_root);
    use ed25519_dalek::Signer;
    let sig_a: ed25519_dalek::Signature = ed_kp.sign(&on_chain_msg);
    let sig_a_bytes = sig_a.to_bytes();

    let ix = build_open_channel_ix(
        &state.program_id,
        &channel_pda,
        &our_pubkey,       // user (signer)
        &user_pubkey,      // user_pubkey (unchecked)
        &provider_pubkey,
        &token_mint,
        &vault_a,
        &vault_b,
        &our_pubkey,       // payer
        &channel_id,
        req.deposit_amount,
        tree_depth,
        open_slot,
        challenge_duration,
        min_challenge_delay,
        &channel_state.metadata.current_root,
    );

    // Build ed25519 verification instruction
    let ed25519_ix = build_open_channel_ed25519_ix(
        &our_pubkey,
        &on_chain_msg,
        &sig_a_bytes,
    );

    Ok(Json(json!({
        "channel_id": hex::encode(channel_id),
        "sequence": channel_state.metadata.sequence,
        "current_root": hex::encode(channel_state.metadata.current_root),
        "ed25519_instructions": [{
            "program_id": ed25519_ix.program_id.to_string(),
            "accounts": [],
            "data": bs58::encode(&ed25519_ix.data).into_string(),
        }],
        "on_chain_instruction": {
            "program_id": ix.program_id.to_string(),
            "accounts": ix.accounts.iter().map(|a| {
                json!({
                    "pubkey": a.pubkey.to_string(),
                    "is_signer": a.is_signer,
                    "is_writable": a.is_writable,
                })
            }).collect::<Vec<_>>(),
            "data": bs58::encode(&ix.data).into_string(),
        },
    })))
}

pub async fn fund_channel(
    State(state): State<AppState>,
    Path(channel_id_hex): Path<String>,
    Json(req): Json<FundChannelRequest>,
) -> Result<Json<Value>, ChannelServiceError> {
    let channel_id_bytes = hex::decode(&channel_id_hex)
        .map_err(|_| ChannelServiceError::BadRequest("invalid channel_id hex".into()))?;
    let channel_id: [u8; 32] = channel_id_bytes.try_into()
        .map_err(|_| ChannelServiceError::BadRequest("channel_id must be 32 bytes".into()))?;

    let source_vault: Pubkey = req.source_vault.parse()
        .map_err(|_| ChannelServiceError::BadRequest("invalid source_vault".into()))?;

    let mgr = state.channel_manager.lock().await;
    let mut channel_state = mgr.load_state(&channel_id)
        .map_err(ChannelServiceError::StateChannel)?;

    let ed_kp = state.ed_keypair();
    let _update = mgr.fund_channel(&mut channel_state, &ed_kp, req.deposit_b, None)
        .map_err(ChannelServiceError::StateChannel)?;

    // Build on-chain instruction
    let (channel_pda, _) = derive_channel_pda(&channel_id, &state.program_id);
    let our_pubkey = state.pubkey();

    let ix = build_fund_channel_ix(
        &state.program_id,
        &channel_pda,
        &our_pubkey,
        &source_vault,
        &channel_state.metadata.vault_b,
        req.deposit_b,
    );

    Ok(Json(json!({
        "channel_id": channel_id_hex,
        "deposit_b": req.deposit_b,
        "total_deposited": channel_state.metadata.total_deposited,
        "on_chain_instruction": {
            "program_id": ix.program_id.to_string(),
            "data": bs58::encode(&ix.data).into_string(),
        },
    })))
}

pub async fn list_channels(
    State(state): State<AppState>,
) -> Result<Json<Value>, ChannelServiceError> {
    let ids = crate::storage::channel_store::list_channel_ids(&state.db)?;
    Ok(Json(json!({ "channels": ids })))
}

pub async fn get_channel(
    State(state): State<AppState>,
    Path(channel_id_hex): Path<String>,
) -> Result<Json<Value>, ChannelServiceError> {
    let channel_id_bytes = hex::decode(&channel_id_hex)
        .map_err(|_| ChannelServiceError::BadRequest("invalid channel_id hex".into()))?;
    let channel_id: [u8; 32] = channel_id_bytes.try_into()
        .map_err(|_| ChannelServiceError::BadRequest("channel_id must be 32 bytes".into()))?;

    let mgr = state.channel_manager.lock().await;
    let channel_state = mgr.load_state(&channel_id)
        .map_err(ChannelServiceError::StateChannel)?;

    let m = &channel_state.metadata;
    Ok(Json(json!({
        "channel_id": hex::encode(m.channel_id),
        "user_pubkey": m.user_pubkey.to_string(),
        "provider_pubkey": m.provider_pubkey.to_string(),
        "token_mint": m.token_mint.to_string(),
        "tree_depth": m.tree_depth,
        "status": format!("{:?}", m.status),
        "sequence": m.sequence,
        "current_root": hex::encode(m.current_root),
        "total_deposited": m.total_deposited,
        "deposit_a": m.deposit_a,
        "deposit_b": m.deposit_b,
        "open_slot": m.open_slot,
        "leaf_count": m.leaf_count,
    })))
}
