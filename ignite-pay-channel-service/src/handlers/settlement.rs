use axum::{
    extract::{Path, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use ed25519_dalek::Signer;
use ignite_pay_solana::channel::{
    build_cooperative_settle_ix, build_cooperative_settle_ed25519_ixs,
    build_trigger_challenge_ix, build_trigger_challenge_ed25519_ix,
    build_settle_after_timeout_ix,
    build_finalize_settlement_ix, build_finalize_settlement_ed25519_ix,
    build_submit_counter_state_ix, build_submit_counter_state_ed25519_ixs,
    derive_channel_pda, derive_escrow_pda,
};
use ignite_pay_state_channel::signing::sign_state;
use ignite_pay_state_channel::types::SignedState;

use crate::error::ChannelServiceError;
use crate::state::AppState;

fn decode_channel_id(hex_str: &str) -> Result<[u8; 32], ChannelServiceError> {
    let bytes = hex::decode(hex_str)
        .map_err(|_| ChannelServiceError::BadRequest("invalid channel_id hex".into()))?;
    bytes.try_into()
        .map_err(|_| ChannelServiceError::BadRequest("channel_id must be 32 bytes".into()))
}

fn decode_32bytes(hex_str: &str, field: &str) -> Result<[u8; 32], ChannelServiceError> {
    hex::decode(hex_str)
        .map_err(|_| ChannelServiceError::BadRequest(format!("invalid {} hex", field)))?
        .try_into()
        .map_err(|_| ChannelServiceError::BadRequest(format!("{} must be 32 bytes", field)))
}

fn decode_64bytes(hex_str: &str, field: &str) -> Result<[u8; 64], ChannelServiceError> {
    hex::decode(hex_str)
        .map_err(|_| ChannelServiceError::BadRequest(format!("invalid {} hex", field)))?
        .try_into()
        .map_err(|_| ChannelServiceError::BadRequest(format!("{} must be 64 bytes", field)))
}

/// Helper to serialize ed25519 instructions into JSON.
fn ed25519_ixs_to_json(ixs: &[solana_sdk::instruction::Instruction]) -> Vec<Value> {
    ixs.iter().map(|ix| {
        json!({
            "program_id": ix.program_id.to_string(),
            "accounts": [],
            "data": bs58::encode(&ix.data).into_string(),
        })
    }).collect()
}

#[derive(Debug, Deserialize)]
pub struct CooperativeCloseRequest {
    pub settle_window: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct CosignRequest {}

#[derive(Debug, Deserialize)]
pub struct ChallengeRequest {
    pub submitted_root: String,
    pub submitted_sequence: u64,
}

#[derive(Debug, Deserialize)]
pub struct SubmitCounterRequest {
    pub sig_a: String,
    pub sig_b: String,
}

#[derive(Debug, Deserialize)]
pub struct SettleRequest {
    pub settle_window: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct ClaimRequest {
    pub leaf_index: u32,
    pub claim_amount: u64,
    pub proof: Vec<String>,
}

pub async fn cooperative_close(
    State(state): State<AppState>,
    Path(channel_id_hex): Path<String>,
    Json(req): Json<CooperativeCloseRequest>,
) -> Result<Json<Value>, ChannelServiceError> {
    let channel_id = decode_channel_id(&channel_id_hex)?;
    let settle_window = req.settle_window.unwrap_or(state.config.channel.default_settle_window);

    let ed_kp = state.ed_keypair();
    let _our_pubkey = state.pubkey();

    let mgr = state.channel_manager.lock().await;
    let mut channel_state = mgr.load_state(&channel_id)
        .map_err(ChannelServiceError::StateChannel)?;

    let current_slot = state.rpc_client.get_slot()
        .map_err(|e| ChannelServiceError::SolanaRpc(e.to_string()))?;

    // Build a SignedState for cooperative close
    let sig_a = sign_state(&channel_id, channel_state.metadata.sequence, &channel_state.metadata.current_root, &ed_kp);
    let user_pk = channel_state.metadata.user_pubkey;
    let provider_pk = channel_state.metadata.provider_pubkey;
    let cosign = channel_state.provider_cosign.unwrap_or([0u8; 64]);
    let signed_state = SignedState {
        channel_id,
        sequence: channel_state.metadata.sequence,
        root: channel_state.metadata.current_root,
        sig_a,
        sig_b: cosign,
    };

    mgr.close_channel(
        &mut channel_state,
        &signed_state,
        &user_pk,
        &provider_pk,
        current_slot,
        settle_window,
    ).map_err(ChannelServiceError::StateChannel)?;

    // Build on-chain instruction (no signatures in data)
    let (channel_pda, _) = derive_channel_pda(&channel_id, &state.program_id);
    let ix = build_cooperative_settle_ix(
        &state.program_id,
        &channel_pda,
        signed_state.sequence,
        &signed_state.root,
        settle_window,
    );

    // Build ed25519 verification instructions (2 signatures)
    let mut coop_msg = Vec::with_capacity(32 + 8 + 32);
    coop_msg.extend_from_slice(&channel_id);
    coop_msg.extend_from_slice(&signed_state.sequence.to_le_bytes());
    coop_msg.extend_from_slice(&signed_state.root);
    let ed25519_ixs = build_cooperative_settle_ed25519_ixs(
        &user_pk,
        &provider_pk,
        &coop_msg,
        &signed_state.sig_a,
        &signed_state.sig_b,
    );

    Ok(Json(json!({
        "channel_id": channel_id_hex,
        "status": "settling",
        "settle_window": settle_window,
        "ed25519_instructions": ed25519_ixs_to_json(&ed25519_ixs),
        "on_chain_instruction": {
            "program_id": ix.program_id.to_string(),
            "data": bs58::encode(&ix.data).into_string(),
        },
    })))
}

pub async fn request_cosign(
    State(state): State<AppState>,
    Path(channel_id_hex): Path<String>,
    Json(_req): Json<CosignRequest>,
) -> Result<Json<Value>, ChannelServiceError> {
    let channel_id = decode_channel_id(&channel_id_hex)?;

    let ed_kp = state.ed_keypair();

    let mgr = state.channel_manager.lock().await;
    let mut channel_state = mgr.load_state(&channel_id)
        .map_err(ChannelServiceError::StateChannel)?;

    let cosignature = mgr.provider_cosign_state(&mut channel_state, &ed_kp)
        .map_err(ChannelServiceError::StateChannel)?;

    Ok(Json(json!({
        "channel_id": channel_id_hex,
        "sequence": channel_state.metadata.sequence,
        "root": hex::encode(channel_state.metadata.current_root),
        "cosignature": hex::encode(cosignature),
    })))
}

pub async fn provider_cosign(
    State(state): State<AppState>,
    Path(channel_id_hex): Path<String>,
    Json(_req): Json<CosignRequest>,
) -> Result<Json<Value>, ChannelServiceError> {
    let channel_id = decode_channel_id(&channel_id_hex)?;

    let ed_kp = state.ed_keypair();

    let mgr = state.channel_manager.lock().await;
    let mut channel_state = mgr.load_state(&channel_id)
        .map_err(ChannelServiceError::StateChannel)?;

    let cosignature = mgr.provider_cosign_state(&mut channel_state, &ed_kp)
        .map_err(ChannelServiceError::StateChannel)?;

    Ok(Json(json!({
        "channel_id": channel_id_hex,
        "sequence": channel_state.metadata.sequence,
        "cosignature": hex::encode(cosignature),
    })))
}

pub async fn trigger_challenge(
    State(state): State<AppState>,
    Path(channel_id_hex): Path<String>,
    Json(req): Json<ChallengeRequest>,
) -> Result<Json<Value>, ChannelServiceError> {
    let channel_id = decode_channel_id(&channel_id_hex)?;
    let submitted_root = decode_32bytes(&req.submitted_root, "submitted_root")?;

    let ed_kp = state.ed_keypair();
    let our_pubkey = state.pubkey();

    let current_slot = state.rpc_client.get_slot()
        .map_err(|e| ChannelServiceError::SolanaRpc(e.to_string()))?;

    // Sign the challenge message: channel_id || current_slot || submitted_root
    let mut msg = Vec::with_capacity(32 + 8 + 32);
    msg.extend_from_slice(&channel_id);
    msg.extend_from_slice(&current_slot.to_le_bytes());
    msg.extend_from_slice(&submitted_root);
    let challenger_signature: ed25519_dalek::Signature = ed_kp.sign(&msg);
    let sig_bytes = challenger_signature.to_bytes();

    let mgr = state.channel_manager.lock().await;
    let mut channel_state = mgr.load_state(&channel_id)
        .map_err(ChannelServiceError::StateChannel)?;

    mgr.trigger_challenge(
        &mut channel_state,
        &our_pubkey,
        current_slot,
        &submitted_root,
        req.submitted_sequence,
        &sig_bytes,
    ).map_err(ChannelServiceError::StateChannel)?;

    // Build on-chain instruction (no signature in data)
    let (channel_pda, _) = derive_channel_pda(&channel_id, &state.program_id);
    let ix = build_trigger_challenge_ix(
        &state.program_id,
        &channel_pda,
        &our_pubkey,
        &submitted_root,
        req.submitted_sequence,
    );

    // Build ed25519 verification instruction
    let ed25519_ix = build_trigger_challenge_ed25519_ix(
        &our_pubkey,
        &msg,
        &sig_bytes,
    );

    Ok(Json(json!({
        "channel_id": channel_id_hex,
        "status": "challenged",
        "challenge_slot": current_slot,
        "ed25519_instructions": ed25519_ixs_to_json(&[ed25519_ix]),
        "on_chain_instruction": {
            "program_id": ix.program_id.to_string(),
            "data": bs58::encode(&ix.data).into_string(),
        },
    })))
}

pub async fn submit_counter(
    State(state): State<AppState>,
    Path(channel_id_hex): Path<String>,
    Json(req): Json<SubmitCounterRequest>,
) -> Result<Json<Value>, ChannelServiceError> {
    let channel_id = decode_channel_id(&channel_id_hex)?;
    let sig_a = decode_64bytes(&req.sig_a, "sig_a")?;
    let sig_b = decode_64bytes(&req.sig_b, "sig_b")?;

    let mgr = state.channel_manager.lock().await;
    let mut channel_state = mgr.load_state(&channel_id)
        .map_err(ChannelServiceError::StateChannel)?;

    // Build the counter-state SignedState from request
    let counter_sequence = channel_state.metadata.sequence + 1;
    let counter_root = channel_state.metadata.current_root; // Use current root or caller provides
    let user_pk = channel_state.metadata.user_pubkey;
    let provider_pk = channel_state.metadata.provider_pubkey;

    let counter_state = SignedState {
        channel_id,
        sequence: counter_sequence,
        root: counter_root,
        sig_a,
        sig_b,
    };

    mgr.submit_counter_state(
        &mut channel_state,
        &counter_state,
        None,
        &user_pk,
        &provider_pk,
    ).map_err(ChannelServiceError::StateChannel)?;

    // Build on-chain instruction (no signatures in data)
    let (channel_pda, _) = derive_channel_pda(&channel_id, &state.program_id);
    let ix = build_submit_counter_state_ix(
        &state.program_id,
        &channel_pda,
        counter_sequence,
        &counter_root,
    );

    // Build ed25519 verification instructions (2 signatures)
    let mut counter_msg = Vec::with_capacity(32 + 8 + 32);
    counter_msg.extend_from_slice(&channel_id);
    counter_msg.extend_from_slice(&counter_sequence.to_le_bytes());
    counter_msg.extend_from_slice(&counter_root);
    let ed25519_ixs = build_submit_counter_state_ed25519_ixs(
        &user_pk,
        &provider_pk,
        &counter_msg,
        &sig_a,
        &sig_b,
    );

    Ok(Json(json!({
        "channel_id": channel_id_hex,
        "sequence": counter_sequence,
        "ed25519_instructions": ed25519_ixs_to_json(&ed25519_ixs),
        "on_chain_instruction": {
            "program_id": ix.program_id.to_string(),
            "data": bs58::encode(&ix.data).into_string(),
        },
    })))
}

pub async fn settle(
    State(state): State<AppState>,
    Path(channel_id_hex): Path<String>,
    Json(req): Json<SettleRequest>,
) -> Result<Json<Value>, ChannelServiceError> {
    let channel_id = decode_channel_id(&channel_id_hex)?;
    let settle_window = req.settle_window.unwrap_or(state.config.channel.default_settle_window);

    let current_slot = state.rpc_client.get_slot()
        .map_err(|e| ChannelServiceError::SolanaRpc(e.to_string()))?;

    let mgr = state.channel_manager.lock().await;
    let mut channel_state = mgr.load_state(&channel_id)
        .map_err(ChannelServiceError::StateChannel)?;

    mgr.settle_after_timeout(&mut channel_state, current_slot, settle_window)
        .map_err(ChannelServiceError::StateChannel)?;

    // Build on-chain instruction
    let (channel_pda, _) = derive_channel_pda(&channel_id, &state.program_id);
    let ix = build_settle_after_timeout_ix(&state.program_id, &channel_pda, settle_window);

    Ok(Json(json!({
        "channel_id": channel_id_hex,
        "status": "settling",
        "settle_window": settle_window,
        "on_chain_instruction": {
            "program_id": ix.program_id.to_string(),
            "data": bs58::encode(&ix.data).into_string(),
        },
    })))
}

pub async fn claim(
    State(state): State<AppState>,
    Path(channel_id_hex): Path<String>,
    Json(req): Json<ClaimRequest>,
) -> Result<Json<Value>, ChannelServiceError> {
    let channel_id = decode_channel_id(&channel_id_hex)?;

    let proof: Vec<[u8; 32]> = req.proof.iter().map(|p| decode_32bytes(p, "proof entry")).collect::<Result<Vec<_>, _>>()?;

    let ed_kp = state.ed_keypair();
    let our_pubkey = state.pubkey();

    let current_slot = state.rpc_client.get_slot()
        .map_err(|e| ChannelServiceError::SolanaRpc(e.to_string()))?;

    // Sign the claim message
    let mut claim_msg = Vec::with_capacity(32 + 8 + 32);
    claim_msg.extend_from_slice(&channel_id);
    claim_msg.extend_from_slice(&current_slot.to_le_bytes());
    claim_msg.extend_from_slice(&channel_state_current_root_from_mgr(&state, &channel_id).await?);
    let claimer_sig: ed25519_dalek::Signature = ed_kp.sign(&claim_msg);

    let mgr = state.channel_manager.lock().await;
    let mut channel_state = mgr.load_state(&channel_id)
        .map_err(ChannelServiceError::StateChannel)?;

    mgr.claim_leaf_with_proof(
        &mut channel_state,
        req.leaf_index,
        req.claim_amount,
        &our_pubkey,
        current_slot,
        &claimer_sig.to_bytes(),
        &proof,
    ).map_err(ChannelServiceError::StateChannel)?;

    Ok(Json(json!({
        "channel_id": channel_id_hex,
        "leaf_index": req.leaf_index,
        "claimed": req.claim_amount,
        "total_claimed": channel_state.metadata.total_claimed,
    })))
}

async fn channel_state_current_root_from_mgr(
    state: &AppState,
    channel_id: &[u8; 32],
) -> Result<[u8; 32], ChannelServiceError> {
    let mgr = state.channel_manager.lock().await;
    let cs = mgr.load_state(channel_id).map_err(ChannelServiceError::StateChannel)?;
    Ok(cs.metadata.current_root)
}

pub async fn finalize(
    State(state): State<AppState>,
    Path(channel_id_hex): Path<String>,
) -> Result<Json<Value>, ChannelServiceError> {
    let channel_id = decode_channel_id(&channel_id_hex)?;

    let ed_kp = state.ed_keypair();
    let our_pubkey = state.pubkey();

    let current_slot = state.rpc_client.get_slot()
        .map_err(|e| ChannelServiceError::SolanaRpc(e.to_string()))?;

    let mgr = state.channel_manager.lock().await;
    let mut channel_state = mgr.load_state(&channel_id)
        .map_err(ChannelServiceError::StateChannel)?;

    // Sign the finalize message
    let mut fin_msg = Vec::with_capacity(32 + 8 + 32);
    fin_msg.extend_from_slice(&channel_id);
    fin_msg.extend_from_slice(&current_slot.to_le_bytes());
    fin_msg.extend_from_slice(&channel_state.metadata.current_root);
    let caller_sig: ed25519_dalek::Signature = ed_kp.sign(&fin_msg);

    mgr.finalize_settlement(
        &mut channel_state,
        current_slot,
        &our_pubkey,
        &caller_sig.to_bytes(),
    ).map_err(ChannelServiceError::StateChannel)?;

    // Build on-chain instruction (no signature in data)
    let (channel_pda, _) = derive_channel_pda(&channel_id, &state.program_id);
    let (escrow_pda, _) = derive_escrow_pda(&channel_id, &state.program_id);

    let ix = build_finalize_settlement_ix(
        &state.program_id,
        &channel_pda,
        &our_pubkey,
        &channel_state.metadata.vault_a,
        &channel_state.metadata.vault_b,
        &escrow_pda,
    );

    // Build ed25519 verification instruction
    let ed25519_ix = build_finalize_settlement_ed25519_ix(
        &our_pubkey,
        &fin_msg,
        &caller_sig.to_bytes(),
    );

    Ok(Json(json!({
        "channel_id": channel_id_hex,
        "status": "closed",
        "ed25519_instructions": ed25519_ixs_to_json(&[ed25519_ix]),
        "on_chain_instruction": {
            "program_id": ix.program_id.to_string(),
            "data": bs58::encode(&ix.data).into_string(),
        },
    })))
}
