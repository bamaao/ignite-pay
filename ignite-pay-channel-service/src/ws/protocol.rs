use serde::{Deserialize, Serialize};

/// All WebSocket message types used between channel service peers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WsMessage {
    // ── Auth ──
    #[serde(rename = "auth")]
    Auth {
        pubkey: String,
        signature: Vec<u8>,
        timestamp: u64,
    },
    #[serde(rename = "auth_ok")]
    AuthOk,

    // ── Leaf Updates ──
    #[serde(rename = "leaf_update")]
    LeafUpdate {
        channel_id: String,
        sequence: u64,
        leaf_index: u32,
        prev_leaf_hash: Vec<u8>,
        new_leaf: serde_json::Value,
        signature: Vec<u8>,
    },
    #[serde(rename = "leaf_update_ack")]
    LeafUpdateAck { channel_id: String, sequence: u64 },
    #[serde(rename = "leaf_update_nack")]
    LeafUpdateNack { channel_id: String, sequence: u64, reason: String },

    // ── Batch ──
    #[serde(rename = "batch_start")]
    BatchStart { channel_id: String, count: u32 },
    #[serde(rename = "batch_item")]
    BatchItem {
        channel_id: String,
        index: u32,
        update: Box<WsMessage>,
    },
    #[serde(rename = "batch_commit")]
    BatchCommit { channel_id: String },
    #[serde(rename = "batch_abort")]
    BatchAbort { channel_id: String },
    #[serde(rename = "batch_result")]
    BatchResult {
        channel_id: String,
        applied: u32,
        failed_index: Option<u32>,
    },

    // ── Co-signature ──
    #[serde(rename = "cosign_request")]
    CosignRequest {
        channel_id: String,
        sequence: u64,
        root: Vec<u8>,
    },
    #[serde(rename = "cosign_response")]
    CosignResponse {
        channel_id: String,
        sequence: u64,
        signature: Vec<u8>,
    },

    // ── HTLC ──
    #[serde(rename = "htlc_created")]
    HtlcCreated {
        channel_id: String,
        hash_lock: Vec<u8>,
        amount: u64,
        timelock_slot: u64,
    },
    #[serde(rename = "htlc_preimage")]
    HtlcPreimage {
        channel_id: String,
        hash_lock: Vec<u8>,
        preimage: Vec<u8>,
    },
    #[serde(rename = "htlc_refunded")]
    HtlcRefunded {
        channel_id: String,
        hash_lock: Vec<u8>,
    },

    // ── Multi-hop ──
    #[serde(rename = "multihop_init")]
    MultihopInit {
        payment_id: Vec<u8>,
        hash_lock: Vec<u8>,
        amount: u64,
        timelock_slot: u64,
        next_hop: String,
    },
    #[serde(rename = "multihop_preimage")]
    MultihopPreimage {
        payment_id: Vec<u8>,
        preimage: Vec<u8>,
    },
    #[serde(rename = "multihop_failed")]
    MultihopFailed {
        payment_id: Vec<u8>,
        reason: String,
    },

    // ── Settlement ──
    #[serde(rename = "challenge_triggered")]
    ChallengeTriggered {
        channel_id: String,
        challenge_slot: u64,
    },
    #[serde(rename = "counter_state_submitted")]
    CounterStateSubmitted {
        channel_id: String,
        sequence: u64,
    },
    #[serde(rename = "settlement_started")]
    SettlementStarted {
        channel_id: String,
        deadline: u64,
    },

    // ── State change ──
    #[serde(rename = "channel_state_changed")]
    ChannelStateChanged {
        channel_id: String,
        old_status: String,
        new_status: String,
    },

    // ── Error ──
    #[serde(rename = "error")]
    Error { code: u16, message: String },
}
