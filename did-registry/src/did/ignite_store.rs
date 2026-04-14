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
