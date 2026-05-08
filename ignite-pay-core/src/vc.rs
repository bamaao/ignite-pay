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

use crate::types::{VerifiableCredential, VCProof};
use crate::ipfs::IpfsClient;
use anyhow::Result;
use base64::Engine;
use ed25519_dalek::{VerifyingKey, Signature};

/// Resolve a Verifiable Credential from an IPFS CID.
/// Downloads the JSON from IPFS and deserializes it as a VerifiableCredential.
pub async fn resolve_vc_from_ipfs(ipfs: &dyn IpfsClient, cid: &str) -> Result<VerifiableCredential> {
    let data = ipfs.download(cid).await?;
    let vc: VerifiableCredential = serde_json::from_slice(&data)?;
    Ok(vc)
}

/// Error type for VC verification failures.
#[derive(Debug, thiserror::Error)]
pub enum VCError {
    #[error("Invalid signature: {0}")]
    InvalidSignature(String),
    #[error("VC has expired")]
    Expired,
    #[error("Issuer mismatch: expected {expected}, got {actual}")]
    IssuerMismatch { expected: String, actual: String },
    #[error("Missing proof")]
    MissingProof,
    #[error("Decoding error: {0}")]
    DecodingError(String),
}

impl VerifiableCredential {
    /// Verify a Verifiable Credential's Ed25519 signature and validity.
    ///
    /// Checks:
    /// 1. The VC has not expired
    /// 2. The issuer matches the expected platform DID
    /// 3. The Ed25519 signature is valid
    pub fn verify(&self, platform_verifying_key: &[u8; 32], expected_issuer: &str) -> Result<(), VCError> {
        // Check expiry
        if chrono::Utc::now() > self.expiration_date {
            return Err(VCError::Expired);
        }

        // Check issuer
        if self.issuer != expected_issuer {
            return Err(VCError::IssuerMismatch {
                expected: expected_issuer.to_string(),
                actual: self.issuer.clone(),
            });
        }

        // Verify signature
        let verifying_key = VerifyingKey::from_bytes(platform_verifying_key)
            .map_err(|e| VCError::DecodingError(format!("Invalid verifying key: {:?}", e)))?;

        // Reconstruct the message that was signed (everything except proof)
        let message = self.signing_payload();

        // Decode the proof value
        let proof_bytes = base64::engine::general_purpose::STANDARD_NO_PAD
            .decode(&self.proof.proof_value)
            .map_err(|e| VCError::DecodingError(format!("Base64 decode: {:?}", e)))?;

        if proof_bytes.len() != 64 {
            return Err(VCError::InvalidSignature("Invalid signature length".to_string()));
        }

        let mut sig_bytes = [0u8; 64];
        sig_bytes.copy_from_slice(&proof_bytes);
        let signature = Signature::from_bytes(&sig_bytes);

        verifying_key.verify_strict(&message, &signature)
            .map_err(|e| VCError::InvalidSignature(format!("{:?}", e)))?;

        Ok(())
    }

    /// Create the signing payload (the VC without the proof field).
    /// This is the canonical form that gets signed.
    fn signing_payload(&self) -> Vec<u8> {
        // Create a copy of this VC without the proof
        let mut payload = serde_json::to_value(self).unwrap_or_default();
        payload.as_object_mut().map(|obj| obj.remove("proof"));
        serde_json::to_string(&payload).unwrap_or_default().into_bytes()
    }

