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

#![allow(unexpected_cfgs)]
#![allow(deprecated)]
pub mod state;
pub mod error;

use anchor_lang::prelude::*;
use error::DidError;
use state::{PlatformConfig, RevokedVc};

declare_id!("D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D");

/// Verify an Ed25519 signature against a message and public key.
/// Uses ed25519_dalek v1.x for on-chain verification.
fn verify_ed25519_signature(message: &[u8], signature: &[u8; 64], public_key: &Pubkey) -> bool {
    use ed25519_dalek::{PublicKey, Signature, Verifier};

    let pubkey = match PublicKey::from_bytes(public_key.as_ref()) {
        Ok(key) => key,
        Err(_) => return false,
    };
    let sig = match Signature::from_bytes(signature) {
        Ok(s) => s,
        Err(_) => return false,
    };
    pubkey.verify(message, &sig).is_ok()
}

/// Accounts for revoking a VC. Only the platform authority can invoke.
#[derive(Accounts)]
#[instruction(vc_hash: [u8; 32], _credential_subject_pk: Pubkey, _reason: u8)]
pub struct RevokeVc<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(
        seeds = [b"platform-config"],
        bump = platform_config.bump,
        constraint = platform_config.authority == authority.key() @ DidError::UnauthorizedRevocation
    )]
    pub platform_config: Account<'info, PlatformConfig>,
    #[account(
        init,
        payer = authority,
        space = 8 + 32 + 32 + 8 + 1 + 32 + 1,
        seeds = [b"revoked-vc", vc_hash.as_ref()],
        bump
    )]
    pub revoked_vc: Account<'info, RevokedVc>,
    pub system_program: Program<'info, System>,
}

/// Accounts for the one-time `init_platform` instruction.
#[derive(Accounts)]
pub struct InitPlatform<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(
        init,
        payer = authority,
        space = 8 + 32 + 32 + 1,
        seeds = [b"platform-config"],
        bump
    )]
    pub platform_config: Account<'info, PlatformConfig>,
    pub system_program: Program<'info, System>,
}

// ─── PDA version Account structs (default) ──────────────────────────────

#[cfg(not(feature = "zk-compression"))]
use state::MerchantDidAccount;

#[cfg(not(feature = "zk-compression"))]
#[derive(Accounts)]
#[instruction(credential_subject_pk: Pubkey)]
pub struct InitializeDid<'info> {
    #[account(mut)]
    pub signer: Signer<'info>,
    #[account(
        init,
        payer = signer,
        space = 153,
        seeds = [b"merchant-did", signer.key().as_ref()],
        bump
    )]
    pub merchant_did: Account<'info, MerchantDidAccount>,
    #[account(
        seeds = [b"platform-config"],
        bump = platform_config.bump,
        constraint = platform_config.platform_ed25519_pubkey != [0u8; 32] @ DidError::PlatformNotInitialized
    )]
    pub platform_config: Account<'info, PlatformConfig>,
    pub system_program: Program<'info, System>,
}

#[cfg(not(feature = "zk-compression"))]
#[derive(Accounts)]
pub struct UpdateDidWithVc<'info> {
    #[account(mut)]
    pub signer: Signer<'info>,
    #[account(
        mut,
        seeds = [b"merchant-did", merchant_did.original_pk.as_ref()],
        bump = merchant_did.bump,
        constraint = merchant_did.controller_pk == signer.key() @ DidError::InvalidControllerKey
    )]
    pub merchant_did: Account<'info, MerchantDidAccount>,
    #[account(
        seeds = [b"platform-config"],
        bump = platform_config.bump,
        constraint = platform_config.platform_ed25519_pubkey != [0u8; 32] @ DidError::PlatformNotInitialized
    )]
    pub platform_config: Account<'info, PlatformConfig>,
}

