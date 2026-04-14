# 状态通道用户端部署配置文档

## 1. 概述

用户端（Party A，付款方）是状态通道的发起者。用户通过 `ignite-pay-state-channel` 离链库管理通道生命周期，包括开通道、拆分 UTXO、签名支付、管理 HTLC 和结算。

用户端不运行独立服务，而是将 `ignite-pay-state-channel` 作为 Rust 库集成到客户端应用中。

---

## 2. 核心组件

| 组件 | crate | 说明 |
|:-----|:------|:-----|
| 通道管理 | `ignite-pay-state-channel` | `ChannelManager` — 通道开/关、状态持久化 |
| Merkle 树 | `ignite-pay-state-channel` | `MerkleTree` — UTXO 叶子节点的二叉 Merkle 树 |
| 签名模块 | `ignite-pay-state-channel` | `signing` — Ed25519 签名/验证 |
| 流水线 | `ignite-pay-state-channel` | `Pipeline` — 批量 LeafUpdate 构建 |
| HTLC 管理 | `ignite-pay-state-channel` | `HtlcManager` — 原像生成/揭示/过期 |
| 合规模块 | `ignite-pay-state-channel` | `ComplianceManager` — 消费限额/审计 |
| 链上程序 | `ignite-pay-program` | Anchor 程序 — 链上结算/争议处理 |

---

## 3. 集成步骤

### 3.1 添加依赖

在用户的 Rust 项目 `Cargo.toml` 中：

```toml
[dependencies]
ignite-pay-state-channel = { path = "../ignite-pay-state-channel" }
solana-pubkey = "2"
solana-program = "2"
ed25519-dalek = "1"
```

### 3.2 初始化 ChannelManager

```rust
use ignite_pay_state_channel::channel::ChannelManager;
use ignite_pay_state_channel::signing::{generate_keypair, to_pubkey};
use solana_pubkey::Pubkey;

// 打开 sled 数据库（所有通道状态持久化在此）
let db = sled::open("./user_channel_data")?;
let manager = ChannelManager::new(db)?;

// 生成或加载用户密钥对
let user_keypair = generate_keypair();
let user_pubkey = to_pubkey(&user_keypair);
```

### 3.3 开通通道

```rust
use ignite_pay_state_channel::channel::ChannelManager;

let provider_pubkey = Pubkey::new_from_array(/* 商户/Provider 公钥 */);
let token_mint = Pubkey::new_from_array(/* SPL Token Mint 地址，如 USDC */);
let vault_a = Pubkey::new_from_array(/* 用户 SPL Token 账户 */);
let vault_b = Pubkey::new_from_array(/* Provider SPL Token 账户 */);

let state = manager.open_channel(
    &user_pubkey,           // 用户公钥
    &provider_pubkey,       // Provider 公钥
    &token_mint,            // Token Mint
    1_000_000,              // 存款金额（最小单位）
    3,                      // tree_depth（2^3 = 8 个叶子槽位）
    current_slot,           // 开通 slot
    &vault_a,               // 用户 vault
    &vault_b,               // Provider vault
    500,                    // challenge_duration（slots）
    50,                     // min_challenge_delay（slots）
    None,                   // auto_close_slot（可选）
)?;

println!("通道已开通: channel_id = {}", hex::encode(state.metadata.channel_id));
println!("初始根: {}", hex::encode(state.metadata.current_root));
```

**链上操作**：开通通道后，需调用链上 `open_channel` 指令将通道状态提交到 Solana。

### 3.4 构建拆分树

将初始存款拆分为多个面额的 UTXO 叶子：

```rust
use ignite_pay_state_channel::types::UTXOLeaf;

let leaves = vec![
    UTXOLeaf::standard(user_pubkey, 100_000),  // 100K
    UTXOLeaf::standard(user_pubkey, 200_000),  // 200K
    UTXOLeaf::standard(user_pubkey, 500_000),  // 500K
    UTXOLeaf::standard(user_pubkey, 200_000),  // 200K
    // 剩余空位自动用 UTXOLeaf::empty() 填充
];

let signed_state = manager.construct_split_tree(
    &mut state,
    leaves,
    &user_keypair,
    &provider_keypair,   // 需要 Provider 配签
)?;
```

**注意**：`construct_split_tree` 要求金额守恒 — 所有叶子金额之和必须等于 `total_deposited`。

### 3.5 使用 Pipeline 执行支付

```rust
use ignite_pay_state_channel::pipeline::Pipeline;

let channel_id = state.metadata.channel_id;
let sequence = state.metadata.sequence;

let mut tree = state.tree;
{
    let mut pipeline = Pipeline::new(&mut tree, channel_id, sequence + 1, &user_keypair);

    // 整叶转账：将叶子 0 转给 Provider
    pipeline.transfer_leaf(0, provider_pubkey)?;

    // 部分转账：从叶子 1 拆出 50_000 到空槽位 4
    pipeline.partial_transfer(1, 4, 50_000, provider_pubkey)?;

    // 提交流水线
    let (updates, final_sequence) = pipeline.build();

    // updates 中包含所有签名的 LeafUpdate
    // 发送给 Provider 进行配签
}
```

**Pipeline 安全机制**：
- 如果操作失败，调用 `pipeline.abort()` 回滚树状态
- 如果 Pipeline 被 drop 但未调用 `build()` 或 `abort()`，自动回滚

### 3.6 HTLC 支付

