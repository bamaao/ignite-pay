use ignite_pay_channel_service::config::Role;
use ignite_pay_channel_service::state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();
    let config_path = args.get(1).map(|s| s.as_str()).unwrap_or("config-hub.toml");

    tracing::info!("Loading config from {}", config_path);
    let config = ignite_pay_channel_service::config::Config::load(config_path)?;
    let bind_addr = config.bind_addr();

    // Extract hub registry config before moving config into AppState
    let registry_cfg = config.hub_registry.clone();
    let pubkey_bs58 = {
        let kp_bytes = if config.solana.keypair_path.is_empty() {
            let kp = solana_sdk::signer::keypair::Keypair::new();
            let mut bytes = [0u8; 64];
            bytes.copy_from_slice(&kp.to_bytes());
            bytes
        } else {
            let raw = std::fs::read(&config.solana.keypair_path)?;
            let kp = solana_sdk::signer::keypair::Keypair::try_from(raw.as_slice())?;
            let mut bytes = [0u8; 64];
            bytes.copy_from_slice(&kp.to_bytes());
            bytes
        };
        let ed_kp = ed25519_dalek::Keypair::from_bytes(&kp_bytes)
            .map_err(|e| anyhow::anyhow!("invalid keypair: {}", e))?;
        bs58::encode(ed_kp.public.to_bytes()).into_string()
    };

    let state = AppState::new(config, Role::Hub)?;

    // Spawn hub registry publishing task if configured
    if let Some(ref reg_cfg) = registry_cfg {
        spawn_hub_publish_task(reg_cfg, &bind_addr, &pubkey_bs58);
    }

    let app = ignite_pay_channel_service::server::router::build_router(state);

    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    tracing::info!("Channel Hub service listening on {}", bind_addr);

    axum::serve(listener, app).await?;
    Ok(())
}

fn spawn_hub_publish_task(
    reg_cfg: &ignite_pay_channel_service::config::HubRegistryConfig,
    bind_addr: &str,
    pubkey_bs58: &str,
) {
    let registry_url = reg_cfg.url.clone();
    let interval_secs = reg_cfg.publish_interval_secs;
    let hub_endpoint = format!("http://{}", bind_addr);
    let hub_did = format!("did:ignite:{}", pubkey_bs58);
    let hub_name = format!("Hub-{}", &pubkey_bs58[..8.min(pubkey_bs58.len())]);
    let pubkey_owned = pubkey_bs58.to_string();

    tokio::spawn(async move {
        let client = reqwest::Client::new();
        let mut hub_id: Option<String> = None;

        // Step 1: Register with the hub registry
        tracing::info!("Registering hub with registry at {}", registry_url);
        let register_body = serde_json::json!({
            "hub_did": hub_did,
            "endpoint_url": hub_endpoint,
            "name": hub_name,
            "active_pubkey": pubkey_owned,
            "supported_tokens": ["So11111111111111111111111111111111"],
        });

        match client
            .post(format!("{}/v1/hubs", registry_url))
            .json(&register_body)
            .send()
            .await
        {
            Ok(resp) => {
                if resp.status().is_success() {
                    if let Ok(body) = resp.json::<serde_json::Value>().await {
                        hub_id = body.get("hub_id").and_then(|v| v.as_str()).map(String::from);
                        tracing::info!("Hub registered with registry, hub_id: {:?}", hub_id);
                    }
                } else {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    tracing::warn!("Hub registration returned {}: {}", status, body);
                }
            }
            Err(e) => {
                tracing::warn!("Failed to register hub with registry: {}", e);
            }
        }

        // Step 2: Periodic metrics update
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
        loop {
            ticker.tick().await;

            if let Some(ref id) = hub_id {
                let metrics_body = serde_json::json!({
                    "online_rate": 100,
                    "success_rate": 99,
                    "avg_latency_ms": 50,
                    "active_channels": 0,
                });

                match client
                    .put(format!("{}/v1/hubs/{}/metrics", registry_url, id))
                    .json(&metrics_body)
                    .send()
                    .await
                {
                    Ok(resp) => {
                        if !resp.status().is_success() {
                            let status = resp.status();
                            let body = resp.text().await.unwrap_or_default();
                            tracing::warn!("Metrics update returned {}: {}", status, body);
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to update hub metrics: {}", e);
                    }
                }
            }
        }
    });
}
