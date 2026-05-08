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

use ignite_pay_channel_service::config::{Config, Role};
use ignite_pay_channel_service::storage::{channel_store, peer_store};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::get;
use axum::Router;
use tower::ServiceExt;
use http_body_util::BodyExt;

// ── Config tests ──

#[test]
fn test_config_parse_full() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let toml_str = r#"
[server]
host = "127.0.0.1"
port = 3001

[solana]
rpc_url = "https://api.devnet.solana.com"
channel_program_id = "11111111111111111111111111111111"
keypair_path = ""

[channel]
default_tree_depth = 4
default_challenge_duration = 5000
default_min_challenge_delay = 1000
default_settle_window = 10000
auto_close_offset = 500000
db_path = "./test_data"

[compliance]
spending_threshold = 1000000000
per_channel_limit = 100000000
window_slots = 100000
travel_rule_threshold = 500000000
"#;
    std::fs::write(tmp.path(), toml_str).unwrap();

    let config = Config::load(tmp.path().to_str().unwrap()).unwrap();
    assert_eq!(config.server.host, "127.0.0.1");
    assert_eq!(config.server.port, 3001);
    assert_eq!(config.channel.default_tree_depth, 4);
    assert_eq!(config.channel.default_challenge_duration, 5000);
    assert!(config.compliance.is_some());
    let comp = config.compliance.unwrap();
    assert_eq!(comp.spending_threshold, 1_000_000_000);
}

#[test]
fn test_config_parse_without_compliance() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let toml_str = r#"
[server]
host = "0.0.0.0"
port = 3002

[solana]
rpc_url = "https://api.devnet.solana.com"
channel_program_id = "11111111111111111111111111111111"
keypair_path = ""

[channel]
default_tree_depth = 4
default_challenge_duration = 5000
default_min_challenge_delay = 1000
default_settle_window = 10000
auto_close_offset = 500000
db_path = "./test_data2"
"#;
    std::fs::write(tmp.path(), toml_str).unwrap();

    let config = Config::load(tmp.path().to_str().unwrap()).unwrap();
    assert!(config.compliance.is_none());
}

#[test]
fn test_config_bind_addr() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let toml_str = r#"
[server]
host = "192.168.1.1"
port = 8080

[solana]
rpc_url = "http://localhost:8899"
channel_program_id = "11111111111111111111111111111111"
keypair_path = ""

[channel]
default_tree_depth = 4
default_challenge_duration = 5000
default_min_challenge_delay = 1000
default_settle_window = 10000
auto_close_offset = 500000
db_path = "./test_data3"
"#;
    std::fs::write(tmp.path(), toml_str).unwrap();

    let config = Config::load(tmp.path().to_str().unwrap()).unwrap();
    assert_eq!(config.bind_addr(), "192.168.1.1:8080");
}

#[test]
fn test_config_invalid_toml() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), "not valid toml {{{{").unwrap();
    let result = Config::load(tmp.path().to_str().unwrap());
    assert!(result.is_err());
}

#[test]
fn test_config_missing_file() {
    let result = Config::load("/nonexistent/path/config.toml");
    assert!(result.is_err());
}

#[test]
fn test_role_serde_roundtrip() {
    let roles = vec![Role::User, Role::Provider, Role::Hub];
    for role in roles {
        let json = serde_json::to_string(&role).unwrap();
        let back: Role = serde_json::from_str(&json).unwrap();
        assert_eq!(role, back);
    }
}

// ── Storage: peer_store tests ──

#[test]
fn test_peer_store_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let db = sled::open(dir.path()).unwrap();

    let peer = peer_store::PeerInfo {
        pubkey: "AlicePubkey123".to_string(),
        endpoint: "ws://localhost:3001".to_string(),
        role: Role::User,
    };

    peer_store::store_peer(&db, &peer).unwrap();
    let loaded = peer_store::load_peer(&db, "AlicePubkey123").unwrap().unwrap();
    assert_eq!(loaded.pubkey, "AlicePubkey123");
    assert_eq!(loaded.endpoint, "ws://localhost:3001");
    assert_eq!(loaded.role, Role::User);
}

#[test]
fn test_peer_store_missing() {
    let dir = tempfile::tempdir().unwrap();
    let db = sled::open(dir.path()).unwrap();
    let result = peer_store::load_peer(&db, "nonexistent").unwrap();
    assert!(result.is_none());
}

