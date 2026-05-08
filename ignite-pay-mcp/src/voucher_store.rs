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

/// A stored voucher record (persists to sled).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoredVoucher {
    pub channel_id: [u8; 32],
    pub merchant: [u8; 32],
    pub seq: u64,
    pub amount: u64,
    #[serde(with = "sig64_serde")]
    pub buyer_sig: [u8; 64],
}

mod sig64_serde {
    use serde::{self, Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(sig: &[u8; 64], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bytes(sig)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 64], D::Error> {
        let bytes: Vec<u8> = Deserialize::deserialize(d)?;
        let mut arr = [0u8; 64];
        if bytes.len() == 64 {
            arr.copy_from_slice(&bytes);
        }
        Ok(arr)
    }
}

/// Sled-backed voucher storage.
pub struct VoucherStore {
    db: sled::Db,
}

impl VoucherStore {
    pub fn new(db: sled::Db) -> Self {
        Self { db }
    }

    fn vouchers_tree(&self) -> Result<sled::Tree> {
        Ok(self.db.open_tree("vouchers")?)
    }

    /// Store a voucher, keyed by `channel_id || seq` (big-endian).
    pub fn store_voucher(&self, voucher: &StoredVoucher) -> Result<()> {
        let tree = self.vouchers_tree()?;
        let mut key = Vec::with_capacity(40);
        key.extend_from_slice(&voucher.channel_id);
        key.extend_from_slice(&voucher.seq.to_be_bytes());
        let value = serde_json::to_vec(voucher)?;
        tree.insert(&key, value)?;
        tree.flush()?;
        Ok(())
    }

    /// Get all vouchers for a given channel, sorted by seq ascending.
    pub fn get_vouchers_for_channel(&self, channel_id: &[u8; 32]) -> Result<Vec<StoredVoucher>> {
        let tree = self.vouchers_tree()?;
        let prefix = channel_id;

        let mut vouchers = Vec::new();
        for item in tree.scan_prefix(prefix) {
            let (_, value) = item?;
            let v: StoredVoucher = serde_json::from_slice(&value)?;
            vouchers.push(v);
        }

        vouchers.sort_by_key(|v| v.seq);
        Ok(vouchers)
    }

    /// Sum of all outstanding (unsettled) voucher amounts across all channels.
    pub fn total_outstanding(&self) -> Result<u64> {
        let tree = self.vouchers_tree()?;
        let mut total: u64 = 0;
        for item in tree.iter() {
            let (_, value) = item?;
            let v: StoredVoucher = serde_json::from_slice(&value)?;
            total = total.saturating_add(v.amount);
        }
        Ok(total)
    }
}
