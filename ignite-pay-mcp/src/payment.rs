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
    // V1.2: SPL token mint for this session
    pub token_mint: Option<String>,
    // V1.3: payment method chosen by user (session_key, magicblock, relayer)
    pub payment_method: Option<String>,
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

/// Result of a session fund response from the phone.
#[derive(Debug, Clone)]
pub struct FundResponse {
    pub funded: bool,
    pub new_balance: u64,
    pub tx_signature: Option<String>,
}

/// In-memory store for pending session fund requests with oneshot channels.
#[derive(Debug)]
pub struct PendingFundStore {
    pending: Arc<DashMap<String, oneshot::Sender<FundResponse>>>,
}

impl PendingFundStore {
    pub fn new() -> Self {
        Self {
            pending: Arc::new(DashMap::new()),
        }
    }

    /// Register a new pending fund request. Returns a oneshot receiver.
    pub fn register(&self, session_key_pubkey: &str) -> oneshot::Receiver<FundResponse> {
        let (tx, rx) = oneshot::channel();
        self.pending.insert(session_key_pubkey.to_string(), tx);
        rx
    }

    /// Resolve a pending fund request.
    pub fn resolve(&self, session_key_pubkey: &str, response: FundResponse) -> bool {
        if let Some((_, tx)) = self.pending.remove(session_key_pubkey) {
            let _ = tx.send(response);
            true
        } else {
            false
        }
    }
}

/// Result of a session renew response from the phone.
#[derive(Debug, Clone)]
pub struct RenewResponse {
    pub renewed: bool,
    pub new_session_key_pubkey: String,
    pub tx_signature: Option<String>,
}

/// In-memory store for pending session renew requests with oneshot channels.
#[derive(Debug)]
pub struct PendingRenewStore {
    pending: Arc<DashMap<String, oneshot::Sender<RenewResponse>>>,
}

impl PendingRenewStore {
    pub fn new() -> Self {
        Self {
            pending: Arc::new(DashMap::new()),
        }
    }

    /// Register a new pending renew request. Returns a oneshot receiver.
    pub fn register(&self, old_session_key_pubkey: &str) -> oneshot::Receiver<RenewResponse> {
        let (tx, rx) = oneshot::channel();
        self.pending.insert(old_session_key_pubkey.to_string(), tx);
        rx
    }

