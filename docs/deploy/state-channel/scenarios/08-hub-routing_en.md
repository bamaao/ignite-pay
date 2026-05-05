# Scenario 8: Hub Routing Network

## 1. Scenario Description

Hub nodes form a routing network that provides path discovery and liquidity information for cross-channel payments. Hubs register their own information, report performance metrics, maintain a routing topology graph, and respond to routing query requests.

## 2. Participants

| Role | Responsibility |
|:-----|:-----|
| Hub | Register information, update metrics, maintain routing graph, respond to routing queries |

## 3. Prerequisites

- Hub has deployed the `channel-hub` service
- Hub has generated a DID and key pair
- Hub has established channels with multiple users and merchants

## 4. Operation Flow

### Hub Registration and Metrics Update

```
Hub                                     HubManager
 │  1. Build HubLeaf                       │
 │  {hub_did_hash, active_pubkey,          │
 │   endpoint_hash, collateral,            │
 │   platform_vc_hash, metrics_hash,       │
 │   slot_updated}                         │
 │                                        │
 │  2. POST /v1/hub/register              │
 │───────────────────────────────────────→│
 │  register_hub(hub_leaf)                │
 │                                        │
 │  3. Periodically update metrics         │
 │  POST /v1/hub/metrics                  │
 │───────────────────────────────────────→│
 │  update_metrics(did_hash, metrics)     │
 │  compute_metrics_hash(&metrics)        │
```

### Route Discovery

```
Requester                               RouteService
 │  1. Maintain topology                   │
 │  POST /v1/routes/add-edge              │
 │  POST /v1/routes/refresh               │
 │                                        │
 │  2. POST /v1/routes/find               │
 │  {from, to, amount, token, max_hops}   │
 │───────────────────────────────────────→│
 │  discover_routes(req)                  │
 │  → DFS search all paths                │
 │  → Calculate score for each path       │
 │  → Return sorted by score descending   │
 │←───────────────────────────────────────│
 │  routes[] sorted by score              │
```

## 5. HTTP API Calls

### Register Hub

```bash
curl -X POST http://localhost:3003/v1/hub/register \
  -H "Content-Type: application/json" \
  -d '{
    "hub_did_hash": "hex...",
    "active_pubkey": "Hub Solana public key (Base58)",
    "endpoint_hash": "hex...",
    "collateral": 10000000,
    "platform_vc_hash": "hex..."
  }'
```

### Update Metrics

```bash
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
```

### Add Route Edge

```bash
curl -X POST http://localhost:3003/v1/routes/add-edge \
  -H "Content-Type: application/json" \
  -d '{
    "from_did_hash": "hex...",
    "to_did_hash": "hex..."
  }'
```

### Discover Route

```bash
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

### List All Hubs

```bash
curl http://localhost:3003/v1/hub/list
```

## 6. Rust Library Calls

```rust
use ignite_pay_state_channel::hub::{HubLeaf, HubMetrics, HubManager};
use ignite_pay_state_channel::routing::RouteService;

// Register Hub
hub_manager.register_hub(hub_leaf)?;

// Update metrics
hub_manager.update_metrics(did_hash, metrics)?;
let metrics_hash = HubManager::compute_metrics_hash(&metrics);

// Route service
let route_service = RouteService::new(hub_manager);
route_service.add_channel_edge(hub1_did, hub2_did)?;
route_service.refresh_graph()?;

let routes = route_service.discover_routes(&RouteRequest {
    from_did_hash, to_did_hash, amount, token_mint, max_hops: 3,
})?;

let best = RouteService::select_best_route(&routes);
```

### Route Scoring Formula

```
score = 0.3 × fee_score + 0.3 × latency_score + 0.4 × reliability_score

fee_score       = 1 / (1 + total_fee / amount)
latency_score   = 1 / (1 + max_latency_ms / 1000)
reliability_score = min(success_rate across hops)
```

## 7. On-Chain Operations

Hub registration and route discovery are entirely off-chain operations with no on-chain transactions involved.

## 8. Error Handling

| Error | Cause | Handling |
|:-----|:-----|:-----|
| `HubNotFound` | DID hash not registered | Call register_hub first |
| `NoRouteFound` | No reachable path | Add more route edges or increase max_hops |
| `InvalidMetrics` | Metric value out of bounds | online_rate/success_rate ≤ 10000 |

## 9. Notes

- `online_rate` and `success_rate` use basis points (10000 = 100%)
- `fee_rate_bps` uses basis points (10 = 0.1%)
- It is recommended to periodically call `refresh_graph` on the routing graph to reflect channel changes
- When no explicit edges exist, `refresh_graph` will connect all Hubs with liquidity (full mesh graph)
- The metrics hash `compute_metrics_hash` is used for on-chain verification of authenticity

---

## Related Scenarios

| Scenario | Relationship |
|:-----|:-----|
| [01 Channel Open](01-channel-open.md) | Channels must be opened between Hub and users/merchants |
| [09 Multi-hop Payment](09-multihop-payment.md) | Create multi-hop payments after route discovery |
