use crate::ipfs::IpfsClient;
use crate::types::{MerchantListEntry, MerchantLists, WhitelistResult};
use anyhow::Result;
use std::sync::Mutex;

const WHITELIST_TREE: &str = "__whitelist__";
const BLACKLIST_TREE: &str = "__blacklist__";

/// Persistent store for merchant whitelist and blacklist.
/// Caches entries locally in sled, syncs with IPFS for cross-device sharing.
#[derive(Debug)]
pub struct ListStore {
    db: sled::Db,
    current_cid: Mutex<Option<String>>,
}

impl ListStore {
    /// Create a new ListStore backed by a sled database.
    pub fn new(db: sled::Db) -> Self {
        Self {
            db,
            current_cid: Mutex::new(None),
        }
    }

    /// Check if a DID is on the blacklist.
    pub fn is_blacklisted(&self, did: &str) -> Result<bool> {
        let tree = self.db.open_tree(BLACKLIST_TREE)?;
        Ok(tree.contains_key(did)?)
    }

    /// Check whitelist status for a DID and amount.
    pub fn check_whitelist(&self, did: &str, amount: u64) -> Result<WhitelistResult> {
        let tree = self.db.open_tree(WHITELIST_TREE)?;
        if let Some(bytes) = tree.get(did)? {
            let entry: MerchantListEntry = serde_json::from_slice(&bytes)?;
            let within_limit = entry.max_amount.map_or(true, |max| amount <= max);
            Ok(WhitelistResult {
                is_whitelisted: within_limit,
                max_amount: entry.max_amount,
            })
        } else {
            Ok(WhitelistResult {
                is_whitelisted: false,
                max_amount: None,
            })
        }
    }

    /// Add an entry to the whitelist.
    pub fn add_to_whitelist(&self, entry: MerchantListEntry) -> Result<()> {
        let tree = self.db.open_tree(WHITELIST_TREE)?;
        let value = serde_json::to_vec(&entry)?;
        tree.insert(&entry.did, value)?;
        tree.flush()?;
        Ok(())
    }

    /// Add an entry to the blacklist.
    pub fn add_to_blacklist(&self, entry: MerchantListEntry) -> Result<()> {
        let tree = self.db.open_tree(BLACKLIST_TREE)?;
        let value = serde_json::to_vec(&entry)?;
        tree.insert(&entry.did, value)?;
        tree.flush()?;
        Ok(())
    }

    /// Remove a DID from the whitelist.
    pub fn remove_from_whitelist(&self, did: &str) -> Result<()> {
        let tree = self.db.open_tree(WHITELIST_TREE)?;
        tree.remove(did)?;
        tree.flush()?;
        Ok(())
    }

    /// Remove a DID from the blacklist.
    pub fn remove_from_blacklist(&self, did: &str) -> Result<()> {
        let tree = self.db.open_tree(BLACKLIST_TREE)?;
        tree.remove(did)?;
        tree.flush()?;
        Ok(())
    }

    /// Get the current IPFS CID for the lists.
    pub fn current_cid(&self) -> Option<String> {
        self.current_cid.lock().unwrap().clone()
    }

    /// Sync lists from an IPFS CID, replacing local cache.
    pub async fn sync_from_ipfs(&self, ipfs: &dyn IpfsClient, cid: &str) -> Result<()> {
        let data = ipfs.download(cid).await?;
        let lists: MerchantLists = serde_json::from_slice(&data)?;

        // Clear and rebuild local trees
        let wl_tree = self.db.open_tree(WHITELIST_TREE)?;
        let bl_tree = self.db.open_tree(BLACKLIST_TREE)?;
        wl_tree.clear()?;
        bl_tree.clear()?;

        for entry in &lists.whitelist {
            let value = serde_json::to_vec(entry)?;
            wl_tree.insert(&entry.did, value)?;
        }
        for entry in &lists.blacklist {
            let value = serde_json::to_vec(entry)?;
            bl_tree.insert(&entry.did, value)?;
        }
        wl_tree.flush()?;
        bl_tree.flush()?;

        *self.current_cid.lock().unwrap() = Some(cid.to_string());
        Ok(())
    }

