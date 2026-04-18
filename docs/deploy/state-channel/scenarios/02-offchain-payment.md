# 场景二：离链支付与拆分

## 1. 场景描述

用户在已开通的通道内向 Provider 发起离链支付。支持两种方式：整叶转账（将整个 UTXO 叶子所有权转移给 Provider）和部分转账（从叶子中拆出一部分金额到新槽位，转移给 Provider）。每次支付都通过 Ed25519 签名验证，Provider 配签确认后生效。

## 2. 参与角色

| 角色 | 职责 |
|:-----|:-----|
| User | 构建签名 LeafUpdate，发送支付请求 |
| Provider | 验证签名，应用更新，配签确认 |

## 3. 前置条件

- 通道已开通，状态为 `Open`
- 已完成拆分树（有多个可用 UTXO 叶子）
- User 知道要支付的叶子索引和金额

## 4. 操作流程

```
User                                   Provider
 │                                        │
 │  1. 构建 LeafUpdate                    │
 │  sign_leaf_update(channel_id, seq,     │
 │    leaf_index, prev_leaf, new_leaf)    │
 │                                        │
 │  2. POST /v1/channels/{id}/pay         │
 │───────────────────────────────────────→│
 │                                        │  3. 验证用户签名
 │                                        │  apply_leaf_update(state, update, &user_pk)
 │                                        │
 │  4. POST /v1/channels/{id}/cosign      │
 │───────────────────────────────────────→│
 │                                        │  5. Provider 配签
 │                                        │  provider_cosign_state(state, &provider_kp)
 │←───────────────────────────────────────│
 │  cosignature                           │
```

## 5. HTTP API 调用

### 单笔支付

```bash
curl -X POST http://localhost:3001/v1/channels/{channel_id}/pay \
  -H "Content-Type: application/json" \
  -d '{
    "leaf_index": 0,
    "new_owner": "Provider公钥(Base58)",
    "amount": 100000
  }'
```

### 请求 Provider 配签

```bash
curl -X POST http://localhost:3001/v1/channels/{channel_id}/cosign \
  -H "Content-Type: application/json" \
  -d '{}'
```

### Provider 接受支付

```bash
curl -X POST http://localhost:3002/v1/channels/{channel_id}/accept-payment \
  -H "Content-Type: application/json" \
  -d '{
    "leaf_index": 0,
    "new_owner": "Provider公钥",
    "amount": 100000,
    "sequence": 3,
    "signature": "签名hex..."
  }'
```

## 6. Rust 库调用

```rust
use ignite_pay_state_channel::signing::sign_leaf_update;

// User 签名 LeafUpdate
let sig = sign_leaf_update(
    &channel_id,
    state.metadata.sequence + 1,
    leaf_index,
    &prev_leaf,
    &new_leaf,
    &user_keypair,
);

// Provider 验证并应用
mgr.apply_leaf_update(&mut state, &leaf_update, &user_pubkey)?;

// Provider 配签
let cosignature = mgr.provider_cosign_state(&mut state, &provider_keypair)?;
```

## 7. 链上操作

离链支付无需链上操作。所有变更仅在双方的 sled 数据库中持久化。

## 8. 错误处理

| 错误 | 原因 | 处理 |
|:-----|:-----|:-----|
| `InvalidSignature` | 签名验证失败 | 检查签名者公钥和签名数据 |
| `SequenceMismatch` | 序列号不连续 | 使用当前 sequence + 1 |
| `LeafNotFound` | 叶子索引超出范围 | 检查 tree_depth 限制 |
| `AmountConservation` | 支付后总金额不一致 | 部分转账时确保拆分金额正确 |
| `ComplianceHold` | 合规审查中，支付被阻止 | 联系合规管理员清除 hold |

## 9. 注意事项

- 签名消息格式：`SHA-256(channel_id || sequence || leaf_index || prev_leaf_hash || new_leaf_hash)`
- 每次支付 sequence 递增 1，不可跳过或回退
- Provider 配签表示同意当前状态，后续可用于协作关闭
- 部分转账会消耗一个空叶子槽位，注意 `tree_depth` 限制
