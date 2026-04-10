use serde::{Deserialize, Serialize};

/// A payment authorization request received from the MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthRequest {
    pub payment_id: String,
    pub merchant_did: String,
    pub amount: u64,
    pub description: String,
}

/// A payment authorization response sent back to the MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthResponse {
    pub payment_id: String,
    pub authorized: bool,
    pub list_action: String,
}

/// List action choices for the phone user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ListAction {
    /// No list change.
    None,
    /// Add merchant to whitelist.
    Whitelist,
    /// Add merchant to blacklist.
    Blacklist,
}

impl ListAction {
    pub fn as_str(&self) -> &str {
        match self {
            ListAction::None => "none",
            ListAction::Whitelist => "whitelist",
            ListAction::Blacklist => "blacklist",
        }
    }
}
