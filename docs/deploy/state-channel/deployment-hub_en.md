# State Channel Hub Deployment Configuration

## 1. Overview

Hub is the intermediate routing node in the state channel network, providing cross-channel payment routing and liquidity for users and merchants. Hub inherits all Provider role capabilities (accepting payments, co-signing, settlement) and additionally provides route discovery, multi-hop relay, and Hub network management.

Hub runs as a persistent server process via the `channel-hub` binary from `ignite-pay-channel-service`.

---

## 2. Core Components

| Component | Module | Description |
|:----------|:-------|:------------|
| HTTP Service | `ignite-pay-channel-service` | REST + WebSocket service for the Hub role |
| On-chain Instructions | `ignite-pay-solana::channel` | 10 on-chain Instruction builders |
| Hub Registration | `hub::HubManager` | Register/query Hub info and metrics |
| Route Discovery | `routing::RouteService` | DFS route search, scoring, selection |
| Multi-hop Payment | `multihop::MultiHopManager` | Multi-hop HTLC with decreasing timelocks |
| Channel Management | `channel::ChannelManager` | Bidirectional channels with each party |
| HTLC Management | `htlc::HtlcManager` | Per-hop HTLC preimage management |

---

## 3. Service Deployment

### 3.1 Build

```bash
cd ignite-pay-channel-service
cargo build --release --bin channel-hub
```

Output: `target/release/channel-hub`

### 3.2 Generate Keypair

```bash
solana-keygen new --outfile ./keys/hub.key
```

> If `keypair_path` is left empty, the service will automatically generate a temporary keypair on startup (for testing only).

### 3.3 Configuration File

Create `config-hub.toml`:

```toml
[server]
host = "0.0.0.0"        # Listen address; for production, use "127.0.0.1" + reverse proxy
port = 3003              # Listen port

[solana]
rpc_url = "https://api.devnet.solana.com"
channel_program_id = "DJBHr35jL3JAGoU7bKMsEFmpeNMrCSK7oYQE4HJ3GBUe"
keypair_path = "./keys/hub.key"

[channel]
default_tree_depth = 4
default_challenge_duration = 5000
default_min_challenge_delay = 1000
default_settle_window = 10000
auto_close_offset = 500000
db_path = "./data/channel_hub"

[compliance]
spending_threshold = 1000000000
per_channel_limit = 100000000
window_slots = 100000
travel_rule_threshold = 500000000
```

### 3.4 Start the Service

```bash
# Use the default config file config-hub.toml
./channel-hub

# Specify a config file
./channel-hub /path/to/config-hub.toml

# Enable debug logging
RUST_LOG=debug ./channel-hub
```

### 3.5 API Endpoints

Hub inherits all Provider endpoints and additionally registers Hub-specific routes.

#### General Endpoints

| Method | Path | Description |
|:-------|:-----|:------------|
| GET | `/health` | Health check |
| WS | `/ws` | WebSocket connection |

#### Inherited Provider Endpoints

| Method | Path | Description |
|:-------|:-----|:------------|
| POST | `/v1/channels/{id}/fund` | Fund a channel |
| GET | `/v1/channels` | List channels |
| GET | `/v1/channels/{id}` | Query channel status |
| POST | `/v1/channels/{id}/cosign` | Provider co-signing |
| POST | `/v1/channels/{id}/accept-payment` | Accept payment |
| POST | `/v1/channels/{id}/accept-batch` | Accept batch payment |
| POST | `/v1/channels/{id}/close` | Cooperative close |
| POST | `/v1/channels/{id}/challenge` | Initiate dispute |
| POST | `/v1/channels/{id}/submit-counter` | Submit counter-state |
| POST | `/v1/channels/{id}/claim` | Claim leaf |
| POST | `/v1/channels/{id}/finalize` | Final settlement |

#### Hub-Specific Endpoints

| Method | Path | Description | Off-chain API |
|:-------|:-----|:------------|:--------------|
| POST | `/v1/hub/register` | Hub registration | `HubManager::register_hub` |
| GET | `/v1/hub/info` | Hub info query | `HubManager::get_hub` |
| POST | `/v1/hub/metrics` | Update metrics | `HubManager::update_metrics` |
| GET | `/v1/hub/list` | List all Hubs | `HubManager::list_hubs` |
| POST | `/v1/routes/find` | Route discovery | `RouteService::discover_routes` |
| POST | `/v1/routes/add-edge` | Add route edge | `RouteService::add_channel_edge` |
| POST | `/v1/routes/refresh` | Refresh route graph | `RouteService::refresh_graph` |
| POST | `/v1/multihop/relay` | Relay multi-hop | `MultiHopManager::resolve_hop` |
| GET | `/v1/multihop/{id}` | Query multi-hop payment | `MultiHopManager::load_payment` |

### 3.6 Example Requests

