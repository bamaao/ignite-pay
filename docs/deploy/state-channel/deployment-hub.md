# 状态通道 Hub 端部署配置文档

## 1. 概述

Hub 是状态通道网络中的中间路由节点，为用户和商户提供跨通道的支付路由和流动性。Hub 通过 `ignite-pay-state-channel` 离链库管理多个通道、路由发现、多跳支付和流动性管理。

Hub 是一个持续运行的服务端进程，维护与多个用户和商户的支付通道。

---

## 2. 核心组件

| 组件 | 模块 | 说明 |
|:-----|:-----|:-----|
| Hub 注册 | `hub::HubManager` | 注册/查询 Hub 信息和指标 |
| 路由发现 | `routing::RouteService` | DFS 路由搜索、评分、选择 |
| 多跳支付 | `multihop::MultiHopManager` | 递减 timelock 的多跳 HTLC |
| 通道管理 | `channel::ChannelManager` | 与每方的双向通道 |
| HTLC 管理 | `htlc::HtlcManager` | 每跳的 HTLC 原像管理 |

---

## 3. Hub 注册

### 3.1 HubLeaf 数据结构

Hub 在网络中的注册信息：

```rust
use ignite_pay_state_channel::hub::{HubLeaf, HubMetrics, HubManager};

let hub_leaf = HubLeaf {
    hub_did_hash: [/* SHA-256(Hub DID) */],   // Hub DID 哈希
    active_pubkey: hub_pubkey,                  // Hub 活跃公钥
    endpoint_hash: [/* SHA-256(endpoint URL) */], // 端点哈希
    collateral: 10_000_000,                    // 质押金额
    platform_vc_hash: [/* SHA-256(VC JSON) */], // 平台 VC 哈希
    metrics_hash: [/* ... */],                  // 指标哈希
    slot_updated: current_slot,                 // 更新 slot
};
```

### 3.2 注册 Hub

```rust
let db = sled::open("./hub_data")?;
let hub_manager = HubManager::new(db.clone())?;

// 注册
hub_manager.register_hub(hub_leaf)?;

// 更新指标
let metrics = HubMetrics {
    online_rate: 9900,          // 在线率 (基点, 10000=100%)
    success_rate: 9950,         // 成功率 (基点)
    avg_latency_ms: 50,         // 平均延迟 (ms)
    total_routed: 1_000_000_000,// 总路由量
    total_transactions: 5000,   // 总交易数
    active_channels: 20,        // 活跃通道数
    available_liquidity: 50_000_000, // 可用流动性
    fee_rate_bps: 10,           // 费率 (基点, 10 = 0.1%)
};
hub_manager.update_metrics(hub_did_hash, metrics)?;

// 计算指标哈希（用于链上验证）
let metrics_hash = HubManager::compute_metrics_hash(&metrics);
```

---

## 4. 路由发现

### 4.1 初始化路由服务

```rust
use ignite_pay_state_channel::routing::RouteService;

let route_service = RouteService::new(hub_manager);

// 方式 A：显式拓扑
route_service.add_channel_edge(hub1_did_hash, hub2_did_hash);
route_service.add_channel_edge(hub2_did_hash, hub3_did_hash);

// 方式 B：自动发现（基于有流动性的 Hub 全连接）
route_service.refresh_graph()?;
```

### 4.2 发现路由

```rust
use ignite_pay_state_channel::routing::RouteRequest;

let req = RouteRequest {
    from_did_hash: user_did_hash,
    to_did_hash: merchant_did_hash,
    amount: 1_000_000,                 // 路由金额
    token_mint: usdc_mint_pubkey,
    max_hops: 3,                        // 最大跳数
};

let routes = route_service.discover_routes(&req)?;

// 路由按评分降序排列
for route in &routes {
    println!("路由: {} 跳, 总费 {}, 延迟 {}ms, 评分 {:.3}",
        route.hops.len(), route.total_fee, route.max_latency_ms, route.score);
}
```

### 4.3 路由评分公式

```
score = 0.3 * fee_score + 0.3 * latency_score + 0.4 * reliability_score
```

其中：
- `fee_score = 1 / (1 + total_fee / amount)` — 费用越低越好
- `latency_score = 1 / (1 + max_latency_ms / 1000)` — 延迟越低越好
- `reliability_score = min(success_rate across hops)` — 使用最低成功率

### 4.4 选择最佳路由

```rust
let best = RouteService::select_best_route(&routes);
if let Some(route) = best {
    println!("最佳路由: 评分 {:.3}", route.score);
}
```

---

## 5. 多跳支付

### 5.1 创建多跳支付

```rust
use ignite_pay_state_channel::multihop::MultiHopManager;

let multihop_mgr = MultiHopManager::new(db.clone())?;

// 准备跳元数据：(owner, beneficiary, amount, leaf_index, channel_id)
let hops_metadata = vec![
    (user_pubkey, hub1_pubkey, 1_001_000, 0, channel_id_1),
    (hub1_pubkey, hub2_pubkey, 1_000_500, 1, channel_id_2),
    (hub2_pubkey, merchant_pubkey, 1_000_000, 2, channel_id_3),
];

let payment = multihop_mgr.create_payment(
    hash_lock,
    preimage,
    hops_metadata,
    current_slot,
    challenge_duration,
)?;

// 每跳 timelock 递减：
// hop[0].timelock = base_timelock
// hop[1].timelock = base_timelock - HOP_MARGIN (1000 slots)
// hop[2].timelock = base_timelock - 2 * HOP_MARGIN
```

