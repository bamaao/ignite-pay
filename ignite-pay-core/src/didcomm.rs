use affinidi_messaging_didcomm::identity::PrivateIdentity;
use affinidi_messaging_didcomm::{DIDCommAgent, Message, UnpackResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Payment method choices for the phone user during authorization.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PaymentMethod {
    /// Direct on-chain SOL/SPL transfer via session key contract.
    #[serde(rename = "session_key")]
    SessionKey,
    /// Off-chain MagicBlock voucher from unified global vault.
    #[serde(rename = "magicblock")]
    MagicBlock,
    /// Future: delegated payment via relayer service.
    #[serde(rename = "relayer")]
    Relayer,
}

impl PaymentMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            PaymentMethod::SessionKey => "session_key",
            PaymentMethod::MagicBlock => "magicblock",
            PaymentMethod::Relayer => "relayer",
        }
    }
}

/// Build a plaintext `mediate-request` message.
pub fn build_mediate_request(from_did: &str) -> Message {
    Message::new(
        "https://didcomm.org/coordinate-mediation/2.0/mediate-request",
        json!({}),
    )
    .from(from_did.to_string())
}

/// Build a plaintext `keylist-update` message.
pub fn build_keylist_update(from_did: &str) -> Message {
    Message::new(
        "https://didcomm.org/coordinate-mediation/2.0/keylist-update",
        json!({
            "updates": [{
                "recipient_key": format!("{}#key-1", from_did),
                "action": "add"
            }]
        }),
    )
    .from(from_did.to_string())
}

/// Build a WS challenge-response message for authentication.
/// Sent by the client (MCP/Phone) to prove ownership of their DID keys.
/// The client encrypts this with `pack_authcrypt` before sending.
pub fn build_ws_challenge_response(
    from_did: &str,
    to_did: &str,
    nonce: &str,
    did_doc: &Value,
) -> Message {
    Message::new(
        "https://didcomm.org/ignite-pay/1.0/ws-challenge-response",
        json!({
            "nonce": nonce,
            "did_document": did_doc,
        }),
    )
    .from(from_did.to_string())
    .to(vec![to_did.to_string()])
}

/// Build a `peer-did-discovery/1.0/discover` message carrying our DID document.
pub fn build_peer_introduction(from_did: &str, did_doc: &Value) -> Message {
    Message::new(
        "https://didcomm.org/peer-did-discovery/1.0/discover",
        json!({
            "did_document": did_doc
        }),
    )
    .from(from_did.to_string())
}

/// Build an Out-of-Band invitation for P2P pairing.
/// The MCP generates this; the phone scans it (as QR) to learn the MCP's DID,
/// DID document, and the mediator endpoint.
pub fn build_oob_invitation(
    from_did: &str,
    label: &str,
    mediator_ws_url: &str,
    did_doc: &Value,
) -> Message {
    Message::new(
        "https://didcomm.org/out-of-band/2.0/invitation",
        json!({
            "label": label,
            "goal_code": "p2p-messaging",
            "accept": ["didcomm/v2"],
            "did_document": did_doc,
            "services": [{
                "id": "#mediator",
                "type": "did-communication",
                "service_endpoint": mediator_ws_url,
                "routing_keys": [from_did]
            }]
        }),
    )
    .from(from_did.to_string())
}

/// Build a connection request sent by the phone to the MCP during pairing.
/// Contains the phone's push channel preference and optional FCM token.
pub fn build_connection_request(
    from_did: &str,
    to_did: &str,
    push_channel: &str,
    fcm_token: Option<&str>,
) -> Message {
    let mut body = json!({
        "push_channel": push_channel,
    });
    if let Some(token) = fcm_token {
        body["fcm_token"] = json!(token);
    }
    Message::new(
        "https://didcomm.org/ignite-pay/1.0/connection-request",
        body,
    )
    .from(from_did.to_string())
    .to(vec![to_did.to_string()])
}

/// Build a connection request with mediator_http_url so the MCP knows where to forward messages.
pub fn build_connection_request_with_mediator(
    from_did: &str,
    to_did: &str,
    push_channel: &str,
    fcm_token: Option<&str>,
    mediator_http_url: &str,
) -> Message {
    let mut body = json!({
        "push_channel": push_channel,
        "mediator_http_url": mediator_http_url,
    });
    if let Some(token) = fcm_token {
        body["fcm_token"] = json!(token);
    }
    Message::new(
        "https://didcomm.org/ignite-pay/1.0/connection-request",
        body,
    )
    .from(from_did.to_string())
    .to(vec![to_did.to_string()])
}

/// Build a connection response sent by the MCP to acknowledge a pairing request.
/// When accepted, includes the MCP's DID document, mediator URL, and a signed nonce
/// so the phone can verify the MCP's identity before completing pairing.
pub fn build_connection_response(
    from_did: &str,
    to_did: &str,
    accepted: bool,
    did_doc: Option<&Value>,
    mediator_http_url: Option<&str>,
    mcp_nonce: Option<&str>,
    mcp_signature: Option<&str>,
) -> Message {
    let body = if accepted {
        let mut b = json!({ "accepted": true });
        if let Some(doc) = did_doc {
            b["did_document"] = doc.clone();
        }
        if let Some(url) = mediator_http_url {
            b["mediator_http_url"] = json!(url);
        }
        if let Some(nonce) = mcp_nonce {
            b["mcp_nonce"] = json!(nonce);
        }
        if let Some(sig) = mcp_signature {
            b["mcp_signature"] = json!(sig);
        }
        b
    } else {
        json!({ "accepted": false })
    };
    Message::new(
        "https://didcomm.org/ignite-pay/1.0/connection-response",
        body,
    )
    .from(from_did.to_string())
    .to(vec![to_did.to_string()])
}

/// Build a connection-confirm message sent by the phone/merchant app after
/// receiving a connection-response. Contains a random nonce and its Ed25519
/// signature to prove ownership of the DID's signing key.
pub fn build_connection_confirm(
    from_did: &str,
    to_did: &str,
    nonce: &str,
    signature: &str,
) -> Message {
    Message::new(
        "https://didcomm.org/ignite-pay/1.0/connection-confirm",
        json!({
            "phone_nonce": nonce,
            "phone_signature": signature,
        }),
    )
    .from(from_did.to_string())
    .to(vec![to_did.to_string()])
}

/// Build a connection-confirm-response message sent by the MCP after
/// verifying the phone's connection-confirm signature. Contains the MCP's
/// own nonce and signature, plus an echo of the phone's nonce.
pub fn build_connection_confirm_response(
    from_did: &str,
    to_did: &str,
    mcp_nonce: &str,
    mcp_signature: &str,
    phone_nonce_echo: &str,
    accepted: bool,
) -> Message {
    Message::new(
        "https://didcomm.org/ignite-pay/1.0/connection-confirm-response",
        json!({
            "mcp_nonce": mcp_nonce,
            "mcp_signature": mcp_signature,
            "phone_nonce_echo": phone_nonce_echo,
            "accepted": accepted,
        }),
    )
    .from(from_did.to_string())
    .to(vec![to_did.to_string()])
}

