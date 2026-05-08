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

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Full Hub record from the database.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Hub {
    pub hub_id: Uuid,
    pub hub_did: String,
    pub endpoint_url: String,
    pub name: String,
    pub description: String,
    pub status: String,
    pub active_pubkey: Option<String>,
    pub collateral: i64,
    pub available_liquidity: i64,
    pub fee_rate_bps: i16,
    pub supported_tokens: Vec<String>,
    pub online_rate: i16,
    pub success_rate: i16,
    pub avg_latency_ms: i32,
    pub active_channels: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Request to register a new Hub.
#[derive(Debug, Deserialize)]
pub struct RegisterHubRequest {
    pub hub_did: String,
    pub endpoint_url: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub active_pubkey: Option<String>,
    #[serde(default)]
    pub collateral: i64,
    #[serde(default)]
    pub available_liquidity: i64,
    #[serde(default)]
    pub fee_rate_bps: i16,
    #[serde(default)]
    pub supported_tokens: Vec<String>,
}

/// Request to update a Hub's info.
#[derive(Debug, Deserialize)]
pub struct UpdateHubRequest {
    pub endpoint_url: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub status: Option<String>,
    pub active_pubkey: Option<String>,
    pub collateral: Option<i64>,
    pub available_liquidity: Option<i64>,
    pub fee_rate_bps: Option<i16>,
    pub supported_tokens: Option<Vec<String>>,
}

/// Request to update Hub metrics.
#[derive(Debug, Deserialize)]
pub struct UpdateMetricsRequest {
    pub online_rate: Option<i16>,
    pub success_rate: Option<i16>,
    pub avg_latency_ms: Option<i32>,
    pub active_channels: Option<i32>,
    pub available_liquidity: Option<i64>,
    pub fee_rate_bps: Option<i16>,
}

/// Query parameters for listing hubs.
#[derive(Debug, Deserialize)]
pub struct ListHubsQuery {
    pub status: Option<String>,
    pub token_mint: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}
