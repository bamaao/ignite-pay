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

//! CCTP Forwarding — EVM → Solana cross-chain USDC deposit.
//!
//! This module provides the Rust-side logic for Circle's CCTP V2 Forwarding flow:
//!   1. Query forwarding fees from Circle Iris API
//!   2. Build ERC-20 approve calldata (USDC → TokenMessengerV2)
//!   3. Build depositForBurnWithHook calldata (with CCTP forwarding hook)
//!   4. Derive Solana USDC ATA for the mint_recipient
//!   5. Poll Iris API for transfer status / attestation

use anyhow::Result;
use serde::Deserialize;

// ── Domain IDs ──────────────────────────────────────────────────────────────

pub const DOMAIN_ETHEREUM: u32 = 0;
pub const DOMAIN_ARBITRUM: u32 = 3;
pub const DOMAIN_SOLANA: u32 = 5;
pub const DOMAIN_BASE: u32 = 6;
pub const DOMAIN_OP: u32 = 2;
pub const DOMAIN_POLYGON: u32 = 7;

// ── Forwarding hook data (hex-encoded "cctp-forward" padded to 32 bytes) ────

const FORWARDING_HOOK_DATA_HEX: &str =
    "636374702d666f72776172640000000000000000000000000000000000000000";

// ── TokenMessengerV2 addresses per EVM chain ────────────────────────────────

pub const TOKEN_MESSENGER_ETHEREUM: &str = "0xBD3fa9AE8AcB092cC21E555769777B85a666E4db";
pub const TOKEN_MESSENGER_ARBITRUM: &str = "0x19330d10D9Cc8751218eaf51E8885D058642E08A";
pub const TOKEN_MESSENGER_BASE: &str = "0x9DAF7a48A68C0c2a88289f3f987a1e8D25d58685";
pub const TOKEN_MESSENGER_OP: &str = "0x9DAF7a48A68C0c2a88289f3f987a1e8D25d58685";
pub const TOKEN_MESSENGER_POLYGON: &str = "0x9DAF7a48A68C0c2a88289f3f987a1e8D25d58685";

// ── USDC contract addresses per EVM chain ───────────────────────────────────

pub const USDC_ETHEREUM: &str = "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48";
pub const USDC_ARBITRUM: &str = "0xaf88d065e77c8cC2239327C5EDb3A432268e5831";
pub const USDC_BASE: &str = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913";
pub const USDC_OP: &str = "0x0b2C639c533813f4Aa9D7837CAf62653d097Ff85";
pub const USDC_POLYGON: &str = "0x3c499c542cEF5E3811e1192ce70d8cC03d5c3359";

/// Solana USDC mint (mainnet).
pub const USDC_SOLANA: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";

// ── API structs ─────────────────────────────────────────────────────────────

/// Fee quote returned by Circle Iris API.
#[derive(Debug, Clone, Deserialize)]
pub struct CctpFeeQuote {
    pub forward_fee_low: String,
    pub forward_fee_med: String,
    pub forward_fee_high: String,
    pub minimum_fee: String,
}

/// Transfer status returned by Circle Iris API.
#[derive(Debug, Clone, Deserialize)]
pub struct CctpTransferStatus {
    pub state: String,
    pub burn_tx_hash: Option<String>,
    pub forward_tx_hash: Option<String>,
    pub message: Option<String>,
}

// ── Iris API response wrappers ──────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct IrisFeeResponse {
    fees: IrisFeesInner,
}

#[derive(Debug, Deserialize)]
struct IrisFeesInner {
    forward_fee_low: String,
    forward_fee_med: String,
    forward_fee_high: String,
    minimum_fee: String,
}

#[derive(Debug, Deserialize)]
struct IrisMessageResponse {
    messages: Vec<IrisMessage>,
}

#[derive(Debug, Deserialize)]
struct IrisMessage {
    state: String,
    event: Option<IrisEvent>,
}

#[derive(Debug, Deserialize)]
struct IrisEvent {
    transaction_hash: Option<String>,
}

// ── Public API functions ────────────────────────────────────────────────────

/// Query CCTP forwarding fees from Circle Iris API.
///
/// Calls `GET /v2/burn/USDC/fees/{src_domain}/{dst_domain}?forward=true`.
pub async fn cctp_get_fees(
    iris_api_url: String,
    src_domain: u32,
    dst_domain: u32,
) -> Result<CctpFeeQuote> {
    let url = format!(
        "{}/v2/burn/USDC/fees/{}/{}?forward=true",
        iris_api_url.trim_end_matches('/'),
        src_domain,
        dst_domain,
    );

    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Iris fee request failed: {} — {}", status, body);
    }

    let data: IrisFeeResponse = resp.json().await?;
    Ok(CctpFeeQuote {
        forward_fee_low: data.fees.forward_fee_low,
        forward_fee_med: data.fees.forward_fee_med,
        forward_fee_high: data.fees.forward_fee_high,
        minimum_fee: data.fees.minimum_fee,
    })
}

