use serde::{Deserialize, Serialize};
use sled::Db;

use crate::config::Role;

/// Information about a known peer in the network.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    pub pubkey: String,
    pub endpoint: String,
    pub role: Role,
}

/// Store a peer in the database.
pub fn store_peer(db: &Db, peer: &PeerInfo) -> Result<(), sled::Error> {
    let key = format!("peer:{}", peer.pubkey);
    let data = serde_json::to_vec(peer).expect("PeerInfo serialization");
    db.insert(key.as_bytes(), data)?;
    db.flush()?;
    Ok(())
}

/// Load a peer from the database.
pub fn load_peer(db: &Db, pubkey: &str) -> Result<Option<PeerInfo>, sled::Error> {
    let key = format!("peer:{}", pubkey);
    match db.get(key.as_bytes())? {
        Some(data) => {
            let peer: PeerInfo = serde_json::from_slice(&data).expect("PeerInfo deserialization");
            Ok(Some(peer))
        }
        None => Ok(None),
    }
}

/// List all known peers.
pub fn list_peers(db: &Db) -> Result<Vec<PeerInfo>, sled::Error> {
    let prefix = b"peer:";
    let mut peers = Vec::new();
    for item in db.scan_prefix(prefix) {
        let (_, value) = item?;
        if let Ok(peer) = serde_json::from_slice::<PeerInfo>(&value) {
            peers.push(peer);
        }
    }
    Ok(peers)
}