#[test]
fn test_peer_store_list_multiple() {
    let dir = tempfile::tempdir().unwrap();
    let db = sled::open(dir.path()).unwrap();

    let peers = vec![
        peer_store::PeerInfo {
            pubkey: "pk1".to_string(),
            endpoint: "ws://a:3001".to_string(),
            role: Role::User,
        },
        peer_store::PeerInfo {
            pubkey: "pk2".to_string(),
            endpoint: "ws://b:3002".to_string(),
            role: Role::Provider,
        },
        peer_store::PeerInfo {
            pubkey: "pk3".to_string(),
            endpoint: "ws://c:3003".to_string(),
            role: Role::Hub,
        },
    ];

    for p in &peers {
        peer_store::store_peer(&db, p).unwrap();
    }

    let loaded = peer_store::list_peers(&db).unwrap();
    assert_eq!(loaded.len(), 3);
}

#[test]
fn test_peer_store_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let db = sled::open(dir.path()).unwrap();

    let peer_v1 = peer_store::PeerInfo {
        pubkey: "pk".to_string(),
        endpoint: "ws://old:3001".to_string(),
        role: Role::User,
    };
    peer_store::store_peer(&db, &peer_v1).unwrap();

    let peer_v2 = peer_store::PeerInfo {
        pubkey: "pk".to_string(),
        endpoint: "ws://new:3002".to_string(),
        role: Role::Hub,
    };
    peer_store::store_peer(&db, &peer_v2).unwrap();

    let loaded = peer_store::load_peer(&db, "pk").unwrap().unwrap();
    assert_eq!(loaded.endpoint, "ws://new:3002");
    assert_eq!(loaded.role, Role::Hub);
}

// ── Storage: channel_store tests ──

#[test]
fn test_channel_store_empty() {
    let dir = tempfile::tempdir().unwrap();
    let db = sled::open(dir.path()).unwrap();
    let ids = channel_store::list_channel_ids(&db).unwrap();
    assert!(ids.is_empty());
}

#[test]
fn test_channel_store_lists_from_channel_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let db = sled::open(dir.path()).unwrap();

    // Insert keys in the format that ChannelManager uses
    db.insert(b"channel:deadbeef:meta", b"some_value").unwrap();
    db.insert(b"channel:deadbeef:leaves", b"some_value").unwrap();
    db.insert(b"channel:cafecafe:meta", b"some_value").unwrap();
    db.insert(b"unrelated:key", b"value").unwrap();
    db.flush().unwrap();

    let ids = channel_store::list_channel_ids(&db).unwrap();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&"deadbeef".to_string()));
    assert!(ids.contains(&"cafecafe".to_string()));
}

#[test]
fn test_channel_store_deduplication() {
    let dir = tempfile::tempdir().unwrap();
    let db = sled::open(dir.path()).unwrap();

    db.insert(b"channel:abc123:meta", b"v1").unwrap();
    db.insert(b"channel:abc123:leaves", b"v2").unwrap();
    db.insert(b"channel:abc123:cosign", b"v3").unwrap();
    db.flush().unwrap();

    let ids = channel_store::list_channel_ids(&db).unwrap();
    assert_eq!(ids.len(), 1);
    assert_eq!(ids[0], "abc123");
}

// ── Error → HTTP status mapping tests ──

#[tokio::test]
async fn test_error_into_response_status_codes() {
    use ignite_pay_channel_service::error::ChannelServiceError;
    use axum::response::IntoResponse;

    let cases: Vec<(ChannelServiceError, StatusCode)> = vec![
        (ChannelServiceError::ChannelNotFound("x".into()), StatusCode::NOT_FOUND),
        (ChannelServiceError::BadRequest("x".into()), StatusCode::BAD_REQUEST),
        (ChannelServiceError::Unauthorized("x".into()), StatusCode::UNAUTHORIZED),
        (ChannelServiceError::ComplianceHold, StatusCode::FORBIDDEN),
        (ChannelServiceError::OnChain("x".into()), StatusCode::INTERNAL_SERVER_ERROR),
        (ChannelServiceError::SolanaRpc("x".into()), StatusCode::INTERNAL_SERVER_ERROR),
        (ChannelServiceError::Storage("x".into()), StatusCode::INTERNAL_SERVER_ERROR),
        (ChannelServiceError::PeerUnreachable("x".into()), StatusCode::INTERNAL_SERVER_ERROR),
        (ChannelServiceError::Internal("x".into()), StatusCode::INTERNAL_SERVER_ERROR),
    ];

    for (err, expected_status) in cases {
        let response = err.into_response();
        assert_eq!(response.status(), expected_status);
    }
}

