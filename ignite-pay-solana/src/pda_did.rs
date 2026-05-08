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

//! PDA-based DidService for interacting with ignite-pay-did-program.
//!
//! Uses standard Solana PDAs instead of ZK Compression.
//! Seeds: [b"merchant-did", original_pk]

use crate::error::{Result, SolanaError};
use solana_client::rpc_client::RpcClient;
use solana_sdk::hash::hash;
use solana_sdk::instruction::{AccountMeta, Instruction};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signature};
use solana_sdk::signer::Signer;
use solana_sdk::transaction::Transaction;

/// On-chain PDA account data layout (mirrors ignite-pay-did-program MerchantDidAccount).
/// Space = 8(discriminator) + 32 + 32 + 32 + 32 + 8 + 8 + 1 = 153 bytes.
#[derive(Debug, Clone, borsh::BorshSerialize, borsh::BorshDeserialize)]
pub struct MerchantDidAccountOnchain {
    pub original_pk: Pubkey,
    pub controller_pk: Pubkey,
    pub recovery_pk: Pubkey,
    pub vc_hash: [u8; 32],
    pub last_updated: i64,
    pub nonce: u64,
    pub bump: u8,
}

/// Compute the Anchor instruction discriminator: sha256("global:<name>")[..8]
fn anchor_sighash(name: &str) -> [u8; 8] {
    let preimage = format!("global:{}", name);
    let h = hash(preimage.as_bytes());
    let mut disc = [0u8; 8];
    disc.copy_from_slice(&h.to_bytes()[..8]);
    disc
}

/// Service for interacting with the ignite-pay-did-program using standard PDA accounts.
pub struct DidService {
    pub rpc_client: RpcClient,
    pub did_program_id: Pubkey,
}

impl std::fmt::Debug for DidService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DidService")
            .field("did_program_id", &self.did_program_id)
            .finish()
    }
}

impl DidService {
    /// Create a new DidService.
    pub fn new(rpc_url: &str, did_program_id: &str) -> Result<Self> {
        let did_program_id = did_program_id
            .parse::<Pubkey>()
            .map_err(|e| SolanaError::InvalidPubkey(e.to_string()))?;
        Ok(Self {
            rpc_client: RpcClient::new(rpc_url.to_string()),
            did_program_id,
        })
    }

    /// Derive the merchant DID PDA address for a given original public key.
    /// Seeds: [b"merchant-did", original_pk]
    pub fn derive_merchant_pda(&self, original_pk: &Pubkey) -> (Pubkey, u8) {
        Pubkey::find_program_address(
            &[b"merchant-did", original_pk.as_ref()],
            &self.did_program_id,
        )
    }

    /// Read a merchant DID PDA account from chain.
    pub fn read_merchant_did(&self, original_pk: &Pubkey) -> Result<Option<MerchantDidAccountOnchain>> {
        let (pda, _) = self.derive_merchant_pda(original_pk);
        let account_data = match self.rpc_client.get_account_data(&pda) {
            Ok(data) => data,
            Err(e) => {
                if e.to_string().contains("AccountNotFound") {
                    return Ok(None);
                }
                return Err(SolanaError::RpcError(e.to_string()));
            }
        };

        // Skip 8-byte discriminator
        if account_data.len() < 8 {
            return Ok(None);
        }
        let onchain: MerchantDidAccountOnchain =
            borsh::BorshDeserialize::deserialize(&mut &account_data[8..])
                .map_err(SolanaError::BorshError)?;
        Ok(Some(onchain))
    }

    /// Derive the PlatformConfig PDA address.
    pub fn derive_platform_config_pda(&self) -> Pubkey {
        Pubkey::find_program_address(
            &[b"platform-config"],
            &self.did_program_id,
        ).0
    }

    // ── Private instruction builders ──────────────────────────────────

