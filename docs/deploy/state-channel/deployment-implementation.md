# 状态通道实施文档

## 1. 概述

Ignite Pay 状态通道系统实现了基于 UTXO + Merkle Tree 的离链支付通道。本文档描述从零部署完整系统的实施步骤。

**涉及组件**：
- `ignite-pay-state-channel`：离链 Rust 库（通道管理、Merkle 树、HTLC、路由）
- `ignite-pay-program`：链上 Solana 程序（Anchor 框架，结算/争议处理）

---

## 2. 系统架构总览

```
┌─────────────┐   LeafUpdate + CoSign   ┌──────────────┐
│  用户 (A)    │ ←──────────────────────→ │  商户/Hub (B) │
│             │                           │              │
│  ChannelMgr │   SignedState (双签)      │  ChannelMgr  │
│  Pipeline   │ ←──────────────────────→ │  CoSign      │
│  HtlcMgr    │                           │  HtlcMgr     │
│  sled DB    │                           │  sled DB     │
└──────┬──────┘                           └──────┬───────┘
       │                                         │
       │         链上结算 (Solana)                 │
       └──────────────┬──────────────────────────┘
                      ▼
              ┌──────────────┐
              │ ignite-pay-  │
              │   program    │
              │              │
              │ PDA: channel │
              │ PDA: escrow  │
              └──────────────┘
```

---

## 3. 离链库：ignite-pay-state-channel

### 3.1 模块结构

| 模块 | 文件 | 说明 |
|:-----|:-----|:-----|
| `channel` | `channel.rs` | `ChannelManager` — 通道生命周期管理，sled 持久化 |
| `merkle` | `merkle.rs` | `MerkleTree` — 排序对哈希二叉树 |
| `types` | `types.rs` | `UTXOLeaf`, `LeafUpdate`, `SignedState`, `ChannelMetadata` |
| `signing` | `signing.rs` | Ed25519 签名/验证，消息构造 |
| `pipeline` | `pipeline.rs` | `Pipeline` — 批量 LeafUpdate 构建器，自动回滚 |
| `htlc` | `htlc.rs` | `HtlcManager` — HTLC 原像/生命周期管理 |
| `hub` | `hub.rs` | `HubManager` — Hub 注册/指标，sled 持久化 |
| `routing` | `routing.rs` | `RouteService` — DFS 路由发现/评分 |
| `multihop` | `multihop.rs` | `MultiHopManager` — 多跳支付，递减 timelock |
| `compliance` | `compliance.rs` | `ComplianceManager` — 消费限额/审计 |
| `error` | `error.rs` | `StateChannelError` 统一错误类型 |
| `helpers` | `helpers.rs` | 辅助工具函数 |

### 3.2 依赖

```toml
[dependencies]
solana-program = "2"           # Solana 核心类型（无 OpenSSL 依赖）
solana-pubkey = "2"            # Pubkey 类型
ed25519-dalek = "1"            # Ed25519 签名
borsh = "1"                    # 序列化
serde = { version = "1", features = ["derive"] }
sled = "0.34"                  # 嵌入式数据库
anyhow = "1"                   # 错误处理
rand = "0.7"                   # 随机数生成
hex = "0.4"                    # 十六进制编解码
tracing = "0.1"                # 日志

[dev-dependencies]
tempfile = "3"                 # 测试用临时目录
```

### 3.3 编译

```bash
cd ignite-pay-state-channel
cargo build
cargo test
```

### 3.4 关键常量

| 常量 | 值 | 说明 |
|:-----|:---|:-----|
| `HTLC_SAFETY_MARGIN` | 1000 slots | HTLC timelock 安全余量（~6.7 分钟） |
| `HOP_MARGIN` | 1000 slots | 多跳 timelock 递减步长（~6.7 分钟） |
| 最大 `tree_depth` | 12 | 链上程序限制，最多 4096 叶子 |

---

## 4. 链上程序：ignite-pay-program

### 4.1 程序信息

| 属性 | 值 |
|:-----|:---|
| Program ID | `DJBHr35jL3JAGoU7bKMsEFmpeNMrCSK7oYQE4HJ3GBUe` |
| 框架 | Anchor 1.0.0 |
| SPL Token | anchor-spl 1.0.0 |

