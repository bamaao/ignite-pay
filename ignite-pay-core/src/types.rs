use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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

/// An entry in the merchant whitelist or blacklist.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MerchantListEntry {
    pub did: String,
    /// Optional human-readable name.
    pub name: Option<String>,
    /// Max single-payment amount allowed (whitelist only).
    pub max_amount: Option<u64>,
    /// When this entry was added.
    pub added_at: DateTime<Utc>,
    // V1.1:
    /// User-assigned tag (e.g. "ShopX Marketplace").
    #[serde(default)]
    pub label: Option<String>,
    /// Entry expiry, None = never expires.
    #[serde(default)]
    pub expires: Option<DateTime<Utc>>,
}

/// Combined whitelist and blacklist structure for IPFS storage.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MerchantLists {
    pub whitelist: Vec<MerchantListEntry>,
    pub blacklist: Vec<MerchantListEntry>,
}

/// Result of a whitelist check.
#[derive(Debug, Clone)]
pub struct WhitelistResult {
    pub is_whitelisted: bool,
    /// The max_amount from the whitelist entry, if applicable.
    pub max_amount: Option<u64>,
    // V1.1:
    /// User-assigned label for this entry.
    pub label: Option<String>,
    /// When this entry expires, None = never expires.
    pub expires_at: Option<DateTime<Utc>>,
}

/// Risk control decision for a payment request (V1.1).
#[derive(Debug, Clone, PartialEq)]
pub enum RiskControlDecision {
    /// Merchant on blacklist (or expired VC).
    Blocked,
    /// Merchant on whitelist + within limit.
    AutoApproved {
        max_amount: Option<u64>,
        label: Option<String>,
    },
    /// Not on any list, proceed to phone auth.
    NeedsAuth,
}

/// A Verifiable Credential for merchant attestation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifiableCredential {
    /// VC context.
    #[serde(rename = "@context")]
    pub context: Vec<String>,
    /// Unique credential ID.
    pub id: String,
    /// Credential type(s).
    #[serde(rename = "type")]
    pub credential_type: Vec<String>,
    /// The DID that issued this credential (platform DID).
    pub issuer: String,
    /// When the credential was issued.
    pub issuance_date: DateTime<Utc>,
    /// When the credential expires.
    pub expiration_date: DateTime<Utc>,
    /// The credential subject (merchant DID + claims).
    pub credential_subject: VCCredentialSubject,
    /// Revocation status reference. Verifiers derive PDA
    /// `seeds = [b"revoked-vc", sha256(vc_json)]` and check on-chain existence.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential_status: Option<CredentialStatus>,
    /// Cryptographic proof.
    pub proof: VCProof,
}

/// Subject of a Verifiable Credential.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VCCredentialSubject {
    /// The merchant's DID.
    pub id: String,
    /// Human-readable merchant name.
    pub name: String,
    /// Merchant category.
    pub category: Option<String>,
}

/// Cryptographic proof attached to a VC.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VCProof {
    pub proof_type: String,
    pub created: DateTime<Utc>,
    pub proof_purpose: String,
    pub verification_method: String,
    /// Base64-encoded Ed25519 signature.
    pub proof_value: String,
}

/// Revocation status reference embedded in a VC.
/// Verifiers compute vc_hash = SHA256(canonical_vc_json), derive PDA
/// `seeds = [b"revoked-vc", vc_hash]` using program_id, and check
/// on-chain existence. If PDA exists, the VC has been revoked.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialStatus {
    #[serde(rename = "type")]
    pub status_type: String,
    /// The DID program ID where the RevokedVc PDA lives.
    pub program_id: String,
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
    fn test_merchant_lists_serde() {
        let lists = MerchantLists::default();
        let json = serde_json::to_string(&lists).unwrap();
        let back: MerchantLists = serde_json::from_str(&json).unwrap();
        assert!(back.whitelist.is_empty());
        assert!(back.blacklist.is_empty());
    }

    #[test]
    fn test_merchant_list_entry_backward_compat() {
        // Old JSON without label/expires should deserialize correctly
        let old_json = r#"{"did":"did:ignite:zTest","name":"Test","max_amount":1000,"added_at":"2024-01-01T00:00:00Z"}"#;
        let entry: MerchantListEntry = serde_json::from_str(old_json).unwrap();
        assert_eq!(entry.did, "did:ignite:zTest");
        assert!(entry.label.is_none());
        assert!(entry.expires.is_none());
    }

    #[test]
    fn test_merchant_list_entry_with_v11_fields() {
        let json = r#"{"did":"did:ignite:zTest","name":"Test","max_amount":1000,"added_at":"2024-01-01T00:00:00Z","label":"ShopX","expires":"2025-12-31T23:59:59Z"}"#;
        let entry: MerchantListEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.label.as_deref(), Some("ShopX"));
        assert!(entry.expires.is_some());
    }
}