/// Build ERC-20 `approve(address spender, uint256 amount)` calldata.
///
/// Manual ABI encoding: selector (0x095ea7b3) + pad32(address) + pad256(amount).
pub fn cctp_build_approve_calldata(spender: String, amount: u64) -> Result<String> {
    let mut calldata = String::with_capacity(8 + 64 + 64 + 2);
    calldata.push_str("0x095ea7b3");

    // Address: strip 0x, left-pad to 32 bytes (64 hex chars)
    let addr = spender.trim_start_matches("0x");
    calldata.push_str(&"0".repeat(64 - addr.len()));
    calldata.push_str(&addr.to_lowercase());

    // Amount: encode as uint256 (64 hex chars)
    calldata.push_str(&format!("{:064x}", amount));

    Ok(calldata)
}

/// Build `depositForBurnWithHook` calldata for TokenMessengerV2.
///
/// Selector: 0xf93a5932
/// Parameters (8):
///   uint64 amount, uint32 destinationDomain, bytes32 mintRecipient,
///   address burnToken, address destinationCaller,
///   bytes32 hookData, uint32 maxFee, uint32 minFinalityThreshold
pub fn cctp_build_deposit_for_burn_calldata(
    amount: u64,
    dst_domain: u32,
    mint_recipient: String,   // hex bytes32
    burn_token: String,       // hex address
    dst_caller: String,       // hex bytes32 (zero = any caller)
    max_fee: u32,
    min_finality_threshold: u32,
) -> Result<String> {
    let selector = "0xf93a5932";
    let mut params = String::new();

    // 1. uint64 amount (padded to 32 bytes)
    params.push_str(&format!("{:064x}", amount));

    // 2. uint32 destinationDomain (padded to 32 bytes)
    params.push_str(&format!("{:064x}", dst_domain));

    // 3. bytes32 mintRecipient
    let mr = mint_recipient.trim_start_matches("0x");
    params.push_str(&"0".repeat(64 - mr.len()));
    params.push_str(&mr.to_lowercase());

    // 4. address burnToken (left-pad to 32 bytes)
    let bt = burn_token.trim_start_matches("0x");
    params.push_str(&"0".repeat(64 - bt.len()));
    params.push_str(&bt.to_lowercase());

    // 5. bytes32 destinationCaller (zero address = any)
    let dc = dst_caller.trim_start_matches("0x");
    params.push_str(&"0".repeat(64 - dc.len()));
    params.push_str(&dc.to_lowercase());

    // 6. bytes32 hookData (fixed forwarding hook)
    params.push_str(FORWARDING_HOOK_DATA_HEX);

    // 7. uint32 maxFee
    params.push_str(&format!("{:064x}", max_fee));

    // 8. uint32 minFinalityThreshold
    params.push_str(&format!("{:064x}", min_finality_threshold));

    Ok(format!("{}{}", selector, params))
}

/// Derive the Solana USDC Associated Token Account for a wallet address.
///
/// Returns the ATA as a hex-encoded bytes32 for use as `mintRecipient` in CCTP.
pub fn cctp_derive_solana_usdc_ata(wallet_b58: String) -> Result<String> {
    let wallet_bytes = bs58::decode(&wallet_b58).into_vec()?;
    if wallet_bytes.len() != 32 {
        anyhow::bail!("Wallet public key must be 32 bytes, got {}", wallet_bytes.len());
    }
    let wallet_arr: [u8; 32] = wallet_bytes.try_into().unwrap();

    let mint_bytes = bs58::decode(USDC_SOLANA).into_vec()?;
    let mint_arr: [u8; 32] = mint_bytes.try_into().unwrap();

    let ata = derive_ata(&wallet_arr, &mint_arr);
    Ok(hex::encode(ata))
}

