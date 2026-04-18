# 场景七：HTLC 结算与退款

## 1. 场景描述

在争议或结算阶段，对通道内的 HTLC 叶子进行链上认领或退款。受益人可通过揭示原像领取锁定资金；若 timelock 过期无人揭示，所有者可退回资金。

## 2. 参与角色

| 角色 | 职责 |
|:-----|:-----|
| Beneficiary | 持有原像，认领 HTLC 资金 |
| Owner | HTLC 超时后退款 |

## 3. 前置条件

- 通道处于 `Challenged` 或 `Settling` 状态
- HTLC 叶子存在于 Merkle 树中
- 认领：有正确原像且 `current_slot < timelock_slot`
- 退款：`current_slot > timelock_slot`

## 4. 操作流程

### Case A — 受益人认领

```
Beneficiary                              Solana
 │  1. 准备参数：leaf_index, preimage      │
 │     hash_lock, amount, beneficiary      │
 │     Merkle proof, claimer_signature     │
 │                                        │
 │  2. claim_htlc_verify                  │
 │───────────────────────────────────────→│
 │                                        │  验证 SHA-256(preimage) == hash_lock
 │                                        │  验证 current_slot < timelock_slot
 │                                        │  验证 Merkle proof
 │                                        │  资金转入 beneficiary
```

### Case B — 超时退款

```
Owner                                    Solana
 │  1. 确认 timelock 已过期                │
 │     current_slot > timelock_slot        │
 │                                        │
 │  2. claim_htlc_refund                  │
 │───────────────────────────────────────→│
 │                                        │  验证 timelock 过期
 │                                        │  验证 Merkle proof
 │                                        │  资金退回 owner
```

## 5. HTTP API 调用

### HTLC 认领（通过 claim 端点）

```bash
curl -X POST http://localhost:3001/v1/channels/{channel_id}/claim \
  -H "Content-Type: application/json" \
  -d '{
    "leaf_index": 2,
    "claim_amount": 100000,
    "proof": ["hash1_hex", "hash2_hex", ...]
  }'
```

### HTLC 退款

```bash
curl -X POST http://localhost:3001/v1/channels/{channel_id}/htlc/refund \
  -H "Content-Type: application/json" \
  -d '{
    "leaf_index": 2
  }'
```

## 6. Rust 库调用

```rust
// 受益人认领 HTLC
mgr.claim_htlc_verify(
    &mut state,
    leaf_index,
    &preimage,
    &claimer_pubkey,
    current_slot,
    &claimer_signature,
)?;

// 超时退款
mgr.claim_htlc_refund(
    &mut state,
    leaf_index,
    &claimer_pubkey,
    current_slot,
    &claimer_signature,
)?;
```

## 7. 链上操作

| 指令 | 函数 | 参数 |
|:-----|:-----|:-----|
| `verify_htlc` | `build_verify_htlc_ix` | leaf_index, preimage, hash_lock, amount, beneficiary, leaf_hash, timelock_slot, leaf_data, proof[], claimer_sig |
| `htlc_refund` | `build_htlc_refund_ix` | leaf_index, hash_lock, amount, owner, leaf_hash, timelock_slot, leaf_data, proof[], claimer_sig |

## 8. 错误处理

| 错误 | 原因 | 处理 |
|:-----|:-----|:-----|
| `InvalidPreimage` | 原像 hash 不匹配 hash_lock | 提供正确原像 |
| `HtlcNotExpired` | 退款时 timelock 未到期 | 等待更多 slots |
| `HtlcExpired` | 认领时 timelock 已过期 | 无法认领，改用退款 |
| `ProofVerificationFailed` | Merkle proof 无效 | 重新从当前树生成 proof |
| `NotBeneficiary` | 认领者不是受益人 | 使用 beneficiary 的签名 |

## 9. 注意事项

- HTLC 认领和退款都只能在 `Challenged` 或 `Settling` 状态下执行
- `verify_htlc` 需要 11 个参数，是最复杂的链上指令
- Merkle proof 必须基于当前 `current_root` 生成
- claimer 签名消息格式：`channel_id || leaf_index || amount || current_slot`
- 退款时链上会验证 `timelock_slot < current_slot`