/// Build a Message Pickup 3.0 `status-request` message.
/// Asks the mediator how many messages are queued for this DID.
pub fn build_status_request(from_did: &str) -> Message {
    Message::new(
        "https://didcomm.org/messagepickup/3.0/status-request",
        json!({}),
    )
    .from(from_did.to_string())
}

/// Build a Message Pickup 3.0 `batch-pickup` message.
/// Requests up to `count` queued messages from the mediator.
pub fn build_batch_pickup(from_did: &str, count: usize) -> Message {
    Message::new(
        "https://didcomm.org/messagepickup/3.0/batch-pickup",
        json!({
            "count": count,
        }),
    )
    .from(from_did.to_string())
}

/// Build a payment authorization request message.
/// Sent from the MCP server to the phone app via the mediator.
/// New session key data sent from MCP to phone in the payment-auth-request.
/// When included, the phone should register this session key on-chain, fund it,
/// and authorize the payment — all in one user interaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewSessionKeyRequest {
    /// Base58-encoded ephemeral public key (MCP-generated).
    pub session_key_pubkey: String,
    /// Maximum spending limit in lamports for this session.
    pub spending_limit: u64,
    /// Session duration in seconds.
    pub duration_secs: i64,
    /// Permission scopes (e.g., ["sol:transfer", "spl:transfer"]).
    pub scopes: Vec<String>,
    /// SPL Token mint address (base58). None/empty for SOL sessions.
    pub token_mint: Option<String>,
    /// Suggested SOL funding amount (lamports) for gas + payments.
    pub suggested_sol_funding: u64,
    /// Suggested stablecoin funding amount (smallest unit) if applicable.
    pub suggested_token_funding: Option<u64>,
}

pub fn build_authorization_request(
    from_did: &str,
    to_did: &str,
    payment_id: &str,
    merchant_did: &str,
    amount: u64,
    description: &str,
) -> Message {
    build_authorization_request_inner(
        from_did, to_did, payment_id, merchant_did, amount, description, None, None, None, None,
    )
}

/// Build a payment authorization request with optional new session key info.
/// When `new_session_key` is provided, the phone should create the session key
/// account on-chain, fund it, and authorize the payment in one step.
pub fn build_authorization_request_with_session_key(
    from_did: &str,
    to_did: &str,
    payment_id: &str,
    merchant_did: &str,
    amount: u64,
    description: &str,
    new_session_key: &NewSessionKeyRequest,
) -> Message {
    build_authorization_request_inner(
        from_did, to_did, payment_id, merchant_did, amount, description, Some(new_session_key), None, None, None,
    )
}

/// Build a payment authorization request with session key and available payment methods.
pub fn build_authorization_request_with_methods(
    from_did: &str,
    to_did: &str,
    payment_id: &str,
    merchant_did: &str,
    amount: u64,
    description: &str,
    new_session_key: Option<&NewSessionKeyRequest>,
    available_payment_methods: &[PaymentMethod],
) -> Message {
    build_authorization_request_inner(
        from_did, to_did, payment_id, merchant_did, amount, description, new_session_key, Some(available_payment_methods), None, None,
    )
}

/// Build a payment authorization request with session key, payment methods, and relayer info.
pub fn build_authorization_request_with_relayer(
    from_did: &str,
    to_did: &str,
    payment_id: &str,
    merchant_did: &str,
    amount: u64,
    description: &str,
    new_session_key: Option<&NewSessionKeyRequest>,
    available_payment_methods: &[PaymentMethod],
    relayer_pubkey: &str,
    relayer_url: &str,
) -> Message {
    build_authorization_request_inner(
        from_did, to_did, payment_id, merchant_did, amount, description, new_session_key, Some(available_payment_methods), Some(relayer_pubkey), Some(relayer_url),
    )
}

fn build_authorization_request_inner(
    from_did: &str,
    to_did: &str,
    payment_id: &str,
    merchant_did: &str,
    amount: u64,
    description: &str,
    new_session_key: Option<&NewSessionKeyRequest>,
    available_payment_methods: Option<&[PaymentMethod]>,
    relayer_pubkey: Option<&str>,
    relayer_url: Option<&str>,
) -> Message {
    let mut body = json!({
        "payment_id": payment_id,
        "merchant_did": merchant_did,
        "amount": amount,
        "description": description,
    });

    if let Some(sk) = new_session_key {
        body.as_object_mut().unwrap().insert(
            "new_session_key".to_string(),
            json!({
                "session_key_pubkey": sk.session_key_pubkey,
                "spending_limit": sk.spending_limit,
                "duration_secs": sk.duration_secs,
                "scopes": sk.scopes,
                "token_mint": sk.token_mint,
                "suggested_sol_funding": sk.suggested_sol_funding,
                "suggested_token_funding": sk.suggested_token_funding,
            }),
        );
    }

    if let Some(methods) = available_payment_methods {
        body["available_payment_methods"] = json!(
            methods.iter().map(|m| m.as_str()).collect::<Vec<_>>()
        );
    }

    if let (Some(pk), Some(url)) = (relayer_pubkey, relayer_url) {
        body["relayer_pubkey"] = json!(pk);
        body["relayer_url"] = json!(url);
    }

    Message::new(
        "https://didcomm.org/ignite-pay/1.0/payment-auth-request",
        body,
    )
    .from(from_did.to_string())
    .to(vec![to_did.to_string()])
}

/// Build a payment authorization response message.
/// Sent from the phone app back to the MCP server.
pub fn build_authorization_response(
    from_did: &str,
    to_did: &str,
    payment_id: &str,
    authorized: bool,
    list_action: &str,
) -> Message {
    Message::new(
        "https://didcomm.org/ignite-pay/1.0/payment-auth-response",
        json!({
            "payment_id": payment_id,
            "authorized": authorized,
            "list_action": list_action,
        }),
    )
    .from(from_did.to_string())
    .to(vec![to_did.to_string()])
}

