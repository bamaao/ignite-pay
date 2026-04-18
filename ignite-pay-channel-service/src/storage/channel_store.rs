use sled::Db;

/// List all channel IDs stored in the database.
///
/// Scans the `channel:` prefix where ChannelManager stores per-channel metadata.
/// Returns hex-encoded channel IDs.
pub fn list_channel_ids(db: &Db) -> Result<Vec<String>, sled::Error> {
    let prefix = b"channel:";
    let mut ids = Vec::new();
    for item in db.scan_prefix(prefix) {
        let (key, _) = item?;
        // Key format: "channel:{hex_id}:meta"
        if let Ok(key_str) = std::str::from_utf8(&key) {
            if let Some(rest) = key_str.strip_prefix("channel:") {
                // Extract the hex channel_id before any suffix like ":meta"
                if let Some(hex_id) = rest.split(':').next() {
                    if !ids.contains(&hex_id.to_string()) {
                        ids.push(hex_id.to_string());
                    }
                }
            }
        }
    }
    Ok(ids)
}
