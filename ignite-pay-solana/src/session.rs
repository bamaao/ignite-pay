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

use crate::error::{Result, SolanaError};
use crate::types::SessionTokenData;
use borsh::BorshDeserialize;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Keypair;
use solana_sdk::signer::Signer;
use std::time::{SystemTime, UNIX_EPOCH};

/// A session keypair with its associated metadata.
pub struct SessionKeypair {
    pub keypair: Keypair,
    pub session_data: SessionTokenData,
}

/// Manager for session keys — creation, persistence, validation.
pub struct SessionManager {
    db: sled::Db,
}

impl std::fmt::Debug for SessionManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionManager").finish()
    }
}

impl SessionManager {
    /// Create a new SessionManager backed by a sled database.
    pub fn new(db: sled::Db) -> Result<Self> {
        Ok(Self { db })
    }

    /// Create a new session key (local operation, no on-chain transaction).
    /// The ephemeral keypair is generated randomly and persisted to sled.
    pub fn create_session(
        &self,
        owner: &Pubkey,
        target_program: &Pubkey,
        scopes: Vec<String>,
        spending_limit: u64,
        duration_secs: i64,
        per_tx_limit: u64,
        daily_tx_count_limit: u32,
    ) -> Result<SessionKeypair> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| SolanaError::Other(anyhow::anyhow!("System time error: {}", e)))?
            .as_secs() as i64;

        let ephemeral = Keypair::new();

        let today_start = now - (now % 86400);

        let session_data = SessionTokenData {
            owner: *owner,
            ephemeral_signer: ephemeral.pubkey(),
            target_program: *target_program,
            token_mint: Pubkey::default(),
            expires_at: now + duration_secs,
            spending_limit,
            current_spent: 0,
            per_tx_limit,
            daily_tx_count_limit,
            scopes,
            current_daily_count: 0,
            last_daily_reset: today_start,
        };

        // Persist: key = session:{pubkey_base58}, value = borsh(SessionTokenData) + 64-byte keypair
        let key = format!("session:{}", ephemeral.pubkey());
        let data_bytes = borsh::to_vec(&session_data)?;
        let mut value = data_bytes;
        value.extend_from_slice(&ephemeral.to_bytes());

        self.db.insert(key.as_bytes(), value)?;

        Ok(SessionKeypair {
            keypair: ephemeral,
            session_data,
        })
    }

    /// Get the active (non-expired) session for an owner.
    pub fn get_active_session(&self, owner: &Pubkey) -> Result<Option<SessionKeypair>> {
        let prefix = b"session:";
        for item in self.db.scan_prefix(prefix) {
            let (_, value) = item?;
            if value.len() < 64 {
                continue;
            }
            let data_len = value.len() - 64;
            let session_data: SessionTokenData = SessionTokenData::deserialize(&mut &value[..data_len])?;

            if session_data.owner == *owner && !self.is_expired(&session_data) {
                let keypair_bytes: [u8; 64] = value[data_len..].try_into().map_err(|_| {
                    SolanaError::InvalidKeypair("Invalid keypair bytes length".into())
                })?;
                let keypair = Keypair::try_from(&keypair_bytes as &[u8])
                    .map_err(|e| SolanaError::InvalidKeypair(e.to_string()))?;

                return Ok(Some(SessionKeypair {
                    keypair,
                    session_data,
                }));
            }
        }
        Ok(None)
    }

    /// Get a session by its ephemeral public key.
    pub fn get_session_by_pubkey(
        &self,
        ephemeral_pubkey: &Pubkey,
    ) -> Result<Option<SessionKeypair>> {
        let key = format!("session:{}", ephemeral_pubkey);
        if let Some(value) = self.db.get(key.as_bytes())? {
            let value = value;
            if value.len() < 64 {
                return Ok(None);
            }
            let data_len = value.len() - 64;
            let session_data: SessionTokenData = SessionTokenData::deserialize(&mut &value[..data_len])?;

            let keypair_bytes: [u8; 64] = value[data_len..]
                .try_into()
                .map_err(|_| SolanaError::InvalidKeypair("Invalid keypair bytes length".into()))?;
            let keypair = Keypair::try_from(&keypair_bytes as &[u8])
                .map_err(|e| SolanaError::InvalidKeypair(e.to_string()))?;

            Ok(Some(SessionKeypair {
                keypair,
                session_data,
            }))
        } else {
            Ok(None)
        }
    }

    /// Check if a session has expired.
    pub fn is_expired(&self, session: &SessionTokenData) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        now >= session.expires_at
    }

    /// Check if a spending amount is within the session's limit.
    pub fn check_spending_limit(&self, session: &SessionTokenData, amount: u64) -> bool {
        session.current_spent.saturating_add(amount) <= session.spending_limit
    }

    /// Check if the daily transaction count would still be within limit after one more tx.
    /// Handles day-rollover by resetting the counter when a new UTC day starts.
    pub fn check_daily_tx_count(&self, session: &SessionTokenData) -> bool {
        if session.daily_tx_count_limit == 0 {
            return true; // no limit
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        // Check if we're in a new UTC day
        let today_start = now - (now % 86400);
        let current_count = if session.last_daily_reset < today_start {
            0 // new day, count resets
        } else {
            session.current_daily_count
        };
        current_count < session.daily_tx_count_limit
    }

    /// Record spent amount and increment daily tx count for a session.
    pub fn record_spent(&self, ephemeral_pubkey: &Pubkey, amount: u64) -> Result<()> {
        let key = format!("session:{}", ephemeral_pubkey);
        if let Some(value) = self.db.get(&key)? {
            let value = value;
            if value.len() < 64 {
                return Err(SolanaError::SessionNotFound(key));
            }
            let data_len = value.len() - 64;
            let mut session_data: SessionTokenData = SessionTokenData::deserialize(&mut &value[..data_len])?;

            session_data.current_spent = session_data.current_spent.saturating_add(amount);

            // Update daily tx count with day-rollover handling
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let today_start = now - (now % 86400);
            if session_data.last_daily_reset < today_start {
                session_data.current_daily_count = 1;
                session_data.last_daily_reset = today_start;
            } else {
                session_data.current_daily_count = session_data.current_daily_count.saturating_add(1);
            }

            let keypair_bytes = &value[data_len..];

            let mut new_value = borsh::to_vec(&session_data)?;
            new_value.extend_from_slice(keypair_bytes);

            self.db.insert(key.as_bytes(), new_value)?;
            self.db.flush()?;
        }
        Ok(())
    }

    /// Close a session and remove it from the database.
    pub fn close_session(&self, ephemeral_pubkey: &Pubkey) -> Result<()> {
        let key = format!("session:{}", ephemeral_pubkey);
        self.db.remove(key.as_bytes())?;
        self.db.flush()?;
        Ok(())
    }

    /// Get a reference to the underlying sled database.
    pub fn db(&self) -> &sled::Db {
        &self.db
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::pubkey::Pubkey;

    fn temp_db() -> sled::Db {
        let dir = tempfile::tempdir().unwrap();
        sled::open(dir.path()).unwrap()
    }

    #[test]
    fn test_create_and_get_session() {
        let db = temp_db();
        let mgr = SessionManager::new(db).unwrap();

        let owner = Pubkey::new_unique();
        let target = solana_sdk::system_program::id();

        let session = mgr
            .create_session(
                &owner,
                &target,
                vec!["sol:transfer".into()],
                1_000_000,
                3600,
                0,
                0,
            )
            .unwrap();

        assert_eq!(session.session_data.owner, owner);
        assert_eq!(session.session_data.spending_limit, 1_000_000);
        assert_eq!(session.session_data.current_spent, 0);

        // Retrieve it back
        let loaded = mgr.get_active_session(&owner).unwrap().unwrap();
        assert_eq!(
            loaded.session_data.ephemeral_signer,
            session.keypair.pubkey()
        );
    }

    #[test]
    fn test_spending_limit() {
        let db = temp_db();
        let mgr = SessionManager::new(db).unwrap();

        let owner = Pubkey::new_unique();
        let session = mgr
            .create_session(
                &owner,
                &solana_sdk::system_program::id(),
                vec![],
                1000,
                3600,
                0,
                0,
            )
            .unwrap();

        assert!(mgr.check_spending_limit(&session.session_data, 500));
        assert!(mgr.check_spending_limit(&session.session_data, 1000));
        assert!(!mgr.check_spending_limit(&session.session_data, 1001));
    }

    #[test]
    fn test_record_spent() {
        let db = temp_db();
        let mgr = SessionManager::new(db).unwrap();

        let owner = Pubkey::new_unique();
        let session = mgr
            .create_session(
                &owner,
                &solana_sdk::system_program::id(),
                vec![],
                10_000,
                3600,
                0,
                0,
            )
            .unwrap();

        mgr.record_spent(&session.keypair.pubkey(), 3_000).unwrap();

        let loaded = mgr.get_active_session(&owner).unwrap().unwrap();
        assert_eq!(loaded.session_data.current_spent, 3_000);
    }

    #[test]
    fn test_close_session() {
        let db = temp_db();
        let mgr = SessionManager::new(db).unwrap();

        let owner = Pubkey::new_unique();
        let session = mgr
            .create_session(
                &owner,
                &solana_sdk::system_program::id(),
                vec![],
                1000,
                3600,
                0,
                0,
            )
            .unwrap();

        let pubkey = session.keypair.pubkey();
        mgr.close_session(&pubkey).unwrap();
        assert!(mgr.get_active_session(&owner).unwrap().is_none());
    }

    #[test]
    fn test_expired_session() {
        let db = temp_db();
        let mgr = SessionManager::new(db).unwrap();

        let owner = Pubkey::new_unique();
        // Create session that expires immediately (0 seconds duration)
        let session = mgr
            .create_session(&owner, &solana_sdk::system_program::id(), vec![], 1000, 0, 0, 0)
            .unwrap();

        // Should be expired already
        assert!(mgr.is_expired(&session.session_data));
        // get_active_session should not return it
        assert!(mgr.get_active_session(&owner).unwrap().is_none());
    }

    #[test]
    fn test_persistence_across_restart() {
        let dir = tempfile::tempdir().unwrap();
        let db = sled::open(dir.path()).unwrap();

        let owner = Pubkey::new_unique();
        let pubkey = {
            let mgr = SessionManager::new(db).unwrap();
            let session = mgr
                .create_session(
                    &owner,
                    &solana_sdk::system_program::id(),
                    vec![],
                    5000,
                    3600,
                    0,
                    0,
                )
                .unwrap();
            session.keypair.pubkey()
        };

        // Reopen the same database
        let db2 = sled::open(dir.path()).unwrap();
        let mgr2 = SessionManager::new(db2).unwrap();
        let loaded = mgr2.get_active_session(&owner).unwrap().unwrap();
        assert_eq!(loaded.session_data.ephemeral_signer, pubkey);
    }
}
