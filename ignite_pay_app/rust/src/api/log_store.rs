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

use anyhow::Result;
use serde::{Deserialize, Serialize};

use ignite_pay_core::audit_proto::{
    ChunkManifest, TransactionEntry as ProtoTransactionEntry,
};
use ignite_pay_core::ipfs::IpfsClient;
use ignite_pay_core::log_chunk::ChunkConfig;
use ignite_pay_core::log_sync;

/// A single transaction log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionEntry {
    pub nonce: i64,
    pub delta_amount: i64,
    pub cumulative_amount: i64,
    pub signature: String,
    pub timestamp: i64,
    pub service_id: String,
    pub payment_id: String,
    pub merchant_did: String,
    pub synced: bool,
}

/// Local log store backed by SQLite for phone-side transaction persistence.

/// Convert a phone TransactionEntry to a proto TransactionEntry.
fn phone_entry_to_proto(entry: &TransactionEntry) -> ProtoTransactionEntry {
    ProtoTransactionEntry {
        nonce: entry.nonce as u64,
        delta_amount: entry.delta_amount,
        cumulative_amount: entry.cumulative_amount as u64,
        signature: entry.signature.as_bytes().to_vec(),
        timestamp: entry.timestamp,
        service_id: entry.service_id.clone(),
        payment_id: entry.payment_id.clone(),
        merchant_did: entry.merchant_did.clone(),
        memo: vec![],
    }
}

/// Convert a proto TransactionEntry to a phone TransactionEntry.
fn proto_entry_to_phone(entry: &ProtoTransactionEntry) -> TransactionEntry {
    TransactionEntry {
        nonce: entry.nonce as i64,
        delta_amount: entry.delta_amount,
        cumulative_amount: entry.cumulative_amount as i64,
        signature: String::from_utf8_lossy(&entry.signature).to_string(),
        timestamp: entry.timestamp,
        service_id: entry.service_id.clone(),
        payment_id: entry.payment_id.clone(),
        merchant_did: entry.merchant_did.clone(),
        synced: true,
    }
}
pub struct LocalLogStore {
    db: rusqlite::Connection,
}

