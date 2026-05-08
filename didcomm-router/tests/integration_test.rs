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

use didcomm_router::config::{Config, FcmConfig, RouterConfig, ServerConfig, StorageConfig, TlsConfig};
use didcomm_router::state::RouterState;

fn test_config() -> Config {
    Config {
        server: ServerConfig {
            host: "127.0.0.1".to_string(),
            port: 0, // Use port 0 for testing (OS assigns)
        },
        router: RouterConfig {
            max_queued_messages: 100,
            max_message_age_seconds: 3600,
            known_peers: vec![],
            jwt_secret: "test-secret".to_string(),
        },
        tls: TlsConfig::default(),
        fcm: FcmConfig::default(),
        storage: StorageConfig { path: None },
    }
}

#[tokio::test]
async fn test_router_state_creation() {
    let config = test_config();
    let state = RouterState::new(config).unwrap();
    // Verify state was created successfully (no DID agent needed)
    assert!(state.sessions.is_online("did:test:nobody") == false);
}

#[tokio::test]
async fn test_session_register_unregister() {
    let config = test_config();
    let state = RouterState::new(config).unwrap();

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    state.sessions.register("did:test:alice".to_string(), tx);

    assert!(state.sessions.is_online("did:test:alice"));
    assert!(!state.sessions.is_online("did:test:bob"));

    // Send a message
    state
        .sessions
        .send_to("did:test:alice", "hello")
        .unwrap();
    let msg = rx.try_recv().unwrap();
    match msg {
        axum::extract::ws::Message::Text(t) => assert_eq!(t, "hello"),
        _ => panic!("Expected text message"),
    }

    state.sessions.unregister("did:test:alice");
    assert!(!state.sessions.is_online("did:test:alice"));
}

#[tokio::test]
async fn test_keylist_store() {
    let config = test_config();
    let state = RouterState::new(config).unwrap();

    // Add a keylist entry
    state
        .keylist_store
        .add_key("did:test:alice", "did:test:alice#key-1")
        .await
        .unwrap();

    // Resolve session
    let session = state
        .keylist_store
        .resolve_session("did:test:alice#key-1")
        .await
        .unwrap();
    assert_eq!(session.as_deref(), Some("did:test:alice"));

    // List keys
    let keys = state.keylist_store.list_keys("did:test:alice").await.unwrap();
    assert!(keys.contains("did:test:alice#key-1"));

    // Remove
    state
        .keylist_store
        .remove_key("did:test:alice", "did:test:alice#key-1")
        .await
        .unwrap();
    let session = state
        .keylist_store
        .resolve_session("did:test:alice#key-1")
        .await
        .unwrap();
    assert!(session.is_none());
}

#[tokio::test]
async fn test_message_store_enqueue_dequeue() {
    let config = test_config();
    let state = RouterState::new(config).unwrap();

    let msg = didcomm_router::storage::QueuedMessage {
        id: "msg-1".to_string(),
        sender_did: "did:test:bob".to_string(),
        recipient_did: "did:test:alice".to_string(),
        encrypted_envelope: r#"{"ciphertext":"abc"}"#.to_string(),
        queued_at: chrono::Utc::now(),
    };

    state.message_store.enqueue("did:test:alice", msg).await.unwrap();
    assert_eq!(state.message_store.count("did:test:alice").await.unwrap(), 1);

    let batch = state
        .message_store
        .dequeue_batch("did:test:alice", 10)
        .await
        .unwrap();
    assert_eq!(batch.len(), 1);
    assert_eq!(batch[0].id, "msg-1");

    // Queue should be empty now
    assert_eq!(state.message_store.count("did:test:alice").await.unwrap(), 0);
}

#[tokio::test]
async fn test_mediate_request_response() {
    let config = test_config();
    let state = RouterState::new(config).unwrap();

    // Register a session for Alice
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    state.sessions.register("did:test:alice".to_string(), tx);

    // Create a mediate-request message
    let msg = serde_json::from_str(r#"{
        "id": "msg-1",
        "type": "https://didcomm.org/coordinate-mediation/2.0/mediate-request",
        "from": "did:test:alice",
        "body": {}
    }"#).unwrap();

    didcomm_router::protocols::coordinate_mediation::handle_mediate_request(
        &msg,
        &state,
        Some("did:test:alice"),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn test_forward_queuing() {
    let config = test_config();
    let state = RouterState::new(config).unwrap();

    // Register Alice's key with the router
    state
        .keylist_store
        .add_key("did:test:alice", "did:test:alice#key-1")
        .await
        .unwrap();

    // Create a forward message
    let msg = serde_json::from_str(r#"{
        "id": "fwd-1",
        "type": "https://didcomm.org/routing/2.0/forward",
        "body": { "next": "did:test:alice#key-1" },
        "attachments": [{
            "data": { "json": {"ciphertext": "encrypted-payload"} }
        }]
    }"#).unwrap();

    // Alice is offline, so the message should be queued
    didcomm_router::protocols::routing::handle_forward(&msg, &state)
        .await
        .unwrap();

    let count = state
        .message_store
        .count("did:test:alice#key-1")
        .await
        .unwrap();
    assert_eq!(count, 1);
}
