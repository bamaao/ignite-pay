use crate::error::{Result, StateChannelError};
use crate::types::{LeafUpdate, UTXOLeaf, LeafType};
use borsh::{BorshSerialize, BorshDeserialize};
use serde::{Deserialize, Serialize};
use sled::Db;
use solana_program::hash::hash;
use solana_pubkey::Pubkey;

/// Spending limit configuration for a channel.
#[derive(Debug, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct SpendingLimit {
    /// Maximum cumulative spend before compliance review.
    pub threshold: u64,
    /// Maximum spend per single channel.
    pub per_channel: u64,
    /// Rolling window in slots for threshold enforcement.
    pub window_slots: u64,
}

/// Travel rule data for regulatory compliance.
#[derive(Debug, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct TravelRuleData {
    /// DID or identifier of the originator.
    pub originator_id: Vec<u8>,
    /// DID or identifier of the beneficiary.
    pub beneficiary_id: Vec<u8>,
    /// Amount of the transfer.
    pub amount: u64,
    /// Slot when the transfer was recorded.
    pub created_slot: u64,
    /// Channel ID of the transfer.
    pub channel_id: [u8; 32],
    /// Jurisdiction code of the originator.
    pub originator_jurisdiction: Vec<u8>,
    /// Jurisdiction code of the beneficiary.
    pub beneficiary_jurisdiction: Vec<u8>,
}

/// Action returned after recording a payment for compliance.
#[derive(Debug, Clone)]
pub enum ComplianceAction {
    /// No compliance action needed.
    None,
    /// Insert a compliance marker leaf into the channel tree.
    InsertMarker {
        /// Hash of the compliance marker.
        compliance_hash: [u8; 32],
        /// Threshold that was exceeded.
        threshold: u64,
    },
}

/// A single payment record within a sliding window.
#[derive(Debug, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct PaymentRecord {
    /// Slot when the payment was recorded.
    pub slot: u64,
    /// Amount of the payment.
    pub amount: u64,
}

/// Per-channel compliance state.
#[derive(Debug, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct ChannelComplianceState {
    /// Channel ID this state belongs to.
    pub channel_id: [u8; 32],
    /// Cumulative amount spent from this channel (all-time).
    pub cumulative_spent: u64,
    /// Last slot when a compliance check was performed.
    pub last_check_slot: u64,
    /// Whether a compliance hold is active.
    pub compliance_hold: bool,
    /// Spending limits for this channel.
    pub limits: SpendingLimit,
    /// Travel rule records for this channel.
    pub travel_rules: Vec<TravelRuleData>,
    /// DEV-11: Payments within the current sliding window for threshold enforcement.
    /// Older entries are pruned on each `record_payment` call.
    pub window_payments: Vec<PaymentRecord>,
}

/// Manager for compliance operations backed by sled.
pub struct ComplianceManager {
    db: Db,
}

impl std::fmt::Debug for ComplianceManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ComplianceManager").finish()
    }
}

impl ComplianceManager {
    /// Create a new ComplianceManager backed by a sled database.
    pub fn new(db: Db) -> Result<Self> {
        Ok(Self { db })
    }

    /// Initialize compliance state for a channel.
    pub fn init_channel_compliance(
        &self,
        channel_id: [u8; 32],
        limits: SpendingLimit,
    ) -> Result<()> {
        let key = format!("compliance:{}", hex::encode(channel_id));
        let state = ChannelComplianceState {
            channel_id,
            cumulative_spent: 0,
            last_check_slot: 0,
            compliance_hold: false,
            limits,
            travel_rules: Vec::new(),
            window_payments: Vec::new(),
        };
        let data = borsh::to_vec(&state)?;
        self.db.insert(key.as_bytes(), data)?;
        self.db.flush()?;
        Ok(())
    }

    /// Load compliance state for a channel.
    pub fn load_state(&self, channel_id: [u8; 32]) -> Result<ChannelComplianceState> {
        let key = format!("compliance:{}", hex::encode(channel_id));
        let data = self
            .db
            .get(key.as_bytes())?
            .ok_or_else(|| StateChannelError::ChannelNotFound(hex::encode(channel_id)))?;
        let state: ChannelComplianceState = borsh::from_slice(&data)?;
        Ok(state)
    }

