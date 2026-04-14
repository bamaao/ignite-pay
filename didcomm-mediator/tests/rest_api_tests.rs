use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use didcomm_mediator::config::{Config, MediatorConfig, ServerConfig};
use didcomm_mediator::server::build_router;
use didcomm_mediator::state::AppState;
use tower::util::ServiceExt;

fn test_app() -> Router {
    let config = Config {
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
    };
    let state = AppState::new(config);
    build_router(state)
}

fn get_auth_header(token: &str) -> String {
    format!("Bearer {}", token)
}

async fn get_test_token(app: &Router) -> String {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/auth/token")
                .header("Content-Type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "did": "did:ignite:zTestUser",
                        "signature": "test"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    resp.get("token").unwrap().as_str().unwrap().to_string()
}

#[tokio::test]
async fn test_health() {
    let app = test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::from(""))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_auth_token_valid_did() {
    let app = test_app();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/auth/token")
                .header("Content-Type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "did": "did:ignite:zTestUser123",
                        "signature": "test_sig"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(resp.get("token").is_some());
    assert_eq!(resp.get("expires_in").unwrap().as_u64(), Some(3600));
}

#[tokio::test]
async fn test_auth_token_invalid_did() {
    let app = test_app();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/auth/token")
                .header("Content-Type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "did": "invalid_did",
                        "signature": "test_sig"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_sync_list_unauthorized() {
    let app = test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/sync/list")
                .body(Body::from(""))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_sync_list_empty() {
    let app = test_app();
    let token = get_test_token(&app).await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/sync/list?limit=10")
                .header("Authorization", get_auth_header(&token))
                .body(Body::from(""))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let messages = resp.get("messages").unwrap().as_array().unwrap();
    assert!(messages.is_empty());
    assert_eq!(resp.get("has_more").unwrap().as_bool(), Some(false));
}

#[tokio::test]
async fn test_get_message_not_found() {
    let app = test_app();
    let token = get_test_token(&app).await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/sync/messages/nonexistent-id")
                .header("Authorization", get_auth_header(&token))
                .body(Body::from(""))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_register_device_token() {
    let app = test_app();
    let token = get_test_token(&app).await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/devices/register-token")
                .header("Authorization", get_auth_header(&token))
                .header("Content-Type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "fcm_token": "test_fcm_token_123"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}
