//! Bridge layer between ignite-pay-core and ignite-pay-solana.
//! Provides DID-related on-chain operations for merchant verification.
//!
//! In PDA mode (default), uses standard Solana RPC to query PDA accounts.
//! In ZK Compression mode, uses Photon API to query compressed accounts.

use anyhow::Result;
use ignite_pay_solana::compression::DidService;
use ignite_pay_solana::solana_sdk::pubkey::Pubkey;

use crate::identity::extract_pubkey_from_did;

// ─── PDA version (default) ────────────────────────────────────────────

#[cfg(not(feature = "zk-compression"))]
pub struct SolanaDidBridge {
    did_service: DidService,
}

#[cfg(not(feature = "zk-compression"))]
impl std::fmt::Debug for SolanaDidBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SolanaDidBridge")
            .field("did_program_id", &self.did_service.did_program_id)
            .finish()
    }
}

#[cfg(not(feature = "zk-compression"))]
impl SolanaDidBridge {
    /// Create a new SolanaDidBridge for PDA-based DID verification.
    pub fn new(rpc_url: &str, did_program_id: &str) -> Result<Self> {
        let did_service = DidService::new(rpc_url, did_program_id)?;
        Ok(Self { did_service })
    }

    /// Get a reference to the underlying DidService.
    pub fn did_service(&self) -> &DidService {
        &self.did_service
    }

    /// Verify a merchant DID exists on-chain as a PDA.
    ///
    /// Steps:
    /// 1. Extract Ed25519 public key from `did:ignite:z...`
    /// 2. Derive the merchant PDA via `DidService::derive_merchant_pda`
    /// 3. Query standard Solana RPC `get_account` to check existence
    pub async fn quick_verify(&self, merchant_did: &str) -> Result<bool> {
        let pk_bytes = extract_pubkey_from_did(merchant_did)
            .ok_or_else(|| anyhow::anyhow!("Invalid DID format: {}", merchant_did))?;

        let original_pk = Pubkey::new_from_array(pk_bytes);
        let (pda, _) = self.did_service.derive_merchant_pda(&original_pk);

        // Standard RPC: account exists = DID is registered
        match self.did_service.rpc_client.get_account(&pda) {
            Ok(_) => Ok(true),
            Err(e) => {
                if e.to_string().contains("AccountNotFound") {
                    Ok(false)
                } else {
                    Err(anyhow::anyhow!("RPC error: {}", e))
                }
            }
        }
    }
}

// ─── ZK Compression version (optional) ─────────────────────────────────

#[cfg(feature = "zk-compression")]
use serde_json::json;

#[cfg(feature = "zk-compression")]
/// Bridge for Solana DID verification operations via ZK compressed accounts.
pub struct SolanaDidBridge {
    did_service: DidService,
    photon_url: String,
    address_tree: Pubkey,
}

#[cfg(feature = "zk-compression")]
impl std::fmt::Debug for SolanaDidBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SolanaDidBridge")
            .field("photon_url", &self.photon_url)
            .field("address_tree", &self.address_tree)
            .finish()
    }
}

#[cfg(feature = "zk-compression")]
impl SolanaDidBridge {
    /// Create a new SolanaDidBridge for ZK Compression verification.
    pub fn new(rpc_url: &str, did_program_id: &str, photon_url: &str, address_tree: &str) -> Result<Self> {
        let did_service = DidService::new(rpc_url, did_program_id)?;
        let address_tree = address_tree.parse::<Pubkey>()
            .map_err(|e| anyhow::anyhow!("Invalid address_tree pubkey: {}", e))?;
        Ok(Self {
            did_service,
            photon_url: photon_url.to_string(),
            address_tree,
        })
    }

    /// Get a reference to the underlying DidService.
    pub fn did_service(&self) -> &DidService {
        &self.did_service
    }

    /// Verify a merchant DID exists on-chain as a ZK compressed account.
    pub async fn quick_verify(&self, merchant_did: &str) -> Result<bool> {
        let pk_bytes = extract_pubkey_from_did(merchant_did)
            .ok_or_else(|| anyhow::anyhow!("Invalid DID format: {}", merchant_did))?;

        let original_pk = Pubkey::new_from_array(pk_bytes);

        let (address, _) = self.did_service.derive_compressed_address(&original_pk, &self.address_tree);
        let address_b58 = bs58::encode(address).into_string();

        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}/v1/getCompressedAccount", self.photon_url))
            .json(&json!({ "address": address_b58 }))
            .send()
            .await?;

        if !resp.status().is_success() {
            return Ok(false);
        }

        let body: serde_json::Value = resp.json().await?;
        Ok(body.get("data").is_some())
    }
}