#[tokio::test]
async fn test_error_response_body_contains_message() {
    use ignite_pay_channel_service::error::ChannelServiceError;
    use axum::response::IntoResponse;
    let err = ChannelServiceError::ChannelNotFound("test-channel-id".into());
    let response = err.into_response();
    let body = BodyExt::collect(response.into_body()).await.unwrap().to_bytes();
    let body_str = String::from_utf8(body.to_vec()).unwrap();
    assert!(body_str.contains("test-channel-id"));
    assert!(body_str.contains("error"));
}

// ── Health endpoint integration test ──

#[tokio::test]
async fn test_health_endpoint() {
    let app = Router::new().route("/health", get(ignite_pay_channel_service::handlers::health::health));

    let response = app
        .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = BodyExt::collect(response.into_body()).await.unwrap().to_bytes();
    let body_str = String::from_utf8(body.to_vec()).unwrap();
    assert!(body_str.contains("ok"));
}

// ── Router tests (build_router compiles for each role) ──

fn make_test_config(db_path: &str) -> Config {
    Config {
        server: ignite_pay_channel_service::config::ServerConfig {
            host: "127.0.0.1".to_string(),
            port: 0,
        },
        solana: ignite_pay_channel_service::config::SolanaConfig {
            rpc_url: "https://api.devnet.solana.com".to_string(),
            channel_program_id: "11111111111111111111111111111111".to_string(),
            keypair_path: String::new(),
        },
        channel: ignite_pay_channel_service::config::ChannelConfig {
            default_tree_depth: 4,
            default_challenge_duration: 5000,
            default_min_challenge_delay: 1000,
            default_settle_window: 10000,
            auto_close_offset: 500000,
            db_path: db_path.to_string(),
        },
        compliance: None,
        hub_registry: None,
    }
}