```bash
# Health check
curl http://localhost:3003/health

# Register Hub
curl -X POST http://localhost:3003/v1/hub/register \
  -H "Content-Type: application/json" \
  -d '{
    "hub_did_hash": "hex...",
    "active_pubkey": "Hub's Solana public key",
    "endpoint_hash": "hex...",
    "collateral": 10000000,
    "platform_vc_hash": "hex..."
  }'

# Update metrics
curl -X POST http://localhost:3003/v1/hub/metrics \
  -H "Content-Type: application/json" \
  -d '{
    "hub_did_hash": "hex...",
    "online_rate": 9900,
    "success_rate": 9950,
    "avg_latency_ms": 50,
    "total_routed": 1000000000,
    "total_transactions": 5000,
    "active_channels": 20,
    "available_liquidity": 50000000,
    "fee_rate_bps": 10
  }'

# Route discovery
curl -X POST http://localhost:3003/v1/routes/find \
  -H "Content-Type: application/json" \
  -d '{
    "from_did_hash": "hex...",
    "to_did_hash": "hex...",
    "amount": 1000000,
    "token_mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
    "max_hops": 3
  }'
```

### 3.7 systemd Service

```ini
[Unit]
Description=Ignite Pay Channel Hub Service
After=network.target

[Service]
Type=simple
User=ignite
WorkingDirectory=/opt/ignite-pay
ExecStart=/opt/ignite-pay/channel-hub /opt/ignite-pay/config-hub.toml
Environment=RUST_LOG=info
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

### 3.8 Nginx Reverse Proxy

```nginx
server {
    listen 443 ssl;
    server_name hub.ignite-pay.example.com;

    ssl_certificate     /etc/ssl/certs/ignite-pay.pem;
    ssl_certificate_key /etc/ssl/private/ignite-pay.key;

    location / {
        proxy_pass http://127.0.0.1:3003;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
    }

    location /ws {
        proxy_pass http://127.0.0.1:3003;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
    }
}
```

---

## 4. Hub Registration

### 4.1 HubLeaf Data Structure

Hub registration information on the network:

```rust
use ignite_pay_state_channel::hub::{HubLeaf, HubMetrics, HubManager};

let hub_leaf = HubLeaf {
    hub_did_hash: [/* SHA-256(Hub DID) */],
    active_pubkey: hub_pubkey,
    endpoint_hash: [/* SHA-256(endpoint URL) */],
    collateral: 10_000_000,
    platform_vc_hash: [/* SHA-256(VC JSON) */],
    metrics_hash: [/* ... */],
    slot_updated: current_slot,
};
```

### 4.2 Register a Hub

```rust
let db = sled::open("./hub_data")?;
let hub_manager = HubManager::new(db.clone())?;

hub_manager.register_hub(hub_leaf)?;

let metrics = HubMetrics {
    online_rate: 9900,
    success_rate: 9950,
    avg_latency_ms: 50,
    total_routed: 1_000_000_000,
    total_transactions: 5000,
    active_channels: 20,
    available_liquidity: 50_000_000,
    fee_rate_bps: 10,
};
hub_manager.update_metrics(hub_did_hash, metrics)?;

let metrics_hash = HubManager::compute_metrics_hash(&metrics);
```

---

## 5. Route Discovery

### 5.1 Initialize Route Service

```rust
use ignite_pay_state_channel::routing::RouteService;

let route_service = RouteService::new(hub_manager);

// Option A: Explicit topology
route_service.add_channel_edge(hub1_did_hash, hub2_did_hash);

// Option B: Auto-discovery (based on full mesh of Hubs with liquidity)
route_service.refresh_graph()?;
```

### 5.2 Discover Routes

```rust
use ignite_pay_state_channel::routing::RouteRequest;

let req = RouteRequest {
    from_did_hash: user_did_hash,
    to_did_hash: merchant_did_hash,
    amount: 1_000_000,
    token_mint: usdc_mint_pubkey,
    max_hops: 3,
};

let routes = route_service.discover_routes(&req)?;

for route in &routes {
    println!("Route: {} hops, total fee {}, latency {}ms, score {:.3}",
        route.hops.len(), route.total_fee, route.max_latency_ms, route.score);
}
```

### 5.3 Route Scoring Formula

```
score = 0.3 * fee_score + 0.3 * latency_score + 0.4 * reliability_score
```

- `fee_score = 1 / (1 + total_fee / amount)`
- `latency_score = 1 / (1 + max_latency_ms / 1000)`
- `reliability_score = min(success_rate across hops)`

---

## 6. Multi-hop Payments

### 6.1 Create a Multi-hop Payment

```rust
use ignite_pay_state_channel::multihop::MultiHopManager;

let multihop_mgr = MultiHopManager::new(db.clone())?;