    /// Record a payment and check compliance limits.
    ///
    /// DEV-11 fix: uses a sliding window based on `window_slots` to compute
    /// the rolling spend. Only payments within the window count toward the
    /// threshold. The `cumulative_spent` field remains as an all-time tracker.
    ///
    /// Returns a `ComplianceAction` indicating whether a compliance marker
    /// should be inserted into the channel tree.
    pub fn record_payment(
        &self,
        channel_id: [u8; 32],
        amount: u64,
        slot: u64,
        user: Pubkey,
        provider: Pubkey,
    ) -> Result<ComplianceAction> {
        let key = format!("compliance:{}", hex::encode(channel_id));
        let data = self
            .db
            .get(key.as_bytes())?
            .ok_or_else(|| StateChannelError::ChannelNotFound(hex::encode(channel_id)))?;
        let mut state: ChannelComplianceState = borsh::from_slice(&data)?;

        if state.compliance_hold {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "compliance hold is active for this channel"
            )));
        }

        // Check per-channel limit
        if amount > state.limits.per_channel {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "payment {} exceeds per_channel limit {}",
                amount, state.limits.per_channel
            )));
        }

        // DEV-11: Add payment to window, then prune expired entries
        // BUG-29/BUG-30: Only add to window and prune if slot > 0
        if slot > 0 {
            state.window_payments.push(PaymentRecord { slot, amount });

            // Prune payments outside the sliding window
            if state.limits.window_slots > 0 && slot >= state.limits.window_slots {
                let window_start = slot - state.limits.window_slots;
                state.window_payments.retain(|p| p.slot >= window_start);
            }
        }

        // Compute window spend (only from payments with valid slots)
        let window_spend: u64 = state.window_payments.iter()
            .map(|p| p.amount)
            .fold(0u64, |acc, a| acc.saturating_add(a));

        let new_cumulative = state.cumulative_spent.saturating_add(amount);
        state.cumulative_spent = new_cumulative;
        state.last_check_slot = slot;

        // BUG-29/BUG-30: When slot=0, use cumulative_spent for threshold check instead of window_spend
        let effective_spend = if slot == 0 { new_cumulative } else { window_spend };

        // Check if threshold is crossed based on effective spend
        let action = if effective_spend >= state.limits.threshold {
            let compliance_hash = Self::compute_compliance_hash(
                &channel_id, &user, &provider, effective_spend, slot,
            );
            state.compliance_hold = true;
            ComplianceAction::InsertMarker {
                compliance_hash,
                threshold: state.limits.threshold,
            }
        } else {
            ComplianceAction::None
        };

        let updated = borsh::to_vec(&state)?;
        self.db.insert(key.as_bytes(), updated)?;
        self.db.flush()?;
        Ok(action)
    }

    /// Clear a compliance hold on a channel.
    pub fn clear_hold(&self, channel_id: [u8; 32]) -> Result<()> {
        let key = format!("compliance:{}", hex::encode(channel_id));
        let data = self
            .db
            .get(key.as_bytes())?
            .ok_or_else(|| StateChannelError::ChannelNotFound(hex::encode(channel_id)))?;
        let mut state: ChannelComplianceState = borsh::from_slice(&data)?;
        state.compliance_hold = false;
        let updated = borsh::to_vec(&state)?;
        self.db.insert(key.as_bytes(), updated)?;
        self.db.flush()?;
        Ok(())
    }

    /// Record a LeafUpdate in the audit trail.
    pub fn record_audit(&self, update: &LeafUpdate) -> Result<()> {
        // ISSUE-2 fix: use distinct key prefix "__seq__" to avoid collision with LeafUpdate entries
        let seq_key = format!(
            "audit:{}:__seq__",
            hex::encode(update.channel_id)
        );
        let next_seq: u64 = self
            .db
            .get(seq_key.as_bytes())?
            .map(|v| {
                let s: u64 = borsh::from_slice(&v).unwrap();
                s + 1
            })
            .unwrap_or(0);

        let audit_key = format!(
            "audit:{}:{}",
            hex::encode(update.channel_id),
            next_seq
        );
        let data = borsh::to_vec(update)?;
        self.db.insert(audit_key.as_bytes(), data)?;
        self.db.insert(seq_key.as_bytes(), borsh::to_vec(&next_seq)?)?;
        self.db.flush()?;
        Ok(())
    }

    /// Get the full audit trail for a channel.
    pub fn get_audit_trail(&self, channel_id: [u8; 32]) -> Result<Vec<LeafUpdate>> {
        let prefix = format!("audit:{}:", hex::encode(channel_id));
        let mut trail = Vec::new();
        for item in self.db.scan_prefix(prefix.as_bytes()) {
            let (key, value) = item?;
            // ISSUE-2 fix: skip the sequence counter key by checking key suffix
            let key_str = String::from_utf8_lossy(&key);
            if key_str.ends_with(":__seq__") {
                continue;
            }
            if let Ok(update) = borsh::from_slice::<LeafUpdate>(&value) {
                trail.push(update);
            }
        }
        // Sort by sequence number
        trail.sort_by_key(|u| u.sequence);
        Ok(trail)
    }

    /// Compute a compliance hash from payment data.
    fn compute_compliance_hash(
        channel_id: &[u8; 32],
        user: &Pubkey,
        provider: &Pubkey,
        cumulative: u64,
        slot: u64,
    ) -> [u8; 32] {
        let mut data = Vec::with_capacity(32 + 32 + 32 + 8 + 8);
        data.extend_from_slice(channel_id);
        data.extend_from_slice(user.as_ref());
        data.extend_from_slice(provider.as_ref());
        data.extend_from_slice(&cumulative.to_le_bytes());
        data.extend_from_slice(&slot.to_le_bytes());
        hash(&data).to_bytes()
    }
}

