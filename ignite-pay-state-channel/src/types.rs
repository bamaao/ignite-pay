use borsh::{BorshSerialize, BorshDeserialize};
use serde::{Deserialize, Serialize};
use solana_program::hash::hash;
use solana_pubkey::Pubkey;
use std::collections::BTreeSet;

/// Type of UTXO leaf in the state channel tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub enum LeafType {
    Standard,
    HTLC,
    Compliance,
}

/// A UTXO leaf node in the off-chain Merkle tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct UTXOLeaf {
    pub leaf_type: LeafType,
    pub owner: Pubkey,
    pub amount: u64,
    /// SHA-256 hash of the preimage for HTLC leaves; None for Standard/Compliance.
    pub hash_lock: Option<[u8; 32]>,
    /// Absolute slot after which HTLC can be refunded; None for non-HTLC.
    pub timelock_slot: Option<u64>,
    /// Intended recipient for HTLC resolution; None for non-HTLC.
    pub beneficiary: Option<Pubkey>,
}

impl UTXOLeaf {
    /// Create a standard (non-HTLC) UTXO leaf.
    pub fn standard(owner: Pubkey, amount: u64) -> Self {
        Self {
            leaf_type: LeafType::Standard,
            owner,
            amount,
            hash_lock: None,
            timelock_slot: None,
            beneficiary: None,
        }
    }

    /// Create an HTLC-locked UTXO leaf.
    pub fn htlc(
        owner: Pubkey,
        amount: u64,
        hash_lock: [u8; 32],
        timelock_slot: u64,
        beneficiary: Pubkey,
    ) -> Self {
        Self {
            leaf_type: LeafType::HTLC,
            owner,
            amount,
            hash_lock: Some(hash_lock),
            timelock_slot: Some(timelock_slot),
            beneficiary: Some(beneficiary),
        }
    }

    /// Create an empty (zero-amount) leaf used for padding.
    pub fn empty() -> Self {
        Self {
            leaf_type: LeafType::Standard,
            owner: Pubkey::default(),
            amount: 0,
            hash_lock: None,
            timelock_slot: None,
            beneficiary: None,
        }
    }

    /// Returns true if this leaf has zero amount (empty/padding slot).
    pub fn is_empty(&self) -> bool {
        self.amount == 0
    }

    /// Deterministic leaf hash: SHA-256 of the borsh-serialized UTXOLeaf.
    pub fn hash(&self) -> [u8; 32] {
        let data = borsh::to_vec(self).expect("UTXOLeaf serialization should not fail");
        hash(&data).to_bytes()
    }
}

/// A single leaf update message signed by one party.
#[derive(Debug, Clone, BorshSerialize, BorshDeserialize)]
pub struct LeafUpdate {
    pub channel_id: [u8; 32],
    pub sequence: u64,
    pub leaf_index: u32,
    pub prev_leaf_hash: [u8; 32],
    pub new_leaf: UTXOLeaf,
    /// Ed25519 signature bytes (fixed 64 bytes).
    pub signature: [u8; 64],
}

impl Serialize for LeafUpdate {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = s.serialize_struct("LeafUpdate", 6)?;
        state.serialize_field("channel_id", &self.channel_id.to_vec())?;
        state.serialize_field("sequence", &self.sequence)?;
        state.serialize_field("leaf_index", &self.leaf_index)?;
        state.serialize_field("prev_leaf_hash", &self.prev_leaf_hash.to_vec())?;
        state.serialize_field("new_leaf", &self.new_leaf)?;
        state.serialize_field("signature", &self.signature.to_vec())?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for LeafUpdate {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Helper {
            channel_id: Vec<u8>,
            sequence: u64,
            leaf_index: u32,
            prev_leaf_hash: Vec<u8>,
            new_leaf: UTXOLeaf,
            signature: Vec<u8>,
        }
        let h = Helper::deserialize(d)?;
        Ok(LeafUpdate {
            channel_id: h.channel_id.try_into().map_err(|_| serde::de::Error::custom("channel_id must be 32 bytes"))?,
            sequence: h.sequence,
            leaf_index: h.leaf_index,
            prev_leaf_hash: h.prev_leaf_hash.try_into().map_err(|_| serde::de::Error::custom("prev_leaf_hash must be 32 bytes"))?,
            new_leaf: h.new_leaf,
            signature: h.signature.try_into().map_err(|_| serde::de::Error::custom("signature must be 64 bytes"))?,
        })
    }
}

