use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Status of a merchant payment order.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum OrderStatus {
    Pending,
    Confirmed,
    Failed,
    Expired,
}

impl std::fmt::Display for OrderStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OrderStatus::Pending => write!(f, "pending"),
            OrderStatus::Confirmed => write!(f, "confirmed"),
            OrderStatus::Failed => write!(f, "failed"),
            OrderStatus::Expired => write!(f, "expired"),
        }
    }
}

/// A merchant payment order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentOrder {
    pub order_id: String,
    pub merchant_did: String,
    pub amount: u64,
    pub description: String,
    pub hub_endpoint: String,
    pub status: OrderStatus,
    pub created_at: DateTime<Utc>,
    pub confirmed_at: Option<DateTime<Utc>>,
    pub channel_id: Option<String>,
    pub leaf_index: Option<u32>,
    pub sequence: Option<u64>,
}

/// Persistent payment order store backed by sled.
pub struct PaymentOrderStore {
    db: sled::Db,
}

impl PaymentOrderStore {
    pub fn new(path: &str) -> Result<Self, anyhow::Error> {
        let db = sled::open(path)?;
        Ok(Self { db })
    }

    pub fn from_db(db: sled::Db) -> Self {
        Self { db }
    }

    pub fn get_db(&self) -> sled::Db {
        self.db.clone()
    }

    fn orders_tree(&self) -> Result<sled::Tree, anyhow::Error> {
        self.db.open_tree("orders")
            .map_err(|e| anyhow::anyhow!("Failed to open orders tree: {}", e))
    }

    pub fn save_order(&self, order: &PaymentOrder) -> Result<(), anyhow::Error> {
        let tree = self.orders_tree()?;
        let value = serde_json::to_vec(order)?;
        tree.insert(order.order_id.as_bytes(), value)?;
        tree.flush()?;
        Ok(())
    }

    pub fn get_order(&self, order_id: &str) -> Result<Option<PaymentOrder>, anyhow::Error> {
        let tree = self.orders_tree()?;
        if let Some(bytes) = tree.get(order_id)? {
            let order: PaymentOrder = serde_json::from_slice(&bytes)?;
            Ok(Some(order))
        } else {
            Ok(None)
        }
    }

    pub fn update_status(&self, order_id: &str, status: &OrderStatus) -> Result<(), anyhow::Error> {
        if let Some(mut order) = self.get_order(order_id)? {
            order.status = status.clone();
            if status == &OrderStatus::Confirmed {
                order.confirmed_at = Some(Utc::now());
            }
            self.save_order(&order)?;
        }
        Ok(())
    }

    pub fn confirm_order(
        &self,
        order_id: &str,
        channel_id: &str,
        leaf_index: u32,
        sequence: u64,
    ) -> Result<(), anyhow::Error> {
        if let Some(mut order) = self.get_order(order_id)? {
            order.status = OrderStatus::Confirmed;
            order.confirmed_at = Some(Utc::now());
            order.channel_id = Some(channel_id.to_string());
            order.leaf_index = Some(leaf_index);
            order.sequence = Some(sequence);
            self.save_order(&order)?;
        }
        Ok(())
    }

    pub fn list_orders(&self, limit: usize) -> Result<Vec<PaymentOrder>, anyhow::Error> {
        let tree = self.orders_tree()?;
        let mut orders = Vec::new();
        for item in tree.iter().rev() {
            if orders.len() >= limit {
                break;
            }
            let (_, value) = item?;
            let order: PaymentOrder = serde_json::from_slice(&value)?;
            orders.push(order);
        }
        Ok(orders)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_order_store_crud() {
        let dir = tempfile::tempdir().unwrap();
        let store = PaymentOrderStore::new(dir.path().to_str().unwrap()).unwrap();

        let order = PaymentOrder {
            order_id: "order-1".to_string(),
            merchant_did: "did:ignite:zTest".to_string(),
            amount: 100_000,
            description: "Coffee".to_string(),
            hub_endpoint: "http://localhost:3003".to_string(),
            status: OrderStatus::Pending,
            created_at: Utc::now(),
            confirmed_at: None,
            channel_id: None,
            leaf_index: None,
            sequence: None,
        };

        store.save_order(&order).unwrap();
        let loaded = store.get_order("order-1").unwrap().unwrap();
        assert_eq!(loaded.order_id, "order-1");
        assert_eq!(loaded.status, OrderStatus::Pending);

        store.confirm_order("order-1", "ab12cd34", 3, 15).unwrap();
        let loaded = store.get_order("order-1").unwrap().unwrap();
        assert_eq!(loaded.status, OrderStatus::Confirmed);
        assert_eq!(loaded.channel_id.as_deref(), Some("ab12cd34"));
        assert_eq!(loaded.leaf_index, Some(3));
        assert_eq!(loaded.sequence, Some(15));
        assert!(loaded.confirmed_at.is_some());
    }
}
