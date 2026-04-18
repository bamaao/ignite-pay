# 场景九：多跳支付

## 1. 场景描述

用户通过多个 Hub 中继向远程商户发起支付。利用递减 timelock 的 HTLC 链实现原子性：要么所有跳都完成，要么全部回滚。每跳 Hub 收取一定费率。

## 2. 参与角色

| 角色 | 职责 |
|:-----|:-----|
| User | 发起多跳支付，锁定第一跳 HTLC |
| Hub 1..N | 中继支付，锁定/解锁 HTLC |
| Provider (Merchant) | 最终收款方，揭示原像触发反向解决 |

## 3. 前置条件

- 路由已发现（参见场景八）
- 每跳通道已开通且有足够流动性
- 共享的 hash_lock + preimage 已生成

## 4. 操作流程

```
User           Hub1            Hub2           Provider
 │               │               │               │
 │  1. compute_hop_amounts       │               │
 │  计算: User→Hub1: 1001000     │               │
 │        Hub1→Hub2: 1000500     │               │
 │        Hub2→Prov: 1000000     │               │
 │               │               │               │
 │  2. create_payment            │               │
 │  共享 hash_lock + preimage    │               │
 │  递减 timelock:               │               │
 │  hop0: base_timelock          │               │
 │  hop1: base - HOP_MARGIN     │               │
 │  hop2: base - 2×HOP_MARGIN   │               │
 │               │               │               │
 │  3. 为每跳创建 HTLC 叶子      │               │
 │  create_htlc_leaf_update      │               │
 │──────────────→│──────────────→│──────────────→│
 │               │               │    4. Provider │
 │               │               │    揭示 preimage│
 │               │               │←──────────────│
 │               │               │               │
 │               │  5. 反向逐跳解决               │
 │←──────────────│←──────────────│               │
 │  resolve_hop  │  resolve_hop  │  resolve_hop  │
 │               │               │               │
 │       全部解决 → Completed     │               │
```

### Timelock 计算

```
HOP_MARGIN = 1000 slots (~6.7 分钟)
HTLC_SAFETY_MARGIN = 1000 slots

min_timelock = challenge_duration + 3 × HOP_MARGIN
base_timelock = current_slot + min_timelock + (num_hops - 1) × HOP_MARGIN

hop[i].timelock = base_timelock - i × HOP_MARGIN
```

### 费率计算

```rust
// compute_hop_amounts(target_amount, &[fee_rate_bps...])
// 最后跳 = 目标金额
// 每跳向上加费: amount[i] = amount[i+1] * (1 + fee_rate[i]/10000)
```

## 5. HTTP API 调用

### 创建多跳支付

```bash
curl -X POST http://localhost:3001/v1/multihop/create \
  -H "Content-Type: application/json" \
  -d '{
    "hops": [
      {"owner": "User公钥", "beneficiary": "Hub1公钥", "amount": 1001000, "leaf_index": 0, "channel_id": "hex..."},
      {"owner": "Hub1公钥", "beneficiary": "Hub2公钥", "amount": 1000500, "leaf_index": 1, "channel_id": "hex..."},
      {"owner": "Hub2公钥", "beneficiary": "Prov公钥", "amount": 1000000, "leaf_index": 2, "channel_id": "hex..."}
    ],
    "current_slot": 123456789,
    "challenge_duration": 5000
  }'
```

### 解决单跳

```bash
curl -X POST http://localhost:3001/v1/multihop/{payment_id}/resolve \
  -H "Content-Type: application/json" \
  -d '{"hop_index": 2}'
```

### Hub 中继

```bash
curl -X POST http://localhost:3003/v1/multihop/relay \
  -H "Content-Type: application/json" \
  -d '{"payment_id": "hex...", "hop_index": 1}'
```

### 查询支付状态

```bash
curl http://localhost:3003/v1/multihop/{payment_id}
```

## 6. Rust 库调用

```rust
use ignite_pay_state_channel::multihop::MultiHopManager;

let multihop = MultiHopManager::new(db.clone())?;

// 创建多跳支付
let hops_metadata = vec![
    (user_pk, hub1_pk, 1_001_000, 0, channel_id_1),
    (hub1_pk, hub2_pk, 1_000_500, 1, channel_id_2),
    (hub2_pk, prov_pk, 1_000_000, 2, channel_id_3),
];

let payment = multihop.create_payment(
    hash_lock, preimage, hops_metadata, current_slot, challenge_duration,
)?;

// 为每跳创建 HTLC LeafUpdate
for hop in &payment.hops {
    let update = MultiHopManager::create_htlc_leaf_update(hop, sequence, &prev_leaf, &signer_kp);
}

// 揭示原像（最终收款方）
let payment = multihop.reveal_preimage(&payment_id, &preimage)?;

// 反向逐跳解决
for i in (0..payment.hops.len()).rev() {
    let payment = multihop.resolve_hop(&payment_id, i)?;
}
```

## 7. 链上操作

多跳支付的离链操作无需链上交易。链上 HTLC 验证仅在结算阶段触发（参见场景七）。

## 8. 错误处理

| 错误 | 原因 | 处理 |
|:-----|:-----|:-----|
| `InsufficientLiquidity` | 某跳通道余额不足 | 重新路由或补充流动性 |
| `HopExpired` | 某跳 timelock 已过期 | 支付自动 Failed，处理退款 |
| `InvalidHopOrder` | 解决顺序错误 | 必须从最后一跳向前依次解决 |
| `PreimageMismatch` | 原像不匹配 hash_lock | 确认 preimage 正确 |

## 9. 注意事项

- 多跳支付状态：`Pending → Locked → Resolving → Completed` 或 `Pending → Failed`
- 递减 timelock 确保上游总有足够时间在下游超时后退款
- `HOP_MARGIN = 1000 slots` 是安全余量，不可调小
- 支付失败时，各跳需独立处理 HTLC 退款
- 每跳的 `channel_id` 不同（跨通道路由）