/// Session key data sent from phone to MCP in the V1.0 authorization response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionKeyResponseData {
    /// Base58-encoded ephemeral public key.
    pub session_key_pubkey: String,
    /// Base58-encoded ephemeral secret key (seed) — needed by MCP to sign payments.
    pub session_key_secret_key: String,
    /// On-chain registration transaction signature.
    pub session_key_tx_signature: String,
    /// Unix timestamp when the session expires.
    pub session_expires_at: i64,
    /// Maximum spending limit in lamports.
    pub spending_limit: u64,
    /// Permission scopes (e.g., ["sol:transfer", "spl:transfer"]).
    pub scopes: Vec<String>,
    /// Max number of transactions per day (application-layer, for MCP self-enforcement).
    pub daily_tx_count_limit: u32,
    /// Per-transaction spending limit in lamports (application-layer, for MCP self-enforcement).
    pub per_tx_limit: u64,
    /// SPL Token mint address (base58). None for SOL sessions.
    pub token_mint: Option<String>,
}

/// Build a V1.0 payment authorization response with optional session key data.
/// Extends the base auth response with session key fields when authorized=true.
/// Backward compatible: old fields are always present, new fields added only when session key data is provided.
pub fn build_authorization_response_v1(
    from_did: &str,
    to_did: &str,
    payment_id: &str,
    authorized: bool,
    list_action: &str,
    session_key_data: Option<&SessionKeyResponseData>,
) -> Message {
    build_authorization_response_v1_inner(
        from_did,
        to_did,
        payment_id,
        authorized,
        list_action,
        session_key_data,
        None,
        None,
        None,
    )
}

/// Build a V1.1 payment authorization response with optional session key data and list metadata.
pub fn build_authorization_response_v1_1(
    from_did: &str,
    to_did: &str,
    payment_id: &str,
    authorized: bool,
    list_action: &str,
    session_key_data: Option<&SessionKeyResponseData>,
    list_label: Option<&str>,
    list_max_amount: Option<u64>,
) -> Message {
    build_authorization_response_v1_inner(
        from_did,
        to_did,
        payment_id,
        authorized,
        list_action,
        session_key_data,
        list_label,
        list_max_amount,
        None,
    )
}

/// Build a V1.3 payment authorization response with payment method selection.
pub fn build_authorization_response_v1_3(
    from_did: &str,
    to_did: &str,
    payment_id: &str,
    authorized: bool,
    list_action: &str,
    session_key_data: Option<&SessionKeyResponseData>,
    list_label: Option<&str>,
    list_max_amount: Option<u64>,
    payment_method: Option<&PaymentMethod>,
) -> Message {
    build_authorization_response_v1_inner(
        from_did,
        to_did,
        payment_id,
        authorized,
        list_action,
        session_key_data,
        list_label,
        list_max_amount,
        payment_method,
    )
}

fn build_authorization_response_v1_inner(
    from_did: &str,
    to_did: &str,
    payment_id: &str,
    authorized: bool,
    list_action: &str,
    session_key_data: Option<&SessionKeyResponseData>,
    list_label: Option<&str>,
    list_max_amount: Option<u64>,
    payment_method: Option<&PaymentMethod>,
) -> Message {
    let mut body = json!({
        "payment_id": payment_id,
        "authorized": authorized,
        "list_action": list_action,
    });

    if let Some(sk) = session_key_data {
        body["session_key_pubkey"] = json!(sk.session_key_pubkey);
        body["session_key_secret_key"] = json!(sk.session_key_secret_key);
        body["session_key_tx_signature"] = json!(sk.session_key_tx_signature);
        body["session_expires_at"] = json!(sk.session_expires_at);
        body["spending_limit"] = json!(sk.spending_limit);
        body["scopes"] = json!(sk.scopes);
        body["daily_tx_count_limit"] = json!(sk.daily_tx_count_limit);
        body["per_tx_limit"] = json!(sk.per_tx_limit);
        if let Some(ref mint) = sk.token_mint {
            body["token_mint"] = json!(mint);
        }
    }

    // V1.1: list metadata
    if let Some(label) = list_label {
        body["list_label"] = json!(label);
    }
    if let Some(max_amt) = list_max_amount {
        body["list_max_amount"] = json!(max_amt);
    }

    // V1.3: payment method chosen by user
    if let Some(method) = payment_method {
        body["payment_method"] = json!(method.as_str());
    }

    Message::new(
        "https://didcomm.org/ignite-pay/1.0/payment-auth-response",
        body,
    )
    .from(from_did.to_string())
    .to(vec![to_did.to_string()])
}

/// Build a state channel payment request message.
/// Sent from the merchant MCP to the user App (via mediator) for QR code payments.
pub fn build_channel_payment_request(
    from_did: &str,
    to_did: &str,
    merchant_did: &str,
    amount: u64,
    description: &str,
    order_id: &str,
    hub_endpoint: &str,
    timestamp: i64,
) -> Message {
    Message::new(
        "https://didcomm.org/ignite-pay/1.0/channel-payment-request",
        json!({
            "merchant_did": merchant_did,
            "amount": amount,
            "description": description,
            "order_id": order_id,
            "hub_endpoint": hub_endpoint,
            "timestamp": timestamp,
        }),
    )
    .from(from_did.to_string())
    .to(vec![to_did.to_string()])
}

/// Build a state channel payment confirmation message.
/// Sent from the merchant MCP to the user App after successful channel payment.
pub fn build_channel_payment_confirm(
    from_did: &str,
    to_did: &str,
    order_id: &str,
    channel_id: &str,
    leaf_index: u32,
    sequence: u64,
) -> Message {
    Message::new(
        "https://didcomm.org/ignite-pay/1.0/channel-payment-confirm",
        json!({
            "order_id": order_id,
            "status": "confirmed",
            "channel_id": channel_id,
            "leaf_index": leaf_index,
            "sequence": sequence,
        }),
    )
    .from(from_did.to_string())
    .to(vec![to_did.to_string()])
}

/// Build a QR payment request message.
/// Sent from the Phone App to the MCP server when user scans a merchant QR code.
/// The MCP executes the payment and returns a qr-payment-response.
pub fn build_qr_payment_request(
    from_did: &str,
    to_did: &str,
    merchant_did: &str,
    amount: u64,
    description: &str,
    order_id: &str,
    payment_method: &str,
    token: &str,
    merchant_mediator_url: &str,
) -> Message {
    let mut body = json!({
        "merchant_did": merchant_did,
        "amount": amount,
        "description": description,
        "order_id": order_id,
        "payment_method": payment_method,
        "token": token,
    });
    if !merchant_mediator_url.is_empty() {
        body["merchant_mediator_url"] = json!(merchant_mediator_url);
    }
    Message::new(
        "https://didcomm.org/ignite-pay/1.0/qr-payment-request",
        body,
    )
    .from(from_did.to_string())
    .to(vec![to_did.to_string()])
}

