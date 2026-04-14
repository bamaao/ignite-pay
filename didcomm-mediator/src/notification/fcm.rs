use async_trait::async_trait;
use serde_json::json;
use tracing::{error, info};

use super::NotificationSender;

/// FCM push notification sender using Firebase Cloud Messaging HTTP v1 API.
pub struct FcmSender {
    server_key: String,
    client: reqwest::Client,
}

impl FcmSender {
    pub fn new(server_key: String) -> Self {
        Self {
            server_key,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl NotificationSender for FcmSender {
    async fn send_signal(&self, device_token: &str, msg_id: &str) -> anyhow::Result<()> {
        let payload = json!({
            "to": device_token,
            "data": {
                "type": "SIGNAL",
                "msg_id": msg_id,
            },
            "android": {
                "priority": "high"
            },
            "content_available": true,
        });

        let response = self
            .client
            .post("https://fcm.googleapis.com/fcm/send")
            .header("Authorization", format!("key={}", self.server_key))
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