    /// Resolve a pending renew request.
    pub fn resolve(&self, old_session_key_pubkey: &str, response: RenewResponse) -> bool {
        if let Some((_, tx)) = self.pending.remove(old_session_key_pubkey) {
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
        assert_eq!(PaymentStatus::Authorized.to_string(), "authorized");
        assert_eq!(PaymentStatus::Executed.to_string(), "executed");
        assert_eq!(PaymentStatus::Rejected.to_string(), "rejected");
        assert_eq!(PaymentStatus::Expired.to_string(), "expired");
    }

    #[test]
    fn test_payment_status_serde_roundtrip() {
        let statuses = vec![
            PaymentStatus::PendingAuth,
            PaymentStatus::Authorized,
            PaymentStatus::Executed,
            PaymentStatus::Rejected,
            PaymentStatus::Expired,
        ];
        for s in &statuses {
            let json = serde_json::to_string(s).unwrap();
            let back: PaymentStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(*s, back);
        }
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
        // Each call should produce a unique tx signature
        let tx2 = execute_mock_payment(&payment);
        assert_ne!(tx, tx2);
    }

    #[test]
    fn test_pending_auth_store_resolve() {
        let store = PendingAuthStore::new();
        let rx = store.register("pay-1");
        assert!(store.resolve("pay-1", AuthResponse {
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
            token_mint: None,
            payment_method: None,
        }));
        let resp = rx.blocking_recv().unwrap();
        assert!(resp.authorized);
    }

    #[test]
    fn test_pending_auth_store_double_resolve() {
        let store = PendingAuthStore::new();
        let _rx = store.register("pay-1");
        // First resolve succeeds
        assert!(store.resolve("pay-1", AuthResponse {
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
            token_mint: None,
            payment_method: None,
        }));
        // Second resolve returns false (already consumed)
        assert!(!store.resolve("pay-1", AuthResponse {
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
            token_mint: None,
            payment_method: None,
        }));
    }

    #[test]
    fn test_pending_auth_store_unknown_id() {
        let store = PendingAuthStore::new();
        assert!(!store.resolve("nonexistent", AuthResponse {
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
            token_mint: None,
            payment_method: None,
        }));
    }

    #[test]
    fn test_pending_auth_store_multiple_pending() {
        let store = PendingAuthStore::new();
        let rx1 = store.register("pay-1");
        let mut rx2 = store.register("pay-2");
        let rx3 = store.register("pay-3");

        // Resolve out of order
        assert!(store.resolve("pay-3", AuthResponse {
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
            token_mint: None,
            payment_method: None,
        }));
        assert!(store.resolve("pay-1", AuthResponse {
            authorized: false,
            list_action: "whitelist".to_string(),
            merchant_did: "did:ignite:zMerchant".to_string(),
            session_key_pubkey: None,
            session_key_secret_key: None,
            session_key_tx_signature: None,
            session_expires_at: None,
            spending_limit: None,
            scopes: None,
            list_label: None,
            list_max_amount: None,
            token_mint: None,
            payment_method: None,
        }));

        assert!(!rx1.blocking_recv().unwrap().authorized);
        assert!(rx3.blocking_recv().unwrap().authorized);
        // rx2 is still pending
        assert!(rx2.try_recv().is_err());
    }

    #[test]
    fn test_payment_store_crud() {
        let dir = tempfile::tempdir().unwrap();
        let store = PaymentStore::new(dir.path().to_str().unwrap()).unwrap();

        let payment = PaymentRequest {
            id: "crud-1".to_string(),
            recipient: "wallet_xyz".to_string(),
            merchant_did: "did:ignite:zMerchant".to_string(),
            amount: 5000,
            token: "SOL".to_string(),
            network: "solana".to_string(),
            description: "test crud".to_string(),
            status: PaymentStatus::PendingAuth,
            created_at: Utc::now(),
            tx_signature: None,
        };

        // Save
        store.save_payment(&payment).unwrap();

        // Get
        let loaded = store.get_payment("crud-1").unwrap().unwrap();
        assert_eq!(loaded.id, "crud-1");
        assert_eq!(loaded.amount, 5000);
        assert_eq!(loaded.status, PaymentStatus::PendingAuth);
        assert!(loaded.tx_signature.is_none());

        // Update status
        store.update_status("crud-1", &PaymentStatus::Executed).unwrap();
        let loaded = store.get_payment("crud-1").unwrap().unwrap();
        assert_eq!(loaded.status, PaymentStatus::Executed);

        // Set tx signature
        store.set_tx_signature("crud-1", "sig_abc123").unwrap();
        let loaded = store.get_payment("crud-1").unwrap().unwrap();
        assert_eq!(loaded.tx_signature.as_deref(), Some("sig_abc123"));

        // Get nonexistent
        assert!(store.get_payment("nonexistent").unwrap().is_none());
    }

    #[test]
    fn test_payment_store_list() {
        let dir = tempfile::tempdir().unwrap();
        let store = PaymentStore::new(dir.path().to_str().unwrap()).unwrap();

        // Create 5 payments
        for i in 0..5 {
            let payment = PaymentRequest {
                id: format!("list-{}", i),
                recipient: format!("wallet_{}", i),
                merchant_did: "did:ignite:zMerchant".to_string(),
                amount: i * 100,
                token: "USDC".to_string(),
                network: "solana".to_string(),
                description: format!("payment {}", i),
                status: PaymentStatus::PendingAuth,
                created_at: Utc::now(),
                tx_signature: None,
            };
            store.save_payment(&payment).unwrap();
        }

        // List with limit
        let all = store.list_payments(10).unwrap();
        assert_eq!(all.len(), 5);

        let limited = store.list_payments(3).unwrap();
        assert_eq!(limited.len(), 3);
    }

    #[test]
    fn test_auth_response_with_session_data() {
        let store = PendingAuthStore::new();
        let rx = store.register("sess-1");
        assert!(store.resolve("sess-1", AuthResponse {
            authorized: true,
            list_action: "whitelist".to_string(),
            merchant_did: "did:ignite:zMerchant".to_string(),
            session_key_pubkey: Some("base58_pubkey".to_string()),
            session_key_secret_key: Some("base58_secret".to_string()),
            session_key_tx_signature: Some("sig123".to_string()),
            session_expires_at: Some(1700000000),
            spending_limit: Some(100_000),
            scopes: Some(vec!["sol:transfer".to_string()]),
            list_label: Some("trusted".to_string()),
            list_max_amount: Some(50_000),
            token_mint: None,
            payment_method: None,
        }));

        let resp = rx.blocking_recv().unwrap();
        assert!(resp.authorized);
        assert_eq!(resp.list_action, "whitelist");
        assert_eq!(resp.merchant_did, "did:ignite:zMerchant");
        assert_eq!(resp.session_key_pubkey.as_deref(), Some("base58_pubkey"));
        assert_eq!(resp.session_key_secret_key.as_deref(), Some("base58_secret"));
        assert_eq!(resp.spending_limit, Some(100_000));
        assert_eq!(resp.scopes.as_deref(), Some(&["sol:transfer".to_string()][..]));
        assert_eq!(resp.list_label.as_deref(), Some("trusted"));
        assert_eq!(resp.list_max_amount, Some(50_000));
    }
}
