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

/// Persistent channel info stored in sled.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelInfo {
    pub channel_id: String,
    pub hub_endpoint: String,
    pub user_pubkey: String,
    pub provider_pubkey: String,
    pub status: String,
    pub sequence: u64,
    pub balance: u64,
    pub total_deposited: u64,
    pub tree_depth: u32,
}

/// Channel store backed by sled.
pub struct ChannelStore {
    db: sled::Db,
}

impl ChannelStore {
    pub fn new(storage_path: &str) -> Result<Self> {
        let db = sled::open(format!("{}/channels", storage_path))?;
        Ok(Self { db })
    }

    fn tree(&self) -> Result<sled::Tree> {
        self.db
            .open_tree("channel_info")
            .map_err(|e| anyhow::anyhow!("Failed to open channel tree: {}", e))
    }

    pub fn save(&self, info: &ChannelInfo) -> Result<()> {
        let tree = self.tree()?;
        let value = serde_json::to_vec(info)?;
        tree.insert(info.channel_id.as_bytes(), value)?;
        tree.flush()?;
        Ok(())
    }

    pub fn get(&self, channel_id: &str) -> Result<Option<ChannelInfo>> {
        let tree = self.tree()?;
        if let Some(bytes) = tree.get(channel_id)? {
            let info: ChannelInfo = serde_json::from_slice(&bytes)?;
            Ok(Some(info))
        } else {
            Ok(None)
        }
    }

    pub fn list(&self) -> Result<Vec<ChannelInfo>> {
        let tree = self.tree()?;
        let mut channels = Vec::new();
        for item in tree.iter() {
            let (_, value) = item?;
            let info: ChannelInfo = serde_json::from_slice(&value)?;
            channels.push(info);
        }
        Ok(channels)
    }

    pub fn find_open(&self) -> Result<Option<ChannelInfo>> {
        let channels = self.list()?;
        Ok(channels
            .into_iter()
            .find(|c| c.status == "Open" || c.status == "open"))
    }

    pub fn update_status(&self, channel_id: &str, status: &str) -> Result<()> {
        if let Some(mut info) = self.get(channel_id)? {
            info.status = status.to_string();
            self.save(&info)?;
        }
        Ok(())
    }

    pub fn delete(&self, channel_id: &str) -> Result<()> {
        let tree = self.tree()?;
        tree.remove(channel_id.as_bytes())?;
        tree.flush()?;
        Ok(())
    }
}
