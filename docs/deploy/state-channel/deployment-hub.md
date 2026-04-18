# 状态通道 Hub 端部署配置文档

## 1. 概述

Hub 是状态通道网络中的中间路由节点，为用户和商户提供跨通道的支付路由和流动性。Hub 同时继承 Provider 角色的全部功能（接受支付、配签、结算），额外提供路由发现、多跳中继和 Hub 网络管理。

Hub 通过 `ignite-pay-channel-service` 的 `channel-hub` 二进制作为持续运行的服务端进程。

---

## 2. 核心组件

| 组件 | 模块 | 说明 |
|:-----|:-----|:-----|
| HTTP 服务 | `ignite-pay-channel-service` | Hub 角色的 REST + WebSocket 服务 |
| 链上指令 | `ignite-pay-solana::channel` | 10 个链上 Instruction 构建器 |
| Hub 注册 | `hub::HubManager` | 注册/查询 Hub 信息和指标 |
| 路由发现 | `routing::RouteService` | DFS 路由搜索、评分、选择 |
| 多跳支付 | `multihop::MultiHopManager` | 递减 timelock 的多跳 HTLC |
| 通道管理 | `channel::ChannelManager` | 与每方的双向通道 |
| HTLC 管理 | `htlc::HtlcManager` | 每跳的 HTLC 原像管理 |

---

## 3. 服务部署

### 3.1 编译

```bash
cd ignite-pay-channel-service
cargo build --release --bin channel-hub
```

产物：`target/release/channel-hub`

### 3.2 生成密钥

```bash
solana-keygen new --outfile ./keys/hub.key
```

> 如果 `keypair_path` 留空，服务启动时自动生成临时密钥（仅测试用）。

### 3.3 配置文件

创建 `config-hub.toml`：

```toml
[server]
host = "0.0.0.0"        # 监听地址，生产环境建议 "127.0.0.1" + 反向代理
port = 3003              # 监听端口

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

### 3.4 启动服务

```bash
# 使用默认配置文件 config-hub.toml
./channel-hub

# 指定配置文件
./channel-hub /path/to/config-hub.toml

# 启用 debug 日志
RUST_LOG=debug ./channel-hub
```

### 3.5 API 接口

Hub 继承 Provider 所有端点，额外注册 Hub 专属路由。

#### 通用端点

| 方法 | 路径 | 说明 |
|:-----|:-----|:-----|
| GET | `/health` | 健康检查 |
| WS | `/ws` | WebSocket 连接 |

#### 继承 Provider 端点

| 方法 | 路径 | 说明 |
|:-----|:-----|:-----|
| POST | `/v1/channels/{id}/fund` | 注资通道 |
| GET | `/v1/channels` | 列出通道 |
| GET | `/v1/channels/{id}` | 查询通道状态 |
| POST | `/v1/channels/{id}/cosign` | Provider 配签 |
| POST | `/v1/channels/{id}/accept-payment` | 接受支付 |
| POST | `/v1/channels/{id}/accept-batch` | 接受批量支付 |
| POST | `/v1/channels/{id}/close` | 协作关闭 |
| POST | `/v1/channels/{id}/challenge` | 发起争议 |
| POST | `/v1/channels/{id}/submit-counter` | 提交反状态 |
| POST | `/v1/channels/{id}/claim` | 认领叶子 |
| POST | `/v1/channels/{id}/finalize` | 最终结算 |

#### Hub 专属端点

| 方法 | 路径 | 说明 | 离链 API |
|:-----|:-----|:-----|:---------|
| POST | `/v1/hub/register` | Hub 注册 | `HubManager::register_hub` |
| GET | `/v1/hub/info` | Hub 信息查询 | `HubManager::get_hub` |
| POST | `/v1/hub/metrics` | 更新指标 | `HubManager::update_metrics` |
| GET | `/v1/hub/list` | 列出所有 Hub | `HubManager::list_hubs` |
| POST | `/v1/routes/find` | 路由发现 | `RouteService::discover_routes` |
| POST | `/v1/routes/add-edge` | 添加路由边 | `RouteService::add_channel_edge` |
| POST | `/v1/routes/refresh` | 刷新路由图 | `RouteService::refresh_graph` |
| POST | `/v1/multihop/relay` | 中继多跳 | `MultiHopManager::resolve_hop` |
| GET | `/v1/multihop/{id}` | 查询多跳支付 | `MultiHopManager::load_payment` |

### 3.6 示例请求

```bash
# 健康检查
curl http://localhost:3003/health

# 注册 Hub
curl -X POST http://localhost:3003/v1/hub/register \
  -H "Content-Type: application/json" \
  -d '{
    "hub_did_hash": "hex...",
    "active_pubkey": "Hub的Solana公钥",
    "endpoint_hash": "hex...",
    "collateral": 10000000,
    "platform_vc_hash": "hex..."
  }'

# 更新指标
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

# 路由发现
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

### 3.7 systemd 服务

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

### 3.8 Nginx 反向代理

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

## 4. Hub 注册

### 4.1 HubLeaf 数据结构

Hub 在网络中的注册信息：

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

### 4.2 注册 Hub

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

## 5. 路由发现

### 5.1 初始化路由服务

```rust
use ignite_pay_state_channel::routing::RouteService;

let route_service = RouteService::new(hub_manager);

// 方式 A：显式拓扑
route_service.add_channel_edge(hub1_did_hash, hub2_did_hash);

// 方式 B：自动发现（基于有流动性的 Hub 全连接）
route_service.refresh_graph()?;
```