/// Create a compliance marker UTXO leaf.
///
/// This leaf represents a compliance hold in the Merkle tree.
/// It uses the Compliance leaf type with a zero amount.
pub fn create_compliance_leaf(compliance_hash: [u8; 32]) -> UTXOLeaf {
    UTXOLeaf {
        leaf_type: LeafType::Compliance,
        owner: Pubkey::default(),
        amount: 0,
        hash_lock: Some(compliance_hash),
        timelock_slot: None,
        beneficiary: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signing::{generate_keypair, to_pubkey};
    use crate::types::UTXOLeaf;

    fn temp_db() -> Db {
        let dir = tempfile::tempdir().unwrap();
        sled::open(dir.path()).unwrap()
    }

    fn default_limits() -> SpendingLimit {
        SpendingLimit {
            threshold: 1_000_000,
            per_channel: 2_000_000,
            window_slots: 1000,
        }
    }

    #[test]
    fn test_init_and_load_state() {
        let db = temp_db();
        let mgr = ComplianceManager::new(db).unwrap();
        let channel_id = [42u8; 32];

        mgr.init_channel_compliance(channel_id, default_limits()).unwrap();

        let state = mgr.load_state(channel_id).unwrap();
        assert_eq!(state.channel_id, channel_id);
        assert_eq!(state.cumulative_spent, 0);
        assert!(!state.compliance_hold);
        assert_eq!(state.limits.threshold, 1_000_000);
    }

    #[test]
    fn test_record_payment_no_action() {
        let db = temp_db();
        let mgr = ComplianceManager::new(db).unwrap();
        let channel_id = [1u8; 32];

        mgr.init_channel_compliance(channel_id, default_limits()).unwrap();

        let action = mgr.record_payment(
            channel_id, 100_000, 100,
            Pubkey::new_unique(), Pubkey::new_unique(),
        ).unwrap();

        assert!(matches!(action, ComplianceAction::None));

        let state = mgr.load_state(channel_id).unwrap();
        assert_eq!(state.cumulative_spent, 100_000);
        assert!(!state.compliance_hold);
    }

    #[test]
    fn test_record_payment_triggers_marker() {
        let db = temp_db();
        let mgr = ComplianceManager::new(db).unwrap();
        let channel_id = [2u8; 32];

        mgr.init_channel_compliance(channel_id, default_limits()).unwrap();

        // Record payments until threshold is crossed
        let user = Pubkey::new_unique();
        let provider = Pubkey::new_unique();

        mgr.record_payment(channel_id, 500_000, 100, user, provider).unwrap();
        let action = mgr.record_payment(channel_id, 500_000, 200, user, provider).unwrap();

        match action {
            ComplianceAction::InsertMarker { threshold, .. } => {
                assert_eq!(threshold, 1_000_000);
            }
            ComplianceAction::None => panic!("expected InsertMarker"),
        }

        let state = mgr.load_state(channel_id).unwrap();
        assert!(state.compliance_hold);
        assert_eq!(state.cumulative_spent, 1_000_000);
    }

    #[test]
    fn test_record_payment_rejected_on_hold() {
        let db = temp_db();
        let mgr = ComplianceManager::new(db).unwrap();
        let channel_id = [3u8; 32];

        mgr.init_channel_compliance(channel_id, default_limits()).unwrap();

        let user = Pubkey::new_unique();
        let provider = Pubkey::new_unique();

        // Trigger hold
        mgr.record_payment(channel_id, 1_000_000, 100, user, provider).unwrap();

        // Should be rejected now
        let result = mgr.record_payment(channel_id, 100, 200, user, provider);
        assert!(result.is_err());
    }

    #[test]
    fn test_clear_hold() {
        let db = temp_db();
        let mgr = ComplianceManager::new(db).unwrap();
        let channel_id = [4u8; 32];

        mgr.init_channel_compliance(channel_id, default_limits()).unwrap();

        let user = Pubkey::new_unique();
        let provider = Pubkey::new_unique();

        // Trigger hold
        mgr.record_payment(channel_id, 1_000_000, 100, user, provider).unwrap();
        assert!(mgr.load_state(channel_id).unwrap().compliance_hold);

        // Clear hold
        mgr.clear_hold(channel_id).unwrap();
        assert!(!mgr.load_state(channel_id).unwrap().compliance_hold);
    }

    #[test]
    fn test_record_payment_exceeds_per_channel() {
        let db = temp_db();
        let mgr = ComplianceManager::new(db).unwrap();
        let channel_id = [5u8; 32];

        mgr.init_channel_compliance(channel_id, default_limits()).unwrap();

        let result = mgr.record_payment(
            channel_id, 3_000_000, 100,
            Pubkey::new_unique(), Pubkey::new_unique(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_audit_trail() {
        let db = temp_db();
        let mgr = ComplianceManager::new(db).unwrap();
        let channel_id = [6u8; 32];

        let signer = generate_keypair();
        let prev_leaf = UTXOLeaf::standard(to_pubkey(&signer), 100_000);
        let new_leaf = UTXOLeaf::standard(Pubkey::new_unique(), 50_000);

        let update1 = crate::signing::sign_leaf_update(
            &channel_id, 1, 0, &prev_leaf, new_leaf.clone(), &signer,
        );
        let update2 = crate::signing::sign_leaf_update(
            &channel_id, 2, 0, &new_leaf, UTXOLeaf::standard(Pubkey::new_unique(), 25_000), &signer,
        );

        mgr.record_audit(&update1).unwrap();
        mgr.record_audit(&update2).unwrap();

        let trail = mgr.get_audit_trail(channel_id).unwrap();
        assert_eq!(trail.len(), 2);
        assert_eq!(trail[0].sequence, 1);
        assert_eq!(trail[1].sequence, 2);
    }

    #[test]
    fn test_create_compliance_leaf() {
        let compliance_hash = [99u8; 32];
        let leaf = create_compliance_leaf(compliance_hash);
        assert_eq!(leaf.leaf_type, LeafType::Compliance);
        assert_eq!(leaf.amount, 0);
        assert_eq!(leaf.hash_lock, Some(compliance_hash));
        assert!(leaf.timelock_slot.is_none());
        assert!(leaf.beneficiary.is_none());
    }

    #[test]
    fn test_load_nonexistent_channel() {
        let db = temp_db();
        let mgr = ComplianceManager::new(db).unwrap();
        let result = mgr.load_state([0u8; 32]);
        assert!(result.is_err());
    }

    #[test]
    fn test_window_slots_sliding_window() {
        let db = temp_db();
        let mgr = ComplianceManager::new(db).unwrap();
        let channel_id = [10u8; 32];

        // Threshold 1000, window 100 slots
        mgr.init_channel_compliance(channel_id, SpendingLimit {
            threshold: 1_000,
            per_channel: 10_000,
            window_slots: 100,
        }).unwrap();

        let user = Pubkey::new_unique();
        let provider = Pubkey::new_unique();

        // Spend 600 at slot 100
        let action1 = mgr.record_payment(channel_id, 600, 100, user, provider).unwrap();
        assert!(matches!(action1, ComplianceAction::None));

        // Spend 600 at slot 150 — window [50..150] contains 600+600=1200 > threshold
        let action2 = mgr.record_payment(channel_id, 600, 150, user, provider).unwrap();
        match &action2 {
            ComplianceAction::InsertMarker { threshold, .. } => assert_eq!(*threshold, 1_000),
            ComplianceAction::None => panic!("expected InsertMarker"),
        }

        // Verify hold is active
        let state = mgr.load_state(channel_id).unwrap();
        assert!(state.compliance_hold);
        assert_eq!(state.window_payments.len(), 2);

        // Clear hold and test window expiry
        mgr.clear_hold(channel_id).unwrap();

        // Spend 600 at slot 300 — window [200..300], old payments at 100 and 150 are pruned
        // Only 600 in window, under threshold
        let action3 = mgr.record_payment(channel_id, 600, 300, user, provider).unwrap();
        assert!(matches!(action3, ComplianceAction::None));

        let state2 = mgr.load_state(channel_id).unwrap();
        // Old payments should be pruned, only the slot 300 payment remains
        assert_eq!(state2.window_payments.len(), 1);
        assert!(!state2.compliance_hold);
    }
}