### 4.2 指令列表

| # | 指令 | 说明 | 签名要求 |
|:--|:-----|:-----|:---------|
| 1 | `open_channel` | 创建通道 PDA + 初始根 | User 签名 |
| 2 | `fund_channel` | Provider 注入资金 | Provider 签名 |
| 3 | `cooperative_settle` | 双方签名 → 进入结算窗口 | User + Provider |
| 4 | `trigger_challenge` | 单方发起争议 | User 或 Provider |
| 5 | `submit_counter_state` | 提交更新的双签状态 | 验证 sig_a + sig_b |
| 6 | `settle_after_timeout` | challenge_duration 过期后进入结算 | 任何人 |
| 7 | `claim` | 提交 Merkle Proof 认领标准叶子 | 叶子所有者 |
| 8 | `verify_htlc` | 提交原像 + Merkle Proof 认领 HTLC 叶子 | 受益人 |
| 9 | `htlc_refund` | timelock 过期后退还 HTLC 资金 | 叶子所有者 |
| 10 | `finalize_settlement` | 结算窗口关闭，分配未认领资金 | User 或 Provider |

### 4.3 PDA 账户

| 账户 | Seeds | 说明 |
|:-----|:------|:-----|
| `ChannelAccount` | `["channel", channel_id]` | 通道状态 |
| `Escrow Vault` | `["escrow", channel_id]` | 托管 Token 账户 |

### 4.4 ChannelAccount 字段

```
channel_id: [u8; 32]           — 通道唯一标识
user_pubkey: Pubkey            — 用户公钥 (Party A)
provider_pubkey: Pubkey        — Provider 公钥 (Party B)
token_mint: Pubkey             — SPL Token Mint
status: ChannelStatus          — Open / Challenged / Settling / Closed
sequence: u64                  — 当前序列号
current_root: [u8; 32]         — 当前 Merkle 根
total_deposited: u64           — 总存款
total_claimed: u64             — 已认领总额
vault_a / vault_b: Pubkey      — 双方 Token 账户
deposit_a / deposit_b: u64     — 各方存款
challenge_duration: u64        — 争议期（slots）
min_challenge_delay: u64       — 最短争议延迟
challenge_slot: Option<u64>    — 争议发起 slot（Challenged 状态下设置）
settle_deadline: Option<u64>   — 结算窗口截止（Settling 状态下设置）
tree_depth: u32                — Merkle 树深度（最大 12）
claimed_leaves: Vec<u32>       — 已认领叶子索引
auto_close_slot: Option<u64>   — 自动关闭 slot
```

### 4.5 编译与部署

```bash
cd ignite-pay-program

# 编译
anchor build

# 部署到 Devnet
anchor deploy --provider.cluster devnet

# 部署到 Mainnet
anchor deploy --provider.cluster mainnet
```

### 4.6 账户空间计算

```rust
ChannelAccount::space(tree_depth)
// tree_depth=3 → 8 叶子 → 约 520 bytes
// tree_depth=4 → 16 叶子 → 约 552 bytes
// tree_depth=8 → 256 叶子 → 约 1112 bytes
// tree_depth=12 → 4096 叶子 → 约 16472 bytes
```

---

## 5. 实施阶段

### 阶段一：本地开发与测试

**目标**：使用离链库完成通道全流程，不涉及链上操作。

```bash
# 编译离链库
cd ignite-pay-state-channel
cargo test

# 验证所有模块
cargo test -- --nocapture
```

关键测试场景：
1. 开通通道 → 拆分树 → 转账 → 协作关闭
2. HTLC 创建 → 揭示原像 → 解决
3. HTLC 创建 → 超时 → 退款
4. Pipeline 批量操作 + abort 回滚
5. 合规限额触发

### 阶段二：链上 Devnet 集成

**目标**：部署链上程序，实现离链操作 + 链上结算。

#### 步骤 1：部署链上程序

