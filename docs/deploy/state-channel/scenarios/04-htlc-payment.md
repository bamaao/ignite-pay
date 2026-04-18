# 场景四：HTLC 条件支付

## 1. 场景描述

使用 Hash Time-Locked Contract (HTLC) 实现条件支付。资金被 hash_lock 锁定在 UTXO 叶子中，只有提供正确原像 (preimage) 的受益人才能领取。如果在 timelock 之前未揭示原像，资金退还给原所有者。

适用于：原子交换、条件交付、跨通道多跳支付的基础构建块。

## 2. 参与角色

| 角色 | 职责 |
|:-----|:-----|
| User | 创建 HTLC，持有原像，条件满足后揭示 |
| Provider | 验证 hash_lock，服务交付后接收原像领取资金 |

## 3. 前置条件

- 通道已开通并完成拆分树
- 有标准类型叶子可用于转换为 HTLC 叶子
- 双方约定 HTLC 金额、timelock 时长

## 4. 操作流程

### Flow A — 正常完成

```
User                                   Provider
 │  1. 创建 HTLC                         │
 │  HtlcManager::create_htlc(...)        │
 │  → (hash_lock, preimage)              │
 │                                        │
 │  2. 发送 hash_lock 给 Provider        │
 │  (preimage 保密)                       │
 │───────────────────────────────────────→│
 │                                        │
 │  3. Pipeline::create_htlc(leaf_idx,   │
 │     hash_lock, timelock, beneficiary)  │
 │  → 生成签名的 LeafUpdate              │
 │───────────────────────────────────────→│
 │                                        │  4. Provider 配签
 │←───────────────────────────────────────│
 │                                        │
 │  ===== 服务交付 =====                  │
 │                                        │
 │  5. 揭示原像                           │
 │  HtlcManager::reveal_preimage(...)     │
 │  Pipeline::resolve_htlc(leaf_idx,      │
 │    &preimage)                          │
 │───────────────────────────────────────→│
 │                                        │  6. 资金转入 Provider
 │  HtlcManager::mark_fulfilled(...)      │
```

### Flow B — 超时退款

```
User                                   Provider
 │  ...HTLC 已创建，timelock 到期...      │
 │                                        │
 │  HtlcManager::check_expiry(slot)       │
 │  → 标记为 Expired                      │
 │                                        │
 │  Pipeline::refund_htlc(leaf_idx)       │
 │  → 资金退回 User                       │
 │                                        │
 │  HtlcManager::mark_refunded(...)       │
```

## 5. HTTP API 调用

### 创建 HTLC

```bash
curl -X POST http://localhost:3001/v1/channels/{channel_id}/htlc/create \
  -H "Content-Type: application/json" \
  -d '{
    "amount": 100000,
    "leaf_index": 2,
    "beneficiary": "Provider公钥(Base58)",
    "duration": 500
  }'
```

### 解决 HTLC（揭示原像）

```bash
curl -X POST http://localhost:3001/v1/channels/{channel_id}/htlc/resolve \
  -H "Content-Type: application/json" \
  -d '{
    "leaf_index": 2,
    "preimage": "hex编码的32字节原像"
  }'
```

### HTLC 退款（超时后）

```bash
curl -X POST http://localhost:3001/v1/channels/{channel_id}/htlc/refund \
  -H "Content-Type: application/json" \
  -d '{
    "leaf_index": 2
  }'
```

## 6. Rust 库调用

```rust
use ignite_pay_state_channel::htlc::HtlcManager;

let htlc_mgr = HtlcManager::with_db(db.clone(), channel_id);

// 创建 HTLC
let (hash_lock, preimage) = htlc_mgr.create_htlc(
    100_000,           // 金额
    2,                  // 叶子索引
    user_pubkey,        // 所有者
    provider_pubkey,    // 受益人
    current_slot,       // 当前 slot
    500,                // 持续时间 (slots)
);

// 在 Pipeline 中创建 HTLC 叶子
{
    let mut pipeline = Pipeline::new(&mut tree, channel_id, seq, &user_keypair);
    pipeline.create_htlc(2, hash_lock, timelock_slot, provider_pubkey, current_slot, challenge_duration)?;
    let (updates, _) = pipeline.build();
}

// 揭示原像
htlc_mgr.reveal_preimage(&hash_lock, &preimage)?;

// 在 Pipeline 中解决 HTLC
{
    let mut pipeline = Pipeline::new(&mut tree, channel_id, seq, &user_keypair);
    pipeline.resolve_htlc(2, &preimage)?;
    let (updates, _) = pipeline.build();
}

htlc_mgr.mark_fulfilled(&hash_lock)?;
```

## 7. 链上操作

| 指令 | 函数 | 触发条件 |
|:-----|:-----|:---------|
| `verify_htlc` | `build_verify_htlc_ix` | 结算阶段，受益人提供原像认领 |
| `htlc_refund` | `build_htlc_refund_ix` | 结算阶段，timelock 过期后退款 |

## 8. 错误处理

| 错误 | 原因 | 处理 |
|:-----|:-----|:-----|
| `InvalidPreimage` | 原像 hash 不匹配 hash_lock | 确认正确的 preimage |
| `HtlcNotExpired` | 退款时 timelock 未到期 | 等待更多 slots |
| `HtlcAlreadyResolved` | 重复解决已完成的 HTLC | 检查 HTLC 状态 |
| `TimelockConstraint` | timelock 不满足设计约束 | `timelock > current_slot + challenge_duration + HTLC_SAFETY_MARGIN` |

## 9. 注意事项

- HTLC 生命周期：`Pending → Revealed → Fulfilled` 或 `Pending → Expired → Refunded`
- 原像 (preimage) 在服务确认前必须保密
- HTLC 叶子会占用一个叶子槽位，解决后释放
- timelock 约束：`timelock_slot > current_slot + challenge_duration + HTLC_SAFETY_MARGIN`（1000 slots）
- 通道关闭前必须解决所有活跃 HTLC（`close_channel` 会检查）
