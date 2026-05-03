use solana_sdk::pubkey::Pubkey;

/// Derive the global state PDA: `[b"global_state", buyer]`
pub fn derive_global_state_pda(
    program_id: &Pubkey,
    buyer: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[b"global_state", buyer.as_ref()],
        program_id,
    )
}

/// Derive the global vault PDA: `[b"global_buyer_vault", buyer]`
pub fn derive_global_vault_pda(
    program_id: &Pubkey,
    buyer: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[b"global_buyer_vault", buyer.as_ref()],
        program_id,
    )
}

/// Derive the channel PDA: `[b"channel", buyer, merchant]`
pub fn derive_channel_pda(
    program_id: &Pubkey,
    buyer: &Pubkey,
    merchant: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[b"channel", buyer.as_ref(), merchant.as_ref()],
        program_id,
    )
}

/// Derive the settlement escrow PDA: `[b"settlement", channel, nonce_le]`
pub fn derive_settlement_pda(
    program_id: &Pubkey,
    channel: &Pubkey,
    nonce: u64,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[
            b"settlement",
            channel.as_ref(),
            &nonce.to_le_bytes(),
        ],
        program_id,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program_id() -> Pubkey {
        Pubkey::new_from_array([1u8; 32])
    }

    #[test]
    fn test_global_state_pda_deterministic() {
        let buyer = Pubkey::new_unique();
        let (p1, b1) = derive_global_state_pda(&program_id(), &buyer);
        let (p2, b2) = derive_global_state_pda(&program_id(), &buyer);
        assert_eq!(p1, p2);
        assert_eq!(b1, b2);
    }

    #[test]
    fn test_global_vault_pda_different_from_state() {
        let buyer = Pubkey::new_unique();
        let (state_pda, _) = derive_global_state_pda(&program_id(), &buyer);
        let (vault_pda, _) = derive_global_vault_pda(&program_id(), &buyer);
        assert_ne!(state_pda, vault_pda);
    }

    #[test]
    fn test_channel_pda_different_buyers() {
        let merchant = Pubkey::new_unique();
        let buyer1 = Pubkey::new_unique();
        let buyer2 = Pubkey::new_unique();
        let (p1, _) = derive_channel_pda(&program_id(), &buyer1, &merchant);
        let (p2, _) = derive_channel_pda(&program_id(), &buyer2, &merchant);
        assert_ne!(p1, p2);
    }

    #[test]
    fn test_channel_pda_different_merchants() {
        let buyer = Pubkey::new_unique();
        let merchant1 = Pubkey::new_unique();
        let merchant2 = Pubkey::new_unique();
        let (p1, _) = derive_channel_pda(&program_id(), &buyer, &merchant1);
        let (p2, _) = derive_channel_pda(&program_id(), &buyer, &merchant2);
        assert_ne!(p1, p2);
    }

    #[test]
    fn test_settlement_pda_nonce_dependent() {
        let channel = Pubkey::new_unique();
        let (p1, _) = derive_settlement_pda(&program_id(), &channel, 0);
        let (p2, _) = derive_settlement_pda(&program_id(), &channel, 1);
        assert_ne!(p1, p2);
    }

    #[test]
    fn test_bump_is_valid() {
        let buyer = Pubkey::new_unique();
        let (_, bump) = derive_global_state_pda(&program_id(), &buyer);
        assert!((1..=255).contains(&bump));
    }
}