```bash
cd ignite-pay-program

# 构建
anchor build

# 部署到 Devnet
anchor deploy --provider.cluster devnet

# 记录 Program ID
# 当前: DJBHr35jL3JAGoU7bKMsEFmpeNMrCSK7oYQE4HJ3GBUe
```

#### 步骤 2：SVM 集成测试

```bash
# 在 Anchor workspace 中运行 litesvm 测试
cd anchor-workspace/tests/svm-litesvm
cargo test
```

#### 步骤 3：端到端流程测试

1. 用户调用 `open_channel` → 链上创建 PDA
2. 资金转入 Escrow Vault
3. 离链拆分树 + 签名 LeafUpdate
4. Provider 配签
5. 离链支付流转
6. 调用 `cooperative_settle` → 进入结算窗口
7. 双方调用 `claim` 认领叶子
8. 调用 `finalize_settlement` 关闭通道

### 阶段三：Hub 路由网络

**目标**：多个 Hub 组成路由网络，实现跨通道支付。

#### 步骤 1：Hub 注册

```rust
// 每个 Hub 生成 DID + 密钥对
// 注册到 HubManager
// 上报指标
```

#### 步骤 2：路由发现测试

```bash
cd ignite-pay-state-channel
cargo test routing
cargo test multihop
```

#### 步骤 3：多跳支付测试

1. 发现路由 User → Hub1 → Hub2 → Merchant
2. 创建递减 timelock 的 HTLC
3. 终端揭示原像
4. 反向逐跳解决

### 阶段四：合规与审计

**目标**：集成合规管理，支持监管审计。

#### 步骤 1：配置消费限额

```rust
compliance.init_channel_compliance(channel_id, SpendingLimit {
    threshold: 1_000_000,
    per_channel: 5_000_000,
    window_slots: 432_000,   // 约 1 个 epoch
})?;
```

#### 步骤 2：审计追踪

```rust
// 每次 LeafUpdate 记录到审计
compliance.record_audit(&update)?;
```

---

## 6. 测试指南

### 6.1 离链库单元测试

```bash
cd ignite-pay-state-channel

# 全部测试
cargo test

# 按模块
cargo test --lib channel
cargo test --lib merkle
cargo test --lib signing
cargo test --lib pipeline
cargo test --lib htlc
cargo test --lib hub
cargo test --lib routing
cargo test --lib multihop
cargo test --lib compliance
```

### 6.2 SVM 集成测试

```bash
# 在 Anchor workspace 中
cd anchor-workspace/tests/svm-litesvm
cargo test
```

### 6.3 关键测试场景

| 场景 | 涉及模块 | 验证要点 |
|:-----|:---------|:---------|
| 开通 → 拆分 → 转账 → 关闭 | channel, pipeline | 金额守恒、签名验证 |
| HTLC 完整生命周期 | htlc, pipeline | 原像验证、timelock |
| 争议流程 | channel | challenge → counter → settle → claim |
| 多跳支付 | multihop, routing | 递减 timelock、费率计算 |
| 合规触发 | compliance | 滑动窗口、阈值触发 |
| Pipeline 回滚 | pipeline | abort/drop 自动恢复 |
| 批量更新 | channel | BatchFailureInfo 正确报告 |

---

## 7. 签名机制

### 7.1 两层签名

**Leaf 级签名**（LeafUpdate）— 链下验证：
```
message = SHA-256(channel_id || sequence || leaf_index || prev_leaf_hash || new_leaf_hash)
signature = Ed25519.sign(message, signer_private_key)
```

**State 级签名**（SignedState）— 链下预哈希，链上验证：
```
链下: message = SHA-256(channel_id || sequence || root)
sig_a = Ed25519.sign(message, user_private_key)
sig_b = Ed25519.sign(message, provider_private_key)
```

### 7.2 链上签名消息格式

链上合约构造原始字节拼接（不哈希），通过 Solana ed25519 指令内省验证。按格式分三族：

**族 A — OpenChannel**（用户单签）：
```
message = channel_id || deposit_a(8 LE) || tree_depth(4 LE) || initial_root
```

**族 B — CooperativeSettle / SubmitCounterState**（双方双签）：
```
message = channel_id || sequence(8 LE) || root
```