/// Build a QR payment response message.
/// Sent from the MCP server back to the Phone App with the payment result.
pub fn build_qr_payment_response(
    from_did: &str,
    to_did: &str,
    order_id: &str,
    success: bool,
    payment_proof: &str,
    payment_method: &str,
    error: Option<&str>,
) -> Message {
    let mut body = json!({
        "order_id": order_id,
        "success": success,
        "payment_proof": payment_proof,
        "payment_method": payment_method,
    });
    if let Some(err) = error {
        body["error"] = json!(err);
    }
    Message::new(
        "https://didcomm.org/ignite-pay/1.0/qr-payment-response",
        body,
    )
    .from(from_did.to_string())
    .to(vec![to_did.to_string()])
}

/// Build a QR payment notification message.
/// Sent from the buyer's MCP to the merchant's MCP after a successful QR payment.
/// The merchant MCP then forwards a `channel-payment-confirm` to the merchant app
/// for voice announcement.
pub fn build_qr_payment_notify(
    from_did: &str,
    to_did: &str,
    order_id: &str,
    amount: u64,
    payment_method: &str,
    payment_proof: &str,
) -> Message {
    Message::new(
        "https://didcomm.org/ignite-pay/1.0/qr-payment-notify",
        json!({
            "order_id": order_id,
            "amount": amount,
            "payment_method": payment_method,
            "payment_proof": payment_proof,
        }),
    )
    .from(from_did.to_string())
    .to(vec![to_did.to_string()])
}

/// Build a create-channel request message.
/// Sent from the App to the MCP server to request opening a state channel with a Hub.
pub fn build_create_channel_request(
    from_did: &str,
    to_did: &str,
    hub_endpoint: &str,
    provider_pubkey: &str,
    token_mint: &str,
    deposit: u64,
    tree_depth: u32,
) -> Message {
    Message::new(
        "https://didcomm.org/ignite-pay/1.0/create-channel-request",
        json!({
            "hub_endpoint": hub_endpoint,
            "provider_pubkey": provider_pubkey,
            "token_mint": token_mint,
            "deposit": deposit,
            "tree_depth": tree_depth,
        }),
    )
    .from(from_did.to_string())
    .to(vec![to_did.to_string()])
}

/// Build a create-channel response message.
/// Sent from the MCP server back to the App after attempting to open a channel.
pub fn build_create_channel_response(
    from_did: &str,
    to_did: &str,
    channel_id: &str,
    sequence: u64,
    current_root: &str,
    success: bool,
    error_message: Option<&str>,
) -> Message {
    let mut body = json!({
        "channel_id": channel_id,
        "sequence": sequence,
        "current_root": current_root,
        "success": success,
    });
    if let Some(msg) = error_message {
        body["error_message"] = json!(msg);
    }
    Message::new(
        "https://didcomm.org/ignite-pay/1.0/create-channel-response",
        body,
    )
    .from(from_did.to_string())
    .to(vec![to_did.to_string()])
}

/// Build an MB deposit request message.
/// Sent from the App to the MCP server to deposit into the MagicBlock shared vault.
pub fn build_mb_deposit_request(
    from_did: &str,
    to_did: &str,
    amount: u64,
    token: &str,
) -> Message {
    Message::new(
        "https://didcomm.org/ignite-pay/1.0/mb-deposit-request",
        json!({
            "amount": amount,
            "token": token,
        }),
    )
    .from(from_did.to_string())
    .to(vec![to_did.to_string()])
}

/// Build an MB deposit response message.
/// Sent from MCP back to the App after deposit attempt.
pub fn build_mb_deposit_response(
    from_did: &str,
    to_did: &str,
    success: bool,
    deposit_amount: u64,
    total_deposited: Option<u64>,
    tx_signature: Option<&str>,
    token: &str,
    error: Option<&str>,
) -> Message {
    let mut body = json!({
        "success": success,
        "deposit_amount": deposit_amount,
        "token": token,
    });
    if let Some(total) = total_deposited {
        body["total_deposited"] = json!(total);
    }
    if let Some(sig) = tx_signature {
        body["tx_signature"] = json!(sig);
    }
    if let Some(err) = error {
        body["error"] = json!(err);
    }
    Message::new(
        "https://didcomm.org/ignite-pay/1.0/mb-deposit-response",
        body,
    )
    .from(from_did.to_string())
    .to(vec![to_did.to_string()])
}

/// Build a list-sync notification message.
/// Sent from the MCP server to the phone after updating whitelist/blacklist.
pub fn build_list_sync_notification(
    from_did: &str,
    to_did: &str,
    list_type: &str,
    action: &str,
    entry_did: &str,
    new_cid: &str,
) -> Message {
    Message::new(
        "https://didcomm.org/ignite-pay/1.0/list-sync-notification",
        json!({
            "list_type": list_type,
            "action": action,
            "entry_did": entry_did,
            "new_cid": new_cid,
        }),
    )
    .from(from_did.to_string())
    .to(vec![to_did.to_string()])
}

/// Encrypt a DIDComm message using authcrypt (authenticated encryption).
/// Returns a JWE JSON string.
pub fn pack_encrypted(
    agent: &DIDCommAgent,
    msg: &Message,
    sender_did: &str,
    recipient_did: &str,
) -> Result<String, String> {
    agent
        .pack_authcrypt(msg, sender_did, recipient_did)
        .map_err(|e| format!("pack_authcrypt failed: {:?}", e))
}

/// Unpack a JWE (or plaintext) message. Returns the inner Message.
pub fn unpack_message(
    agent: &DIDCommAgent,
    jwe: &str,
    sender_did: Option<&str>,
) -> Result<Message, String> {
    let result = agent
        .unpack(jwe, sender_did)
        .map_err(|e| format!("unpack failed: {:?}", e))?;

    match result {
        UnpackResult::Encrypted { message, .. } => Ok(message),
        UnpackResult::Signed { message, .. } => Ok(message),
        UnpackResult::Plaintext(message) => Ok(message),
    }
}

/// Check if a raw JSON string looks like a JWE (has ciphertext + recipients).
pub fn is_jwe(text: &str) -> bool {
    if let Ok(v) = serde_json::from_str::<Value>(text) {
        v.get("ciphertext").is_some() && v.get("recipients").is_some()
    } else {
        false
    }
}

