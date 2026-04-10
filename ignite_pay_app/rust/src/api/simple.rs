use anyhow::Result;

/// Return type for the DID identity info.
pub struct DidInfo {
    pub did: String,
    pub did_doc_json: String,
}

/// Auth grant returned from payment signing.
pub struct AuthGrant {
    pub merchant_did: String,
    pub amount: u64,
    pub signature: String,
}

/// Get or create a DID identity, persisting it to sled.
/// Returns the DID string and DID document JSON.
pub fn get_or_create_did(storage_path: String) -> Result<DidInfo> {
    let mgr = crate::api::identity::IdentityManager::new(&storage_path)?;
    Ok(DidInfo {
        did: mgr.did().to_string(),
        did_doc_json: serde_json::to_string(mgr.did_doc())?,
    })
}

/// Mock payment signing (placeholder for real signing).
pub async fn sign_payment(merchant_did: String, amount: u64) -> Result<AuthGrant> {
    let mock_signature = format!("sig_of_{}_for_{}", merchant_did, amount);
    Ok(AuthGrant {
        merchant_did,
        amount,
        signature: mock_signature,
    })
}
