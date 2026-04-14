use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Mutex;

/// Trait for IPFS client operations.
#[async_trait]
pub trait IpfsClient: Send + Sync {
    /// Upload data to IPFS and return the CID.
    async fn upload(&self, data: &[u8]) -> Result<String, anyhow::Error>;

    /// Download data from IPFS by CID.
    async fn download(&self, cid: &str) -> Result<Vec<u8>, anyhow::Error>;
}

#[async_trait]
impl IpfsClient for Box<dyn IpfsClient> {
    async fn upload(&self, data: &[u8]) -> Result<String, anyhow::Error> {
        (**self).upload(data).await
    }

    async fn download(&self, cid: &str) -> Result<Vec<u8>, anyhow::Error> {
        (**self).download(cid).await
    }
}

/// Mock IPFS client using in-memory storage. For development and testing.
pub struct MockIpfsClient {
    store: Mutex<HashMap<String, Vec<u8>>>,
    counter: Mutex<u64>,
}

impl MockIpfsClient {
    pub fn new() -> Self {
        Self {
            store: Mutex::new(HashMap::new()),
            counter: Mutex::new(0),
        }
    }
}

impl Default for MockIpfsClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl IpfsClient for MockIpfsClient {
    async fn upload(&self, data: &[u8]) -> Result<String, anyhow::Error> {
        let mut counter = self.counter.lock().unwrap();
        *counter += 1;
        let cid = format!("bafyreiMock{}", counter);

        let mut store = self.store.lock().unwrap();
        store.insert(cid.clone(), data.to_vec());
        Ok(cid)
    }

    async fn download(&self, cid: &str) -> Result<Vec<u8>, anyhow::Error> {
        let store = self.store.lock().unwrap();
        store
            .get(cid)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("CID not found: {}", cid))
    }
}

/// Kubo RPC IPFS client. Calls the local Kubo node's RPC API.
#[cfg(feature = "kubo")]
pub struct KuboIpfsClient {
    base_url: String,
}

#[cfg(feature = "kubo")]
impl KuboIpfsClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.to_string(),
        }
    }
}

#[cfg(feature = "kubo")]
#[async_trait]
impl IpfsClient for KuboIpfsClient {
    async fn upload(&self, data: &[u8]) -> Result<String, anyhow::Error> {
        let client = reqwest::Client::new();
        let form = reqwest::multipart::Form::new()
            .part(
                "file",
                reqwest::multipart::Part::bytes(data.to_vec())
                    .file_name("data.json")
                    .mime_str("application/json")?,
            );

        let resp = client
            .post(format!("{}/api/v0/add", self.base_url))
            .multipart(form)
            .send()
            .await?;

        let result: serde_json::Value = resp.json().await?;
        let cid = result["Hash"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("No Hash in IPFS response"))?;
        Ok(cid.to_string())
    }

    async fn download(&self, cid: &str) -> Result<Vec<u8>, anyhow::Error> {
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}/api/v0/cat?arg={}", self.base_url, cid))
            .send()
            .await?;

        Ok(resp.bytes().await?.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_ipfs_roundtrip() {
        let client = MockIpfsClient::new();
        let data = b"hello ipfs";
        let cid = client.upload(data).await.unwrap();
        let downloaded = client.download(&cid).await.unwrap();
        assert_eq!(downloaded, data);
    }

    #[tokio::test]
    async fn test_mock_ipfs_unique_cids() {
        let client = MockIpfsClient::new();
        let cid1 = client.upload(b"data1").await.unwrap();
        let cid2 = client.upload(b"data2").await.unwrap();
        assert_ne!(cid1, cid2);
    }
}
