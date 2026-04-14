/// Sled-based persistent storage for merchant DID records.
pub struct MerchantStore {
    db: sled::Db,
}

impl MerchantStore {
    pub fn new(db: sled::Db) -> Self {
        Self { db }
    }

    /// Save a merchant record keyed by DID hash.
    pub fn save_merchant(&self, did_hash: &[u8], data: &[u8]) -> anyhow::Result<()> {
        let key = format!("merchant:{}", hex::encode(did_hash));
        self.db.insert(key, data)?;
        Ok(())
    }

    /// Get a merchant record by DID hash.
    pub fn get_merchant(&self, did_hash: &[u8]) -> Option<Vec<u8>> {
        let key = format!("merchant:{}", hex::encode(did_hash));
        self.db.get(key).ok().flatten().map(|ivec| ivec.to_vec())
    }

    /// Save a leaf index mapping: did_hash -> leaf_index.
    pub fn save_leaf_index(&self, did_hash: &[u8], leaf_index: u32) -> anyhow::Result<()> {
        let key = format!("leaf_index:{}", hex::encode(did_hash));
        self.db.insert(key, &leaf_index.to_le_bytes())?;
        Ok(())
    }

    /// Get the leaf index for a DID hash.
    pub fn get_leaf_index(&self, did_hash: &[u8]) -> Option<u32> {
        let key = format!("leaf_index:{}", hex::encode(did_hash));
        let bytes = self.db.get(key).ok().flatten()?;
        if bytes.len() == 4 {
            let mut arr = [0u8; 4];
            arr.copy_from_slice(&bytes);
            Some(u32::from_le_bytes(arr))
        } else {
            None
        }
    }
}
