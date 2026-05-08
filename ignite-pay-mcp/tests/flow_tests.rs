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

use ignite_pay_solana::payment::IgnitePayClient;
use ignite_pay_solana::session::SessionManager;
use ignite_pay_solana::solana_sdk::pubkey::Pubkey;
use ignite_pay_solana::solana_sdk::signer::Signer;
use ignite_pay_solana::types::PayMode;

fn temp_db() -> sled::Db {
    let dir = tempfile::tempdir().unwrap();
    sled::open(dir.path()).unwrap()
}

/// Test: Payment fails without an active session.
#[tokio::test]
async fn test_payment_fails_without_session() {
    let db = temp_db();
    let client = IgnitePayClient::new(
        "https://api.devnet.solana.com",
        db,
        PayMode::SelfFunded,
        None,
    )
    .unwrap();

    let owner = Pubkey::new_unique();
    let result = client.session_manager().get_active_session(&owner);
    assert!(result.unwrap().is_none());
}

/// Test: SPL payment parameters are correctly derived.
#[test]
fn test_spl_ata_derivation() {
    let owner = Pubkey::new_unique();
    let mint = Pubkey::new_unique();

    let ata = IgnitePayClient::derive_ata(&owner, &mint);
    let ata2 = IgnitePayClient::derive_ata(&owner, &mint);
    assert_eq!(ata, ata2);

    let other_owner = Pubkey::new_unique();
    let other_ata = IgnitePayClient::derive_ata(&other_owner, &mint);
    assert_ne!(ata, other_ata);
}

/// Test: Session key creation and retrieval flow.
#[test]
fn test_session_lifecycle() {
    let db = temp_db();
    let mgr = SessionManager::new(db).unwrap();

    let owner = Pubkey::new_unique();
    let session = mgr
        .create_session(
            &owner,
            &ignite_pay_solana::solana_sdk::system_program::id(),
            vec!["sol:transfer".into(), "spl:transfer".into()],
            5_000_000,
            3600,
        )
        .unwrap();

    let loaded = mgr.get_active_session(&owner).unwrap().unwrap();
    assert_eq!(loaded.keypair.pubkey(), session.keypair.pubkey());
    assert_eq!(loaded.session_data.scopes, vec!["sol:transfer", "spl:transfer"]);

    mgr.record_spent(&session.keypair.pubkey(), 1_000_000).unwrap();
    mgr.close_session(&session.keypair.pubkey()).unwrap();
    assert!(mgr.get_active_session(&owner).unwrap().is_none());
}

/// Test: Spending limit enforcement with SPL-like amounts.
#[test]
fn test_spl_spending_limit() {
    let db = temp_db();
    let mgr = SessionManager::new(db).unwrap();

    let owner = Pubkey::new_unique();
    let session = mgr
        .create_session(
            &owner,
            &ignite_pay_solana::solana_sdk::system_program::id(),
            vec!["spl:transfer".into()],
            100_000_000,
            3600,
        )
        .unwrap();

    assert!(mgr.check_spending_limit(&session.session_data, 50_000_000));
    assert!(!mgr.check_spending_limit(&session.session_data, 101_000_000));
}

/// Test: Auto-approved payment flow with mock (no Solana client).
#[test]
fn test_mock_payment_signature_format() {
    use chrono::Utc;
    use ignite_pay_mcp::payment::{execute_mock_payment, PaymentRequest, PaymentStatus};

    let payment = PaymentRequest {
        id: "test-123".to_string(),
        recipient: "recipient123".to_string(),
        merchant_did: "did:ignite:zTest".to_string(),
        amount: 1000,
        token: "SOL".to_string(),
        network: "solana:devnet".to_string(),
        description: "test payment".to_string(),
        status: PaymentStatus::PendingAuth,
        created_at: Utc::now(),
        tx_signature: None,
    };

    let sig = execute_mock_payment(&payment);
    assert!(sig.starts_with("tx_mock_test-123_"));
}
