# 场景五：协作关闭通道

## 1. 场景描述

用户和 Provider 双方同意当前通道状态，共同签名关闭通道。资金按双方持有的 UTXO 叶子分配，通过链上 `cooperative_settle` 指令执行。

## 2. 参与角色

| 角色 | 职责 |
|:-----|:-----|
| User | 发起关闭请求，提供己方签名 |
| Provider | 配签确认，领取属于己方的叶子 |

## 3. 前置条件

- 通道状态为 `Open`
- 通道内无活跃 HTLC（所有 HTLC 已解决或退款）
- 双方同意当前 Merkle 根

## 4. 操作流程

```
User                                   Provider                                Solana
 │  1. 请求配签                           │                                       │
 │  POST /v1/channels/{id}/cosign         │                                       │
 │───────────────────────────────────────→│                                       │
 │                                        │  2. Provider 配签                     │
 │←───────────────────────────────────────│  provider_cosign_state                │
 │  cosignature                           │                                       │
 │                                        │                                       │
 │  3. 构建双签状态                        │                                       │
 │  SignedState{sig_a, sig_b}             │                                       │
 │                                        │                                       │
 │  4. POST /v1/channels/{id}/close       │                                       │
 │  close_channel(signed_state, ...)      │                                       │
 │  build_cooperative_settle_ix           │                                       │
 │────────────────────────────────────────────────────────────────────────────────→│
 │                                        │                        通道进入 Settling │
 │                                        │                                       │
 │  5. 结算窗口内认领叶子                   │                                       │
 │  POST /v1/channels/{id}/claim          │                                       │
 │  claim_leaf_with_proof                 │                                       │
 │────────────────────────────────────────────────────────────────────────────────→│
 │                                        │  6. Provider 同样认领                  │
 │                                        │──────────────────────────────────────→│
 │                                        │                                       │
 │  7. POST /v1/channels/{id}/finalize    │                                       │
 │  finalize_settlement                   │                                       │
 │────────────────────────────────────────────────────────────────────────────────→│
 │                                        │                        通道 Closed      │
```

## 5. HTTP API 调用

### 协作关闭

```bash
curl -X POST http://localhost:3001/v1/channels/{channel_id}/close \
  -H "Content-Type: application/json" \
  -d '{"settle_window": 10000}'
```

响应：
```json
{
  "channel_id": "hex...",
  "status": "settling",
  "settle_window": 10000,
  "on_chain_instruction": {
    "program_id": "...",
    "data": "bs58编码的指令数据"
  }
}
```

### 认领叶子

```bash
curl -X POST http://localhost:3001/v1/channels/{channel_id}/claim \
  -H "Content-Type: application/json" \
  -d '{
    "leaf_index": 0,
    "claim_amount": 500000,
    "proof": ["hash1_hex", "hash2_hex", "hash3_hex", "hash4_hex"]
  }'
```

### 最终结算

```bash
curl -X POST http://localhost:3001/v1/channels/{channel_id}/finalize
```

## 6. Rust 库调用

```rust
// 构建双签状态
let sig_a = sign_state(&channel_id, sequence, &root, &user_keypair);
let cosignature = mgr.provider_cosign_state(&mut state, &provider_keypair)?;

let signed_state = SignedState {
    channel_id,
    sequence: state.metadata.sequence,
    root: state.metadata.current_root,
    sig_a,
    sig_b: cosignature,
};

// 协作关闭
mgr.close_channel(&mut state, &signed_state, &user_pk, &provider_pk, current_slot, settle_window)?;

// 认领叶子
mgr.claim_leaf_with_proof(&mut state, leaf_index, claim_amount, &claimer_pk, current_slot, &claimer_sig, &proof)?;

// 最终结算
mgr.finalize_settlement(&mut state, current_slot, &caller_pk, &caller_sig)?;
```

## 7. 链上操作

| 指令 | 函数 | 说明 |
|:-----|:-----|:-----|
| `cooperative_settle` | `build_cooperative_settle_ix` | 验证双签，进入 Settling |
| `claim` | `build_claim_ix` | Merkle Proof 认领标准叶子 |
| `finalize_settlement` | `build_finalize_settlement_ix` | 分配未认领资金，关闭通道 |

## 8. 错误处理

| 错误 | 原因 | 处理 |
|:-----|:-----|:-----|
| `ActiveHtlcsExist` | 通道内仍有未解决 HTLC | 先解决或退回所有 HTLC |
| `InvalidSignature` | 双签验证失败 | 确认 sig_a 和 sig_b 正确 |
| `ProofVerificationFailed` | Merkle Proof 无效 | 重新生成 proof |
| `SettleWindowNotExpired` | 结算窗口未结束 | 等待更多 slots |

## 9. 注意事项

- `close_channel` 会拒绝有活跃 HTLC 的通道
- `settle_window` 决定了认领叶子的时间窗口（slots）
- 未认领的资金在 `finalize_settlement` 时按 `deposit_a/deposit_b` 比例分配
- 链上 `cooperative_settle` 验证两个签名对应通道的 user_pubkey 和 provider_pubkey