**族 C — Claim / VerifyHTLC / HTLCRefund / HTLCRefund / FinalizeSettlement / TriggerChallenge**（单签）：
```
message = channel_id || current_slot(8 LE) || current_root
```

> **注意**: 链下 `claim_message()` 辅助函数使用 `SHA-256("claim" || channel_id || leaf_index || amount || current_slot)` 格式，与链上族 C 格式不同。链上 Claim 验证使用 `channel_id || current_slot || current_root`，不含 leaf 级字段。

---

## 8. 通道生命周期

```
                          ┌───────────┐
                          │   Open    │ ← open_channel (链上)
                          └─────┬─────┘
                                │
                    离链操作（转账、HTLC、拆分）
                          ┌─────┴─────┐
                          │           │
                 ┌────────▼──┐  ┌─────▼──────────┐
                 │Cooperative│  │    Challenge    │
                 │  Settle   │  │                 │
                 └────┬──────┘  └────┬────────────┘
                      │              │
                      │         ┌────▼────────────┐
                      │         │ Counter State    │
                      │         │ (可选)            │
                      │         └────┬────────────┘
                      │              │
                 ┌────▼──────────────▼────┐
                 │       Settling          │
                 │  (结算窗口: claim 叶子)  │
                 └────────────┬───────────┘
                              │
                 ┌────────────▼───────────┐
                 │  Finalize Settlement   │
                 │  (分配未认领资金)        │
                 └────────────┬───────────┘
                              │
                 ┌────────────▼───────────┐
                 │        Closed          │
                 └────────────────────────┘
```

---

## 9. 故障排查

### 9.1 签名验证失败

**现象**：`verify_leaf_update_signature` 或链上 `InvalidSignature`

**排查**：
1. 检查签名者公钥是否正确（User 或 Provider）
2. 检查 `prev_leaf_hash` 是否匹配当前叶子
3. 检查 sequence 是否连续
4. 确认使用相同的 `channel_id`

### 9.2 金额守恒错误

**现象**：`AmountConservation { expected, actual }`

**排查**：
1. 拆分树时确保所有叶子金额之和 = `total_deposited`
2. Pipeline 操作中 partial_transfer 的金额不超过源叶子
3. 检查是否有并发修改

### 9.3 Merkle Proof 验证失败

**现象**：链上 `ProofVerificationFailed`

**排查**：
1. 确认离链 `MerkleTree` 使用排序对哈希：`hashv(&[min, max])`
2. 检查叶子是否在正确的索引位置
3. 确认 `current_root` 是最新的

### 9.4 HTLC 超时/退款问题

**现象**：`HtlcNotExpired` 或 `HtlcExpired`

**排查**：
1. Solana slot 时间：1 slot ≈ 400ms（正常），devnet 可能更慢
2. 检查 `timelock_slot` 是否满足约束：`> current_slot + challenge_duration + HTLC_SAFETY_MARGIN`
3. 多跳时检查 timelock 递减是否正确

### 9.5 sled 数据库问题

**现象**：数据库损坏或锁冲突

**排查**：
1. sled 不支持多进程同时打开同一数据库
2. 确保 `sled::open` 路径有正确的文件系统权限
3. 异常退出后可能需要删除 `*.lock` 文件

---

## 10. 版本升级路径

```
V0.1 (当前)                        V1.0                         V2.0
┌──────────────────┐    ┌────────────────────────┐    ┌───────────────────────┐
│ 离链通道管理      │    │ 链上程序部署            │    │ Hub 路由网络           │
│ 单通道支付        │ →  │ Devnet 集成测试         │ →  │ 多跳支付              │
│ Mock 结算         │    │ 协作关闭 + 争议         │    │ 流动性管理            │
│ Pipeline 批量操作 │    │ HTLC 链上验证           │    │ 完整合规引擎          │
└──────────────────┘    └────────────────────────┘    └───────────────────────┘
```

升级要点：
- **V0.1 → V1.0**：部署 `ignite-pay-program`，初始化 SPL Token 账户，配置链上参数
- **V1.0 → V2.0**：部署多 Hub 节点，配置路由拓扑，启用合规模块
