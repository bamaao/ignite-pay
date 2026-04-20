# 场景一：通道开通与注资

## 1. 场景描述

用户 (User) 与商户/Provider 建立一个双向支付通道。用户存入初始资金，通道内创建一棵 Merkle 树来管理 UTXO 余额。可选地，Provider 也可以向通道注资。

## 2. 参与角色

| 角色 | 职责 |
|:-----|:-----|
| User | 发起通道，存入初始资金，构建拆分树 |
| Provider | 可选注资，对初始状态配签 |

## 3. 前置条件

- User 和 Provider 均已部署 `channel-service` 服务
- 双方已持有 Solana 密钥对和 SPL Token 账户
- 链上程序 (`ignite-pay-program`) 已部署到目标集群
- User 知道 Provider 的公钥和 Token 账户地址

## 4. 操作流程

```
User                                  Provider                                  Solana
 │                                       │                                       │
 │  1. POST /v1/channels/open            │                                       │
 │──────────────────────────────────────→│                                       │
 │  ChannelManager::open_channel         │                                       │
 │  (创建 Merkle 树, 生成 channel_id)     │                                       │
 │                                       │                                       │
 │  2. 构建 open_channel 指令             │                                       │
 │  build_open_channel_ix(...)           │                                       │
 │──────────────────────────────────────────────────────────────────────────────→│
 │                                       │                          创建 PDA 账户    │
 │                                       │                          存入初始资金      │
 │  3. (可选) POST /v1/channels/{id}/fund│                                       │
 │──────────────────────────────────────→│                                       │
 │                                       │  4. Provider 注资                     │
 │                                       │  build_fund_channel_ix                │
 │                                       │──────────────────────────────────────→│
 │                                       │                                       │
 │  5. POST /v1/channels/{id}/split      │                                       │
 │  construct_split_tree(leaves)         │                                       │
 │──────────────────────────────────────→│                                       │
 │                                       │  6. Provider 配签                     │
 │                                       │  provider_cosign_state                │
 │←──────────────────────────────────────│                                       │
```

## 5. HTTP API 调用

### 开通通道

```bash
curl -X POST http://localhost:3001/v1/channels/open \
  -H "Content-Type: application/json" \
  -d '{
    "provider_pubkey": "Provider的Solana公钥(Base58)",
    "token_mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
    "deposit_amount": 1000000,
    "tree_depth": 4,
    "vault_a": "用户的SPL Token账户",
    "vault_b": "Provider的SPL Token账户"
  }'
```

响应包含 `channel_id` 和链上指令数据，用于组装 Solana 交易。

### Provider 注资

```bash
curl -X POST http://localhost:3002/v1/channels/{channel_id}/fund \
  -H "Content-Type: application/json" \
  -d '{
    "deposit_amount": 500000
  }'
```

### 构建拆分树

```bash
curl -X POST http://localhost:3001/v1/channels/{channel_id}/split \
  -H "Content-Type: application/json" \
  -d '{
    "leaves": [
      {"owner": "用户公钥", "amount": 500000},
      {"owner": "用户公钥", "amount": 200000},
      {"owner": "用户公钥", "amount": 300000}
    ]
  }'
```

## 6. Rust 库调用

```rust
use ignite_pay_state_channel::channel::ChannelManager;
use ignite_pay_state_channel::signing::{generate_keypair, to_pubkey};

let db = sled::open("./data/user")?;
let mgr = ChannelManager::new(db)?;

// 开通通道
let mut state = mgr.open_channel(
    &user_pubkey,
    &provider_pubkey,
    &token_mint,
    1_000_000,          // deposit_a
    4,                   // tree_depth (16 叶子)
    current_slot,
    &vault_a,
    &vault_b,
    5000,                // challenge_duration
    1000,                // min_challenge_delay
    None,                // auto_close_slot
)?;

// Provider 注资
mgr.fund_channel(&mut state, &provider_kp, 500_000, None)?;

// 构建拆分树
let leaves = vec![
    UTXOLeaf::standard(user_pubkey, 500_000),
    UTXOLeaf::standard(user_pubkey, 200_000),
    UTXOLeaf::standard(user_pubkey, 300_000),
];
let signed = mgr.construct_split_tree(&mut state, leaves, &user_kp, &provider_kp)?;
```

## 7. 链上操作

| 指令 | 函数 | 说明 |
|:-----|:-----|:-----|
| `open_channel` | `build_open_channel_ix` | 创建 ChannelAccount PDA + Escrow PDA |
| `fund_channel` | `build_fund_channel_ix` | Provider 向 Escrow 注入资金 |

PDA 推导：
- Channel PDA: `seeds = ["channel", channel_id]`
- Escrow PDA: `seeds = ["escrow", channel_id]`

## 8. 错误处理

| 错误 | 原因 | 处理 |
|:-----|:-----|:-----|
| `InvalidKeypair` | 密钥对文件格式错误 | 检查 keypair_path 配置 |
| `ChannelNotFound` | channel_id 不存在 | 确认通道已开通 |
| `AmountConservation` | 拆分树金额不守恒 | 检查 leaves 金额之和 = total_deposited |
| `SolanaRpc` | RPC 调用失败 | 检查 rpc_url 配置和网络连接 |

## 9. 注意事项

- `tree_depth` 范围 3-12，对应 8-4096 个叶子槽位，链上程序硬限制
- 拆分树要求金额守恒：所有叶子金额之和必须等于 `total_deposited`
- `construct_split_tree` 需要 Provider 的 keypair 进行配签
- `keypair_path` 为空时会自动生成临时密钥（每次重启变化，仅测试用）