#[cfg(not(feature = "zk-compression"))]
#[derive(Accounts)]
pub struct SetRecoveryKey<'info> {
    #[account(mut)]
    pub signer: Signer<'info>,
    #[account(
        mut,
        seeds = [b"merchant-did", merchant_did.original_pk.as_ref()],
        bump = merchant_did.bump,
        constraint = merchant_did.controller_pk == signer.key() @ DidError::InvalidControllerKey
    )]
    pub merchant_did: Account<'info, MerchantDidAccount>,
}

#[cfg(not(feature = "zk-compression"))]
#[derive(Accounts)]
pub struct RecoverController<'info> {
    #[account(mut)]
    pub signer: Signer<'info>,
    #[account(
        mut,
        seeds = [b"merchant-did", merchant_did.original_pk.as_ref()],
        bump = merchant_did.bump,
        constraint = merchant_did.recovery_pk == signer.key() @ DidError::InvalidRecoveryKey
    )]
    pub merchant_did: Account<'info, MerchantDidAccount>,
}

// ─── ZK Compression Account structs (optional) ──────────────────────────

#[cfg(feature = "zk-compression")]
use light_sdk::{
    account::LightAccount,
    address::v1::derive_address,
    cpi::{
        v1::{CpiAccounts, LightSystemProgramCpi},
        CpiSigner, InvokeLightSystemProgram, LightCpiInstruction,
    },
    derive_light_cpi_signer,
    instruction::{account_meta::CompressedAccountMeta, PackedAddressTreeInfo},
    PackedAddressTreeInfoExt,
};

#[cfg(feature = "zk-compression")]
use state::MerchantCompressedDid;

/// Compile-time CPI signer derived from the program ID.
#[cfg(feature = "zk-compression")]
pub const LIGHT_CPI_SIGNER: CpiSigner =
    derive_light_cpi_signer!("D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D");

#[cfg(feature = "zk-compression")]
#[derive(Accounts)]
pub struct GenericAnchorAccounts<'info> {
    #[account(mut)]
    pub signer: Signer<'info>,
}

#[cfg(feature = "zk-compression")]
#[derive(Accounts)]
pub struct DidWithPlatformAccounts<'info> {
    #[account(mut)]
    pub signer: Signer<'info>,
    #[account(
        seeds = [b"platform-config"],
        bump = platform_config.bump,
        constraint = platform_config.platform_ed25519_pubkey != [0u8; 32] @ DidError::PlatformNotInitialized
    )]
    pub platform_config: Account<'info, PlatformConfig>,
}

// ─── Program instructions ───────────────────────────────────────────────

#[program]
pub mod ignite_pay_did_program {
    use super::*;

    // ─── 0. init_platform (shared) ───

    /// One-time initialization of the platform Ed25519 public key.
    pub fn init_platform(
        ctx: Context<InitPlatform>,
        platform_ed25519_pubkey: [u8; 32],
    ) -> Result<()> {
        let config = &mut ctx.accounts.platform_config;
        config.platform_ed25519_pubkey = platform_ed25519_pubkey;
        config.authority = ctx.accounts.authority.key();
        config.bump = ctx.bumps.platform_config;
        Ok(())
    }

    // ─── PDA version instructions (default) ───────────────────────────

    /// Create a new merchant DID as a standard PDA.
    /// Seeds: [b"merchant-did", signer.key()]
    /// Requires a valid platform signature over (credential_subject_pk || vc_hash).
    #[cfg(not(feature = "zk-compression"))]
    pub fn initialize_did(
        ctx: Context<InitializeDid>,
        vc_hash: [u8; 32],
        platform_signature: [u8; 64],
        _credential_subject_pk: Pubkey,
    ) -> Result<()> {
        // Verify platform signature: sign(credential_subject_pk || vc_hash)
        let platform_pk_bytes = ctx.accounts.platform_config.platform_ed25519_pubkey;
        let credential_subject_pk = ctx.accounts.signer.key();
        let mut message = Vec::with_capacity(64);
        message.extend_from_slice(credential_subject_pk.as_ref());
        message.extend_from_slice(&vc_hash);
        require!(
            verify_ed25519_signature(
                &message,
                &platform_signature,
                &Pubkey::new_from_array(platform_pk_bytes)
            ),
            DidError::InvalidPlatformSignature
        );

        let did = &mut ctx.accounts.merchant_did;
        did.original_pk = ctx.accounts.signer.key();
        did.controller_pk = ctx.accounts.signer.key();
        did.recovery_pk = Pubkey::default();
        did.vc_hash = vc_hash;
        did.last_updated = Clock::get()?.unix_timestamp;
        did.nonce = 0;
        did.bump = ctx.bumps.merchant_did;

        Ok(())
    }

