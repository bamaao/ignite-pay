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
use state::MerchantCompressedDid;

declare_id!("D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D");

/// Compile-time CPI signer derived from the program ID.
/// Seeds: [b"authority"] relative to the program ID.
pub const LIGHT_CPI_SIGNER: CpiSigner =
    derive_light_cpi_signer!("D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D");

/// Generic Anchor accounts struct for all compressed DID instructions.
/// Compressed accounts are passed via `ctx.remaining_accounts` and
/// assembled into `CpiAccounts` at runtime.
#[derive(Accounts)]
pub struct GenericAnchorAccounts<'info> {
    #[account(mut)]
    pub signer: Signer<'info>,
}

#[program]
pub mod ignite_pay_did_program {
    use super::*;

    // ─── 1. initialize_did ───

    /// Create a new compressed merchant DID.
    /// Derives a compressed PDA address from [b"merchant-did", original_pk]
    /// and creates it in the output state tree via Light System Program CPI.
    pub fn initialize_did<'info>(
        ctx: Context<'_, '_, '_, 'info, GenericAnchorAccounts<'info>>,
        proof: light_sdk::borsh_compat::ValidityProof,
        address_tree_info: PackedAddressTreeInfo,
        output_state_tree_index: u8,
    ) -> Result<()> {
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
        did.vc_hash = [0u8; 32];
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
    pub fn update_did_with_vc<'info>(
        ctx: Context<'_, '_, '_, 'info, GenericAnchorAccounts<'info>>,
        proof: light_sdk::borsh_compat::ValidityProof,
        current_did: MerchantCompressedDid,
        account_meta: CompressedAccountMeta,
        vc_hash: [u8; 32],
        nonce: u64,
    ) -> Result<()> {
        // Verify controller authorization
        require!(
            current_did.controller_pk == ctx.accounts.signer.key(),
            DidError::InvalidControllerKey
        );
        // Anti-replay: caller must supply the current nonce
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
}
