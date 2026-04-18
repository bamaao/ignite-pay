# 场景八：Hub 路由网络

## 1. 场景描述

Hub 节点组成路由网络，为跨通道支付提供路径发现和流动性信息。Hub 注册自身信息、上报性能指标，维护路由拓扑图，响应路由查询请求。

## 2. 参与角色

| 角色 | 职责 |
|:-----|:-----|
| Hub | 注册信息，更新指标，维护路由图，响应路由查询 |

## 3. 前置条件

- Hub 已部署 `channel-hub` 服务
- Hub 已生成 DID 和密钥对
- Hub 已与多个用户和商户建立通道

## 4. 操作流程

### Hub 注册与指标更新

```
Hub                                     HubManager
 │  1. 构建 HubLeaf                       │
 │  {hub_did_hash, active_pubkey,          │
 │   endpoint_hash, collateral,            │
 │   platform_vc_hash, metrics_hash,       │
 │   slot_updated}                         │
 │                                        │
 │  2. POST /v1/hub/register              │
 │───────────────────────────────────────→│
 │  register_hub(hub_leaf)                │
 │                                        │
 │  3. 定期更新指标                        │
 │  POST /v1/hub/metrics                  │
 │───────────────────────────────────────→│
 │  update_metrics(did_hash, metrics)     │
 │  compute_metrics_hash(&metrics)        │
```

### 路由发现

```
Requester                               RouteService
 │  1. 维护拓扑                            │
 │  POST /v1/routes/add-edge              │
 │  POST /v1/routes/refresh               │
 │                                        │
 │  2. POST /v1/routes/find               │
 │  {from, to, amount, token, max_hops}   │
 │───────────────────────────────────────→│
 │  discover_routes(req)                  │
 │  → DFS 搜索所有路径                     │
 │  → 每条路径计算评分                      │
 │  → 按评分降序返回                       │
 │←───────────────────────────────────────│
 │  routes[] sorted by score              │
```

## 5. HTTP API 调用

### 注册 Hub

```bash
curl -X POST http://localhost:3003/v1/hub/register \
  -H "Content-Type: application/json" \
  -d '{
    "hub_did_hash": "hex...",
    "active_pubkey": "Hub的Solana公钥(Base58)",
    "endpoint_hash": "hex...",
    "collateral": 10000000,
    "platform_vc_hash": "hex..."
  }'
```

### 更新指标

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

### 添加路由边

```bash
curl -X POST http://localhost:3003/v1/routes/add-edge \
  -H "Content-Type: application/json" \
  -d '{
    "from_did_hash": "hex...",
    "to_did_hash": "hex..."
  }'
```

### 发现路由

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

### 列出所有 Hub

```bash
curl http://localhost:3003/v1/hub/list
```

## 6. Rust 库调用

```rust
use ignite_pay_state_channel::hub::{HubLeaf, HubMetrics, HubManager};
use ignite_pay_state_channel::routing::RouteService;

// 注册 Hub
hub_manager.register_hub(hub_leaf)?;

// 更新指标
hub_manager.update_metrics(did_hash, metrics)?;
let metrics_hash = HubManager::compute_metrics_hash(&metrics);

// 路由服务
let route_service = RouteService::new(hub_manager);
route_service.add_channel_edge(hub1_did, hub2_did)?;
route_service.refresh_graph()?;

let routes = route_service.discover_routes(&RouteRequest {
    from_did_hash, to_did_hash, amount, token_mint, max_hops: 3,
})?;

let best = RouteService::select_best_route(&routes);
```

### 路由评分公式

```
score = 0.3 × fee_score + 0.3 × latency_score + 0.4 × reliability_score

fee_score       = 1 / (1 + total_fee / amount)
latency_score   = 1 / (1 + max_latency_ms / 1000)
reliability_score = min(success_rate across hops)
```

## 7. 链上操作

Hub 注册和路由发现均为离链操作，不涉及链上交易。

## 8. 错误处理

| 错误 | 原因 | 处理 |
|:-----|:-----|:-----|
| `HubNotFound` | DID hash 未注册 | 先调用 register_hub |
| `NoRouteFound` | 无可达路径 | 添加更多路由边或增加 max_hops |
| `InvalidMetrics` | 指标值越界 | online_rate/success_rate ≤ 10000 |

## 9. 注意事项

- `online_rate` 和 `success_rate` 使用基点（10000 = 100%）
- `fee_rate_bps` 使用基点（10 = 0.1%）
- 路由图建议定期 `refresh_graph` 以反映通道变化
- 无显式边时，`refresh_graph` 会连接所有有流动性的 Hub（全连接图）
- 指标哈希 `compute_metrics_hash` 用于链上验证真实性