    /// Update the VC hash on an existing PDA merchant DID.
    /// Requires the current controller as signer + valid platform signature.
    #[cfg(not(feature = "zk-compression"))]
    pub fn update_did_with_vc(
        ctx: Context<UpdateDidWithVc>,
        vc_hash: [u8; 32],
        nonce: u64,
        platform_signature: [u8; 64],
        _credential_subject_pk: Pubkey,
    ) -> Result<()> {
        let did = &mut ctx.accounts.merchant_did;

        // Anti-replay: caller must supply the current nonce
        require!(did.nonce == nonce, DidError::NonceMismatch);

        // Verify platform signature: sign(credential_subject_pk || vc_hash)
        let platform_pk_bytes = ctx.accounts.platform_config.platform_ed25519_pubkey;
        let credential_subject_pk = ctx.accounts.signer.key();
        let mut message = Vec::with_capacity(64);
        message.extend_from_slice(credential_subject_pk.as_ref());
        message.extend_from_slice(&vc_hash);
        require!(
            verify_ed25519_signature(
                &message,
                &platform_signature,
                &Pubkey::new_from_array(platform_pk_bytes)
            ),
            DidError::InvalidPlatformSignature
        );

        did.vc_hash = vc_hash;
        did.last_updated = Clock::get()?.unix_timestamp;
        did.nonce = did
            .nonce
            .checked_add(1)
            .ok_or(DidError::ArithmeticOverflow)?;

        Ok(())
    }

    /// Set or change the recovery public key on a PDA merchant DID.
    /// Requires the current controller as signer + valid nonce.
    #[cfg(not(feature = "zk-compression"))]
    pub fn set_recovery_key(
        ctx: Context<SetRecoveryKey>,
        recovery_pk: Pubkey,
        nonce: u64,
    ) -> Result<()> {
        let did = &mut ctx.accounts.merchant_did;

        require!(did.nonce == nonce, DidError::NonceMismatch);

        did.recovery_pk = recovery_pk;
        did.last_updated = Clock::get()?.unix_timestamp;
        did.nonce = did
            .nonce
            .checked_add(1)
            .ok_or(DidError::ArithmeticOverflow)?;

        Ok(())
    }

    /// Recover controller by proving ownership of the recovery key.
    /// Sets controller_pk to new_controller_pk.
    #[cfg(not(feature = "zk-compression"))]
    pub fn recover_controller(
        ctx: Context<RecoverController>,
        new_controller_pk: Pubkey,
        nonce: u64,
    ) -> Result<()> {
        let did = &mut ctx.accounts.merchant_did;

        require!(did.nonce == nonce, DidError::NonceMismatch);

        did.controller_pk = new_controller_pk;
        did.last_updated = Clock::get()?.unix_timestamp;
        did.nonce = did
            .nonce
            .checked_add(1)
            .ok_or(DidError::ArithmeticOverflow)?;

        Ok(())
    }

    // ─── ZK Compression version instructions (optional) ────────────────