### 5.2 发现路由

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
    println!("路由: {} 跳, 总费 {}, 延迟 {}ms, 评分 {:.3}",
        route.hops.len(), route.total_fee, route.max_latency_ms, route.score);
}
```

### 5.3 路由评分公式

```
score = 0.3 * fee_score + 0.3 * latency_score + 0.4 * reliability_score
```

- `fee_score = 1 / (1 + total_fee / amount)`
- `latency_score = 1 / (1 + max_latency_ms / 1000)`
- `reliability_score = min(success_rate across hops)`

---

## 6. 多跳支付

### 6.1 创建多跳支付

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

### 6.2 Timelock 计算

```
min_timelock = challenge_duration + 3 * HOP_MARGIN
base_timelock = current_slot + min_timelock + (num_hops - 1) * HOP_MARGIN
hop[i].timelock = base_timelock - i * HOP_MARGIN
```

- `HOP_MARGIN = 1000 slots`（约 6.7 分钟）
- `HTLC_SAFETY_MARGIN = 1000 slots`

### 6.3 路由费计算

```rust
use ignite_pay_state_channel::multihop::compute_hop_amounts;

let fee_rates_bps = &[10, 5, 8];
let amounts = compute_hop_amounts(1_000_000, fee_rates_bps)?;
```

### 6.4 原像揭示与解决

```rust
let payment = multihop_mgr.reveal_preimage(&payment_id, &preimage)?;

for i in (0..payment.hops.len()).rev() {
    let payment = multihop_mgr.resolve_hop(&payment_id, i)?;
}
```

### 6.5 多跳支付状态

```
Pending → Locked → Resolving → Completed
Pending → Failed (任一跳过期)
```

---

## 7. Hub 运维配置

### 7.1 流动性管理

| 参数 | 建议 | 说明 |
|:-----|:-----|:-----|
| 最低流动性 | > 10 倍平均路由金额 | 确保可路由 |
| 质押金额 | 根据业务量级调整 | 影响路由信任度 |
| 通道数量 | 与主要用户/商户建立 | 减少跳数 |
| 费率 | 竞争力分析后设置 | 影响路由选择 |

### 7.2 指标更新频率

建议每个 epoch（约 432000 slots ≈ 2.4 天）更新一次指标，或在重大事件后立即更新。

### 7.3 拓扑维护

```rust
route_service.add_channel_edge(my_hub_did, new_partner_did);
route_service.refresh_graph()?;
```

---

## 8. 数据持久化

### 8.1 sled 存储

| key 前缀 | 内容 |
|:---------|:-----|
| `hub:{hex(did_hash)}` | HubLeaf 注册数据 |
| `hub_metrics:{hex(did_hash)}` | HubMetrics 指标 |
| `multihop:{hex(payment_id)}` | 多跳支付记录 |
| `htlc:{hex(channel_id)}` | HTLC 记录 |
| `compliance:{hex(channel_id)}` | 合规状态 |

### 8.2 存储大小估算

| 组件 | 每条记录大小 | 1000 通道估算 |
|:-----|:-----------|:-------------|
| ChannelMetadata | ~500 bytes | ~500 KB |
| HubLeaf | ~200 bytes | ~200 KB |
| HubMetrics | ~64 bytes | ~64 KB |
| MultiHopPayment | ~200 bytes/跳 | ~1 MB |
| HTLC Record | ~200 bytes | ~200 KB |

---

## 9. 配置参数详解

| 参数 | 类型 | 说明 |
|:-----|:-----|:-----|
| `server.host` | string | HTTP 监听地址 |
| `server.port` | u16 | HTTP 监听端口（默认 3003） |
| `solana.rpc_url` | string | Solana JSON RPC 端点 |
| `solana.channel_program_id` | string | 链上通道程序 ID |
| `solana.keypair_path` | string | Ed25519 密钥对文件路径 |
| `channel.db_path` | string | sled 数据库路径 |
| `channel.default_tree_depth` | u32 | 默认 Merkle 树深度 |
| `compliance` | section | 可选合规配置 |

---

## 10. 监控建议

| 指标 | 阈值 | 处理 |
|:-----|:-----|:-----|
| 可用流动性 | < 2x 平均路由量 | 补充流动性 |
| 通道成功率 | < 95% | 检查通道状态 |
| 平均延迟 | > 200ms | 优化网络/节点 |
| 过期多跳支付 | > 5% | 调整 timelock |
| sled 数据库大小 | > 2 GB | 归档历史数据 |
| 活跃通道数 | 趋势下降 | 检查服务质量 |

---

## 11. 安全检查清单

| 检查项 | 说明 | 状态 |
|:-------|:-----|:-----|
| Hub 密钥安全 | 使用 HSM 或密钥管理服务 | 必须 |
| 质押充足 | 足以覆盖路由风险 | 必须 |
| 流动性监控 | 定期检查并补充 | 必须 |
| 费率合理 | 避免恶意定价 | 建议 |
| 拓扑更新 | 通道变更后及时更新路由图 | 必须 |
| 多跳超时 | 确保 timelock 递减正确 | 必须 |
| 原像安全 | 原像在确认前不提前揭示 | 必须 |
| 指标真实性 | 上报的指标反映真实状态 | 必须 |
| 反向代理 TLS | 生产环境通过 Nginx 启用 HTTPS | 必须 |
