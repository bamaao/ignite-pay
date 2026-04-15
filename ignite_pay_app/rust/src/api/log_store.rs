use anyhow::Result;
use serde::{Deserialize, Serialize};

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