    fn build_initialize_did_ix(
        &self,
        signer_pubkey: &Pubkey,
        merchant_pda: &Pubkey,
        platform_config: &Pubkey,
        vc_hash: [u8; 32],
        platform_signature: [u8; 64],
        credential_subject_pk: &Pubkey,
    ) -> Instruction {
        let mut data = Vec::with_capacity(8 + 32 + 64 + 32);
        data.extend_from_slice(&anchor_sighash("initialize_did"));
        data.extend_from_slice(&vc_hash);
        data.extend_from_slice(&platform_signature);
        data.extend_from_slice(credential_subject_pk.as_ref());

        let accounts = vec![
            AccountMeta::new(*signer_pubkey, true),
            AccountMeta::new(*merchant_pda, false),
            AccountMeta::new_readonly(*platform_config, false),
            AccountMeta::new_readonly(solana_sdk::system_program::id(), false),
        ];

        Instruction {
            program_id: self.did_program_id,
            accounts,
            data,
        }
    }

    fn build_update_did_with_vc_ix(
        &self,
        signer_pubkey: &Pubkey,
        merchant_pda: &Pubkey,
        platform_config: &Pubkey,
        vc_hash: [u8; 32],
        nonce: u64,
        platform_signature: [u8; 64],
        credential_subject_pk: &Pubkey,
    ) -> Instruction {
        let mut data = Vec::with_capacity(8 + 32 + 8 + 64 + 32);
        data.extend_from_slice(&anchor_sighash("update_did_with_vc"));
        data.extend_from_slice(&vc_hash);
        data.extend_from_slice(&nonce.to_le_bytes());
        data.extend_from_slice(&platform_signature);
        data.extend_from_slice(credential_subject_pk.as_ref());

        let accounts = vec![
            AccountMeta::new(*signer_pubkey, true),
            AccountMeta::new(*merchant_pda, false),
            AccountMeta::new_readonly(*platform_config, false),
        ];

        Instruction {
            program_id: self.did_program_id,
            accounts,
            data,
        }
    }

    fn build_set_recovery_key_ix(
        &self,
        signer_pubkey: &Pubkey,
        merchant_pda: &Pubkey,
        recovery_pk: &Pubkey,
        nonce: u64,
    ) -> Instruction {
        let mut data = Vec::with_capacity(8 + 32 + 8);
        data.extend_from_slice(&anchor_sighash("set_recovery_key"));
        data.extend_from_slice(recovery_pk.as_ref());
        data.extend_from_slice(&nonce.to_le_bytes());

        let accounts = vec![
            AccountMeta::new(*signer_pubkey, true),
            AccountMeta::new(*merchant_pda, false),
        ];

        Instruction {
            program_id: self.did_program_id,
            accounts,
            data,
        }
    }

    fn build_recover_controller_ix(
        &self,
        signer_pubkey: &Pubkey,
        merchant_pda: &Pubkey,
        new_controller_pk: &Pubkey,
        nonce: u64,
    ) -> Instruction {
        let mut data = Vec::with_capacity(8 + 32 + 8);
        data.extend_from_slice(&anchor_sighash("recover_controller"));
        data.extend_from_slice(new_controller_pk.as_ref());
        data.extend_from_slice(&nonce.to_le_bytes());

        let accounts = vec![
            AccountMeta::new(*signer_pubkey, true),
            AccountMeta::new(*merchant_pda, false),
        ];

        Instruction {
            program_id: self.did_program_id,
            accounts,
            data,
        }
    }

    // ── Sponsored (sign + send) ───────────────────────────────────────

