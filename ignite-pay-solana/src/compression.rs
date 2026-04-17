use crate::error::{Result, SolanaError};
use crate::types::MerchantDidAccount;
use light_sdk::{
    address::v1::derive_address,
    instruction::PackedAddressTreeInfo,
};
use solana_client::rpc_client::RpcClient;
use solana_sdk::hash::hash;
use solana_sdk::instruction::{AccountMeta, Instruction};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signature};
use solana_sdk::signer::Signer;
use solana_sdk::transaction::Transaction;

/// Compute the Anchor instruction discriminator: sha256("global:<name>")[..8]
fn anchor_sighash(name: &str) -> [u8; 8] {
    let preimage = format!("global:{}", name);
    let h = hash(preimage.as_bytes());
    let mut disc = [0u8; 8];
    disc.copy_from_slice(&h.to_bytes()[..8]);
    disc
}

/// Service for interacting with the ignite-pay-did-program using ZK compressed accounts.
///
/// Compressed DID accounts are stored as hashes in Light Protocol state Merkle trees
/// rather than as traditional on-chain accounts. This means:
/// - No rent-exemption required
/// - Account data is passed as instruction data (not fetched from on-chain PDAs)
/// - A validity proof is required for all operations
/// - Tree accounts are passed as remaining accounts
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

    /// Derive a compressed PDA address for a given original public key.
    /// Seeds: [b"merchant-did", original_pk]
    /// Returns (address_bytes, address_seed) for use with the Light System Program CPI.
    pub fn derive_compressed_address(
        &self,
        original_pk: &Pubkey,
        address_tree_pubkey: &Pubkey,
    ) -> ([u8; 32], light_sdk::address::AddressSeed) {
        derive_address(
            &[b"merchant-did", original_pk.as_ref()],
            address_tree_pubkey,
            &self.did_program_id,
        )
    }

    /// Build the accounts list for a compressed DID instruction.
    /// Includes: signer, system program, and remaining accounts for the Light CPI.
    pub fn build_instruction_accounts(
        &self,
        signer: &Keypair,
        remaining_accounts: &[AccountMeta],
    ) -> Vec<AccountMeta> {
        let mut accounts = vec![
            AccountMeta::new(signer.pubkey(), true),
        ];
        accounts.extend_from_slice(remaining_accounts);
        accounts
    }

    // ── Private instruction builders ──────────────────────────────────

    /// Build the `initialize_did` instruction.
    fn build_initialize_did_ix(
        &self,
        signer_pubkey: &Pubkey,
        proof_bytes: &[u8],
        address_tree_info: &PackedAddressTreeInfo,
        output_state_tree_index: u8,
        remaining_accounts: &[AccountMeta],
    ) -> Result<Instruction> {
        let mut data = Vec::with_capacity(8 + proof_bytes.len() + 8);
        data.extend_from_slice(&anchor_sighash("initialize_did"));
        data.extend_from_slice(proof_bytes);
        data.extend_from_slice(&borsh::to_vec(address_tree_info).map_err(SolanaError::BorshError)?);
        data.extend_from_slice(&output_state_tree_index.to_le_bytes());

        let mut accounts = vec![
            AccountMeta::new(*signer_pubkey, true),
        ];
        accounts.extend_from_slice(remaining_accounts);

        Ok(Instruction {
            program_id: self.did_program_id,
            accounts,
            data,
        })
    }

    /// Build the `update_did_with_vc` instruction.
    fn build_update_did_with_vc_ix(
        &self,
        signer_pubkey: &Pubkey,
        proof_bytes: &[u8],
        current_did: &MerchantDidAccount,
        account_meta: &light_sdk::instruction::account_meta::CompressedAccountMeta,
        vc_hash: [u8; 32],
        nonce: u64,
        remaining_accounts: &[AccountMeta],
    ) -> Result<Instruction> {
        let mut data = Vec::new();
        data.extend_from_slice(&anchor_sighash("update_did_with_vc"));
        data.extend_from_slice(proof_bytes);
        data.extend_from_slice(&borsh::to_vec(current_did).map_err(SolanaError::BorshError)?);
        data.extend_from_slice(&borsh::to_vec(account_meta).map_err(SolanaError::BorshError)?);
        data.extend_from_slice(&vc_hash);
        data.extend_from_slice(&nonce.to_le_bytes());

        let mut accounts = vec![
            AccountMeta::new(*signer_pubkey, true),
        ];
        accounts.extend_from_slice(remaining_accounts);

        Ok(Instruction {
            program_id: self.did_program_id,
            accounts,
            data,
        })
    }

    /// Build the `set_recovery_key` instruction.
    fn build_set_recovery_key_ix(
        &self,
        signer_pubkey: &Pubkey,
        proof_bytes: &[u8],
        current_did: &MerchantDidAccount,
        account_meta: &light_sdk::instruction::account_meta::CompressedAccountMeta,
        recovery_pk: &Pubkey,
        nonce: u64,
        remaining_accounts: &[AccountMeta],
    ) -> Result<Instruction> {
        let mut data = Vec::new();
        data.extend_from_slice(&anchor_sighash("set_recovery_key"));
        data.extend_from_slice(proof_bytes);
        data.extend_from_slice(&borsh::to_vec(current_did).map_err(SolanaError::BorshError)?);
        data.extend_from_slice(&borsh::to_vec(account_meta).map_err(SolanaError::BorshError)?);
        data.extend_from_slice(recovery_pk.as_ref());
        data.extend_from_slice(&nonce.to_le_bytes());

        let mut accounts = vec![
            AccountMeta::new(*signer_pubkey, true),
        ];
        accounts.extend_from_slice(remaining_accounts);

        Ok(Instruction {
            program_id: self.did_program_id,
            accounts,
            data,
        })
    }

    // ── Sponsored (sign + send) ───────────────────────────────────────

    /// Initialize a new compressed merchant DID (platform signs and sends).
    pub async fn initialize_did(
        &self,
        payer: &Keypair,
        proof_bytes: &[u8],
        address_tree_info: &PackedAddressTreeInfo,
        output_state_tree_index: u8,
        remaining_accounts: &[AccountMeta],
    ) -> Result<Signature> {
        let recent_blockhash = self
            .rpc_client
            .get_latest_blockhash()
            .map_err(|e| SolanaError::RpcError(e.to_string()))?;

        let ix = self.build_initialize_did_ix(
            &payer.pubkey(),
            proof_bytes,
            address_tree_info,
            output_state_tree_index,
            remaining_accounts,
        )?;

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

    /// Update the VC hash on an existing compressed DID (platform signs and sends).
    pub async fn update_did_with_vc(
        &self,
        controller: &Keypair,
        proof_bytes: &[u8],
        current_did: &MerchantDidAccount,
        account_meta: &light_sdk::instruction::account_meta::CompressedAccountMeta,
        vc_hash: [u8; 32],
        nonce: u64,
        remaining_accounts: &[AccountMeta],
    ) -> Result<Signature> {
        let recent_blockhash = self
            .rpc_client
            .get_latest_blockhash()
            .map_err(|e| SolanaError::RpcError(e.to_string()))?;

        let ix = self.build_update_did_with_vc_ix(
            &controller.pubkey(),
            proof_bytes,
            current_did,
            account_meta,
            vc_hash,
            nonce,
            remaining_accounts,
        )?;

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
        proof_bytes: &[u8],
        current_did: &MerchantDidAccount,
        account_meta: &light_sdk::instruction::account_meta::CompressedAccountMeta,
        recovery_pk: &Pubkey,
        nonce: u64,
        remaining_accounts: &[AccountMeta],
    ) -> Result<Signature> {
        let recent_blockhash = self
            .rpc_client
            .get_latest_blockhash()
            .map_err(|e| SolanaError::RpcError(e.to_string()))?;

        let ix = self.build_set_recovery_key_ix(
            &controller.pubkey(),
            proof_bytes,
            current_did,
            account_meta,
            recovery_pk,
            nonce,
            remaining_accounts,
        )?;

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
    /// Sets controller_pk to new_controller_pk.
    pub async fn recover_controller(
        &self,
        recovery_signer: &Keypair,
        proof_bytes: &[u8],
        current_did: &MerchantDidAccount,
        account_meta: &light_sdk::instruction::account_meta::CompressedAccountMeta,
        new_controller_pk: &Pubkey,
        nonce: u64,
        remaining_accounts: &[AccountMeta],
    ) -> Result<Signature> {
        let recent_blockhash = self
            .rpc_client
            .get_latest_blockhash()
            .map_err(|e| SolanaError::RpcError(e.to_string()))?;

        let mut data = Vec::new();
        data.extend_from_slice(&anchor_sighash("recover_controller"));
        data.extend_from_slice(proof_bytes);
        data.extend_from_slice(&borsh::to_vec(current_did).map_err(SolanaError::BorshError)?);
        data.extend_from_slice(&borsh::to_vec(account_meta).map_err(SolanaError::BorshError)?);
        data.extend_from_slice(new_controller_pk.as_ref());
        data.extend_from_slice(&nonce.to_le_bytes());

        let mut accounts = vec![
            AccountMeta::new(recovery_signer.pubkey(), true),
        ];
        accounts.extend_from_slice(remaining_accounts);

        let ix = Instruction {
            program_id: self.did_program_id,
            accounts,
            data,
        };

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

    /// Build an unsigned `initialize_did` transaction for the merchant to sign and broadcast.
    pub async fn prepare_initialize_did(
        &self,
        signer_pubkey: &Pubkey,
        proof_bytes: &[u8],
        address_tree_info: &PackedAddressTreeInfo,
        output_state_tree_index: u8,
        remaining_accounts: &[AccountMeta],
    ) -> Result<Transaction> {
        let recent_blockhash = self
            .rpc_client
            .get_latest_blockhash()
            .map_err(|e| SolanaError::RpcError(e.to_string()))?;

        let ix = self.build_initialize_did_ix(
            signer_pubkey,
            proof_bytes,
            address_tree_info,
            output_state_tree_index,
            remaining_accounts,
        )?;

        let message = solana_sdk::message::Message::new_with_blockhash(
            &[ix],
            Some(signer_pubkey),
            &recent_blockhash,
        );
        Ok(Transaction::new_unsigned(message))
    }

    /// Build an unsigned `update_did_with_vc` transaction for the merchant to sign and broadcast.
    pub async fn prepare_update_did_with_vc(
        &self,
        signer_pubkey: &Pubkey,
        proof_bytes: &[u8],
        current_did: &MerchantDidAccount,
        account_meta: &light_sdk::instruction::account_meta::CompressedAccountMeta,
        vc_hash: [u8; 32],
        nonce: u64,
        remaining_accounts: &[AccountMeta],
    ) -> Result<Transaction> {
        let recent_blockhash = self
            .rpc_client
            .get_latest_blockhash()
            .map_err(|e| SolanaError::RpcError(e.to_string()))?;

        let ix = self.build_update_did_with_vc_ix(
            signer_pubkey,
            proof_bytes,
            current_did,
            account_meta,
            vc_hash,
            nonce,
            remaining_accounts,
        )?;

        let message = solana_sdk::message::Message::new_with_blockhash(
            &[ix],
            Some(signer_pubkey),
            &recent_blockhash,
        );
        Ok(Transaction::new_unsigned(message))
    }

    /// Build an unsigned `set_recovery_key` transaction for the merchant to sign and broadcast.
    pub async fn prepare_set_recovery_key(
        &self,
        signer_pubkey: &Pubkey,
        proof_bytes: &[u8],
        current_did: &MerchantDidAccount,
        account_meta: &light_sdk::instruction::account_meta::CompressedAccountMeta,
        recovery_pk: &Pubkey,
        nonce: u64,
        remaining_accounts: &[AccountMeta],
    ) -> Result<Transaction> {
        let recent_blockhash = self
            .rpc_client
            .get_latest_blockhash()
            .map_err(|e| SolanaError::RpcError(e.to_string()))?;

        let ix = self.build_set_recovery_key_ix(
            signer_pubkey,
            proof_bytes,
            current_did,
            account_meta,
            recovery_pk,
            nonce,
            remaining_accounts,
        )?;

        let message = solana_sdk::message::Message::new_with_blockhash(
            &[ix],
            Some(signer_pubkey),
            &recent_blockhash,
        );
        Ok(Transaction::new_unsigned(message))
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
    fn test_derive_compressed_address() {
        let service =
            DidService::new("https://api.devnet.solana.com", "11111111111111111111111111111111")
                .unwrap();
        let original_pk = Pubkey::new_unique();
        let tree_pk = Pubkey::new_unique();
        let (addr1, seed1) = service.derive_compressed_address(&original_pk, &tree_pk);
        let (addr2, seed2) = service.derive_compressed_address(&original_pk, &tree_pk);
        assert_eq!(addr1, addr2, "Compressed address derivation should be deterministic");
        assert_eq!(seed1.0, seed2.0, "Address seed should be deterministic");
    }

    #[test]
    fn test_merchant_did_account_borsh_roundtrip() {
        let account = MerchantDidAccount {
            original_pk: Pubkey::new_unique(),
            controller_pk: Pubkey::new_unique(),
            recovery_pk: Pubkey::new_unique(),
            vc_hash: [42u8; 32],
            last_updated: 1700000000,
            nonce: 5,
        };
        let bytes = borsh::to_vec(&account).unwrap();
        let decoded: MerchantDidAccount =
            borsh::BorshDeserialize::deserialize(&mut bytes.as_slice()).unwrap();
        assert_eq!(decoded.original_pk, account.original_pk);
        assert_eq!(decoded.controller_pk, account.controller_pk);
        assert_eq!(decoded.recovery_pk, account.recovery_pk);
        assert_eq!(decoded.vc_hash, account.vc_hash);
        assert_eq!(decoded.last_updated, account.last_updated);
        assert_eq!(decoded.nonce, account.nonce);
    }

    #[test]
    fn test_compressed_address_differs_for_different_pks() {
        let service =
            DidService::new("https://api.devnet.solana.com", "11111111111111111111111111111111")
                .unwrap();
        let pk1 = Pubkey::new_unique();
        let pk2 = Pubkey::new_unique();
        let tree_pk = Pubkey::new_unique();
        let (addr1, _) = service.derive_compressed_address(&pk1, &tree_pk);
        let (addr2, _) = service.derive_compressed_address(&pk2, &tree_pk);
        assert_ne!(addr1, addr2, "Different original PKs should produce different addresses");
    }
}
