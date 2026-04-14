pub mod fcm;

use async_trait::async_trait;

/// Trait for sending push notifications to devices.
#[async_trait]
pub trait NotificationSender: Send + Sync {
    /// Send a signal notification to a device.
    async fn send_signal(&self, device_token: &str, msg_id: &str) -> anyhow::Result<()>;
}

/// No-op notification sender for when FCM is not configured.
pub struct NoopNotificationSender;

#[async_trait]
impl NotificationSender for NoopNotificationSender {
    async fn send_signal(&self, _device_token: &str, _msg_id: &str) -> anyhow::Result<()> {
        // No-op
        Ok(())
    }
}
