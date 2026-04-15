use serde::{Deserialize, Serialize};
use serde_json::Value;

const AUDIT_TREE: &str = "__audit_log__";

/// A single audit log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub ts: i64,
    pub event: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payment_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merchant_did: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub did: Option<String>,
    #[serde(flatten)]
    pub extra: Value,
}

/// Persistent audit log store backed by sled.
#[derive(Debug)]
pub struct AuditLogStore {
    db: sled::Db,
}

impl AuditLogStore {
    /// Create AuditLogStore from an existing sled::Db (shared with other stores).
    pub fn from_db(db: sled::Db) -> Self {
        Self { db }
    }

    /// Record an authorization event (auth request sent / response received).
    pub fn record_auth_event(
        &self,
        payment_id: &str,
        event_type: &str,
        metadata: Value,
    ) -> anyhow::Result<()> {
        let entry = AuditEntry {
            ts: chrono::Utc::now().timestamp_millis(),
            event: event_type.to_string(),
            payment_id: Some(payment_id.to_string()),
            merchant_did: None,
            amount: None,
            list_type: None,
            action: None,
            did: None,
            extra: metadata,
        };
        self.insert(&entry)
    }

    /// Record a payment execution event.
    pub fn record_payment_event(
        &self,
        payment_id: &str,
        event_type: &str,
        amount: u64,
        merchant_did: &str,
    ) -> anyhow::Result<()> {
        let entry = AuditEntry {
            ts: chrono::Utc::now().timestamp_millis(),
            event: event_type.to_string(),
            payment_id: Some(payment_id.to_string()),
            merchant_did: Some(merchant_did.to_string()),
            amount: Some(amount),
            list_type: None,
            action: None,
            did: None,
            extra: Value::Null,
        };
        self.insert(&entry)
    }

    /// Record a whitelist/blacklist change event.
    pub fn record_list_event(
        &self,
        list_type: &str,
        action: &str,
        did: &str,
    ) -> anyhow::Result<()> {
        let entry = AuditEntry {
            ts: chrono::Utc::now().timestamp_millis(),
            event: "list_updated".to_string(),
            payment_id: None,
            merchant_did: None,
            amount: None,
            list_type: Some(list_type.to_string()),
            action: Some(action.to_string()),
            did: Some(did.to_string()),
            extra: Value::Null,
        };
        self.insert(&entry)
    }

    /// Query audit logs within a time range, returning up to `limit` entries (newest first).
    pub fn query(&self, from_ts: i64, to_ts: i64, limit: usize) -> anyhow::Result<Vec<AuditEntry>> {
        let tree = self.db.open_tree(AUDIT_TREE)?;
        let mut results = Vec::new();

        // Keys are formatted as "{timestamp_ms}:{uuid}" — lexicographic order = chronological.
        // Scan from the end (newest) backwards.
        for item in tree.iter().rev() {
            if results.len() >= limit {
                break;
            }
            let (_, value) = item?;
            let entry: AuditEntry = serde_json::from_slice(&value)?;
            if entry.ts < from_ts {
                break;
            }
            if entry.ts <= to_ts {
                results.push(entry);
            }
        }

        Ok(results)
    }

    fn insert(&self, entry: &AuditEntry) -> anyhow::Result<()> {
        let tree = self.db.open_tree(AUDIT_TREE)?;
        let key = format!("{}:{}", entry.ts, uuid::Uuid::new_v4());
        let value = serde_json::to_vec(entry)?;
        tree.insert(key.as_bytes(), value)?;
        tree.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_store() -> (TempDir, AuditLogStore) {
        let dir = TempDir::new().unwrap();
        let db = sled::open(dir.path()).unwrap();
        let store = AuditLogStore::from_db(db);
        (dir, store)
    }

    #[test]
    fn test_record_and_query_auth_event() {
        let (_dir, store) = make_store();

        store
            .record_auth_event("pay_001", "auth_request_sent", serde_json::json!({"phone_did": "did:test:phone"}))
            .unwrap();

        let entries = store.query(0, i64::MAX, 10).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].event, "auth_request_sent");
        assert_eq!(entries[0].payment_id.as_deref(), Some("pay_001"));
    }

    #[test]
    fn test_record_payment_event() {
        let (_dir, store) = make_store();

        store
            .record_payment_event("pay_002", "payment_executed", 5000, "did:test:merchant")
            .unwrap();

        let entries = store.query(0, i64::MAX, 10).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].event, "payment_executed");
        assert_eq!(entries[0].amount, Some(5000));
        assert_eq!(entries[0].merchant_did.as_deref(), Some("did:test:merchant"));
    }

    #[test]
    fn test_record_list_event() {
        let (_dir, store) = make_store();

        store
            .record_list_event("whitelist", "add_whitelist", "did:test:merchant")
            .unwrap();

        let entries = store.query(0, i64::MAX, 10).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].event, "list_updated");
        assert_eq!(entries[0].list_type.as_deref(), Some("whitelist"));
        assert_eq!(entries[0].action.as_deref(), Some("add_whitelist"));
    }

    #[test]
    fn test_query_time_range_filtering() {
        let (_dir, store) = make_store();

        // Record two events with a small gap
        let base_ts = chrono::Utc::now().timestamp_millis();

        // Manually insert with known timestamps
        let entry1 = AuditEntry {
            ts: base_ts - 2000,
            event: "event_a".to_string(),
            payment_id: None,
            merchant_did: None,
            amount: None,
            list_type: None,
            action: None,
            did: None,
            extra: Value::Null,
        };
        store.insert(&entry1).unwrap();

        let entry2 = AuditEntry {
            ts: base_ts,
            event: "event_b".to_string(),
            payment_id: None,
            merchant_did: None,
            amount: None,
            list_type: None,
            action: None,
            did: None,
            extra: Value::Null,
        };
        store.insert(&entry2).unwrap();

        // Query only the recent one
        let results = store.query(base_ts - 1000, base_ts + 1000, 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].event, "event_b");

        // Query both
        let all = store.query(0, i64::MAX, 10).unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_query_limit() {
        let (_dir, store) = make_store();

        for i in 0..5 {
            store
                .record_auth_event(&format!("pay_{i}"), "test_event", Value::Null)
                .unwrap();
        }

        let limited = store.query(0, i64::MAX, 3).unwrap();
        assert_eq!(limited.len(), 3);
    }
}