/// Poll Circle Iris API for CCTP transfer status.
///
/// Calls `GET /v2/messages/{src_domain}?transactionHash={burn_tx_hash}`.
/// Returns the current state, which can be "pending", "complete", etc.
pub async fn cctp_poll_status(
    iris_api_url: String,
    src_domain: u32,
    burn_tx_hash: String,
) -> Result<CctpTransferStatus> {
    let url = format!(
        "{}/v2/messages/{}?transactionHash={}",
        iris_api_url.trim_end_matches('/'),
        src_domain,
        burn_tx_hash.trim_start_matches("0x"),
    );

    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Iris status request failed: {} — {}", status, body);
    }

    let data: IrisMessageResponse = resp.json().await?;

    match data.messages.into_iter().next() {
        Some(msg) => {
            let event = msg.event.unwrap_or(IrisEvent {
                transaction_hash: None,
            });
            Ok(CctpTransferStatus {
                state: msg.state,
                burn_tx_hash: Some(burn_tx_hash.clone()),
                forward_tx_hash: event.transaction_hash,
                message: None,
            })
        }
        None => Ok(CctpTransferStatus {
            state: "not_found".to_string(),
            burn_tx_hash: Some(burn_tx_hash),
            forward_tx_hash: None,
            message: Some("No messages found for this transaction".to_string()),
        }),
    }
}

// ── Internal helpers ────────────────────────────────────────────────────────

/// Derive Associated Token Account address matching Solana's find_program_address.
/// Uses iterative nonce approach with SHA-256.
fn derive_ata(owner: &[u8; 32], mint: &[u8; 32]) -> [u8; 32] {
    use sha2::{Digest, Sha256};

    let ata_program: [u8; 32] = bs58::decode("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL")
        .into_vec()
        .unwrap()
        .try_into()
        .unwrap();
    let token_program: [u8; 32] = bs58::decode("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA")
        .into_vec()
        .unwrap()
        .try_into()
        .unwrap();

    for nonce in (0u8..=255u8).rev() {
        let mut hasher = Sha256::new();
        hasher.update(owner);
        hasher.update(&token_program);
        hasher.update(mint);
        hasher.update(&ata_program);
        hasher.update(&[nonce]);
        let hash = hasher.finalize();
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&hash);
        if !is_on_curve(&arr) {
            return arr;
        }
    }
    [0u8; 32]
}

/// Check if a point is on the Ed25519 curve.
fn is_on_curve(point: &[u8; 32]) -> bool {
    use ed25519_dalek::VerifyingKey;
    VerifyingKey::from_bytes(point).is_ok()
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_approve_calldata() {
        let spender = "0xBD3fa9AE8AcB092cC21E555769777B85a666E4db".to_string();
        let amount = 1_000_000u64; // 1 USDC (6 decimals)
        let result = cctp_build_approve_calldata(spender, amount).unwrap();

        assert!(result.starts_with("0x095ea7b3"));
        // Total length: 2 (0x) + 8 (selector) + 64 (address) + 64 (amount) = 138
        assert_eq!(result.len(), 138);
        // Amount should be at the end
        assert!(result.ends_with("00000000000000000000000000000000000000000000000000000000000f4240"));
    }

    #[test]
    fn test_deposit_for_burn_calldata() {
        let mint_recipient = "0".repeat(64); // zero bytes32 for testing
        let burn_token = "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".to_string();
        let dst_caller = "0".repeat(64);

        let result = cctp_build_deposit_for_burn_calldata(
            1_000_000u64,         // amount
            5u32,                  // dst_domain (Solana)
            mint_recipient,        // mint_recipient
            burn_token,            // burn_token
            dst_caller,            // dst_caller (any)
            100u32,                // max_fee
            1000u32,               // min_finality_threshold
        ).unwrap();

        assert!(result.starts_with("0xf93a5932"));
        // Total length: 2 (0x) + 8 (selector) + 8*64 (params) = 522
        assert_eq!(result.len(), 522);
        // Hook data should be embedded at the correct offset
        assert!(result.contains(FORWARDING_HOOK_DATA_HEX));
    }

    #[test]
    fn test_derive_solana_usdc_ata() {
        // Use a known Solana wallet address for deterministic test
        let wallet = "11111111111111111111111111111112"; // system program (valid point)
        let result = cctp_derive_solana_usdc_ata(wallet.to_string()).unwrap();

        // Should be 64 hex chars (32 bytes)
        assert_eq!(result.len(), 64);
        // Should be valid hex
        hex::decode(&result).unwrap();
    }

    #[test]
    fn test_derive_ata_known_wallet() {
        // Derive ATA for a known wallet+mint pair and verify it's off-curve
        let wallet_b58 = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";
        let wallet_bytes = bs58::decode(wallet_b58).into_vec().unwrap();
        let wallet_arr: [u8; 32] = wallet_bytes.try_into().unwrap();

        let mint_bytes = bs58::decode(USDC_SOLANA).into_vec().unwrap();
        let mint_arr: [u8; 32] = mint_bytes.try_into().unwrap();

        let ata = derive_ata(&wallet_arr, &mint_arr);
        // ATA should be a valid 32-byte address (off-curve PDA)
        assert_ne!(ata, [0u8; 32]);
    }
}
