//! Bridge layer between ignite-pay-core and ignite-pay-solana.
//! Provides DID-related on-chain operations for merchant verification
//! via ZK compressed accounts.

use anyhow::Result;
use ignite_pay_solana::compression::DidService;

/// Bridge for Solana DID verification operations via ZK compressed accounts.
pub struct SolanaDidBridge {
    did_service: DidService,
}

impl std::fmt::Debug for SolanaDidBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SolanaDidBridge").finish()
    }
}

impl SolanaDidBridge {
    /// Create a new SolanaDidBridge.
    pub fn new(rpc_url: &str, did_program_id: &str) -> Result<Self> {
        let did_service = DidService::new(rpc_url, did_program_id)?;
        Ok(Self { did_service })
    }

    /// Get a reference to the underlying DidService.
    pub fn did_service(&self) -> &DidService {
        &self.did_service
    }

    /// Quick off-chain verification placeholder.
    /// With ZK Compression, verification requires the Photon API.
    pub async fn quick_verify(&self, _merchant_did: &str) -> Result<bool> {
        Ok(true)
    }
}