/// Create a DIDComm agent and register our identity (consumes identity).
/// Returns (agent, did_string).
pub fn create_agent(identity: PrivateIdentity) -> (DIDCommAgent, String) {
    let did = identity.did.clone();
    let mut agent = DIDCommAgent::new();
    agent.add_identity(identity);
    (agent, did)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_DID: &str = "did:ignite:zTestMcp123";
    const PHONE_DID: &str = "did:ignite:zTestPhone456";

    #[test]
    fn test_build_oob_invitation_fields() {
        let did_doc = json!({"id": TEST_DID, "verificationMethod": []});
        let msg = build_oob_invitation(
            TEST_DID,
            "Test MCP",
            "wss://mediator.example.com/ws",
            &did_doc,
        );

        assert_eq!(msg.typ, "https://didcomm.org/out-of-band/2.0/invitation");
        assert_eq!(msg.from.as_ref().unwrap(), TEST_DID);
        assert_eq!(msg.body.get("label").unwrap().as_str(), Some("Test MCP"));
        assert_eq!(msg.body.get("goal_code").unwrap().as_str(), Some("p2p-messaging"));
        assert_eq!(
            msg.body.get("did_document").unwrap().get("id").unwrap().as_str(),
            Some(TEST_DID)
        );

        let services = msg.body.get("services").unwrap().as_array().unwrap();
        assert_eq!(services.len(), 1);
        assert_eq!(
            services[0].get("service_endpoint").unwrap().as_str(),
            Some("wss://mediator.example.com/ws")
        );
        assert_eq!(
            services[0].get("routing_keys").unwrap().as_array().unwrap(),
            &vec![json!(TEST_DID)]
        );
    }

    #[test]
    fn test_build_connection_request_without_fcm() {
        let msg = build_connection_request(PHONE_DID, TEST_DID, "websocket", None);

        assert_eq!(
            msg.typ,
            "https://didcomm.org/ignite-pay/1.0/connection-request"
        );
        assert_eq!(msg.from.as_ref().unwrap(), PHONE_DID);
        assert_eq!(
            msg.to.as_ref().unwrap().first().unwrap(),
            TEST_DID
        );
        assert_eq!(msg.body.get("push_channel").unwrap().as_str(), Some("websocket"));
        assert!(msg.body.get("fcm_token").is_none());
    }

    #[test]
    fn test_build_connection_request_with_fcm() {
        let msg = build_connection_request(
            PHONE_DID,
            TEST_DID,
            "fcm",
            Some("token_abc123"),
        );

        assert_eq!(msg.body.get("push_channel").unwrap().as_str(), Some("fcm"));
        assert_eq!(msg.body.get("fcm_token").unwrap().as_str(), Some("token_abc123"));
    }

    #[test]
    fn test_build_connection_response() {
        let did_doc = json!({"id": TEST_DID});
        let msg = build_connection_response(TEST_DID, PHONE_DID, true, Some(&did_doc), Some("https://example.com"), Some("nonce123"), Some("sig456"));

        assert_eq!(
            msg.typ,
            "https://didcomm.org/ignite-pay/1.0/connection-response"
        );
        assert_eq!(msg.body.get("accepted").unwrap().as_bool(), Some(true));
        assert_eq!(msg.body.get("mcp_nonce").unwrap().as_str(), Some("nonce123"));
        assert_eq!(msg.body.get("mcp_signature").unwrap().as_str(), Some("sig456"));
        assert_eq!(msg.from.as_ref().unwrap(), TEST_DID);
        assert_eq!(
            msg.to.as_ref().unwrap().first().unwrap(),
            PHONE_DID
        );
    }

    #[test]
    fn test_build_connection_response_rejected() {
        let msg = build_connection_response(TEST_DID, PHONE_DID, false, None, None, None, None);
        assert_eq!(msg.body.get("accepted").unwrap().as_bool(), Some(false));
    }

    #[test]
    fn test_is_jwe_valid() {
        let jwe = json!({
            "ciphertext": "abc123",
            "recipients": [{"header": {"kid": "test"}}],
        });
        assert!(is_jwe(&jwe.to_string()));
    }

    #[test]
    fn test_is_jwe_invalid_no_ciphertext() {
        let not_jwe = json!({"recipients": []});
        assert!(!is_jwe(&not_jwe.to_string()));
    }

    #[test]
    fn test_is_jwe_invalid_not_json() {
        assert!(!is_jwe("not json at all"));
    }

    #[test]
    fn test_oob_invitation_roundtrip_url_parse() {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine;

        let did_doc = json!({"id": TEST_DID});
        let msg = build_oob_invitation(
            TEST_DID,
            "MCP",
            "ws://localhost:3000",
            &did_doc,
        );
        let json_str = serde_json::to_string(&msg).unwrap();
        let b64 = URL_SAFE_NO_PAD.encode(json_str.as_bytes());
        let url = format!("didcomm://?_oob={}", b64);

        // Parse it back manually (no url crate needed)
        let query_start = url.find("_oob=").unwrap();
        let oob_param = &url[query_start + 5..];
        let decoded = URL_SAFE_NO_PAD.decode(oob_param).unwrap();
        let parsed_msg: Value = serde_json::from_slice(&decoded).unwrap();

        assert_eq!(parsed_msg["type"], "https://didcomm.org/out-of-band/2.0/invitation");
        assert_eq!(parsed_msg["from"], TEST_DID);
        assert_eq!(parsed_msg["body"]["label"], "MCP");
    }

    // --- build_authorization_request ---

    #[test]
    fn test_build_authorization_request_fields() {
        let msg = build_authorization_request(
            TEST_DID,
            PHONE_DID,
            "pay-001",
            "did:ignite:zMerchant",
            500_000_000,
            "Coffee",
        );
        assert_eq!(msg.typ, "https://didcomm.org/ignite-pay/1.0/payment-auth-request");
        assert_eq!(msg.from.as_ref().unwrap(), TEST_DID);
        assert_eq!(msg.to.as_ref().unwrap().first().unwrap(), PHONE_DID);
        assert_eq!(msg.body["payment_id"], "pay-001");
        assert_eq!(msg.body["merchant_did"], "did:ignite:zMerchant");
        assert_eq!(msg.body["amount"], 500_000_000);
        assert_eq!(msg.body["description"], "Coffee");
        // No payment methods when not provided
        assert!(msg.body.get("available_payment_methods").is_none());
    }

    #[test]
    fn test_build_authorization_request_with_methods() {
        let methods = vec![PaymentMethod::SessionKey, PaymentMethod::MagicBlock];
        let msg = build_authorization_request_with_methods(
            TEST_DID,
            PHONE_DID,
            "pay-methods",
            "did:ignite:zMerchant",
            100,
            "Test",
            None,
            &methods,
        );
        let methods_arr = msg.body["available_payment_methods"].as_array().unwrap();
        assert_eq!(methods_arr.len(), 2);
        assert_eq!(methods_arr[0], "session_key");
        assert_eq!(methods_arr[1], "magicblock");
    }

    #[test]
    fn test_build_authorization_request_with_methods_and_session_key() {
        let sk = NewSessionKeyRequest {
            session_key_pubkey: "pubkey_test".to_string(),
            spending_limit: 1000,
            duration_secs: 3600,
            scopes: vec!["sol:transfer".to_string()],
            token_mint: None,
            suggested_sol_funding: 5000,
            suggested_token_funding: None,
        };
        let methods = vec![PaymentMethod::SessionKey];
        let msg = build_authorization_request_with_methods(
            TEST_DID,
            PHONE_DID,
            "pay-both",
            "did:ignite:zMerchant",
            100,
            "Test",
            Some(&sk),
            &methods,
        );
        assert!(msg.body.get("new_session_key").is_some());
        assert_eq!(msg.body["available_payment_methods"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_payment_method_enum() {
        assert_eq!(PaymentMethod::SessionKey.as_str(), "session_key");
        assert_eq!(PaymentMethod::MagicBlock.as_str(), "magicblock");
        assert_eq!(PaymentMethod::Relayer.as_str(), "relayer");

        // Serde roundtrip
        let json = serde_json::to_string(&PaymentMethod::MagicBlock).unwrap();
        assert_eq!(json, "\"magicblock\"");
        let parsed: PaymentMethod = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, PaymentMethod::MagicBlock);
    }

    #[test]
    fn test_build_auth_response_v1_3_with_payment_method() {
        let msg = build_authorization_response_v1_3(
            PHONE_DID,
            TEST_DID,
            "pay-006",
            true,
            "none",
            None,
            None,
            None,
            Some(&PaymentMethod::MagicBlock),
        );
        assert_eq!(msg.body["payment_method"], "magicblock");
    }

    // --- build_authorization_response_v1 ---

    #[test]
    fn test_build_auth_response_v1_without_session_key() {
        let msg = build_authorization_response_v1(
            PHONE_DID,
            TEST_DID,
            "pay-002",
            true,
            "none",
            None,
        );
        assert_eq!(msg.typ, "https://didcomm.org/ignite-pay/1.0/payment-auth-response");
        assert_eq!(msg.from.as_ref().unwrap(), PHONE_DID);
        assert_eq!(msg.to.as_ref().unwrap().first().unwrap(), TEST_DID);
        assert_eq!(msg.body["payment_id"], "pay-002");
        assert_eq!(msg.body["authorized"], true);
        assert_eq!(msg.body["list_action"], "none");
        assert!(msg.body.get("session_key_pubkey").is_none());
    }

    #[test]
    fn test_build_auth_response_v1_with_session_key() {
        let sk = SessionKeyResponseData {
            session_key_pubkey: "pubkey123".to_string(),
            session_key_secret_key: "secret456".to_string(),
            session_key_tx_signature: "sig789".to_string(),
            session_expires_at: 1700000000,
            spending_limit: 100_000,
            scopes: vec!["sol:transfer".to_string(), "spl:transfer".to_string()],
            daily_tx_count_limit: 50,
            per_tx_limit: 10_000,
            token_mint: None,
        };
        let msg = build_authorization_response_v1(
            PHONE_DID,
            TEST_DID,
            "pay-003",
            true,
            "add_whitelist",
            Some(&sk),
        );
        assert_eq!(msg.body["session_key_pubkey"], "pubkey123");
        assert_eq!(msg.body["session_key_secret_key"], "secret456");
        assert_eq!(msg.body["session_key_tx_signature"], "sig789");
        assert_eq!(msg.body["session_expires_at"], 1700000000);
        assert_eq!(msg.body["spending_limit"], 100_000);
        assert_eq!(msg.body["scopes"], json!(["sol:transfer", "spl:transfer"]));
        assert_eq!(msg.body["daily_tx_count_limit"], 50);
        assert_eq!(msg.body["per_tx_limit"], 10_000);
    }

    // --- build_authorization_response_v1_1 ---

    #[test]
    fn test_build_auth_response_v1_1_with_list_metadata() {
        let msg = build_authorization_response_v1_1(
            PHONE_DID,
            TEST_DID,
            "pay-004",
            true,
            "add_whitelist",
            None,
            Some("Trusted Shop"),
            Some(50_000_000),
        );
        assert_eq!(msg.body["list_label"], "Trusted Shop");
        assert_eq!(msg.body["list_max_amount"], 50_000_000);
    }

    #[test]
    fn test_build_auth_response_v1_1_without_optionals() {
        let msg = build_authorization_response_v1_1(
            PHONE_DID,
            TEST_DID,
            "pay-005",
            false,
            "none",
            None,
            None,
            None,
        );
        assert!(msg.body.get("list_label").is_none());
        assert!(msg.body.get("list_max_amount").is_none());
        assert!(msg.body.get("session_key_pubkey").is_none());
    }

    // --- build_list_sync_notification ---

    #[test]
    fn test_build_list_sync_notification() {
        let msg = build_list_sync_notification(
            TEST_DID,
            PHONE_DID,
            "whitelist",
            "add",
            "did:ignite:zMerchant",
            "QmNewCid123",
        );
        assert_eq!(msg.typ, "https://didcomm.org/ignite-pay/1.0/list-sync-notification");
        assert_eq!(msg.from.as_ref().unwrap(), TEST_DID);
        assert_eq!(msg.to.as_ref().unwrap().first().unwrap(), PHONE_DID);
        assert_eq!(msg.body["list_type"], "whitelist");
        assert_eq!(msg.body["action"], "add");
        assert_eq!(msg.body["entry_did"], "did:ignite:zMerchant");
        assert_eq!(msg.body["new_cid"], "QmNewCid123");
    }

    // --- build_ws_challenge_response ---

    #[test]
    fn test_build_ws_challenge_response() {
        let did_doc = json!({"id": PHONE_DID});
        let msg = build_ws_challenge_response(
            PHONE_DID,
            TEST_DID,
            "nonce-abc-123",
            &did_doc,
        );
        assert_eq!(msg.typ, "https://didcomm.org/ignite-pay/1.0/ws-challenge-response");
        assert_eq!(msg.from.as_ref().unwrap(), PHONE_DID);
        assert_eq!(msg.to.as_ref().unwrap().first().unwrap(), TEST_DID);
        assert_eq!(msg.body["nonce"], "nonce-abc-123");
        assert_eq!(msg.body["did_document"]["id"], PHONE_DID);
    }

    // --- build_connection_confirm ---

    #[test]
    fn test_build_connection_confirm() {
        let msg = build_connection_confirm(
            PHONE_DID,
            TEST_DID,
            "nonce-abc-123",
            "sig-base64-xyz",
        );
        assert_eq!(msg.typ, "https://didcomm.org/ignite-pay/1.0/connection-confirm");
        assert_eq!(msg.from.as_ref().unwrap(), PHONE_DID);
        assert_eq!(msg.to.as_ref().unwrap().first().unwrap(), TEST_DID);
        assert_eq!(msg.body["phone_nonce"], "nonce-abc-123");
        assert_eq!(msg.body["phone_signature"], "sig-base64-xyz");
    }

    // --- build_connection_confirm_response ---

    #[test]
    fn test_build_connection_confirm_response_accepted() {
        let msg = build_connection_confirm_response(
            TEST_DID,
            PHONE_DID,
            "mcp-nonce-456",
            "mcp-sig-789",
            "phone-nonce-echo",
            true,
        );
        assert_eq!(msg.typ, "https://didcomm.org/ignite-pay/1.0/connection-confirm-response");
        assert_eq!(msg.from.as_ref().unwrap(), TEST_DID);
        assert_eq!(msg.to.as_ref().unwrap().first().unwrap(), PHONE_DID);
        assert_eq!(msg.body["mcp_nonce"], "mcp-nonce-456");
        assert_eq!(msg.body["mcp_signature"], "mcp-sig-789");
        assert_eq!(msg.body["phone_nonce_echo"], "phone-nonce-echo");
        assert_eq!(msg.body["accepted"], true);
    }

    #[test]
    fn test_build_connection_confirm_response_rejected() {
        let msg = build_connection_confirm_response(
            TEST_DID,
            PHONE_DID,
            "",
            "",
            "some-nonce",
            false,
        );
        assert_eq!(msg.body["accepted"], false);
    }

    // --- build_status_request ---

    #[test]
    fn test_build_status_request() {
        let msg = build_status_request(PHONE_DID);
        assert_eq!(msg.typ, "https://didcomm.org/messagepickup/3.0/status-request");
        assert_eq!(msg.from.as_ref().unwrap(), PHONE_DID);
    }

    // --- build_batch_pickup ---

    #[test]
    fn test_build_batch_pickup() {
        let msg = build_batch_pickup(PHONE_DID, 25);
        assert_eq!(msg.typ, "https://didcomm.org/messagepickup/3.0/batch-pickup");
        assert_eq!(msg.from.as_ref().unwrap(), PHONE_DID);
        assert_eq!(msg.body["count"], 25);
    }

    // --- build_peer_introduction ---

    #[test]
    fn test_build_peer_introduction() {
        let did_doc = json!({"id": PHONE_DID, "verificationMethod": []});
        let msg = build_peer_introduction(PHONE_DID, &did_doc);
        assert_eq!(msg.typ, "https://didcomm.org/peer-did-discovery/1.0/discover");
        assert_eq!(msg.from.as_ref().unwrap(), PHONE_DID);
        assert_eq!(msg.body["did_document"]["id"], PHONE_DID);
    }

    // --- build_channel_payment_request ---

    #[test]
    fn test_build_channel_payment_request() {
        let msg = build_channel_payment_request(
            TEST_DID,
            PHONE_DID,
            "did:ignite:zMerchant",
            100_000,
            "Coffee",
            "order-123",
            "https://hub.example.com",
            1700000000,
        );
        assert_eq!(msg.typ, "https://didcomm.org/ignite-pay/1.0/channel-payment-request");
        assert_eq!(msg.from.as_ref().unwrap(), TEST_DID);
        assert_eq!(msg.to.as_ref().unwrap().first().unwrap(), PHONE_DID);
        assert_eq!(msg.body["merchant_did"], "did:ignite:zMerchant");
        assert_eq!(msg.body["amount"], 100_000);
        assert_eq!(msg.body["description"], "Coffee");
        assert_eq!(msg.body["order_id"], "order-123");
        assert_eq!(msg.body["hub_endpoint"], "https://hub.example.com");
        assert_eq!(msg.body["timestamp"], 1700000000);
    }

    // --- build_channel_payment_confirm ---

    #[test]
    fn test_build_channel_payment_confirm() {
        let msg = build_channel_payment_confirm(
            TEST_DID,
            PHONE_DID,
            "order-123",
            "hexchannelid",
            3,
            15,
        );
        assert_eq!(msg.typ, "https://didcomm.org/ignite-pay/1.0/channel-payment-confirm");
        assert_eq!(msg.from.as_ref().unwrap(), TEST_DID);
        assert_eq!(msg.to.as_ref().unwrap().first().unwrap(), PHONE_DID);
        assert_eq!(msg.body["order_id"], "order-123");
        assert_eq!(msg.body["status"], "confirmed");
        assert_eq!(msg.body["channel_id"], "hexchannelid");
        assert_eq!(msg.body["leaf_index"], 3);
        assert_eq!(msg.body["sequence"], 15);
    }

    // --- build_create_channel_request ---

    #[test]
    fn test_build_create_channel_request() {
        let msg = build_create_channel_request(
            PHONE_DID,
            TEST_DID,
            "http://hub:3003",
            "Base58SolanaPubkey",
            "Base58MintAddress",
            1_000_000_000,
            8,
        );
        assert_eq!(
            msg.typ,
            "https://didcomm.org/ignite-pay/1.0/create-channel-request"
        );
        assert_eq!(msg.from.as_ref().unwrap(), PHONE_DID);
        assert_eq!(msg.to.as_ref().unwrap().first().unwrap(), TEST_DID);
        assert_eq!(msg.body["hub_endpoint"], "http://hub:3003");
        assert_eq!(msg.body["provider_pubkey"], "Base58SolanaPubkey");
        assert_eq!(msg.body["token_mint"], "Base58MintAddress");
        assert_eq!(msg.body["deposit"], 1_000_000_000);
        assert_eq!(msg.body["tree_depth"], 8);
    }

    // --- build_create_channel_response ---

    #[test]
    fn test_build_create_channel_response_success() {
        let msg = build_create_channel_response(
            TEST_DID,
            PHONE_DID,
            "hexchannelid",
            0,
            "hexroot",
            true,
            None,
        );
        assert_eq!(
            msg.typ,
            "https://didcomm.org/ignite-pay/1.0/create-channel-response"
        );
        assert_eq!(msg.from.as_ref().unwrap(), TEST_DID);
        assert_eq!(msg.to.as_ref().unwrap().first().unwrap(), PHONE_DID);
        assert_eq!(msg.body["channel_id"], "hexchannelid");
        assert_eq!(msg.body["sequence"], 0);
        assert_eq!(msg.body["current_root"], "hexroot");
        assert_eq!(msg.body["success"], true);
        assert!(msg.body.get("error_message").is_none());
    }

    #[test]
    fn test_build_qr_payment_request() {
        let msg = build_qr_payment_request(
            PHONE_DID,
            TEST_DID,
            "did:ignite:zMerchant",
            500_000_000,
            "Coffee",
            "order-123",
            "session_key",
            "SOL",
            "https://merchant-relay.example.com/",
        );
        assert_eq!(msg.typ, "https://didcomm.org/ignite-pay/1.0/qr-payment-request");
        assert_eq!(msg.from.as_ref().unwrap(), PHONE_DID);
        assert_eq!(msg.to.as_ref().unwrap().first().unwrap(), TEST_DID);
        assert_eq!(msg.body["merchant_did"], "did:ignite:zMerchant");
        assert_eq!(msg.body["amount"], 500_000_000);
        assert_eq!(msg.body["order_id"], "order-123");
        assert_eq!(msg.body["payment_method"], "session_key");
        assert_eq!(msg.body["token"], "SOL");
        assert_eq!(msg.body["merchant_mediator_url"], "https://merchant-relay.example.com/");
    }

    #[test]
    fn test_build_qr_payment_notify() {
        let msg = build_qr_payment_notify(
            "did:ignite:zBuyerMcp",
            "did:ignite:zMerchant",
            "order-789",
            250_000_000,
            "session_key",
            "Tx: def456",
        );
        assert_eq!(msg.typ, "https://didcomm.org/ignite-pay/1.0/qr-payment-notify");
        assert_eq!(msg.from.as_ref().unwrap(), "did:ignite:zBuyerMcp");
        assert_eq!(msg.to.as_ref().unwrap().first().unwrap(), "did:ignite:zMerchant");
        assert_eq!(msg.body["order_id"], "order-789");
        assert_eq!(msg.body["amount"], 250_000_000);
        assert_eq!(msg.body["payment_method"], "session_key");
        assert_eq!(msg.body["payment_proof"], "Tx: def456");
    }

    #[test]
    fn test_build_qr_payment_response_success() {
        let msg = build_qr_payment_response(
            TEST_DID,
            PHONE_DID,
            "order-123",
            true,
            "Tx: abc123",
            "session_key",
            None,
        );
        assert_eq!(msg.typ, "https://didcomm.org/ignite-pay/1.0/qr-payment-response");
        assert_eq!(msg.body["order_id"], "order-123");
        assert_eq!(msg.body["success"], true);
        assert_eq!(msg.body["payment_proof"], "Tx: abc123");
        assert_eq!(msg.body["payment_method"], "session_key");
        assert!(msg.body.get("error").is_none());
    }

    #[test]
    fn test_build_qr_payment_response_error() {
        let msg = build_qr_payment_response(
            TEST_DID,
            PHONE_DID,
            "order-456",
            false,
            "",
            "magicblock",
            Some("No channel found"),
        );
        assert_eq!(msg.body["success"], false);
        assert_eq!(msg.body["error"], "No channel found");
    }

    #[test]
    fn test_build_create_channel_response_error() {
        let msg = build_create_channel_response(
            TEST_DID,
            PHONE_DID,
            "",
            0,
            "",
            false,
            Some("Failed to open channel"),
        );
        assert_eq!(msg.body["success"], false);
        assert_eq!(msg.body["error_message"], "Failed to open channel");
    }

    // --- pack/unpack roundtrip ---

    #[test]
    fn test_pack_unpack_roundtrip() {
        let (sender_identity, sender_did) = crate::identity::generate_ignite_did();
        let (receiver_identity, receiver_did) = crate::identity::generate_ignite_did();

        let sender_doc = crate::build_did_document(&sender_did, &sender_identity);
        let receiver_doc = crate::build_did_document(&receiver_did, &receiver_identity);

        let (mut sender_agent, _) = create_agent(sender_identity);
        let (mut receiver_agent, _) = create_agent(receiver_identity);

        // Register each other as peers
        if let Some(resolved) = crate::parse_did_document(&receiver_did, &receiver_doc) {
            sender_agent.add_peer(resolved);
        }
        if let Some(resolved) = crate::parse_did_document(&sender_did, &sender_doc) {
            receiver_agent.add_peer(resolved);
        }

        // Pack
        let msg = build_authorization_request(
            &sender_did,
            &receiver_did,
            "pay-test",
            "did:ignite:zMerchant",
            100,
            "test",
        );
        let jwe = pack_encrypted(&sender_agent, &msg, &sender_did, &receiver_did).unwrap();
        assert!(is_jwe(&jwe));

        // Unpack
        let unpacked = unpack_message(&receiver_agent, &jwe, Some(&sender_did)).unwrap();
        assert_eq!(unpacked.typ, "https://didcomm.org/ignite-pay/1.0/payment-auth-request");
        assert_eq!(unpacked.body["payment_id"], "pay-test");
        assert_eq!(unpacked.body["amount"], 100);
    }

    #[test]
    fn test_pack_unpack_connection_request_roundtrip() {
        let (sender_identity, sender_did) = crate::identity::generate_ignite_did();
        let (receiver_identity, receiver_did) = crate::identity::generate_ignite_did();

        let sender_doc = crate::build_did_document(&sender_did, &sender_identity);
        let receiver_doc = crate::build_did_document(&receiver_did, &receiver_identity);

        let (mut sender_agent, _) = create_agent(sender_identity);
        let (mut receiver_agent, _) = create_agent(receiver_identity);

        if let Some(resolved) = crate::parse_did_document(&receiver_did, &receiver_doc) {
            sender_agent.add_peer(resolved);
        }
        if let Some(resolved) = crate::parse_did_document(&sender_did, &sender_doc) {
            receiver_agent.add_peer(resolved);
        }

        let msg = build_connection_request(&sender_did, &receiver_did, "fcm", Some("token_xyz"));
        let jwe = pack_encrypted(&sender_agent, &msg, &sender_did, &receiver_did).unwrap();

        let unpacked = unpack_message(&receiver_agent, &jwe, Some(&sender_did)).unwrap();
        assert_eq!(unpacked.typ, "https://didcomm.org/ignite-pay/1.0/connection-request");
        assert_eq!(unpacked.body["push_channel"], "fcm");
        assert_eq!(unpacked.body["fcm_token"], "token_xyz");
    }

    #[test]
    fn test_pack_unpack_oob_invitation_roundtrip() {
        let (sender_identity, sender_did) = crate::identity::generate_ignite_did();
        let (receiver_identity, receiver_did) = crate::identity::generate_ignite_did();

        let sender_doc = crate::build_did_document(&sender_did, &sender_identity);
        let receiver_doc = crate::build_did_document(&receiver_did, &receiver_identity);

        let (mut sender_agent, _) = create_agent(sender_identity);
        let (mut receiver_agent, _) = create_agent(receiver_identity);

        if let Some(resolved) = crate::parse_did_document(&receiver_did, &receiver_doc) {
            sender_agent.add_peer(resolved);
        }
        if let Some(resolved) = crate::parse_did_document(&sender_did, &sender_doc) {
            receiver_agent.add_peer(resolved);
        }

        let msg = build_oob_invitation(&sender_did, "MCP", "wss://example.com/ws", &sender_doc);
        let jwe = pack_encrypted(&sender_agent, &msg, &sender_did, &receiver_did).unwrap();

        let unpacked = unpack_message(&receiver_agent, &jwe, Some(&sender_did)).unwrap();
        assert_eq!(unpacked.typ, "https://didcomm.org/out-of-band/2.0/invitation");
        assert_eq!(unpacked.body["label"], "MCP");
    }
}
