use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::oneshot;

/// Status of a payment request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PaymentStatus {
    PendingAuth,
    Authorized,
    Executed,
    Rejected,
    Expired,
}

impl std::fmt::Display for PaymentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PaymentStatus::PendingAuth => write!(f, "pending_auth"),
            PaymentStatus::Authorized => write!(f, "authorized"),
            PaymentStatus::Executed => write!(f, "executed"),
            PaymentStatus::Rejected => write!(f, "rejected"),
            PaymentStatus::Expired => write!(f, "expired"),
        }
    }
}

/// A payment request record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentRequest {
    pub id: String,
    /// Wallet address for payment routing.
    pub recipient: String,
    /// Merchant's did:ignite (from provider_did in 402 response).
    pub merchant_did: String,
    pub amount: u64,
    pub token: String,
    pub network: String,
    pub description: String,
    pub status: PaymentStatus,
    pub created_at: DateTime<Utc>,
    pub tx_signature: Option<String>,
}

/// Persistent payment store backed by sled.
#[derive(Debug)]
pub struct PaymentStore {
    db: sled::Db,
}

impl PaymentStore {
    pub fn new(path: &str) -> Result<Self, anyhow::Error> {
        let db = sled::open(path)?;
        Ok(Self { db })
    }

    /// Create PaymentStore from an existing sled::Db (shared with identity persistence).
    pub fn from_db(db: sled::Db) -> Self {
        Self { db }
    }

    /// Get a reference to the underlying sled database.
    pub fn get_db(&self) -> sled::Db {
        self.db.clone()
    }

    pub fn save_payment(&self, payment: &PaymentRequest) -> Result<(), anyhow::Error> {
        let key = payment.id.as_bytes();
        let value = serde_json::to_vec(payment)?;
        self.db.insert(key, value)?;
        self.db.flush()?;
        Ok(())
    }

    pub fn get_payment(&self, id: &str) -> Result<Option<PaymentRequest>, anyhow::Error> {
        if let Some(bytes) = self.db.get(id)? {
            let payment: PaymentRequest = serde_json::from_slice(&bytes)?;
            Ok(Some(payment))
        } else {
            Ok(None)
        }
    }

    pub fn list_payments(&self, limit: usize) -> Result<Vec<PaymentRequest>, anyhow::Error> {
        let mut payments = Vec::new();
        for item in self.db.iter().rev() {
            if payments.len() >= limit {
                break;
            }
            let (key, value) = item?;
            // Skip internal keys (like __identity__)
            if key.starts_with(b"__") {
                continue;
            }
            let payment: PaymentRequest = serde_json::from_slice(&value)?;
            payments.push(payment);
        }
        Ok(payments)
    }

    pub fn update_status(&self, id: &str, status: &PaymentStatus) -> Result<(), anyhow::Error> {
        if let Some(bytes) = self.db.get(id)? {
            let mut payment: PaymentRequest = serde_json::from_slice(&bytes)?;
            payment.status = status.clone();
            self.save_payment(&payment)?;
        }
        Ok(())
    }

    pub fn set_tx_signature(&self, id: &str, tx_sig: &str) -> Result<(), anyhow::Error> {
        if let Some(bytes) = self.db.get(id)? {
            let mut payment: PaymentRequest = serde_json::from_slice(&bytes)?;
            payment.tx_signature = Some(tx_sig.to_string());
            self.save_payment(&payment)?;
        }
        Ok(())
    }
}

/// Result of an authorization response from the phone.
#[derive(Debug, Clone)]
pub struct AuthResponse {
    pub authorized: bool,
    pub list_action: String,
    pub merchant_did: String,
    // V1.0: session key data from phone
    pub session_key_pubkey: Option<String>,
    pub session_key_secret_key: Option<String>,
    pub session_key_tx_signature: Option<String>,
    pub session_expires_at: Option<i64>,
    pub spending_limit: Option<u64>,
    pub scopes: Option<Vec<String>>,
    // V1.1: list metadata
    pub list_label: Option<String>,
    pub list_max_amount: Option<u64>,
}

/// In-memory store for pending authorization requests with oneshot channels.
/// Allows the receive loop to resolve waiting tool calls.
#[derive(Debug)]
pub struct PendingAuthStore {
    pending: Arc<DashMap<String, oneshot::Sender<AuthResponse>>>,
}

impl PendingAuthStore {
    pub fn new() -> Self {
        Self {
            pending: Arc::new(DashMap::new()),
        }
    }

    /// Register a new pending auth request. Returns a oneshot receiver that
    /// will be resolved when the phone responds (or times out).
    pub fn register(&self, payment_id: &str) -> oneshot::Receiver<AuthResponse> {
        let (tx, rx) = oneshot::channel();
        self.pending.insert(payment_id.to_string(), tx);
        rx
    }

    /// Resolve a pending auth request with full response data.
    /// Returns true if the payment_id was found.
    pub fn resolve(&self, payment_id: &str, response: AuthResponse) -> bool {
        if let Some((_, tx)) = self.pending.remove(payment_id) {
            let _ = tx.send(response);
            true
        } else {
            false
        }
    }
}

/// Execute a mock payment and return a fake transaction signature.
pub fn execute_mock_payment(payment: &PaymentRequest) -> String {
    format!("tx_mock_{}_{}", payment.id, uuid::Uuid::new_v4())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_payment_status_display() {
        assert_eq!(PaymentStatus::PendingAuth.to_string(), "pending_auth");
        assert_eq!(PaymentStatus::Executed.to_string(), "executed");
    }

    #[test]
    fn test_mock_payment() {
        let payment = PaymentRequest {
            id: "test-123".to_string(),
            recipient: "wallet_addr_abc".to_string(),
            merchant_did: "did:ignite:zTest".to_string(),
            amount: 100,
            token: "USDC".to_string(),
            network: "solana".to_string(),
            description: "test payment".to_string(),
            status: PaymentStatus::Authorized,
            created_at: Utc::now(),
            tx_signature: None,
        };
        let tx = execute_mock_payment(&payment);
        assert!(tx.starts_with("tx_mock_test-123_"));
    }

    #[test]
    fn test_pending_auth_store() {
        let store = PendingAuthStore::new();
        let rx = store.register("pay-1");
        assert!(store.resolve(
            "pay-1",
            AuthResponse {
                authorized: true,
                list_action: "none".to_string(),
                merchant_did: String::new(),
                session_key_pubkey: None,
                session_key_secret_key: None,
                session_key_tx_signature: None,
                session_expires_at: None,
                spending_limit: None,
                scopes: None,
                list_label: None,
                list_max_amount: None,
            }
        ));
        let resp = rx.blocking_recv().unwrap();
        assert!(resp.authorized);
        assert!(!store.resolve(
            "pay-1",
            AuthResponse {
                authorized: false,
                list_action: "none".to_string(),
                merchant_did: String::new(),
                session_key_pubkey: None,
                session_key_secret_key: None,
                session_key_tx_signature: None,
                session_expires_at: None,
                spending_limit: None,
                scopes: None,
                list_label: None,
                list_max_amount: None,
            }
        ));
    }
}
