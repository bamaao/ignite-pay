use ignite_pay_solana::session::SessionManager;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signer::Signer;

fn temp_db() -> sled::Db {
    let dir = tempfile::tempdir().unwrap();
    sled::open(dir.path()).unwrap()
}

#[test]
fn test_register_session_key() {
    let db = temp_db();
    let mgr = SessionManager::new(db).unwrap();

    let owner = Pubkey::new_unique();
    let target = solana_sdk::system_program::id();

    let session = mgr
        .create_session(
            &owner,
            &target,
            vec!["sol:transfer".into()],
            1_000_000,
            3600,
        )
        .unwrap();

    // Verify session data
    assert_eq!(session.session_data.owner, owner);
    assert_eq!(session.session_data.ephemeral_signer, session.keypair.pubkey());
    assert_eq!(session.session_data.target_program, target);
    assert_eq!(session.session_data.spending_limit, 1_000_000);
    assert_eq!(session.session_data.current_spent, 0);
    assert!(!mgr.is_expired(&session.session_data));

    // Verify session is retrievable by owner
    let loaded = mgr.get_active_session(&owner).unwrap().unwrap();
    assert_eq!(loaded.keypair.pubkey(), session.keypair.pubkey());

    // Verify session is retrievable by pubkey
    let by_pubkey = mgr
        .get_session_by_pubkey(&session.keypair.pubkey())
        .unwrap()
        .unwrap();
    assert_eq!(by_pubkey.keypair.pubkey(), session.keypair.pubkey());
}

#[test]
fn test_execute_payment_via_session() {
    let db = temp_db();
    let mgr = SessionManager::new(db).unwrap();

    let owner = Pubkey::new_unique();
    let session = mgr
        .create_session(
            &owner,
            &solana_sdk::system_program::id(),
            vec!["sol:transfer".into()],
            10_000,
            3600,
        )
        .unwrap();

    // Simulate spending
    assert!(mgr.check_spending_limit(&session.session_data, 5_000));
    mgr.record_spent(&session.keypair.pubkey(), 5_000).unwrap();

    // Reload and verify
    let loaded = mgr.get_active_session(&owner).unwrap().unwrap();
    assert_eq!(loaded.session_data.current_spent, 5_000);

    // Can spend remaining
    assert!(mgr.check_spending_limit(&loaded.session_data, 5_000));

    // Cannot exceed limit
    assert!(!mgr.check_spending_limit(&loaded.session_data, 5_001));
}

#[test]
fn test_spending_limit_enforced() {
    let db = temp_db();
    let mgr = SessionManager::new(db).unwrap();

    let owner = Pubkey::new_unique();
    let session = mgr
        .create_session(
            &owner,
            &solana_sdk::system_program::id(),
            vec!["sol:transfer".into()],
            1_000,
            3600,
        )
        .unwrap();

    // Spending exactly at limit should be allowed
    assert!(mgr.check_spending_limit(&session.session_data, 1_000));

    // Spending over limit should be denied
    assert!(!mgr.check_spending_limit(&session.session_data, 1_001));

    // After spending 500, remaining 500 should work
    mgr.record_spent(&session.keypair.pubkey(), 500).unwrap();
    let loaded = mgr.get_active_session(&owner).unwrap().unwrap();
    assert!(mgr.check_spending_limit(&loaded.session_data, 500));
    assert!(!mgr.check_spending_limit(&loaded.session_data, 501));
}

#[test]
fn test_expired_session_rejected() {
    let db = temp_db();
    let mgr = SessionManager::new(db).unwrap();

    let owner = Pubkey::new_unique();
    // Create session that expires immediately
    let _session = mgr
        .create_session(
            &owner,
            &solana_sdk::system_program::id(),
            vec!["sol:transfer".into()],
            10_000,
            0,
        )
        .unwrap();

    // get_active_session should not return expired sessions
    let result = mgr.get_active_session(&owner).unwrap();
    assert!(result.is_none());
}