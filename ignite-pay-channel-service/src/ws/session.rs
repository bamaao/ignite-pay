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

use axum::{
    extract::{ws::{Message, WebSocket}, State, WebSocketUpgrade},
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use tokio::sync::mpsc;

use ed25519_dalek::{PublicKey, Signature, Verifier};

use crate::state::AppState;
use crate::ws::protocol::WsMessage;

/// HTTP upgrade handler for WebSocket connections at `/ws`.
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AppState) {
    let (mut ws_sender, mut ws_receiver) = socket.split();

    // Create an mpsc channel for outgoing messages
    let (tx, mut rx) = mpsc::channel::<WsMessage>(256);

    // Wait for auth message
    let auth_result = match ws_receiver.next().await {
        Some(Ok(Message::Text(text))) => serde_json::from_str::<WsMessage>(text.as_str()),
        _ => {
            let _ = ws_sender.send(Message::Close(None)).await;
            return;
        }
    };

    let pubkey_str = match auth_result {
        Ok(WsMessage::Auth { pubkey, signature, timestamp }) => {
            // Verify the auth signature: sign("channel-ws-auth:{timestamp}")
            let msg = format!("channel-ws-auth:{}", timestamp);
            let msg_hash = solana_sdk::hash::hash(msg.as_bytes()).to_bytes();

            let pubkey_bytes = match bs58::decode(&pubkey).into_vec() {
                Ok(b) if b.len() == 32 => {
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(&b);
                    arr
                }
                _ => {
                    let _ = ws_sender
                        .send(Message::Text(
                            serde_json::to_string(&WsMessage::Error {
                                code: 400,
                                message: "invalid pubkey".into(),
                            })
                            .unwrap()
                            .into(),
                        ))
                        .await;
                    return;
                }
            };

            let ed_pubkey = match PublicKey::from_bytes(&pubkey_bytes) {
                Ok(p) => p,
                Err(_) => {
                    let _ = ws_sender
                        .send(Message::Text(
                            serde_json::to_string(&WsMessage::Error {
                                code: 400,
                                message: "invalid pubkey".into(),
                            })
                            .unwrap()
                            .into(),
                        ))
                        .await;
                    return;
                }
            };

            let sig_bytes: [u8; 64] = match signature.try_into() {
                Ok(b) => b,
                Err(_) => {
                    let _ = ws_sender
                        .send(Message::Text(
                            serde_json::to_string(&WsMessage::Error {
                                code: 400,
                                message: "invalid signature length".into(),
                            })
                            .unwrap()
                            .into(),
                        ))
                        .await;
                    return;
                }
            };

            let sig = match Signature::from_bytes(&sig_bytes) {
                Ok(s) => s,
                Err(_) => {
                    let _ = ws_sender
                        .send(Message::Text(
                            serde_json::to_string(&WsMessage::Error {
                                code: 400,
                                message: "invalid signature".into(),
                            })
                            .unwrap()
                            .into(),
                        ))
                        .await;
                    return;
                }
            };

            if ed_pubkey.verify(&msg_hash, &sig).is_err() {
                let _ = ws_sender
                    .send(Message::Text(
                        serde_json::to_string(&WsMessage::Error {
                            code: 401,
                            message: "authentication failed".into(),
                        })
                        .unwrap()
                        .into(),
                    ))
                    .await;
                return;
            }

            // Send auth_ok
            let auth_ok = serde_json::to_string(&WsMessage::AuthOk).unwrap();
            if ws_sender.send(Message::Text(auth_ok.into())).await.is_err() {
                return;
            }

            pubkey
        }
        _ => {
            let _ = ws_sender
                .send(Message::Text(
                    serde_json::to_string(&WsMessage::Error {
                        code: 400,
                        message: "expected auth message first".into(),
                    })
                    .unwrap()
                    .into(),
                ))
                .await;
            return;
        }
    };

    // Register the peer
    state.ws_peers.insert(pubkey_str.clone(), tx);

    // Bidirectional loop
    let state_clone = state.clone();
    let pubkey_str_clone = pubkey_str.clone();

    // Outgoing: forward mpsc messages to WebSocket
    let outgoing = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let text = match serde_json::to_string(&msg) {
                Ok(t) => t,
                Err(_) => continue,
            };
            if ws_sender.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
    });

    // Incoming: parse WebSocket messages and dispatch
    let incoming = tokio::spawn(async move {
        while let Some(result) = ws_receiver.next().await {
            match result {
                Ok(Message::Text(text)) => {
                    match serde_json::from_str::<WsMessage>(text.as_str()) {
                        Ok(msg) => handle_incoming_message(&state_clone, &pubkey_str_clone, msg).await,
                        Err(e) => {
                            tracing::warn!("Failed to parse WS message: {}", e);
                        }
                    }
                }
                Ok(Message::Close(_)) => break,
                Err(_) => break,
                _ => {}
            }
        }
    });

    // Wait for either side to finish
    tokio::select! {
        _ = outgoing => {},
        _ = incoming => {},
    }

    // Cleanup
    state.ws_peers.remove(&pubkey_str);
}

async fn handle_incoming_message(_state: &AppState, _peer: &str, msg: WsMessage) {
    match &msg {
        WsMessage::LeafUpdate { channel_id, .. } => {
            tracing::info!("Received leaf_update for channel {}", channel_id);
            // Handler would apply the leaf update via ChannelManager
        }
        WsMessage::CosignRequest { channel_id, sequence, .. } => {
            tracing::info!("Received cosign_request for channel {} seq {}", channel_id, sequence);
        }
        WsMessage::HtlcPreimage { channel_id, .. } => {
            tracing::info!("Received htlc_preimage for channel {}", channel_id);
        }
        WsMessage::MultihopInit { payment_id, .. } => {
            tracing::info!("Received multihop_init for payment {}", hex::encode(payment_id));
        }
        _ => {
            tracing::debug!("Received WS message: {:?}", msg);
        }
    }
}