    /// Create a new compressed merchant DID via ZK Compression.
    #[cfg(feature = "zk-compression")]
    pub fn initialize_did<'info>(
        ctx: Context<'_, '_, '_, 'info, DidWithPlatformAccounts<'info>>,
        proof: light_sdk::borsh_compat::ValidityProof,
        address_tree_info: PackedAddressTreeInfo,
        output_state_tree_index: u8,
        vc_hash: [u8; 32],
        platform_signature: [u8; 64],
        credential_subject_pk: Pubkey,
    ) -> Result<()> {
        require!(
            credential_subject_pk == ctx.accounts.signer.key(),
            DidError::VcSubjectMismatch
        );

        let platform_pk_bytes = ctx.accounts.platform_config.platform_ed25519_pubkey;
        let mut message = Vec::with_capacity(64);
        message.extend_from_slice(credential_subject_pk.as_ref());
        message.extend_from_slice(&vc_hash);
        require!(
            verify_ed25519_signature(
                &message,
                &platform_signature,
                &Pubkey::new_from_array(platform_pk_bytes)
            ),
            DidError::InvalidPlatformSignature
        );

        let light_cpi_accounts = CpiAccounts::new(
            ctx.accounts.signer.as_ref(),
            ctx.remaining_accounts,
            crate::LIGHT_CPI_SIGNER,
        );

        let address_tree_pubkey = address_tree_info
            .get_tree_pubkey(&light_cpi_accounts)
            .map_err(|_| DidError::InsufficientCpiAccounts)?;

        let (address, address_seed) = derive_address(
            &[b"merchant-did", ctx.accounts.signer.key().as_ref()],
            &address_tree_pubkey,
            &crate::ID,
        );

        let mut did = LightAccount::<MerchantCompressedDid>::new_init(
            &crate::ID,
            Some(address),
            output_state_tree_index,
        );

        did.original_pk = ctx.accounts.signer.key();
        did.controller_pk = ctx.accounts.signer.key();
        did.recovery_pk = Pubkey::default();
        did.vc_hash = vc_hash;
        did.last_updated = Clock::get()?.unix_timestamp;
        did.nonce = 0;

        LightSystemProgramCpi::new_cpi(crate::LIGHT_CPI_SIGNER, proof.into())
            .with_light_account(did)?
            .with_new_addresses(&[address_tree_info.into_new_address_params_packed(address_seed)])
            .invoke(light_cpi_accounts)?;

        Ok(())
    }

    /// Update the VC hash on an existing compressed DID via ZK Compression.
    #[cfg(feature = "zk-compression")]
    pub fn update_did_with_vc<'info>(
        ctx: Context<'_, '_, '_, 'info, DidWithPlatformAccounts<'info>>,
        proof: light_sdk::borsh_compat::ValidityProof,
        current_did: MerchantCompressedDid,
        account_meta: CompressedAccountMeta,
        vc_hash: [u8; 32],
        nonce: u64,
        platform_signature: [u8; 64],
        credential_subject_pk: Pubkey,
    ) -> Result<()> {
        require!(
            current_did.controller_pk == ctx.accounts.signer.key(),
            DidError::InvalidControllerKey
        );
        require!(current_did.nonce == nonce, DidError::NonceMismatch);

        require!(
            credential_subject_pk == ctx.accounts.signer.key(),
            DidError::VcSubjectMismatch
        );

        let platform_pk_bytes = ctx.accounts.platform_config.platform_ed25519_pubkey;
        let mut message = Vec::with_capacity(64);
        message.extend_from_slice(credential_subject_pk.as_ref());
        message.extend_from_slice(&vc_hash);
        require!(
            verify_ed25519_signature(
                &message,
                &platform_signature,
                &Pubkey::new_from_array(platform_pk_bytes)
            ),
            DidError::InvalidPlatformSignature
        );

        let light_cpi_accounts = CpiAccounts::new(
            ctx.accounts.signer.as_ref(),
            ctx.remaining_accounts,
            crate::LIGHT_CPI_SIGNER,
        );

        let mut did = LightAccount::<MerchantCompressedDid>::new_mut(
            &crate::ID,
            &account_meta,
            current_did,
        )?;

        did.vc_hash = vc_hash;
        did.last_updated = Clock::get()?.unix_timestamp;
        did.nonce = did
            .nonce
            .checked_add(1)
            .ok_or(DidError::ArithmeticOverflow)?;

        LightSystemProgramCpi::new_cpi(crate::LIGHT_CPI_SIGNER, proof.into())
            .with_light_account(did)?
            .invoke(light_cpi_accounts)?;

        Ok(())
    }

    /// Set or change the recovery key on a compressed DID via ZK Compression.
    #[cfg(feature = "zk-compression")]
    pub fn set_recovery_key<'info>(
        ctx: Context<'_, '_, '_, 'info, GenericAnchorAccounts<'info>>,
        proof: light_sdk::borsh_compat::ValidityProof,
        current_did: MerchantCompressedDid,
        account_meta: CompressedAccountMeta,
        recovery_pk: Pubkey,
        nonce: u64,
    ) -> Result<()> {
        require!(
            current_did.controller_pk == ctx.accounts.signer.key(),
            DidError::InvalidControllerKey
        );
        require!(current_did.nonce == nonce, DidError::NonceMismatch);

        let light_cpi_accounts = CpiAccounts::new(
            ctx.accounts.signer.as_ref(),
            ctx.remaining_accounts,
            crate::LIGHT_CPI_SIGNER,
        );

        let mut did = LightAccount::<MerchantCompressedDid>::new_mut(
            &crate::ID,
            &account_meta,
            current_did,
        )?;

        did.recovery_pk = recovery_pk;
        did.last_updated = Clock::get()?.unix_timestamp;
        did.nonce = did
            .nonce
            .checked_add(1)
            .ok_or(DidError::ArithmeticOverflow)?;

        LightSystemProgramCpi::new_cpi(crate::LIGHT_CPI_SIGNER, proof.into())
            .with_light_account(did)?
            .invoke(light_cpi_accounts)?;

        Ok(())
    }

    /// Recover controller on a compressed DID via ZK Compression.
    #[cfg(feature = "zk-compression")]
    pub fn recover_controller<'info>(
        ctx: Context<'_, '_, '_, 'info, GenericAnchorAccounts<'info>>,
        proof: light_sdk::borsh_compat::ValidityProof,
        current_did: MerchantCompressedDid,
        account_meta: CompressedAccountMeta,
        new_controller_pk: Pubkey,
        nonce: u64,
    ) -> Result<()> {
        require!(
            current_did.recovery_pk == ctx.accounts.signer.key(),
            DidError::InvalidRecoveryKey
        );
        require!(current_did.nonce == nonce, DidError::NonceMismatch);

        let light_cpi_accounts = CpiAccounts::new(
            ctx.accounts.signer.as_ref(),
            ctx.remaining_accounts,
            crate::LIGHT_CPI_SIGNER,
        );

        let mut did = LightAccount::<MerchantCompressedDid>::new_mut(
            &crate::ID,
            &account_meta,
            current_did,
        )?;

        did.controller_pk = new_controller_pk;
        did.last_updated = Clock::get()?.unix_timestamp;
        did.nonce = did
            .nonce
            .checked_add(1)
            .ok_or(DidError::ArithmeticOverflow)?;

        LightSystemProgramCpi::new_cpi(crate::LIGHT_CPI_SIGNER, proof.into())
            .with_light_account(did)?
            .invoke(light_cpi_accounts)?;

        Ok(())
    }

    // ─── 5. revoke_vc (shared) ───

    /// Revoke a VC by creating an on-chain RevokedVc PDA.
    pub fn revoke_vc(
        ctx: Context<RevokeVc>,
        vc_hash: [u8; 32],
        credential_subject_pk: Pubkey,
        reason: u8,
    ) -> Result<()> {
        let entry = &mut ctx.accounts.revoked_vc;
        entry.vc_hash = vc_hash;
        entry.credential_subject_pk = credential_subject_pk;
        entry.revoked_at = Clock::get()?.unix_timestamp;
        entry.reason = reason;
        entry.authority = ctx.accounts.authority.key();
        entry.bump = ctx.bumps.revoked_vc;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pda_deterministic() {
        let original_pk = Pubkey::new_unique();
        let (pda1, _) = Pubkey::find_program_address(
            &[b"merchant-did", original_pk.as_ref()],
            &crate::id(),
        );
        let (pda2, _) = Pubkey::find_program_address(
            &[b"merchant-did", original_pk.as_ref()],
            &crate::id(),
        );
        assert_eq!(pda1, pda2);
    }

    #[test]
    fn test_platform_config_pda_deterministic() {
        let (pda1, _) = Pubkey::find_program_address(
            &[b"platform-config"],
            &crate::id(),
        );
        let (pda2, _) = Pubkey::find_program_address(
            &[b"platform-config"],
            &crate::id(),
        );
        assert_eq!(pda1, pda2);
    }

    #[test]
    fn test_verify_ed25519_signature_valid() {
        use ed25519_dalek::{Keypair, Signer};
        use rand::rngs::OsRng;

        let keypair = Keypair::generate(&mut OsRng);
        let pubkey = Pubkey::new_from_array(keypair.public.to_bytes());

        let mut message = Vec::with_capacity(64);
        message.extend_from_slice(pubkey.as_ref());
        message.extend_from_slice(&[42u8; 32]);
        let signature: ed25519_dalek::Signature = keypair.sign(&message);
        let sig_bytes = signature.to_bytes();

        assert!(verify_ed25519_signature(&message, &sig_bytes, &pubkey));
    }

    #[test]
    fn test_verify_ed25519_signature_wrong_message() {
        use ed25519_dalek::{Keypair, Signer};
        use rand::rngs::OsRng;

        let keypair = Keypair::generate(&mut OsRng);
        let pubkey = Pubkey::new_from_array(keypair.public.to_bytes());

        let signature: ed25519_dalek::Signature = keypair.sign(b"correct message");
        let sig_bytes = signature.to_bytes();

        assert!(!verify_ed25519_signature(b"wrong message", &sig_bytes, &pubkey));
    }

    #[test]
    fn test_verify_ed25519_signature_wrong_pubkey() {
        use ed25519_dalek::{Keypair, Signer};
        use rand::rngs::OsRng;

        let keypair = Keypair::generate(&mut OsRng);
        let other_keypair = Keypair::generate(&mut OsRng);
        let other_pubkey = Pubkey::new_from_array(other_keypair.public.to_bytes());

        let message = b"test message";
        let signature: ed25519_dalek::Signature = keypair.sign(message);
        let sig_bytes = signature.to_bytes();

        assert!(!verify_ed25519_signature(message, &sig_bytes, &other_pubkey));
    }

    #[test]
    fn test_revoked_vc_pda_deterministic() {
        let vc_hash = [99u8; 32];
        let (pda1, _) = Pubkey::find_program_address(
            &[b"revoked-vc", &vc_hash],
            &crate::id(),
        );
        let (pda2, _) = Pubkey::find_program_address(
            &[b"revoked-vc", &vc_hash],
            &crate::id(),
        );
        assert_eq!(pda1, pda2);
        // Different vc_hash → different PDA
        let other_hash = [0u8; 32];
        let (pda3, _) = Pubkey::find_program_address(
            &[b"revoked-vc", &other_hash],
            &crate::id(),
        );
        assert_ne!(pda1, pda3);
    }

    #[test]
    fn test_merchant_did_account_space() {
        // 8(discriminator) + 32 + 32 + 32 + 32 + 8 + 8 + 1 = 153
        assert_eq!(
            8 + std::mem::size_of::<Pubkey>() * 3 + 32 + 8 + 8 + 1,
            153
        );
    }
}