    /// Initialize a new merchant DID as a PDA (platform signs and sends).
    pub async fn initialize_did(
        &self,
        payer: &Keypair,
        vc_hash: [u8; 32],
        platform_signature: [u8; 64],
        credential_subject_pk: &Pubkey,
    ) -> Result<Signature> {
        let recent_blockhash = self
            .rpc_client
            .get_latest_blockhash()
            .map_err(|e| SolanaError::RpcError(e.to_string()))?;

        let (merchant_pda, _) = self.derive_merchant_pda(credential_subject_pk);
        let platform_config = self.derive_platform_config_pda();

        let ix = self.build_initialize_did_ix(
            &payer.pubkey(),
            &merchant_pda,
            &platform_config,
            vc_hash,
            platform_signature,
            credential_subject_pk,
        );

        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&payer.pubkey()),
            &[payer],
            recent_blockhash,
        );

        let sig = self
            .rpc_client
            .send_and_confirm_transaction(&tx)
            .map_err(|e| SolanaError::TransactionFailed(e.to_string()))?;

        Ok(sig)
    }

    /// Update the VC hash on an existing PDA merchant DID (platform signs and sends).
    pub async fn update_did_with_vc(
        &self,
        controller: &Keypair,
        vc_hash: [u8; 32],
        nonce: u64,
        platform_signature: [u8; 64],
        credential_subject_pk: &Pubkey,
    ) -> Result<Signature> {
        let recent_blockhash = self
            .rpc_client
            .get_latest_blockhash()
            .map_err(|e| SolanaError::RpcError(e.to_string()))?;

        let (merchant_pda, _) = self.derive_merchant_pda(&controller.pubkey());
        let platform_config = self.derive_platform_config_pda();

        let ix = self.build_update_did_with_vc_ix(
            &controller.pubkey(),
            &merchant_pda,
            &platform_config,
            vc_hash,
            nonce,
            platform_signature,
            credential_subject_pk,
        );

        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&controller.pubkey()),
            &[controller],
            recent_blockhash,
        );

        let sig = self
            .rpc_client
            .send_and_confirm_transaction(&tx)
            .map_err(|e| SolanaError::TransactionFailed(e.to_string()))?;

        Ok(sig)
    }

    /// Set or change the recovery public key (platform signs and sends).
    pub async fn set_recovery_key(
        &self,
        controller: &Keypair,
        recovery_pk: &Pubkey,
        nonce: u64,
    ) -> Result<Signature> {
        let recent_blockhash = self
            .rpc_client
            .get_latest_blockhash()
            .map_err(|e| SolanaError::RpcError(e.to_string()))?;

        let (merchant_pda, _) = self.derive_merchant_pda(&controller.pubkey());

        let ix = self.build_set_recovery_key_ix(
            &controller.pubkey(),
            &merchant_pda,
            recovery_pk,
            nonce,
        );

        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&controller.pubkey()),
            &[controller],
            recent_blockhash,
        );

        let sig = self
            .rpc_client
            .send_and_confirm_transaction(&tx)
            .map_err(|e| SolanaError::TransactionFailed(e.to_string()))?;

        Ok(sig)
    }

    /// Recover controller by proving ownership of the recovery key.
    pub async fn recover_controller(
        &self,
        recovery_signer: &Keypair,
        original_pk: &Pubkey,
        new_controller_pk: &Pubkey,
        nonce: u64,
    ) -> Result<Signature> {
        let recent_blockhash = self
            .rpc_client
            .get_latest_blockhash()
            .map_err(|e| SolanaError::RpcError(e.to_string()))?;

        let (merchant_pda, _) = self.derive_merchant_pda(original_pk);

        let ix = self.build_recover_controller_ix(
            &recovery_signer.pubkey(),
            &merchant_pda,
            new_controller_pk,
            nonce,
        );

        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&recovery_signer.pubkey()),
            &[recovery_signer],
            recent_blockhash,
        );

        let sig = self
            .rpc_client
            .send_and_confirm_transaction(&tx)
            .map_err(|e| SolanaError::TransactionFailed(e.to_string()))?;

        Ok(sig)
    }

    // ── SelfOnchain (build unsigned transaction) ──────────────────────

    /// Build an unsigned `initialize_did` transaction for the merchant to sign.
    pub async fn prepare_initialize_did(
        &self,
        signer_pubkey: &Pubkey,
        vc_hash: [u8; 32],
        platform_signature: [u8; 64],
        credential_subject_pk: &Pubkey,
    ) -> Result<Transaction> {
        let recent_blockhash = self
            .rpc_client
            .get_latest_blockhash()
            .map_err(|e| SolanaError::RpcError(e.to_string()))?;

        let (merchant_pda, _) = self.derive_merchant_pda(credential_subject_pk);
        let platform_config = self.derive_platform_config_pda();

        let ix = self.build_initialize_did_ix(
            signer_pubkey,
            &merchant_pda,
            &platform_config,
            vc_hash,
            platform_signature,
            credential_subject_pk,
        );

        let message = solana_sdk::message::Message::new_with_blockhash(
            &[ix],
            Some(signer_pubkey),
            &recent_blockhash,
        );
        Ok(Transaction::new_unsigned(message))
    }

    /// Build an unsigned `update_did_with_vc` transaction for the merchant to sign.
    pub async fn prepare_update_did_with_vc(
        &self,
        signer_pubkey: &Pubkey,
        vc_hash: [u8; 32],
        nonce: u64,
        platform_signature: [u8; 64],
        credential_subject_pk: &Pubkey,
    ) -> Result<Transaction> {
        let recent_blockhash = self
            .rpc_client
            .get_latest_blockhash()
            .map_err(|e| SolanaError::RpcError(e.to_string()))?;

        let (merchant_pda, _) = self.derive_merchant_pda(signer_pubkey);
        let platform_config = self.derive_platform_config_pda();

        let ix = self.build_update_did_with_vc_ix(
            signer_pubkey,
            &merchant_pda,
            &platform_config,
            vc_hash,
            nonce,
            platform_signature,
            credential_subject_pk,
        );

        let message = solana_sdk::message::Message::new_with_blockhash(
            &[ix],
            Some(signer_pubkey),
            &recent_blockhash,
        );
        Ok(Transaction::new_unsigned(message))
    }

    /// Build an unsigned `set_recovery_key` transaction for the merchant to sign.
    pub async fn prepare_set_recovery_key(
        &self,
        signer_pubkey: &Pubkey,
        recovery_pk: &Pubkey,
        nonce: u64,
    ) -> Result<Transaction> {
        let recent_blockhash = self
            .rpc_client
            .get_latest_blockhash()
            .map_err(|e| SolanaError::RpcError(e.to_string()))?;

        let (merchant_pda, _) = self.derive_merchant_pda(signer_pubkey);

        let ix = self.build_set_recovery_key_ix(
            signer_pubkey,
            &merchant_pda,
            recovery_pk,
            nonce,
        );

        let message = solana_sdk::message::Message::new_with_blockhash(
            &[ix],
            Some(signer_pubkey),
            &recent_blockhash,
        );
        Ok(Transaction::new_unsigned(message))
    }

    // ── VC Revocation ────────────────────────────────────────────────

    /// Revoke a VC by creating an on-chain RevokedVc PDA.
    /// Only the platform authority (from PlatformConfig) can invoke.
    /// PDA seeds: [b"revoked-vc", vc_hash]
    pub async fn revoke_vc(
        &self,
        authority: &Keypair,
        vc_hash: [u8; 32],
        credential_subject_pk: &Pubkey,
        reason: u8,
    ) -> Result<Signature> {
        let platform_config_address = self.derive_platform_config_pda();
        let revoked_vc_address = Pubkey::find_program_address(
            &[b"revoked-vc", &vc_hash],
            &self.did_program_id,
        ).0;

        let mut data = Vec::with_capacity(8 + 32 + 32 + 1);
        data.extend_from_slice(&anchor_sighash("revoke_vc"));
        data.extend_from_slice(&vc_hash);
        data.extend_from_slice(credential_subject_pk.as_ref());
        data.extend_from_slice(&reason.to_le_bytes());

        let accounts = vec![
            AccountMeta::new(authority.pubkey(), true),
            AccountMeta::new_readonly(platform_config_address, false),
            AccountMeta::new(revoked_vc_address, false),
            AccountMeta::new_readonly(solana_sdk::system_program::id(), false),
        ];

        let ix = Instruction {
            program_id: self.did_program_id,
            accounts,
            data,
        };

        let recent_blockhash = self
            .rpc_client
            .get_latest_blockhash()
            .map_err(|e| SolanaError::RpcError(e.to_string()))?;

        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&authority.pubkey()),
            &[authority],
            recent_blockhash,
        );

        let sig = self
            .rpc_client
            .send_and_confirm_transaction(&tx)
            .map_err(|e| SolanaError::TransactionFailed(e.to_string()))?;

        Ok(sig)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_anchor_sighash_deterministic() {
        let h1 = anchor_sighash("initialize_did");
        let h2 = anchor_sighash("initialize_did");
        assert_eq!(h1, h2);
        let h3 = anchor_sighash("update_did_with_vc");
        assert_ne!(
            h1, h3,
            "Different instruction names should have different sighashes"
        );
    }

    #[test]
    fn test_derive_merchant_pda_deterministic() {
        let service =
            DidService::new("https://api.devnet.solana.com", "11111111111111111111111111111111")
                .unwrap();
        let original_pk = Pubkey::new_unique();
        let (pda1, bump1) = service.derive_merchant_pda(&original_pk);
        let (pda2, bump2) = service.derive_merchant_pda(&original_pk);
        assert_eq!(pda1, pda2);
        assert_eq!(bump1, bump2);
    }

    #[test]
    fn test_derive_merchant_pda_differs_for_different_pks() {
        let service =
            DidService::new("https://api.devnet.solana.com", "11111111111111111111111111111111")
                .unwrap();
        let pk1 = Pubkey::new_unique();
        let pk2 = Pubkey::new_unique();
        let (pda1, _) = service.derive_merchant_pda(&pk1);
        let (pda2, _) = service.derive_merchant_pda(&pk2);
        assert_ne!(pda1, pda2, "Different original PKs should produce different PDAs");
    }

    #[test]
    fn test_merchant_did_account_borsh_roundtrip() {
        use crate::types::MerchantDidAccount;
        let account = MerchantDidAccount {
            original_pk: Pubkey::new_unique(),
            controller_pk: Pubkey::new_unique(),
            recovery_pk: Pubkey::new_unique(),
            vc_hash: [42u8; 32],
            last_updated: 1700000000,
            nonce: 5,
        };
        let bytes = borsh::to_vec(&account).unwrap();
        let decoded: crate::types::MerchantDidAccount =
            borsh::BorshDeserialize::deserialize(&mut bytes.as_slice()).unwrap();
        assert_eq!(decoded.original_pk, account.original_pk);
        assert_eq!(decoded.controller_pk, account.controller_pk);
        assert_eq!(decoded.recovery_pk, account.recovery_pk);
        assert_eq!(decoded.vc_hash, account.vc_hash);
        assert_eq!(decoded.last_updated, account.last_updated);
        assert_eq!(decoded.nonce, account.nonce);
    }

    #[test]
    fn test_onchain_account_borsh_roundtrip() {
        let account = MerchantDidAccountOnchain {
            original_pk: Pubkey::new_unique(),
            controller_pk: Pubkey::new_unique(),
            recovery_pk: Pubkey::new_unique(),
            vc_hash: [42u8; 32],
            last_updated: 1700000000,
            nonce: 5,
            bump: 255,
        };
        let bytes = borsh::to_vec(&account).unwrap();
        let decoded: MerchantDidAccountOnchain =
            borsh::BorshDeserialize::deserialize(&mut bytes.as_slice()).unwrap();
        assert_eq!(decoded.original_pk, account.original_pk);
        assert_eq!(decoded.bump, account.bump);
    }
}