/// Dual-signed state representing agreement on a tree root.
#[derive(Debug, Clone, BorshSerialize, BorshDeserialize)]
pub struct SignedState {
    pub channel_id: [u8; 32],
    pub sequence: u64,
    pub root: [u8; 32],
    /// Ed25519 signature from party A (fixed 64 bytes).
    pub sig_a: [u8; 64],
    /// Ed25519 signature from party B (fixed 64 bytes).
    pub sig_b: [u8; 64],
}

impl Serialize for SignedState {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = s.serialize_struct("SignedState", 5)?;
        state.serialize_field("channel_id", &self.channel_id.to_vec())?;
        state.serialize_field("sequence", &self.sequence)?;
        state.serialize_field("root", &self.root.to_vec())?;
        state.serialize_field("sig_a", &self.sig_a.to_vec())?;
        state.serialize_field("sig_b", &self.sig_b.to_vec())?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for SignedState {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Helper {
            channel_id: Vec<u8>,
            sequence: u64,
            root: Vec<u8>,
            sig_a: Vec<u8>,
            sig_b: Vec<u8>,
        }
        let h = Helper::deserialize(d)?;
        Ok(SignedState {
            channel_id: h.channel_id.try_into().map_err(|_| serde::de::Error::custom("channel_id must be 32 bytes"))?,
            sequence: h.sequence,
            root: h.root.try_into().map_err(|_| serde::de::Error::custom("root must be 32 bytes"))?,
            sig_a: h.sig_a.try_into().map_err(|_| serde::de::Error::custom("sig_a must be 64 bytes"))?,
            sig_b: h.sig_b.try_into().map_err(|_| serde::de::Error::custom("sig_b must be 64 bytes"))?,
        })
    }
}

/// Current status of a payment channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub enum ChannelStatus {
    Open,
    Challenged,
    Settling,
    Closed,
}

/// Persistent channel metadata stored in sled.
#[derive(Debug, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct ChannelMetadata {
    pub channel_id: [u8; 32],
    pub user_pubkey: Pubkey,
    pub provider_pubkey: Pubkey,
    pub token_mint: Pubkey,
    pub tree_depth: u32,
    pub status: ChannelStatus,
    pub sequence: u64,
    pub current_root: [u8; 32],
    /// Total amount deposited into this channel.
    pub total_deposited: u64,
    /// Slot when the channel was opened.
    pub open_slot: u64,
    /// Slot when a challenge was triggered (if any).
    pub challenge_slot: Option<u64>,
    /// User's vault (token account for deposits/withdrawals).
    pub vault_a: Pubkey,
    /// Provider's vault (token account for deposits/withdrawals).
    pub vault_b: Pubkey,
    /// Amount deposited by the user.
    pub deposit_a: u64,
    /// Amount deposited by the provider (for dual-funded channels).
    pub deposit_b: u64,
    /// Challenge period duration in slots.
    pub challenge_duration: u64,
    /// Minimum delay before a challenge can be triggered (anti front-running).
    pub min_challenge_delay: u64,
    /// Slot after which the channel auto-closes. None means no auto-close.
    pub auto_close_slot: Option<u64>,
    /// Total amount already claimed during settlement.
    pub total_claimed: u64,
    /// Settlement window deadline slot. None if not settling.
    pub settle_deadline: Option<u64>,
    /// Number of non-empty leaves in the tree (informational).
    pub leaf_count: u32,
    /// Set of leaf indices that have already been claimed during settlement.
    pub claimed_leaves: BTreeSet<u32>,
}