### 5.2 Timelock 计算

```
min_timelock = challenge_duration + 3 * HOP_MARGIN

base_timelock = current_slot + min_timelock + (num_hops - 1) * HOP_MARGIN

hop[i].timelock = base_timelock - i * HOP_MARGIN
```

常量值：
- `HOP_MARGIN = 1000 slots`（约 6.7 分钟）
- `HTLC_SAFETY_MARGIN = 1000 slots`

### 5.3 路由费计算

```rust
use ignite_pay_state_channel::multihop::compute_hop_amounts;

// 每跳费率 (基点)
let fee_rates_bps = &[10, 5, 8];  // hub1: 0.1%, hub2: 0.05%, hub3: 0.08%

let amounts = compute_hop_amounts(
    1_000_000,      // 目标金额（最终商户收到）
    fee_rates_bps,
)?;

// amounts[0] = 用户支付（含所有 Hub 费用）
// amounts[last] = 商户收到
// 每跳金额递减，差值为 Hub 费用
```

### 5.4 创建 HTLC 叶子

```rust
// 为每跳创建 HTLC LeafUpdate
for hop in &payment.hops {
    let prev_leaf = /* 获取当前叶子 */;
    let update = MultiHopManager::create_htlc_leaf_update(
        hop,
        sequence,
        &prev_leaf,
        &signer_keypair,
    );
    // 将 update 应用到对应通道
}
```

### 5.5 原像揭示与解决

```rust
// 终端商户揭示原像
let payment = multihop_mgr.reveal_preimage(&payment_id, &preimage)?;

// 从最后一跳向前依次解决
for i in (0..payment.hops.len()).rev() {
    let payment = multihop_mgr.resolve_hop(&payment_id, i)?;
}

// 所有跳解决后，状态自动变为 Completed
```

### 5.6 过期检查

```rust
let expired = multihop_mgr.check_expiry(&payment_id, current_slot)?;
if !expired.is_empty() {
    // 有跳过期，支付失败
    // 需要处理退款
}
```

### 5.7 多跳支付状态

```
Pending → (所有 HTLC 锁定) → Locked → (原像揭示) → Resolving → (所有跳解决) → Completed
Pending → (任一跳过期) → Failed
```

---

## 6. Hub 运维配置

### 6.1 流动性管理

| 参数 | 建议 | 说明 |
|:-----|:-----|:-----|
| 最低流动性 | > 10 倍平均路由金额 | 确保可路由 |
| 质押金额 | 根据业务量级调整 | 影响路由信任度 |
| 通道数量 | 与主要用户/商户建立 | 减少跳数 |
| 费率 | 竞争力分析后设置 | 影响路由选择 |

### 6.2 指标更新频率

```rust
// 建议每个 epoch (约 432000 slots = ~2.4 天) 更新一次指标
// 或在重大事件后立即更新

hub_manager.update_metrics(hub_did_hash, HubMetrics {
    online_rate: calculate_online_rate(),
    success_rate: calculate_success_rate(),
    avg_latency_ms: measure_avg_latency(),
    total_routed: get_total_routed(),
    total_transactions: get_total_transactions(),
    active_channels: count_active_channels(),
    available_liquidity: calculate_available_liquidity(),
    fee_rate_bps: current_fee_rate,
})?;

let metrics_hash = HubManager::compute_metrics_hash(&metrics);
// 更新 HubLeaf 的 metrics_hash 和 slot_updated
```

### 6.3 拓扑维护

```rust
// 当新通道建立或关闭时更新路由拓扑
route_service.add_channel_edge(my_hub_did, new_partner_did);

// 定期刷新
route_service.refresh_graph()?;
```

---

## 7. 数据持久化

### 7.1 sled 存储

| key 前缀 | 内容 |
|:---------|:-----|
| `hub:{hex(did_hash)}` | HubLeaf 注册数据 |
| `hub_metrics:{hex(did_hash)}` | HubMetrics 指标 |
| `multihop:{hex(payment_id)}` | 多跳支付记录 |
| `htlc:{hex(channel_id)}` | HTLC 记录 |
| `compliance:{hex(channel_id)}` | 合规状态 |

### 7.2 存储大小估算

| 组件 | 每条记录大小 | 1000 通道估算 |
|:-----|:-----------|:-------------|
| ChannelMetadata | ~500 bytes | ~500 KB |
| HubLeaf | ~200 bytes | ~200 KB |
| HubMetrics | ~64 bytes | ~64 KB |
| MultiHopPayment | ~200 bytes/跳 | ~1 MB |
| HTLC Record | ~200 bytes | ~200 KB |

---

## 8. 监控建议

| 指标 | 阈值 | 处理 |
|:-----|:-----|:-----|
| 可用流动性 | < 2x 平均路由量 | 补充流动性 |
| 通道成功率 | < 95% | 检查通道状态 |
| 平均延迟 | > 200ms | 优化网络/节点 |
| 过期多跳支付 | > 5% | 调整 timelock |
| sled 数据库大小 | > 2 GB | 归档历史数据 |
| 活跃通道数 | 趋势下降 | 检查服务质量 |

---

## 9. 安全检查清单

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