    /// Create a new VC with a signature (used by the platform to issue VCs).
    /// `did_program_id` is the on-chain DID program ID used for revocation checks.
    pub fn sign(
        context: Vec<String>,
        id: String,
        credential_type: Vec<String>,
        issuer: String,
        issuance_date: chrono::DateTime<chrono::Utc>,
        expiration_date: chrono::DateTime<chrono::Utc>,
        subject_id: String,
        subject_name: String,
        subject_category: Option<String>,
        signing_key: &ed25519_dalek::SigningKey,
        verification_method: &str,
        did_program_id: &str,
    ) -> Self {
        let credential_subject = crate::types::VCCredentialSubject {
            id: subject_id,
            name: subject_name,
            category: subject_category,
        };

        let credential_status = Some(crate::types::CredentialStatus {
            status_type: "IgniteVcRevocationRegistry".to_string(),
            program_id: did_program_id.to_string(),
        });

        let mut vc = VerifiableCredential {
            context,
            id,
            credential_type,
            issuer: issuer.clone(),
            issuance_date,
            expiration_date,
            credential_subject,
            credential_status,
            proof: VCProof {
                proof_type: String::new(),
                created: chrono::Utc::now(),
                proof_purpose: String::new(),
                verification_method: String::new(),
                proof_value: String::new(),
            },
        };

        // Create signing payload
        let payload = vc.signing_payload();

        // Sign
        use ed25519_dalek::Signer;
        let signature = signing_key.sign(&payload);
        let proof_value = base64::engine::general_purpose::STANDARD_NO_PAD.encode(signature.to_bytes());

        vc.proof = VCProof {
            proof_type: "Ed25519Signature2020".to_string(),
            created: chrono::Utc::now(),
            proof_purpose: "assertionMethod".to_string(),
            verification_method: verification_method.to_string(),
            proof_value,
        };

        vc
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    fn make_test_vc(signing_key: &SigningKey) -> VerifiableCredential {
        let _verifying_key = signing_key.verifying_key();
        let platform_did = "did:ignite:zTestPlatform";

        VerifiableCredential::sign(
            vec!["https://www.w3.org/2018/credentials/v1".to_string()],
            "vc:test:1".to_string(),
            vec!["VerifiableCredential".to_string(), "MerchantAttestation".to_string()],
            platform_did.to_string(),
            chrono::Utc::now() - chrono::Duration::hours(1),
            chrono::Utc::now() + chrono::Duration::hours(24),
            "did:ignite:zTestMerchant".to_string(),
            "Test Shop".to_string(),
            Some("retail".to_string()),
            signing_key,
            &format!("{}#key-signing-1", platform_did),
            "D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D",
        )
    }

    #[test]
    fn test_valid_vc_passes_verification() {
        let signing_key = SigningKey::from_bytes(&[42u8; 32]);
        let verifying_bytes = signing_key.verifying_key().to_bytes();
        let vc = make_test_vc(&signing_key);
        assert!(vc.verify(&verifying_bytes, "did:ignite:zTestPlatform").is_ok());
    }

    #[test]
    fn test_tampered_content_fails() {
        let signing_key = SigningKey::from_bytes(&[42u8; 32]);
        let verifying_bytes = signing_key.verifying_key().to_bytes();
        let mut vc = make_test_vc(&signing_key);
        vc.credential_subject.name = "Tampered Name".to_string();
        assert!(matches!(
            vc.verify(&verifying_bytes, "did:ignite:zTestPlatform"),
            Err(VCError::InvalidSignature(_))
        ));
    }

    #[test]
    fn test_expired_vc_fails() {
        let signing_key = SigningKey::from_bytes(&[42u8; 32]);
        let verifying_bytes = signing_key.verifying_key().to_bytes();
        let mut vc = make_test_vc(&signing_key);
        vc.expiration_date = chrono::Utc::now() - chrono::Duration::hours(1);
        assert!(matches!(
            vc.verify(&verifying_bytes, "did:ignite:zTestPlatform"),
            Err(VCError::Expired)
        ));
    }

    #[test]
    fn test_wrong_issuer_fails() {
        let signing_key = SigningKey::from_bytes(&[42u8; 32]);
        let verifying_bytes = signing_key.verifying_key().to_bytes();
        let vc = make_test_vc(&signing_key);
        assert!(matches!(
            vc.verify(&verifying_bytes, "did:ignite:zWrongPlatform"),
            Err(VCError::IssuerMismatch { .. })
        ));
    }

    #[test]
    fn test_wrong_key_fails() {
        let signing_key = SigningKey::from_bytes(&[42u8; 32]);
        let vc = make_test_vc(&signing_key);
        let wrong_key = SigningKey::from_bytes(&[99u8; 32]);
        let wrong_verifying = wrong_key.verifying_key().to_bytes();
        assert!(matches!(
            vc.verify(&wrong_verifying, "did:ignite:zTestPlatform"),
            Err(VCError::InvalidSignature(_))
        ));
    }
}
