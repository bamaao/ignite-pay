# 场景三：批量支付与原子性操作

## 1. 场景描述

用户使用 Pipeline 执行多步操作的原子批处理。Pipeline 内的所有操作要么全部成功，要么全部回滚。适用于需要同时执行多笔支付、拆分和 HTLC 创建的复杂场景。

## 2. 参与角色

| 角色 | 职责 |
|:-----|:-----|
| User | 构建 Pipeline，执行批量操作 |
| Provider | 接受并配签批量更新 |

## 3. 前置条件

- 通道已开通，状态为 `Open`
- 已完成拆分树，有足够的可用叶子槽位

## 4. 操作流程

```
User
 │
 │  1. 创建 Pipeline
 │  Pipeline::new(&mut tree, channel_id, sequence, &keypair)
 │
 │  2. 执行操作（可叠加多个）
 │  ├─ transfer_leaf(0, provider_pubkey)         // 整叶转账
 │  ├─ partial_transfer(1, 4, 50000, provider)   // 部分转账
 │  ├─ create_htlc(2, hash_lock, timelock, ...)   // 创建 HTLC
 │  └─ ...
 │
 │  3a. 成功 → pipeline.build()
 │  返回 Vec<SignedLeafUpdate>
 │
 │  3b. 失败 → pipeline.abort()
 │  树状态自动恢复到 Pipeline 创建前
 │
 │  4. 发送签名更新给 Provider
 │  POST /v1/channels/{id}/batch
 │→Provider
```

## 5. HTTP API 调用

### 批量支付

```bash
curl -X POST http://localhost:3001/v1/channels/{channel_id}/batch \
  -H "Content-Type: application/json" \
  -d '{
    "updates": [
      {"leaf_index": 0, "new_owner": "Provider公钥", "amount": 100000},
      {"leaf_index": 1, "new_owner": "Provider公钥", "amount": 50000}
    ]
  }'
```

### Provider 接受批量

```bash
curl -X POST http://localhost:3002/v1/channels/{channel_id}/accept-batch \
  -H "Content-Type: application/json" \
  -d '{
    "updates": [...]
  }'
```

## 6. Rust 库调用

```rust
use ignite_pay_state_channel::pipeline::Pipeline;

let mut tree = state.tree.clone();
{
    let mut pipeline = Pipeline::new(&mut tree, channel_id, sequence + 1, &user_keypair);

    // 整叶转账
    pipeline.transfer_leaf(0, provider_pubkey)?;

    // 部分转账：从叶子1拆出50000到空槽位4
    pipeline.partial_transfer(1, 4, 50_000, provider_pubkey)?;

    // 创建 HTLC
    pipeline.create_htlc(2, hash_lock, timelock_slot, provider_pubkey, current_slot, challenge_duration)?;

    // 提交：返回所有签名的 LeafUpdate
    let (updates, final_sequence) = pipeline.build();

    // 发送给 Provider...
}
// 如果中途出错，显式调用 pipeline.abort() 或让 Pipeline drop 自动回滚
```

### 批量应用

```rust
// Provider 批量接受
let result = mgr.apply_leaf_update_batch_with_info(
    &mut state,
    &updates,
    &user_pubkey,
);

match result {
    Ok(()) => { /* 全部成功 */ },
    Err(info) => {
        // info.failed_index — 第一条失败的索引
        // info.error — 失败原因
        // info.applied_count — 已成功应用的条数
    },
}
```

## 7. 链上操作

Pipeline 操作完全离链，无需链上交互。

## 8. 错误处理

| 错误 | 原因 | 处理 |
|:-----|:-----|:-----|
| `BatchFailureInfo` | 批量中间某条失败 | 检查 `failed_index` 和 `error` 字段 |
| `LeafNotEmpty` | 部分转账目标槽位已被占用 | 使用空叶子索引 |
| `InsufficientAmount` | 源叶子金额不足 | 检查叶子余额 |
| `InvalidLeafState` | 对空叶子执行整叶转账 | 确认叶子有余额 |

## 9. 注意事项

- Pipeline 绑定 `&mut tree`，同一时间只能有一个活跃 Pipeline
- `partial_transfer` 先创建目标叶子再扣减源叶子，保证每步金额守恒
- `build()` 消费 Pipeline，之后不可再调用任何方法
- 如果 Pipeline 被 drop 但未调用 `build()` 或 `abort()`，Drop trait 自动回滚
- 批量失败时，已应用的更新不会自动回滚（需协作处理或争议解决）
