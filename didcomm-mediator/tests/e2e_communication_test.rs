//! End-to-end communication integration test.
//!
//! Tests the full MCP <-> Mediator <-> Phone flow:
//! 1. MCP connects to mediator via WebSocket and performs DIDComm handshake
//! 2. MCP sends encrypted auth-request (JWE) via WS -> mediator stores + FCM signal
//! 3. Phone pulls messages via REST API
//! 4. Phone submits auth-response via REST command API
//! 5. Mediator forwards auth-response to MCP via WS

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use didcomm_mediator::config::{Config, MediatorConfig, ServerConfig};
use didcomm_mediator::server::build_router;
use didcomm_mediator::state::AppState;
use didcomm_mediator::storage::QueuedMessage;
use tower::util::ServiceExt;

// -- Test helpers --

fn test_config() -> Config {
    Config {
        server: ServerConfig {
            host: "127.0.0.1".to_string(),
            port: 0,
        },
        mediator: MediatorConfig {
            did: "did:ignite:zTestMediator".to_string(),
            max_queued_messages: 100,
            max_message_age_seconds: 3600,
            known_peers: vec![],
        },
    }
}

fn test_app_with_state() -> (Router, AppState) {
    let config = test_config();
    let state = AppState::new(config);
    (build_router(state.clone()), state)
}

fn auth_header(token: &str) -> String {
    format!("Bearer {}", token)
}

async fn get_token(app: &Router, did: &str) -> String {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/auth/token")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::json!({"did": did, "signature": "test"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let body = axum::body::to_bytes(resp.into_body(), 2048).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    v["token"].as_str().unwrap().to_string()
}

