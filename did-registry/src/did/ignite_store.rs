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

use dashmap::DashMap;

/// In-memory cache for DID documents and resolved identities.
#[derive(Debug, Default)]
pub struct IgniteDidStore {
    cache: DashMap<String, serde_json::Value>,
}

impl IgniteDidStore {
    pub fn new() -> Self {
        Self {
            cache: DashMap::new(),
        }
    }

    /// Cache a DID document.
    pub fn cache_did_document(&self, did: &str, doc: serde_json::Value) {
        self.cache.insert(did.to_string(), doc);
    }

    /// Get a cached DID document.
    pub fn get_cached(&self, did: &str) -> Option<serde_json::Value> {
        self.cache.get(did).map(|v| v.value().clone())
    }

    /// Resolve a DID from the local cache.
    pub fn resolve_local(&self, did: &str) -> Option<serde_json::Value> {
        self.get_cached(did)
    }

    /// Remove a cached DID document.
    pub fn remove(&self, did: &str) {
        self.cache.remove(did);
    }
}
