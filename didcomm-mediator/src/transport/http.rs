use axum::body::Bytes;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use tracing::{error, info};

use crate::state::AppState;

/// HTTP POST endpoint for receiving DIDComm messages.
/// The body should be a JWE/JWS/plaintext DIDComm message.
pub async fn post_message(
    State(state): State<AppState>,
    body: Bytes,
) -> impl IntoResponse {
    let text = match String::from_utf8(body.to_vec()) {
        Ok(t) => t,
        Err(e) => {
            error!("Invalid UTF-8 in POST body: {}", e);
            return (StatusCode::BAD_REQUEST, "Invalid UTF-8".to_string()).into_response();
        }
    };

    info!("Received HTTP DIDComm message ({} bytes)", text.len());

    match crate::protocols::dispatch(&text, &state, None).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => {
            error!("HTTP message dispatch error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}