impl LocalLogStore {
    /// Open (or create) the SQLite database at the given path.
    pub fn open(path: &str) -> Result<Self> {
        let db = rusqlite::Connection::open(path)?;
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS transactions (
                nonce BIGINT PRIMARY KEY,
                delta_amount BIGINT NOT NULL,
                cumulative_amount BIGINT NOT NULL,
                signature TEXT NOT NULL,
                timestamp BIGINT NOT NULL,
                service_id TEXT NOT NULL DEFAULT '',
                payment_id TEXT NOT NULL DEFAULT '',
                merchant_did TEXT NOT NULL DEFAULT '',
                synced BOOLEAN NOT NULL DEFAULT 0
            );"
        )?;
        Ok(Self { db })
    }

    /// Insert a transaction record.
    pub fn record_transaction(&self, entry: &TransactionEntry) -> Result<()> {
        self.db.execute(
            "INSERT INTO transactions (nonce, delta_amount, cumulative_amount, signature, timestamp, service_id, payment_id, merchant_did, synced)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                entry.nonce,
                entry.delta_amount,
                entry.cumulative_amount,
                entry.signature,
                entry.timestamp,
                entry.service_id,
                entry.payment_id,
                entry.merchant_did,
                entry.synced,
            ],
        )?;
        Ok(())
    }

    /// Query the most recent N transactions (newest first).
    pub fn recent_transactions(&self, limit: u32) -> Result<Vec<TransactionEntry>> {
        let mut stmt = self.db.prepare(
            "SELECT nonce, delta_amount, cumulative_amount, signature, timestamp, service_id, payment_id, merchant_did, synced
             FROM transactions ORDER BY nonce DESC LIMIT ?1"
        )?;
        let rows = stmt.query_map(rusqlite::params![limit], |row| {
            Ok(TransactionEntry {
                nonce: row.get(0)?,
                delta_amount: row.get(1)?,
                cumulative_amount: row.get(2)?,
                signature: row.get(3)?,
                timestamp: row.get(4)?,
                service_id: row.get(5)?,
                payment_id: row.get(6)?,
                merchant_did: row.get(7)?,
                synced: row.get(8)?,
            })
        })?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Mark all entries up to (and including) the given nonce as synced.
    pub fn mark_synced(&self, up_to_nonce: i64) -> Result<()> {
        self.db.execute(
            "UPDATE transactions SET synced = 1 WHERE nonce <= ?1",
            rusqlite::params![up_to_nonce],
        )?;
        Ok(())
    }

    /// Export unsynced entries (optionally since a given chunk boundary), oldest first.
    pub fn unsynced_entries(&self, since_nonce: i64) -> Result<Vec<TransactionEntry>> {
        let mut stmt = self.db.prepare(
            "SELECT nonce, delta_amount, cumulative_amount, signature, timestamp, service_id, payment_id, merchant_did, synced
             FROM transactions WHERE synced = 0 AND nonce > ?1 ORDER BY nonce ASC"
        )?;
        let rows = stmt.query_map(rusqlite::params![since_nonce], |row| {
            Ok(TransactionEntry {
                nonce: row.get(0)?,
                delta_amount: row.get(1)?,
                cumulative_amount: row.get(2)?,
                signature: row.get(3)?,
                timestamp: row.get(4)?,
                service_id: row.get(5)?,
                payment_id: row.get(6)?,
                merchant_did: row.get(7)?,
                synced: row.get(8)?,
            })
        })?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Get the highest nonce among synced entries, or 0 if none synced yet.
    pub fn last_synced_nonce(&self) -> Result<i64> {
        let nonce: Option<i64> = self.db.query_row(
            "SELECT MAX(nonce) FROM transactions WHERE synced = 1",
            [],
            |row| row.get(0),
        )?;
        Ok(nonce.unwrap_or(0))
    }

    /// Sync unsynced entries to IPFS via the log_sync pipeline.
    /// Returns (chunk_cid, new_manifest_cid).
    pub async fn sync_to_ipfs(
        &self,
        ipfs: &dyn IpfsClient,
        user_did: &str,
        provider_did: &str,
        log_key: &[u8; 32],
        manifest: &mut ChunkManifest,
    ) -> Result<(String, String)> {
        let last_nonce = self.last_synced_nonce()?;

        let phone_entries = self.unsynced_entries(last_nonce)?;
        if phone_entries.is_empty() {
            return Err(anyhow::anyhow!("no unsynced entries to sync"));
        }

        // Convert phone TransactionEntry → proto TransactionEntry
        let proto_entries: Vec<ProtoTransactionEntry> = phone_entries
            .iter()
            .map(|e| phone_entry_to_proto(e))
            .collect();

        // Determine chunk_id and prev_chunk_hash from manifest
        let chunk_id = if manifest.entries.is_empty() {
            1
        } else {
            manifest.entries.iter().map(|e| e.chunk_id).max().unwrap() + 1
        };
        let prev_chunk_hash = if manifest.entries.is_empty() {
            [0u8; 32]
        } else {
            // Use the last entry's chunk_hash as prev
            let last = manifest.entries.iter().max_by_key(|e| e.chunk_id).unwrap();
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&last.chunk_hash[..32]);
            hash
        };

        let config = ChunkConfig {
            user_did: user_did.to_string(),
            provider_did: provider_did.to_string(),
            chunk_id,
            prev_chunk_hash,
        };

        let end_nonce = phone_entries.last().unwrap().nonce;
        let result =
            log_sync::sync_chunk_to_ipfs(ipfs, &config, &proto_entries, log_key, manifest)
                .await?;

        // Mark local entries as synced
        self.mark_synced(end_nonce)?;

        Ok(result)
    }

    /// Restore all transactions from an IPFS manifest CID into local SQLite.
    pub async fn restore_from_ipfs(
        &self,
        ipfs: &dyn IpfsClient,
        manifest_cid: &str,
        log_key: &[u8; 32],
    ) -> Result<()> {
        let proto_entries =
            log_sync::restore_from_ipfs(ipfs, manifest_cid, log_key).await?;

        for proto_entry in &proto_entries {
            let phone_entry = proto_entry_to_phone(proto_entry);
            self.record_transaction(&phone_entry)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_store() -> LocalLogStore {
        LocalLogStore::open(":memory:").unwrap()
    }

    fn make_entry(nonce: i64, delta: i64, cumulative: i64) -> TransactionEntry {
        TransactionEntry {
            nonce,
            delta_amount: delta,
            cumulative_amount: cumulative,
            signature: format!("sig_{}", nonce),
            timestamp: chrono::Utc::now().timestamp(),
            service_id: "test_service".to_string(),
            payment_id: format!("pay_{}", nonce),
            merchant_did: "did:test:merchant".to_string(),
            synced: false,
        }
    }

    #[test]
    fn test_open_creates_table() {
        let store = make_store();
        // Should not panic — table was created
        let entries = store.recent_transactions(10).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_record_and_query() {
        let store = make_store();

        store.record_transaction(&make_entry(1, -100, 100)).unwrap();
        store.record_transaction(&make_entry(2, -200, 300)).unwrap();
        store.record_transaction(&make_entry(3, -50, 350)).unwrap();

        let entries = store.recent_transactions(10).unwrap();
        assert_eq!(entries.len(), 3);
        // Newest first
        assert_eq!(entries[0].nonce, 3);
        assert_eq!(entries[2].nonce, 1);
    }

    #[test]
    fn test_recent_limit() {
        let store = make_store();

        for i in 1..=5 {
            store.record_transaction(&make_entry(i, -10 * i, 10 * i)).unwrap();
        }

        let entries = store.recent_transactions(3).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].nonce, 5);
    }

    #[test]
    fn test_mark_synced() {
        let store = make_store();

        for i in 1..=5 {
            store.record_transaction(&make_entry(i, -10 * i, 10 * i)).unwrap();
        }

        store.mark_synced(3).unwrap();

        let unsynced = store.unsynced_entries(0).unwrap();
        assert_eq!(unsynced.len(), 2);
        assert_eq!(unsynced[0].nonce, 4);
        assert_eq!(unsynced[1].nonce, 5);
    }

    #[test]
    fn test_unsynced_entries_since_nonce() {
        let store = make_store();

        for i in 1..=5 {
            store.record_transaction(&make_entry(i, -10 * i, 10 * i)).unwrap();
        }

        // All unsynced, starting after nonce 2
        let entries = store.unsynced_entries(2).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].nonce, 3);
    }
}