#[tokio::test]
async fn test_router_user_role_health() {
    let dir = tempfile::tempdir().unwrap();
    let config = make_test_config(dir.path().to_str().unwrap());
    let state = ignite_pay_channel_service::state::AppState::new(config, Role::User).unwrap();
    let app = ignite_pay_channel_service::server::router::build_router(state);

    let response = app
        .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_router_provider_role_health() {
    let dir = tempfile::tempdir().unwrap();
    let config = make_test_config(dir.path().to_str().unwrap());
    let state = ignite_pay_channel_service::state::AppState::new(config, Role::Provider).unwrap();
    let app = ignite_pay_channel_service::server::router::build_router(state);

    let response = app
        .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_router_hub_role_health() {
    let dir = tempfile::tempdir().unwrap();
    let config = make_test_config(dir.path().to_str().unwrap());
    let state = ignite_pay_channel_service::state::AppState::new(config, Role::Hub).unwrap();
    let app = ignite_pay_channel_service::server::router::build_router(state);

    let response = app
        .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_user_routes_include_channel_endpoints() {
    let dir = tempfile::tempdir().unwrap();
    let config = make_test_config(dir.path().to_str().unwrap());
    let state = ignite_pay_channel_service::state::AppState::new(config, Role::User).unwrap();
    let app = ignite_pay_channel_service::server::router::build_router(state);

    // POST /v1/channels/open should exist (will fail due to bad input but not 404)
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/channels/open")
                .header("content-type", "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Should not be 404 (route exists), but some error due to missing body
    assert_ne!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_provider_routes_include_list_channels() {
    let dir = tempfile::tempdir().unwrap();
    let config = make_test_config(dir.path().to_str().unwrap());
    let state = ignite_pay_channel_service::state::AppState::new(config, Role::Provider).unwrap();
    let app = ignite_pay_channel_service::server::router::build_router(state);

    // Test a known provider route: GET /v1/channels
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/channels")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_ne!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_provider_routes_with_path_param() {
    let dir = tempfile::tempdir().unwrap();
    let config = make_test_config(dir.path().to_str().unwrap());
    let state = ignite_pay_channel_service::state::AppState::new(config, Role::Provider).unwrap();
    let app = ignite_pay_channel_service::server::router::build_router(state);

    // Test POST /v1/channels/{id}/fund (provider route with path param, POST)
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/channels/aa/fund")
                .header("content-type", "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Should not be 404 — route exists (will error on missing body, but route exists)
    assert_ne!(response.status(), StatusCode::NOT_FOUND, "POST /v1/channels/{{id}}/fund route not found");
}

#[tokio::test]
async fn test_provider_get_channel_by_id() {
    let dir = tempfile::tempdir().unwrap();
    let config = make_test_config(dir.path().to_str().unwrap());
    let state = ignite_pay_channel_service::state::AppState::new(config, Role::Provider).unwrap();
    let app = ignite_pay_channel_service::server::router::build_router(state);

    // GET /v1/channels/:id — path param route for Provider
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/channels/aa")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Should not be 404 — route exists (will error on invalid hex length, but route exists)
    assert_ne!(response.status(), StatusCode::NOT_FOUND, "GET /v1/channels/:id should be registered for Provider");
}

#[tokio::test]
async fn test_hub_inherits_provider_routes() {
    let dir = tempfile::tempdir().unwrap();
    let config = make_test_config(dir.path().to_str().unwrap());
    let state = ignite_pay_channel_service::state::AppState::new(config, Role::Hub).unwrap();
    let app = ignite_pay_channel_service::server::router::build_router(state);

    // Hub should have provider routes: GET /v1/channels
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/channels")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_ne!(response.status(), StatusCode::NOT_FOUND, "Hub should inherit GET /v1/channels");
}

#[tokio::test]
async fn test_hub_path_param_routes() {
    let dir = tempfile::tempdir().unwrap();
    let config = make_test_config(dir.path().to_str().unwrap());
    let state = ignite_pay_channel_service::state::AppState::new(config, Role::Hub).unwrap();
    let app = ignite_pay_channel_service::server::router::build_router(state);

    // Hub should have GET /v1/multihop/:id (hub-specific path-param route)
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/multihop/aa")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_ne!(response.status(), StatusCode::NOT_FOUND, "Hub should have GET /v1/multihop/:id");
}

#[tokio::test]
async fn test_provider_missing_user_routes() {
    let dir = tempfile::tempdir().unwrap();
    let config = make_test_config(dir.path().to_str().unwrap());
    let state = ignite_pay_channel_service::state::AppState::new(config, Role::Provider).unwrap();
    let app = ignite_pay_channel_service::server::router::build_router(state);

    // Provider should NOT have POST /v1/channels/open (user-only)
    // Note: /v1/channels/open matches /v1/channels/:id with id="open",
    // so POST returns 405 (Method Not Allowed) rather than 404.
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/channels/open")
                .header("content-type", "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED, "Provider should not have POST /v1/channels/open");
}

#[tokio::test]
async fn test_provider_missing_hub_routes() {
    let dir = tempfile::tempdir().unwrap();
    let config = make_test_config(dir.path().to_str().unwrap());
    let state = ignite_pay_channel_service::state::AppState::new(config, Role::Provider).unwrap();
    let app = ignite_pay_channel_service::server::router::build_router(state);

    // Provider should NOT have hub routes
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/hub/register")
                .header("content-type", "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND, "Provider should not have POST /v1/hub/register");
}

#[tokio::test]
async fn test_hub_routes_include_hub_register() {
    let dir = tempfile::tempdir().unwrap();
    let config = make_test_config(dir.path().to_str().unwrap());
    let state = ignite_pay_channel_service::state::AppState::new(config, Role::Hub).unwrap();
    let app = ignite_pay_channel_service::server::router::build_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/hub/register")
                .header("content-type", "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_ne!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_user_role_missing_provider_routes() {
    let dir = tempfile::tempdir().unwrap();
    let config = make_test_config(dir.path().to_str().unwrap());
    let state = ignite_pay_channel_service::state::AppState::new(config, Role::User).unwrap();
    let app = ignite_pay_channel_service::server::router::build_router(state);

    // accept-payment is a provider-only route
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/channels/abc/accept-payment")
                .header("content-type", "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_user_role_missing_hub_routes() {
    let dir = tempfile::tempdir().unwrap();
    let config = make_test_config(dir.path().to_str().unwrap());
    let state = ignite_pay_channel_service::state::AppState::new(config, Role::User).unwrap();
    let app = ignite_pay_channel_service::server::router::build_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/hub/register")
                .header("content-type", "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

// ── AppState initialization tests ──

#[test]
fn test_appstate_user_no_hub_managers() {
    let dir = tempfile::tempdir().unwrap();
    let config = make_test_config(dir.path().to_str().unwrap());
    let state = ignite_pay_channel_service::state::AppState::new(config, Role::User).unwrap();

    assert!(state.hub_manager.is_none());
    assert!(state.route_service.is_none());
    assert!(state.multihop_manager.is_none());
    assert!(state.compliance_manager.is_none()); // No compliance config
}

#[test]
fn test_appstate_hub_has_hub_managers() {
    let dir = tempfile::tempdir().unwrap();
    let config = make_test_config(dir.path().to_str().unwrap());
    let state = ignite_pay_channel_service::state::AppState::new(config, Role::Hub).unwrap();

    assert!(state.hub_manager.is_some());
    assert!(state.route_service.is_some());
    assert!(state.multihop_manager.is_some());
}

#[test]
fn test_appstate_pubkey_consistent() {
    let dir = tempfile::tempdir().unwrap();
    let config = make_test_config(dir.path().to_str().unwrap());
    let state = ignite_pay_channel_service::state::AppState::new(config, Role::User).unwrap();

    let pk1 = state.pubkey();
    let pk2 = state.pubkey();
    assert_eq!(pk1, pk2);
}

#[test]
fn test_appstate_ed_keypair_matches_pubkey() {
    let dir = tempfile::tempdir().unwrap();
    let config = make_test_config(dir.path().to_str().unwrap());
    let state = ignite_pay_channel_service::state::AppState::new(config, Role::User).unwrap();

    let ed_kp = state.ed_keypair();
    let expected_pubkey = solana_sdk::pubkey::Pubkey::new_from_array(ed_kp.public.to_bytes());
    assert_eq!(state.pubkey(), expected_pubkey);
}

// ── WS Protocol serialization tests ──

#[test]
fn test_ws_message_auth_roundtrip() {
    let msg = ignite_pay_channel_service::ws::protocol::WsMessage::Auth {
        pubkey: "test_pk".to_string(),
        signature: vec![1u8; 64],
        timestamp: 12345,
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"type\":\"auth\""));
    assert!(json.contains("test_pk"));

    let back: ignite_pay_channel_service::ws::protocol::WsMessage =
        serde_json::from_str(&json).unwrap();
    if let ignite_pay_channel_service::ws::protocol::WsMessage::Auth { pubkey, timestamp, .. } = back {
        assert_eq!(pubkey, "test_pk");
        assert_eq!(timestamp, 12345);
    } else {
        panic!("Expected Auth variant");
    }
}

#[test]
fn test_ws_message_leaf_update_roundtrip() {
    let msg = ignite_pay_channel_service::ws::protocol::WsMessage::LeafUpdate {
        channel_id: "deadbeef".to_string(),
        sequence: 42,
        leaf_index: 3,
        prev_leaf_hash: vec![0xAA; 32],
        new_leaf: serde_json::json!({"owner": "pk123", "amount": 500}),
        signature: vec![0xBB; 64],
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: ignite_pay_channel_service::ws::protocol::WsMessage =
        serde_json::from_str(&json).unwrap();

    if let ignite_pay_channel_service::ws::protocol::WsMessage::LeafUpdate {
        channel_id, sequence, leaf_index, ..
    } = back
    {
        assert_eq!(channel_id, "deadbeef");
        assert_eq!(sequence, 42);
        assert_eq!(leaf_index, 3);
    } else {
        panic!("Expected LeafUpdate variant");
    }
}

#[test]
fn test_ws_message_error_roundtrip() {
    let msg = ignite_pay_channel_service::ws::protocol::WsMessage::Error {
        code: 400,
        message: "bad request".to_string(),
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"type\":\"error\""));

    let back: ignite_pay_channel_service::ws::protocol::WsMessage =
        serde_json::from_str(&json).unwrap();
    if let ignite_pay_channel_service::ws::protocol::WsMessage::Error { code, message } = back {
        assert_eq!(code, 400);
        assert_eq!(message, "bad request");
    } else {
        panic!("Expected Error variant");
    }
}

#[test]
fn test_ws_message_batch_result_roundtrip() {
    let msg = ignite_pay_channel_service::ws::protocol::WsMessage::BatchResult {
        channel_id: "abc".to_string(),
        applied: 5,
        failed_index: Some(3),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: ignite_pay_channel_service::ws::protocol::WsMessage =
        serde_json::from_str(&json).unwrap();
    if let ignite_pay_channel_service::ws::protocol::WsMessage::BatchResult {
        channel_id, applied, failed_index, ..
    } = back
    {
        assert_eq!(channel_id, "abc");
        assert_eq!(applied, 5);
        assert_eq!(failed_index, Some(3));
    } else {
        panic!("Expected BatchResult variant");
    }
}

#[test]
fn test_ws_message_multihop_init_roundtrip() {
    let msg = ignite_pay_channel_service::ws::protocol::WsMessage::MultihopInit {
        payment_id: vec![1, 2, 3, 4],
        hash_lock: vec![0xFF; 32],
        amount: 1000,
        timelock_slot: 500,
        next_hop: "hub_pk".to_string(),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: ignite_pay_channel_service::ws::protocol::WsMessage =
        serde_json::from_str(&json).unwrap();
    if let ignite_pay_channel_service::ws::protocol::WsMessage::MultihopInit {
        payment_id, amount, next_hop, ..
    } = back
    {
        assert_eq!(payment_id, vec![1, 2, 3, 4]);
        assert_eq!(amount, 1000);
        assert_eq!(next_hop, "hub_pk");
    } else {
        panic!("Expected MultihopInit variant");
    }
}

#[test]
fn test_ws_message_all_variants_serialize() {
    let messages = vec![
        ignite_pay_channel_service::ws::protocol::WsMessage::AuthOk,
        ignite_pay_channel_service::ws::protocol::WsMessage::LeafUpdateAck {
            channel_id: "x".into(),
            sequence: 1,
        },
        ignite_pay_channel_service::ws::protocol::WsMessage::LeafUpdateNack {
            channel_id: "x".into(),
            sequence: 1,
            reason: "bad".into(),
        },
        ignite_pay_channel_service::ws::protocol::WsMessage::BatchStart {
            channel_id: "x".into(),
            count: 3,
        },
        ignite_pay_channel_service::ws::protocol::WsMessage::BatchCommit {
            channel_id: "x".into(),
        },
        ignite_pay_channel_service::ws::protocol::WsMessage::BatchAbort {
            channel_id: "x".into(),
        },
        ignite_pay_channel_service::ws::protocol::WsMessage::CosignRequest {
            channel_id: "x".into(),
            sequence: 1,
            root: vec![0; 32],
        },
        ignite_pay_channel_service::ws::protocol::WsMessage::CosignResponse {
            channel_id: "x".into(),
            sequence: 1,
            signature: vec![0; 64],
        },
        ignite_pay_channel_service::ws::protocol::WsMessage::HtlcCreated {
            channel_id: "x".into(),
            hash_lock: vec![0; 32],
            amount: 100,
            timelock_slot: 500,
        },
        ignite_pay_channel_service::ws::protocol::WsMessage::HtlcPreimage {
            channel_id: "x".into(),
            hash_lock: vec![0; 32],
            preimage: vec![0; 32],
        },
        ignite_pay_channel_service::ws::protocol::WsMessage::HtlcRefunded {
            channel_id: "x".into(),
            hash_lock: vec![0; 32],
        },
        ignite_pay_channel_service::ws::protocol::WsMessage::MultihopPreimage {
            payment_id: vec![0; 32],
            preimage: vec![0; 32],
        },
        ignite_pay_channel_service::ws::protocol::WsMessage::MultihopFailed {
            payment_id: vec![0; 32],
            reason: "timeout".into(),
        },
        ignite_pay_channel_service::ws::protocol::WsMessage::ChallengeTriggered {
            channel_id: "x".into(),
            challenge_slot: 100,
        },
        ignite_pay_channel_service::ws::protocol::WsMessage::CounterStateSubmitted {
            channel_id: "x".into(),
            sequence: 5,
        },
        ignite_pay_channel_service::ws::protocol::WsMessage::SettlementStarted {
            channel_id: "x".into(),
            deadline: 200,
        },
        ignite_pay_channel_service::ws::protocol::WsMessage::ChannelStateChanged {
            channel_id: "x".into(),
            old_status: "Open".into(),
            new_status: "Challenged".into(),
        },
    ];

    for msg in &messages {
        let json = serde_json::to_string(msg).unwrap();
        let back: ignite_pay_channel_service::ws::protocol::WsMessage =
            serde_json::from_str(&json).unwrap();
        // Verify round-trip doesn't panic and produces valid JSON
        let json2 = serde_json::to_string(&back).unwrap();
        assert!(!json2.is_empty());
    }
}
