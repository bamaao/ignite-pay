use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::IntoResponse;
use futures::{SinkExt, StreamExt};
use tracing::{error, info};

use crate::state::AppState;

/// Handler for WebSocket upgrade requests at `/ws`.
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AppState) {
    let (mut ws_sender, mut ws_receiver) = socket.split();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Message>();

    // Spawn a task to forward messages from the channel to the WebSocket
    let mut send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if ws_sender.send(msg).await.is_err() {
                break;
            }
        }
    });

    // Read messages from the client
    let session_mgr = state.sessions.clone();
    let recv_state = state.clone();
    let mut recv_task = tokio::spawn(async move {
        // The first message must identify the client (plaintext DIDComm with their DID)
        let mut session_did: Option<String> = None;

        while let Some(Ok(msg)) = ws_receiver.next().await {
            let text = match msg {
                Message::Text(t) => t,
                Message::Close(_) => break,
                _ => continue,
            };

            // If not yet identified, try to parse as a plaintext message to extract sender DID
            if session_did.is_none() {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                    if let Some(from) = value.get("from").and_then(|v| v.as_str()) {
                        session_did = Some(from.to_string());
                        session_mgr.register(from.to_string(), tx.clone());
                        info!("WebSocket session registered: {}", from);
                    }
                }
            }

            // Process the incoming message through the protocol dispatcher
            if let Err(e) =
                crate::protocols::dispatch(&text, &recv_state, session_did.as_deref()).await
            {
                error!("Protocol dispatch error: {}", e);
            }
        }

        // Clean up session on disconnect
        if let Some(ref did) = session_did {
            session_mgr.unregister(did);
            info!("WebSocket session unregistered: {}", did);
        }

        session_did
    });

    // Wait for either task to finish
    tokio::select! {
        _ = (&mut send_task) => {
            recv_task.abort();
        },
        _ = (&mut recv_task) => {
            send_task.abort();
        },
    }
}
