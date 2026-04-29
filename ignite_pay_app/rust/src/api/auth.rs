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
    // V1.0: session key data for on-chain session registration
    pub session_key_pubkey: Option<String>,
    pub session_key_secret_key: Option<String>,
    pub session_key_tx_signature: Option<String>,
    pub session_expires_at: Option<i64>,
    pub spending_limit: Option<u64>,
    pub scopes: Option<Vec<String>>,
    // V1.1: list metadata
    pub list_label: Option<String>,
    pub list_max_amount: Option<u64>,
    // V1.2: merchant policy fields for MCP
    pub daily_tx_count_limit: Option<u32>,
    pub per_tx_limit: Option<u64>,
    // V1.3: SPL token mint for this session
    pub token_mint: Option<String>,
}

/// List action choices for the phone user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ListAction {
    /// No list change.
    None,
    /// Add merchant to whitelist (backward compat alias).
    Whitelist,
    /// Add merchant to blacklist (backward compat alias).
    Blacklist,
    /// Add merchant to whitelist (V1.1).
    AddWhitelist,
    /// Add merchant to blacklist (V1.1).
    AddBlacklist,
    /// Remove merchant from whitelist (V1.1).
    RemoveWhitelist,
    /// Remove merchant from blacklist (V1.1).
    RemoveBlacklist,
}

impl ListAction {
    pub fn as_str(&self) -> &str {
        match self {
            ListAction::None => "none",
            ListAction::Whitelist => "whitelist",
            ListAction::Blacklist => "blacklist",
            ListAction::AddWhitelist => "add_whitelist",
            ListAction::AddBlacklist => "add_blacklist",
            ListAction::RemoveWhitelist => "remove_whitelist",
            ListAction::RemoveBlacklist => "remove_blacklist",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "none" => Some(ListAction::None),
            "whitelist" => Some(ListAction::Whitelist),
            "blacklist" => Some(ListAction::Blacklist),
            "add_whitelist" => Some(ListAction::AddWhitelist),
            "add_blacklist" => Some(ListAction::AddBlacklist),
            "remove_whitelist" => Some(ListAction::RemoveWhitelist),
            "remove_blacklist" => Some(ListAction::RemoveBlacklist),
            _ => None,
        }
    }
}
