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
    extract::{Path, State},
    Json,
};
use serde_json::{json, Value};

use crate::error::ChannelServiceError;
use crate::state::AppState;

fn decode_channel_id(hex_str: &str) -> Result<[u8; 32], ChannelServiceError> {
    let bytes = hex::decode(hex_str)
        .map_err(|_| ChannelServiceError::BadRequest("invalid channel_id hex".into()))?;
    bytes.try_into()
        .map_err(|_| ChannelServiceError::BadRequest("channel_id must be 32 bytes".into()))
}

pub async fn get_status(
    State(state): State<AppState>,
    Path(channel_id_hex): Path<String>,
) -> Result<Json<Value>, ChannelServiceError> {
    let channel_id = decode_channel_id(&channel_id_hex)?;

    match state.compliance_manager {
        Some(ref mgr) => {
            let mgr = mgr.lock().await;
            let compliance_state = mgr.load_state(channel_id)
                .map_err(ChannelServiceError::StateChannel)?;

            Ok(Json(json!({
                "channel_id": channel_id_hex,
                "cumulative_spent": compliance_state.cumulative_spent,
                "compliance_hold": compliance_state.compliance_hold,
                "last_check_slot": compliance_state.last_check_slot,
                "threshold": compliance_state.limits.threshold,
                "per_channel_limit": compliance_state.limits.per_channel,
                "window_slots": compliance_state.limits.window_slots,
                "window_payment_count": compliance_state.window_payments.len(),
                "travel_rule_count": compliance_state.travel_rules.len(),
            })))
        }
        None => Ok(Json(json!({
            "channel_id": channel_id_hex,
            "compliance_enabled": false,
        }))),
    }
}
