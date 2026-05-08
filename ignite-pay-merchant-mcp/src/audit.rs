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

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Audit log entry for merchant operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub timestamp: DateTime<Utc>,
    pub event_type: String,
    pub order_id: Option<String>,
    pub amount: Option<u64>,
    pub detail: String,
}

/// Persistent audit log backed by sled.
pub struct AuditLogStore {
    db: sled::Db,
}

impl AuditLogStore {
    pub fn from_db(db: sled::Db) -> Self {
        Self { db }
    }

    fn log_tree(&self) -> Result<sled::Tree, anyhow::Error> {
        self.db
            .open_tree("merchant_audit")
            .map_err(|e| anyhow::anyhow!("Failed to open audit tree: {}", e))
    }

    pub fn append(&self, event_type: &str, order_id: Option<&str>, amount: Option<u64>, detail: &str) -> Result<(), anyhow::Error> {
        let tree = self.log_tree()?;
        let entry = AuditEntry {
            timestamp: Utc::now(),
            event_type: event_type.to_string(),
            order_id: order_id.map(String::from),
            amount,
            detail: detail.to_string(),
        };
        let key = format!("{}:{:09}", entry.timestamp.timestamp_micros(), tree.len());
        let value = serde_json::to_vec(&entry)?;
        tree.insert(key.as_bytes(), value)?;
        Ok(())
    }

    pub fn recent(&self, limit: usize) -> Result<Vec<AuditEntry>, anyhow::Error> {
        let tree = self.log_tree()?;
        let mut entries = Vec::new();
        for item in tree.iter().rev() {
            if entries.len() >= limit {
                break;
            }
            let (_, value) = item?;
            let entry: AuditEntry = serde_json::from_slice(&value)?;
            entries.push(entry);
        }
        Ok(entries)
    }
}