    /// Upload current lists to IPFS, returning the new CID.
    pub async fn upload_to_ipfs(&self, ipfs: &dyn IpfsClient) -> Result<String> {
        let lists = self.export_lists()?;
        let data = serde_json::to_vec(&lists)?;
        let cid = ipfs.upload(&data).await?;

        *self.current_cid.lock().unwrap() = Some(cid.clone());
        Ok(cid)
    }

    /// Export current lists to MerchantLists struct.
    fn export_lists(&self) -> Result<MerchantLists> {
        let wl_tree = self.db.open_tree(WHITELIST_TREE)?;
        let bl_tree = self.db.open_tree(BLACKLIST_TREE)?;

        let mut whitelist = Vec::new();
        for item in wl_tree.iter() {
            let (_, value) = item?;
            let entry: MerchantListEntry = serde_json::from_slice(&value)?;
            whitelist.push(entry);
        }

        let mut blacklist = Vec::new();
        for item in bl_tree.iter() {
            let (_, value) = item?;
            let entry: MerchantListEntry = serde_json::from_slice(&value)?;
            blacklist.push(entry);
        }

        Ok(MerchantLists {
            whitelist,
            blacklist,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipfs::MockIpfsClient;
    use chrono::Utc;

    fn test_entry(did: &str) -> MerchantListEntry {
        MerchantListEntry {
            did: did.to_string(),
            name: Some(format!("Test {}", did)),
            max_amount: Some(1000),
            added_at: Utc::now(),
        }
    }

    #[test]
    fn test_whitelist_add_check() {
        let dir = tempfile::tempdir().unwrap();
        let db = sled::open(dir.path()).unwrap();
        let store = ListStore::new(db);

        store.add_to_whitelist(test_entry("did:ignite:zMerchant1")).unwrap();

        let result = store.check_whitelist("did:ignite:zMerchant1", 500).unwrap();
        assert!(result.is_whitelisted);
        assert_eq!(result.max_amount, Some(1000));
    }

    #[test]
    fn test_whitelist_amount_exceeded() {
        let dir = tempfile::tempdir().unwrap();
        let db = sled::open(dir.path()).unwrap();
        let store = ListStore::new(db);

        store.add_to_whitelist(test_entry("did:ignite:zMerchant1")).unwrap();

        let result = store.check_whitelist("did:ignite:zMerchant1", 2000).unwrap();
        assert!(!result.is_whitelisted); // over max_amount
    }

    #[test]
    fn test_blacklist_check() {
        let dir = tempfile::tempdir().unwrap();
        let db = sled::open(dir.path()).unwrap();
        let store = ListStore::new(db);

        assert!(!store.is_blacklisted("did:ignite:zBad").unwrap());

        store.add_to_blacklist(test_entry("did:ignite:zBad")).unwrap();
        assert!(store.is_blacklisted("did:ignite:zBad").unwrap());
    }

    #[test]
    fn test_remove_from_whitelist() {
        let dir = tempfile::tempdir().unwrap();
        let db = sled::open(dir.path()).unwrap();
        let store = ListStore::new(db);

        store.add_to_whitelist(test_entry("did:ignite:zMerchant1")).unwrap();
        store.remove_from_whitelist("did:ignite:zMerchant1").unwrap();

        let result = store.check_whitelist("did:ignite:zMerchant1", 100).unwrap();
        assert!(!result.is_whitelisted);
    }

    #[tokio::test]
    async fn test_ipfs_sync_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let db = sled::open(dir.path()).unwrap();
        let store = ListStore::new(db);
        let ipfs = MockIpfsClient::new();

        // Add entries and upload
        store.add_to_whitelist(test_entry("did:ignite:zGood")).unwrap();
        store.add_to_blacklist(test_entry("did:ignite:zBad")).unwrap();

        let cid = store.upload_to_ipfs(&ipfs).await.unwrap();
        assert!(!cid.is_empty());

        // Create a new store and sync from IPFS
        let dir2 = tempfile::tempdir().unwrap();
        let db2 = sled::open(dir2.path()).unwrap();
        let store2 = ListStore::new(db2);

        store2.sync_from_ipfs(&ipfs, &cid).await.unwrap();

        assert!(store2.check_whitelist("did:ignite:zGood", 100).unwrap().is_whitelisted);
        assert!(store2.is_blacklisted("did:ignite:zBad").unwrap());
    }
}
