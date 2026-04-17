pub mod state;
pub mod error;

use anchor_lang::prelude::*;
use error::DidError;
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
use state::{MerchantCompressedDid, PlatformConfig, RevokedVc};

declare_id!("D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D");

/// Compile-time CPI signer derived from the program ID.
/// Seeds: [b"authority"] relative to the program ID.
pub const LIGHT_CPI_SIGNER: CpiSigner =
    derive_light_cpi_signer!("D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D");

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

/// Generic Anchor accounts struct for compressed DID instructions
/// that do NOT require platform signature verification.
#[derive(Accounts)]
pub struct GenericAnchorAccounts<'info> {
    #[account(mut)]
    pub signer: Signer<'info>,
}

/// Anchor accounts struct for DID instructions that require
/// platform signature verification (initialize_did, update_did_with_vc).
#[derive(Accounts)]
pub struct DidWithPlatformAccounts<'info> {
    #[account(mut)]
    pub signer: Signer<'info>,
    /// Platform config PDA: must be initialized via `init_platform`.
    #[account(
        seeds = [b"platform-config"],
        bump = platform_config.bump,
        constraint = platform_config.platform_ed25519_pubkey != [0u8; 32] @ DidError::PlatformNotInitialized
    )]
    pub platform_config: Account<'info, PlatformConfig>,
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

#[program]
pub mod ignite_pay_did_program {
    use super::*;

    // ─── 0. init_platform ───

    /// One-time initialization of the platform Ed25519 public key.
    /// Stores the platform's verifying key in a PDA so on-chain instructions
    /// can verify platform signatures before accepting VC bindings.
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

    // ─── 1. initialize_did ───

    /// Create a new compressed merchant DID.
    /// Derives a compressed PDA address from [b"merchant-did", original_pk]
    /// and creates it in the output state tree via Light System Program CPI.
    /// Requires a valid platform signature over (signer_pk || vc_hash) to
    /// prevent replay attacks and identity spoofing.
    pub fn initialize_did<'info>(
        ctx: Context<'_, '_, '_, 'info, DidWithPlatformAccounts<'info>>,
        proof: light_sdk::borsh_compat::ValidityProof,
        address_tree_info: PackedAddressTreeInfo,
        output_state_tree_index: u8,
        vc_hash: [u8; 32],
        platform_signature: [u8; 64],
        credential_subject_pk: Pubkey,
    ) -> Result<()> {
        // Subject binding: credential subject must be the signer
        require!(
            credential_subject_pk == ctx.accounts.signer.key(),
            DidError::VcSubjectMismatch
        );

        // Verify platform signature: sign(credential_subject_pk || vc_hash)
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

        // Derive deterministic compressed PDA address
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

    // ─── 2. update_did_with_vc ───

    /// Bind or update a platform verifiable credential hash on an existing
    /// compressed DID. Requires the current controller as signer.
    /// Verifies platform signature over (credential_subject_pk || vc_hash)
    /// and enforces subject binding on-chain.
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
        // Verify controller authorization
        require!(
            current_did.controller_pk == ctx.accounts.signer.key(),
            DidError::InvalidControllerKey
        );
        // Anti-replay: caller must supply the current nonce
        require!(current_did.nonce == nonce, DidError::NonceMismatch);

        // Subject binding: credential subject must be the signer
        require!(
            credential_subject_pk == ctx.accounts.signer.key(),
            DidError::VcSubjectMismatch
        );

        // Verify platform signature: sign(credential_subject_pk || vc_hash)
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

    // ─── 3. set_recovery_key ───

    /// Set or change the recovery public key on a compressed DID.
    /// Requires the current controller as signer + valid nonce.
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

    // ─── 4. recover_controller ───

    /// Recover controller by proving ownership of the recovery key.
    /// Sets controller_pk to new_controller_pk.
    pub fn recover_controller<'info>(
        ctx: Context<'_, '_, '_, 'info, GenericAnchorAccounts<'info>>,
        proof: light_sdk::borsh_compat::ValidityProof,
        current_did: MerchantCompressedDid,
        account_meta: CompressedAccountMeta,
        new_controller_pk: Pubkey,
        nonce: u64,
    ) -> Result<()> {
        // Verify recovery key authorization
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

    // ─── 5. revoke_vc ───

    /// Revoke a VC by creating an on-chain RevokedVc PDA.
    /// Only the platform authority (stored in PlatformConfig) can invoke.
    /// Verifiers check PDA existence to determine revocation status.
    /// Seeds: [b"revoked-vc", vc_hash]
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
}