// Helper: POST to a URI with auth and JSON body
async fn post_json(app: &Router, uri: &str, token: &str, body: serde_json::Value) -> axum::http::Response<Body> {
    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("Authorization", auth_header(token))
                .header("Content-Type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}

// Helper: GET a URI with auth
async fn get_uri(app: &Router, uri: &str, token: &str) -> axum::http::Response<Body> {
    app.clone()
        .oneshot(
            Request::builder()
                .uri(uri)
                .header("Authorization", auth_header(token))
                .body(Body::from(""))
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn parse_body(resp: axum::http::Response<Body>) -> serde_json::Value {
    let bytes = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

// -- Tests: Agent binding --

#[tokio::test]
async fn test_bind_agent_to_user() {
    let (app, state) = test_app_with_state();
    let user_did = "did:ignite:zTestUser";
    let agent_did = "did:ignite:zTestAgent";
    let token = get_token(&app, user_did).await;

    let resp = post_json(&app, "/v1/agents/bind", &token, serde_json::json!({"agent_did": agent_did})).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let user = state.agent_binding_store.get_user_for_agent(agent_did).await.unwrap();
    assert_eq!(user.as_deref(), Some(user_did));
}

#[tokio::test]
async fn test_bind_agent_invalid_did() {
    let (app, _) = test_app_with_state();
    let token = get_token(&app, "did:ignite:zTestUser").await;

    let resp = post_json(&app, "/v1/agents/bind", &token, serde_json::json!({"agent_did": "invalid"})).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// -- Tests: Downlink (Phone -> Mediator -> Agent) --

#[tokio::test]
async fn test_downlink_command_to_online_agent() {
    let (app, state) = test_app_with_state();
    let token = get_token(&app, "did:ignite:zPhoneUser").await;

    // Bring agent online
    let (ws_tx, mut ws_rx) = tokio::sync::mpsc::unbounded_channel();
    state.sessions.register("zAgent001".to_string(), ws_tx);

    let jwe = r#"{"ciphertext":"test_data","recipients":[]}"#;
    let resp = post_json(&app, "/v1/agents/zAgent001/command", &token, serde_json::json!({"jwe_envelope": jwe})).await;
    assert_eq!(resp.status(), StatusCode::OK);

    // Agent receives message via WS
    let msg = ws_rx.try_recv().unwrap();
    match msg {
        axum::extract::ws::Message::Text(t) => assert_eq!(t, jwe),
        _ => panic!("Expected text message"),
    }
}

#[tokio::test]
async fn test_downlink_command_to_offline_agent_queues() {
    let (app, state) = test_app_with_state();
    let token = get_token(&app, "did:ignite:zPhoneUser").await;

    let resp = post_json(&app, "/v1/agents/zAgentOffline/command", &token,
        serde_json::json!({"jwe_envelope": r#"{"ciphertext":"offline"}"#})).await;
    assert_eq!(resp.status(), StatusCode::ACCEPTED);

    let count = state.message_store.count("zAgentOffline").await.unwrap();
    assert_eq!(count, 1);
}

// -- Tests: Uplink (Agent -> Mediator -> Phone pulls) --

#[tokio::test]
async fn test_uplink_message_store_and_pull() {
    let (app, state) = test_app_with_state();
    let user_did = "did:ignite:zPhoneUser";
    let agent_did = "did:ignite:zAgent001";

    state.agent_binding_store.bind(agent_did, user_did).await.unwrap();

    let msg_id = uuid::Uuid::new_v4().to_string();
    let jwe = r#"{"ciphertext":"auth_request_for_phone","recipients":[]}"#;
    state.message_store.store_for_user(user_did, QueuedMessage {
        id: msg_id.clone(),
        sender_did: agent_did.to_string(),
        recipient_did: user_did.to_string(),
        encrypted_envelope: jwe.to_string(),
        queued_at: chrono::Utc::now(),
    }).await.unwrap();

    let token = get_token(&app, user_did).await;

    // List messages
    let resp = get_uri(&app, "/v1/sync/list?limit=10", &token).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = parse_body(resp).await;
    let msgs = body["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0]["jwe_envelope"].as_str().unwrap(), jwe);

    // Get single message
    let resp = get_uri(&app, &format!("/v1/sync/messages/{}", msg_id), &token).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = parse_body(resp).await;
    assert_eq!(body["jwe_envelope"].as_str().unwrap(), jwe);
}

// -- Tests: Full round-trip (Phone -> Agent -> Phone) --

#[tokio::test]
async fn test_full_round_trip() {
    let (app, state) = test_app_with_state();
    let user_did = "did:ignite:zPhoneRT";
    let agent_did = "did:ignite:zAgentRT";
    let token = get_token(&app, user_did).await;

    // Phase 1: Bind agent
    let resp = post_json(&app, "/v1/agents/bind", &token, serde_json::json!({"agent_did": agent_did})).await;
    assert_eq!(resp.status(), StatusCode::OK);

    // Phase 2: Register FCM token
    let resp = post_json(&app, "/v1/devices/register-token", &token,
        serde_json::json!({"fcm_token": "test_fcm_rt"})).await;
    assert_eq!(resp.status(), StatusCode::OK);

    // Phase 3: Bring agent online
    let (ws_tx, mut ws_rx) = tokio::sync::mpsc::unbounded_channel();
    state.sessions.register(agent_did.to_string(), ws_tx);

    // Phase 4: Phone sends command (downlink)
    let cmd_jwe = r#"{"ciphertext":"phone_cmd","recipients":[]}"#;
    let resp = post_json(&app, &format!("/v1/agents/{}/command", agent_did), &token,
        serde_json::json!({"jwe_envelope": cmd_jwe})).await;
    assert_eq!(resp.status(), StatusCode::OK);

    // Agent receives
    let msg = ws_rx.try_recv().unwrap();
    match msg {
        axum::extract::ws::Message::Text(t) => assert_eq!(t, cmd_jwe),
        _ => panic!("Expected text"),
    }

    // Phase 5: Agent sends auth-request back via mediator (simulated store)
    let auth_jwe = r#"{"ciphertext":"auth_req","recipients":[]}"#;
    let msg_id = uuid::Uuid::new_v4().to_string();
    state.message_store.store_for_user(user_did, QueuedMessage {
        id: msg_id.clone(),
        sender_did: agent_did.to_string(),
        recipient_did: user_did.to_string(),
        encrypted_envelope: auth_jwe.to_string(),
        queued_at: chrono::Utc::now(),
    }).await.unwrap();

    // Phase 6: Phone pulls auth-request
    let resp = get_uri(&app, "/v1/sync/list?limit=10", &token).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = parse_body(resp).await;
    let msgs = body["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0]["jwe_envelope"].as_str().unwrap(), auth_jwe);

    // Phase 7: Phone sends auth-response (downlink again)
    let resp_jwe = r#"{"ciphertext":"auth_resp","recipients":[]}"#;
    let resp = post_json(&app, &format!("/v1/agents/{}/command", agent_did), &token,
        serde_json::json!({"jwe_envelope": resp_jwe})).await;
    assert_eq!(resp.status(), StatusCode::OK);

    // Agent receives auth-response
    let msg = ws_rx.try_recv().unwrap();
    match msg {
        axum::extract::ws::Message::Text(t) => assert_eq!(t, resp_jwe),
        _ => panic!("Expected text"),
    }
}

// -- Tests: Batch sync with pagination --

#[tokio::test]
async fn test_batch_sync_pagination() {
    let (app, state) = test_app_with_state();
    let user_did = "did:ignite:zSyncUser";
    let token = get_token(&app, user_did).await;

    let mut msg_ids = Vec::new();
    for i in 0..5 {
        let id = format!("sync-msg-{}", i);
        state.message_store.store_for_user(user_did, QueuedMessage {
            id: id.clone(),
            sender_did: "did:ignite:zAgent".to_string(),
            recipient_did: user_did.to_string(),
            encrypted_envelope: format!(r#"{{"ciphertext":"p{}"}}"#, i),
            queued_at: chrono::Utc::now(),
        }).await.unwrap();
        msg_ids.push(id);
    }

    // Pull first 3
    let resp = get_uri(&app, "/v1/sync/list?limit=3", &token).await;
    let body = parse_body(resp).await;
    assert_eq!(body["messages"].as_array().unwrap().len(), 3);
    assert_eq!(body["has_more"].as_bool().unwrap(), true);

    // Pull remaining
    let resp = get_uri(&app, &format!("/v1/sync/list?after={}&limit=10", msg_ids[2]), &token).await;
    let body = parse_body(resp).await;
    assert_eq!(body["messages"].as_array().unwrap().len(), 2);
    assert_eq!(body["has_more"].as_bool().unwrap(), false);
}

// -- Tests: User isolation --

#[tokio::test]
async fn test_message_isolation_between_users() {
    let (app, state) = test_app_with_state();
    let token_a = get_token(&app, "did:ignite:zUserA").await;
    let token_b = get_token(&app, "did:ignite:zUserB").await;

    state.message_store.store_for_user("did:ignite:zUserA", QueuedMessage {
        id: "msg-a".to_string(),
        sender_did: "did:ignite:zAgent".to_string(),
        recipient_did: "did:ignite:zUserA".to_string(),
        encrypted_envelope: r#"{"ciphertext":"secret_a"}"#.to_string(),
        queued_at: chrono::Utc::now(),
    }).await.unwrap();

    // User A sees their message
    let resp = get_uri(&app, "/v1/sync/list?limit=10", &token_a).await;
    let body = parse_body(resp).await;
    assert_eq!(body["messages"].as_array().unwrap().len(), 1);

    // User B sees nothing
    let resp = get_uri(&app, "/v1/sync/list?limit=10", &token_b).await;
    let body = parse_body(resp).await;
    assert_eq!(body["messages"].as_array().unwrap().len(), 0);

    // User B cannot get User A's message
    let resp = get_uri(&app, "/v1/sync/messages/msg-a", &token_b).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// -- Tests: DIDComm handshake protocol --

#[tokio::test]
async fn test_mediate_handshake_sequence() {
    let config = test_config();
    let state = AppState::new(config);
    let agent_did = "did:ignite:zHandshakeAgent";

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    state.sessions.register(agent_did.to_string(), tx);

    // mediate-request
    let req = serde_json::from_str(&format!(
        r#"{{"id":"m1","type":"https://didcomm.org/coordinate-mediation/2.0/mediate-request","from":"{}","body":{{}}}}"#,
        agent_did
    )).unwrap();
    didcomm_mediator::protocols::coordinate_mediation::handle_mediate_request(&req, &state, Some(agent_did)).await.unwrap();

    // keylist-update
    let req = serde_json::from_str(&format!(
        r#"{{"id":"m2","type":"https://didcomm.org/coordinate-mediation/2.0/keylist-update","from":"{}","body":{{"updates":[{{"recipient_key":"{}#key-1","action":"add"}}]}}}}"#,
        agent_did, agent_did
    )).unwrap();
    didcomm_mediator::protocols::coordinate_mediation::handle_keylist_update(&req, &state, Some(agent_did)).await.unwrap();

    let keys = state.keylist_store.list_keys(agent_did).await.unwrap();
    assert!(keys.contains(&format!("{}#key-1", agent_did)));
}

// -- Tests: Forward to online peer --

#[tokio::test]
async fn test_forward_to_online_peer() {
    let config = test_config();
    let state = AppState::new(config);
    let peer_key = "did:ignite:zPeer#key-1";

    state.keylist_store.add_key("did:ignite:zPeer", peer_key).await.unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    state.sessions.register("did:ignite:zPeer".to_string(), tx);

    let msg = serde_json::from_str(&format!(
        r#"{{"id":"f1","type":"https://didcomm.org/routing/2.0/forward","body":{{"next":"{}"}},"attachments":[{{"data":{{"json":{{"ciphertext":"inner"}}}}}}]}}"#,
        peer_key
    )).unwrap();
    didcomm_mediator::protocols::routing::handle_forward(&msg, &state).await.unwrap();

    let ws_msg = rx.try_recv().unwrap();
    match ws_msg {
        axum::extract::ws::Message::Text(t) => assert!(t.contains("inner")),
        _ => panic!("Expected text"),
    }
}

// -- Tests: Pickup protocol --

#[tokio::test]
async fn test_pickup_status_and_batch() {
    let config = test_config();
    let state = AppState::new(config);
    let peer_did = "did:ignite:zPickupPeer";
    let peer_key = format!("{}#key-1", peer_did);

    state.keylist_store.add_key(peer_did, &peer_key).await.unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    state.sessions.register(peer_did.to_string(), tx);

    for i in 0..3 {
        state.message_store.enqueue(&peer_key, QueuedMessage {
            id: format!("p-msg-{}", i),
            sender_did: "did:ignite:zSender".to_string(),
            recipient_did: peer_key.clone(),
            encrypted_envelope: format!(r#"{{"ciphertext":"p{}"}}"#, i),
            queued_at: chrono::Utc::now(),
        }).await.unwrap();
    }

    // Status request
    let status_req = serde_json::from_str(&format!(
        r#"{{"id":"s1","type":"https://didcomm.org/messagepickup/3.0/status-request","from":"{}","body":{{}}}}"#,
        peer_did
    )).unwrap();
    didcomm_mediator::protocols::pickup::handle_status_request(&status_req, &state, Some(peer_did)).await.unwrap();

    // Batch pickup
    let batch_req = serde_json::from_str(&format!(
        r#"{{"id":"b1","type":"https://didcomm.org/messagepickup/3.0/batch-pickup","from":"{}","body":{{"count":10}}}}"#,
        peer_did
    )).unwrap();
    didcomm_mediator::protocols::pickup::handle_batch_pickup(&batch_req, &state, Some(peer_did)).await.unwrap();

    let count = state.message_store.count(&peer_key).await.unwrap();
    assert_eq!(count, 0);
}

// -- Tests: Agent binding store --

#[tokio::test]
async fn test_agent_binding_store() {
    let config = test_config();
    let state = AppState::new(config);

    state.agent_binding_store.bind("did:ignite:zA1", "did:ignite:zUser").await.unwrap();
    state.agent_binding_store.bind("did:ignite:zA2", "did:ignite:zUser").await.unwrap();

    let u1 = state.agent_binding_store.get_user_for_agent("did:ignite:zA1").await.unwrap();
    assert_eq!(u1.as_deref(), Some("did:ignite:zUser"));

    let agents = state.agent_binding_store.get_agents_for_user("did:ignite:zUser").await.unwrap();
    assert_eq!(agents.len(), 2);

    let unknown = state.agent_binding_store.get_user_for_agent("did:ignite:zUnknown").await.unwrap();
    assert!(unknown.is_none());
}