```rust
use ignite_pay_state_channel::htlc::HtlcManager;

let mut htlc_mgr = HtlcManager::with_db(db.clone(), channel_id);

// 创建 HTLC（生成随机原像）
let (hash_lock, preimage) = htlc_mgr.create_htlc(
    100_000,           // 锁定金额
    2,                 // 叶子索引
    user_pubkey,       // 所有者
    provider_pubkey,   // 受益人
    current_slot,      // 当前 slot
    500,               // 持续 slots
);

// 将 hash_lock 告知 Provider（原像暂不透露）
// Provider 可以用 hash_lock 验证 HTLC 叶子

// 在 Pipeline 中创建 HTLC 叶子
{
    let mut pipeline = Pipeline::new(&mut tree, channel_id, seq, &user_keypair);
    pipeline.create_htlc(
        2,                  // 叶子索引
        hash_lock,
        timelock_slot,
        provider_pubkey,    // beneficiary
        current_slot,
        challenge_duration,
    )?;
    let (updates, _) = pipeline.build();
}

// 服务完成后，Provider 揭示原像
htlc_mgr.reveal_preimage(&hash_lock, &preimage)?;

// 在 Pipeline 中解决 HTLC
{
    let mut pipeline = Pipeline::new(&mut tree, channel_id, seq, &user_keypair);
    pipeline.resolve_htlc(2, &preimage)?;
    let (updates, _) = pipeline.build();
}

htlc_mgr.mark_fulfilled(&hash_lock)?;
```

---

## 4. 通道参数配置

### 4.1 tree_depth 选择

| tree_depth | 最大叶子数 | 适用场景 |
|:-----------|:-----------|:---------|
| 3 | 8 | 小额试用 / 单次支付 |
| 4 | 16 | 日常支付 |
| 5 | 32 | 中等频率交易 |
| 6 | 64 | 高频微支付 |
| 7 | 128 | 大量并发 HTLC |
| 8 | 256 | 生产级高频交易 |

> 链上程序限制 `tree_depth <= 8`。

### 4.2 challenge_duration 选择

| 值 (slots) | 约等于 | 适用场景 |
|:-----------|:-------|:---------|
| 150 | ~1 分钟 | 测试环境 |
| 500 | ~3.3 分钟 | 小额通道 |
| 1500 | ~10 分钟 | 标准 |
| 4500 | ~30 分钟 | 大额通道 |
| 9000 | ~1 小时 | 高价值争议窗口 |

### 4.3 拆分面额建议

以 1,000,000 单位存款为例：

```
tree_depth = 4 (16 槽位):
  [500K, 200K, 100K, 50K, 50K, 50K, 50K, ...empty]
  适用：中等频次支付

tree_depth = 5 (32 槽位):
  [500K, 100K, 100K, 50K, 50K, 20K, 20K, 20K, 20K, 20K, 10K×10, ...empty]
  适用：高频微支付 + HTLC 预留
```

---

## 5. 数据持久化

### 5.1 sled 数据库

`ChannelManager` 使用 sled 嵌入式数据库存储所有通道状态：

| 存储路径 | 内容 |
|:---------|:-----|
| 数据库根目录 | 通道元数据（`ChannelMetadata`）、Merkle 树 |
| `htlc:{channel_id}` | HTLC 记录 |
| `compliance:{channel_id}` | 合规状态 |
| `audit:{channel_id}:{seq}` | 审计追踪 |

### 5.2 备份建议

```bash
# sled 数据目录
./user_channel_data/

# 备份（确保进程已停止或使用快照）
cp -r ./user_channel_data/ ./user_channel_data_backup/
```

> sled 数据自动持久化到磁盘，重启后通过 `ChannelManager::new(sled::open(path))` 恢复。

---

## 6. 结算操作

### 6.1 协作关闭（推荐）

双方同意当前状态，共同签名关闭：

```rust
// 用户和 Provider 双方签名当前根
let sig_a = sign_state(&channel_id, sequence, &root, &user_keypair);
let sig_b = sign_state(&channel_id, sequence, &root, &provider_keypair);

// 调用链上 cooperative_settle
```

### 6.2 争议关闭

如果对方不响应：

```rust
// 用户提交最新的已签名状态触发争议
manager.trigger_challenge(&mut state, submitted_root, submitted_sequence, &user_keypair)?;

// 等待 challenge_duration 过期
// 对方可在期间提交 counter_state

// 超时后进入结算窗口
manager.settle_after_timeout(&mut state, settle_window)?;

// 在结算窗口内提交 Merkle Proof 认领叶子
manager.claim(/* ... */)?;
```

### 6.3 自动关闭

如果通道设置了 `auto_close_slot`：

```rust
let state = manager.open_channel(
    // ...
    Some(current_slot + 100_000),  // auto_close_slot
)?;

// 到期后任何人可以触发结算
manager.auto_settle(&mut state, settle_window)?;
```

---

## 7. 安全检查清单

| 检查项 | 说明 | 状态 |
|:-------|:-----|:-----|
| 用户密钥安全存储 | Ed25519 私钥使用安全存储方案 | 必须 |
| 原像保密 | HTLC 原像在受益人确认前不透露 | 必须 |
| sled 数据目录权限 | 限制数据库文件访问权限 | 建议 |
| 序列号连续性 | 确保不签名低于当前序列的 LeafUpdate | 必须 |
| challenge_duration 合理 | 留足够时间响应争议 | 建议 |
| 金额守恒验证 | 拆分树前检查总金额匹配 | 必须 |