let hops_metadata = vec![
    (user_pubkey, hub1_pubkey, 1_001_000, 0, channel_id_1),
    (hub1_pubkey, hub2_pubkey, 1_000_500, 1, channel_id_2),
    (hub2_pubkey, merchant_pubkey, 1_000_000, 2, channel_id_3),
];

let payment = multihop_mgr.create_payment(
    hash_lock, preimage, hops_metadata, current_slot, challenge_duration,
)?;
```

### 6.2 Timelock Calculation

```
min_timelock = challenge_duration + 3 * HOP_MARGIN
base_timelock = current_slot + min_timelock + (num_hops - 1) * HOP_MARGIN
hop[i].timelock = base_timelock - i * HOP_MARGIN
```

- `HOP_MARGIN = 1000 slots` (approximately 6.7 minutes)
- `HTLC_SAFETY_MARGIN = 1000 slots`

### 6.3 Routing Fee Calculation

```rust
use ignite_pay_state_channel::multihop::compute_hop_amounts;

let fee_rates_bps = &[10, 5, 8];
let amounts = compute_hop_amounts(1_000_000, fee_rates_bps)?;
```

### 6.4 Preimage Reveal and Resolution

```rust
let payment = multihop_mgr.reveal_preimage(&payment_id, &preimage)?;

for i in (0..payment.hops.len()).rev() {
    let payment = multihop_mgr.resolve_hop(&payment_id, i)?;
}
```

### 6.5 Multi-hop Payment States

```
Pending → Locked → Resolving → Completed
Pending → Failed (any hop expired)
```

---

## 7. Hub Operations Configuration

### 7.1 Liquidity Management

| Parameter | Recommendation | Description |
|:----------|:---------------|:------------|
| Minimum liquidity | > 10x average routing amount | Ensures routability |
| Collateral amount | Adjust based on business volume | Affects routing trust |
| Number of channels | Establish with major users/merchants | Reduces hop count |
| Fee rate | Set after competitive analysis | Affects route selection |

### 7.2 Metrics Update Frequency

It is recommended to update metrics once per epoch (approximately 432,000 slots, roughly 2.4 days), or immediately after a significant event.

### 7.3 Topology Maintenance

```rust
route_service.add_channel_edge(my_hub_did, new_partner_did);
route_service.refresh_graph()?;
```

---

## 8. Data Persistence

### 8.1 sled Storage

| Key Prefix | Content |
|:-----------|:--------|
| `hub:{hex(did_hash)}` | HubLeaf registration data |
| `hub_metrics:{hex(did_hash)}` | HubMetrics metrics |
| `multihop:{hex(payment_id)}` | Multi-hop payment records |
| `htlc:{hex(channel_id)}` | HTLC records |
| `compliance:{hex(channel_id)}` | Compliance status |

### 8.2 Storage Size Estimation

| Component | Size per Record | Est. for 1,000 Channels |
|:----------|:----------------|:-------------------------|
| ChannelMetadata | ~500 bytes | ~500 KB |
| HubLeaf | ~200 bytes | ~200 KB |
| HubMetrics | ~64 bytes | ~64 KB |
| MultiHopPayment | ~200 bytes/hop | ~1 MB |
| HTLC Record | ~200 bytes | ~200 KB |

---

## 9. Configuration Parameter Reference

| Parameter | Type | Description |
|:----------|:-----|:------------|
| `server.host` | string | HTTP listen address |
| `server.port` | u16 | HTTP listen port (default 3003) |
| `solana.rpc_url` | string | Solana JSON RPC endpoint |
| `solana.channel_program_id` | string | On-chain channel program ID |
| `solana.keypair_path` | string | Ed25519 keypair file path |
| `channel.db_path` | string | sled database path |
| `channel.default_tree_depth` | u32 | Default Merkle tree depth |
| `compliance` | section | Optional compliance configuration |

---

## 10. Monitoring Recommendations

| Metric | Threshold | Action |
|:-------|:----------|:-------|
| Available liquidity | < 2x average routing volume | Replenish liquidity |
| Channel success rate | < 95% | Check channel status |
| Average latency | > 200ms | Optimize network/node |
| Expired multi-hop payments | > 5% | Adjust timelock |
| sled database size | > 2 GB | Archive historical data |
| Active channel count | Declining trend | Check service quality |

---

## 11. Security Checklist

| Check Item | Description | Status |
|:-----------|:------------|:-------|
| Hub key security | Use HSM or key management service | Required |
| Sufficient collateral | Enough to cover routing risk | Required |
| Liquidity monitoring | Regularly check and replenish | Required |
| Reasonable fee rates | Avoid malicious pricing | Recommended |
| Topology updates | Update route graph promptly after channel changes | Required |
| Multi-hop timeouts | Ensure timelocks decrease correctly | Required |
| Preimage security | Do not reveal preimage before confirmation | Required |
| Metrics authenticity | Reported metrics should reflect real status | Required |
| Reverse proxy TLS | Enable HTTPS via Nginx in production | Required |
