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

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info};

use super::NotificationSender;

/// Cached OAuth2 access token for FCM.
struct CachedToken {
    access_token: String,
    expires_at: chrono::DateTime<chrono::Utc>,
}

/// FCM push notification sender using Firebase Cloud Messaging HTTP v1 API.
///
/// Authenticates using a Firebase service account JSON key file.
/// The service account JSON is downloaded from Firebase Console:
///   Project Settings > Service Accounts > Generate New Private Key.
pub struct FcmSender {
    project_id: String,
    client_email: String,
    private_key_pem: String,
    token_uri: String,
    http: reqwest::Client,
    cached_token: Arc<RwLock<Option<CachedToken>>>,
}

/// Minimal deserialization of the Firebase service account JSON.
#[derive(Debug, Deserialize)]
struct ServiceAccount {
    #[serde(rename = "project_id")]
    project_id: String,
    #[serde(rename = "client_email")]
    client_email: String,
    #[serde(rename = "private_key")]
    private_key: String,
    #[serde(rename = "token_uri", default = "default_token_uri")]
    token_uri: String,
}

fn default_token_uri() -> String {
    "https://oauth2.googleapis.com/token".to_string()
}

impl FcmSender {
    /// Create a new FCM sender from a Firebase service account JSON file.
    pub fn from_service_account_file(path: &str, project_id: Option<String>) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Self::from_service_account_json(&content, project_id)
    }

    /// Create a new FCM sender from raw service account JSON content.
    fn from_service_account_json(json_str: &str, project_id: Option<String>) -> anyhow::Result<Self> {
        let sa: ServiceAccount = serde_json::from_str(json_str)?;

        let project_id = project_id.unwrap_or(sa.project_id);

        Ok(Self {
            project_id,
            client_email: sa.client_email,
            private_key_pem: sa.private_key,
            token_uri: sa.token_uri,
            http: reqwest::Client::new(),
            cached_token: Arc::new(RwLock::new(None)),
        })
    }

    /// Get a valid OAuth2 access token, refreshing if needed.
    async fn get_access_token(&self) -> anyhow::Result<String> {
        // Check cache first
        {
            let cached = self.cached_token.read().await;
            if let Some(ref token) = *cached {
                // Refresh 60 seconds before expiry
                if token.expires_at > chrono::Utc::now() + chrono::Duration::seconds(60) {
                    return Ok(token.access_token.clone());
                }
            }
        }

        // Generate JWT assertion using jsonwebtoken (already in dependencies)
        let now = chrono::Utc::now().timestamp() as usize;
        let claims = json!({
            "iss": self.client_email,
            "scope": "https://www.googleapis.com/auth/firebase.messaging",
            "aud": self.token_uri,
            "iat": now,
            "exp": now + 3600,
        });

        let encoding_key = jsonwebtoken::EncodingKey::from_rsa_pem(self.private_key_pem.as_bytes())
            .map_err(|e| anyhow::anyhow!("Failed to parse RSA private key: {}", e))?;

        let header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
        let assertion = jsonwebtoken::encode(&header, &claims, &encoding_key)
            .map_err(|e| anyhow::anyhow!("Failed to sign JWT assertion: {}", e))?;

        // Exchange assertion for access token
        let resp = self
            .http
            .post(&self.token_uri)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", &assertion),
            ])
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "OAuth2 token request failed: {} - {}",
                status,
                body
            ));
        }

        #[derive(Deserialize)]
        struct TokenResponse {
            access_token: String,
            expires_in: Option<u64>,
        }

        let token_resp: TokenResponse = resp.json().await?;
        let expires_in = token_resp.expires_in.unwrap_or(3600);

        let cached = CachedToken {
            access_token: token_resp.access_token.clone(),
            expires_at: chrono::Utc::now() + chrono::Duration::seconds(expires_in as i64),
        };

        {
            let mut guard = self.cached_token.write().await;
            *guard = Some(cached);
        }

        Ok(token_resp.access_token)
    }
}

#[async_trait]
impl NotificationSender for FcmSender {
    async fn send_signal(&self, device_token: &str, msg_id: &str) -> anyhow::Result<()> {
        let access_token = self.get_access_token().await?;

        let fcm_url = format!(
            "https://fcm.googleapis.com/v1/projects/{}/messages:send",
            self.project_id
        );

        let payload = json!({
            "message": {
                "token": device_token,
                "data": {
                    "type": "SIGNAL",
                    "msg_id": msg_id,
                },
                "android": {
                    "priority": "high"
                },
                "apns": {
                    "payload": {
                        "aps": {
                            "content-available": 1
                        }
                    }
                }
            }
        });

        let response = self
            .http
            .post(&fcm_url)
            .header("Authorization", format!("Bearer {}", access_token))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;

        if response.status().is_success() {
            info!("FCM signal sent for message {}", msg_id);
            Ok(())
        } else {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            error!("FCM send failed: {} - {}", status, body);
            Err(anyhow::anyhow!("FCM send failed: {} - {}", status, body))
        }
    }
}
