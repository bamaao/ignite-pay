# 状态通道用户端部署配置文档

## 1. 概述

用户端（Party A，付款方）是状态通道的发起者。用户通过 `ignite-pay-state-channel` 离链库管理通道生命周期，包括开通道、拆分 UTXO、签名支付、管理 HTLC 和结算。

用户端有两种集成方式：

1. **库集成**：将 `ignite-pay-state-channel` 作为 Rust 库嵌入客户端应用
2. **服务部署**：运行 `ignite-pay-channel-service` 的 `channel-user` 二进制，通过 HTTP REST + WebSocket 接口操作

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
| 链上指令 | `ignite-pay-solana` | `channel` — 10 个链上 Instruction 构建器 |
| HTTP 服务 | `ignite-pay-channel-service` | User 角色的 REST + WebSocket 服务 |

---

## 3. 方式一：服务部署（推荐）

### 3.1 编译

```bash
cd ignite-pay-channel-service
cargo build --release --bin channel-user
```

产物位于 `target/release/channel-user`（Windows: `target/release/channel-user.exe`）。

### 3.2 生成密钥

```bash
# Solana CLI 生成密钥文件（64 字节，JSON 数组格式）
solana-keygen new --outfile ./keys/user.key

# 或使用任意 Ed25519 密钥对，保存为 64 字节原始文件（前 32 字节私钥 + 后 32 字节公钥）
```

> 如果 `keypair_path` 留空（`""`），服务启动时自动生成临时密钥对（每次重启会变，仅用于测试）。

### 3.3 配置文件

创建 `config.toml`：

```toml
[server]
host = "0.0.0.0"        # 监听地址，生产环境建议 "127.0.0.1" + 反向代理
port = 3001              # 监听端口

[solana]
rpc_url = "https://api.devnet.solana.com"          # Solana RPC 端点
channel_program_id = "DJBHr35jL3JAGoU7bKMsEFmpeNMrCSK7oYQE4HJ3GBUe"  # 链上程序 ID
keypair_path = "./keys/user.key"                    # Ed25519 密钥对文件

[channel]
default_tree_depth = 4           # 默认 Merkle 树深度（2^4 = 16 叶子）
default_challenge_duration = 5000   # 默认争议期（slots，约 33 分钟）
default_min_challenge_delay = 1000  # 最短争议延迟（slots）
default_settle_window = 10000       # 默认结算窗口（slots）
auto_close_offset = 500000          # 自动关闭偏移（slots，0 表示不自动关闭）
db_path = "./data/channel_user"     # sled 数据库路径

# 可选：合规配置
[compliance]
spending_threshold = 1000000000     # 累计消费阈值（最小单位）
per_channel_limit = 100000000       # 单通道最大支付
window_slots = 100000               # 滑动窗口（slots）
travel_rule_threshold = 500000000   # Travel Rule 触发金额
```

### 3.4 启动服务

```bash
# 使用默认配置文件 config.toml
./channel-user

# 指定配置文件
./channel-user /path/to/config.toml

# 启用 debug 日志
RUST_LOG=debug ./channel-user
```

日志级别通过环境变量 `RUST_LOG` 控制，支持 `trace` / `debug` / `info` / `warn` / `error`。

### 3.5 API 接口

User 角色注册以下 REST 端点：

| 方法 | 路径 | 说明 | 离链 API | 链上指令 |
|:-----|:-----|:-----|:---------|:---------|
| GET | `/health` | 健康检查 | — | — |
| POST | `/v1/channels/open` | 开通通道 | `ChannelManager::open_channel` | `build_open_channel_ix` |
| POST | `/v1/channels/{id}/fund` | 注资通道 | — | `build_fund_channel_ix` |
| GET | `/v1/channels` | 列出通道 | `list_channel_ids` | — |
| GET | `/v1/channels/{id}` | 查询通道状态 | `load_state` | — |
| POST | `/v1/channels/{id}/split` | 构建拆分树 | `construct_split_tree` | — |
| POST | `/v1/channels/{id}/pay` | 单笔支付 | `apply_leaf_update` | — |
| POST | `/v1/channels/{id}/batch` | 批量支付 | `apply_leaf_update_batch_with_info` | — |
| POST | `/v1/channels/{id}/cosign` | 请求配签 | `provider_cosign_state` | — |
| POST | `/v1/channels/{id}/close` | 协作关闭 | `close_channel` | `build_cooperative_settle_ix` |
| POST | `/v1/channels/{id}/challenge` | 发起争议 | `trigger_challenge` | `build_trigger_challenge_ix` |
| POST | `/v1/channels/{id}/settle` | 超时结算 | `settle_after_timeout` | `build_settle_after_timeout_ix` |
| POST | `/v1/channels/{id}/claim` | 认领叶子 | `claim_leaf_with_proof` | — |
| POST | `/v1/channels/{id}/finalize` | 最终结算 | `finalize_settlement` | `build_finalize_settlement_ix` |
| POST | `/v1/channels/{id}/htlc/create` | 创建 HTLC | `HtlcManager::create_htlc` | — |
| POST | `/v1/channels/{id}/htlc/resolve` | 解决 HTLC | `reveal_preimage` | `build_verify_htlc_ix` |
| POST | `/v1/channels/{id}/htlc/refund` | HTLC 退款 | `claim_htlc_refund` | `build_htlc_refund_ix` |
| GET | `/v1/routes` | 查询路由 | `RouteService::find_routes` | — |
| POST | `/v1/multihop/create` | 创建多跳支付 | `MultiHopManager::create_payment` | — |
| POST | `/v1/multihop/{id}/resolve` | 解决多跳 | `resolve_hop` | — |
| GET | `/v1/compliance/{id}` | 合规状态 | `ComplianceManager` | — |
| WS | `/ws` | WebSocket 连接 | — | — |

### 3.6 示例请求

```bash
# 健康检查
curl http://localhost:3001/health

# 开通通道
curl -X POST http://localhost:3001/v1/channels/open \
  -H "Content-Type: application/json" \
  -d '{
    "user_pubkey": "11111111111111111111111111111111",
    "provider_pubkey": "22222222222222222222222222222222",
    "token_mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
    "deposit_amount": 1000000,
    "tree_depth": 4,
    "vault_a": "...",
    "vault_b": "..."
  }'

# 查询通道列表
curl http://localhost:3001/v1/channels

# 支付
curl -X POST http://localhost:3001/v1/channels/{channel_id}/pay \
  -H "Content-Type: application/json" \
  -d '{
    "leaf_index": 0,
    "new_owner": "22222222222222222222222222222222",
    "amount": 100000
  }'
```

### 3.7 WebSocket 协议

连接 `ws://localhost:3001/ws`，使用 tagged JSON 消息格式：

**认证**：

```json
→ {"type": "auth", "pubkey": "<base58>", "signature": [64 bytes], "timestamp": 1234567890}
← {"type": "auth_ok"}
```

签名内容：`SHA-256("channel-ws-auth:{timestamp}")`

**实时 LeafUpdate 推送**：

```json
→ {"type": "leaf_update", "channel_id": "hex", "sequence": 1, "leaf_index": 0,
   "prev_leaf_hash": [32 bytes], "new_leaf": {...}, "signature": [64 bytes]}
← {"type": "leaf_update_ack", "channel_id": "hex", "sequence": 2}
```

### 3.8 systemd 服务（Linux 生产部署）

创建 `/etc/systemd/system/ignite-channel-user.service`：

```ini
[Unit]
Description=Ignite Pay Channel User Service
After=network.target

[Service]
Type=simple
User=ignite
WorkingDirectory=/opt/ignite-pay
ExecStart=/opt/ignite-pay/channel-user /opt/ignite-pay/config.toml
Environment=RUST_LOG=info
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl daemon-reload
sudo systemctl enable ignite-channel-user
sudo systemctl start ignite-channel-user
sudo journalctl -u ignite-channel-user -f   # 查看日志
```

### 3.9 Nginx 反向代理

```nginx
server {
    listen 443 ssl;
    server_name channel-user.ignite-pay.example.com;

    ssl_certificate     /etc/ssl/certs/ignite-pay.pem;
    ssl_certificate_key /etc/ssl/private/ignite-pay.key;

    location / {
        proxy_pass http://127.0.0.1:3001;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
    }

    location /ws {
        proxy_pass http://127.0.0.1:3001;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
    }
}
```

---

## 4. 方式二：库集成

### 4.1 添加依赖

在用户的 Rust 项目 `Cargo.toml` 中：

```toml
[dependencies]
ignite-pay-state-channel = { path = "../ignite-pay-state-channel" }
solana-pubkey = "2"
solana-program = "2"
ed25519-dalek = "1"
```

### 4.2 初始化 ChannelManager

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

### 4.3 开通通道

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

### 4.4 构建拆分树

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

### 4.5 使用 Pipeline 执行支付

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

### 4.6 HTLC 支付

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

## 5. 通道参数配置

### 5.1 tree_depth 选择

| tree_depth | 最大叶子数 | 适用场景 |
|:-----------|:-----------|:---------|
| 3 | 8 | 小额试用 / 单次支付 |
| 4 | 16 | 日常支付 |
| 5 | 32 | 中等频率交易 |
| 6 | 64 | 高频微支付 |
| 7 | 128 | 大量并发 HTLC |
| 8 | 256 | 生产级高频交易 |

> 链上程序限制 `tree_depth <= 8`。

### 5.2 challenge_duration 选择

| 值 (slots) | 约等于 | 适用场景 |
|:-----------|:-------|:---------|
| 150 | ~1 分钟 | 测试环境 |
| 500 | ~3.3 分钟 | 小额通道 |
| 1500 | ~10 分钟 | 标准 |
| 4500 | ~30 分钟 | 大额通道 |
| 9000 | ~1 小时 | 高价值争议窗口 |

### 5.3 拆分面额建议

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

## 6. 数据持久化

### 6.1 sled 数据库

`ChannelManager` 使用 sled 嵌入式数据库存储所有通道状态：

| 存储路径 | 内容 |
|:---------|:-----|
| 数据库根目录 | 通道元数据（`ChannelMetadata`）、Merkle 树 |
| `htlc:{channel_id}` | HTLC 记录 |
| `compliance:{channel_id}` | 合规状态 |
| `audit:{channel_id}:{seq}` | 审计追踪 |

### 6.2 备份建议

```bash
# sled 数据目录
./data/channel_user/

# 备份（确保进程已停止或使用快照）
cp -r ./data/channel_user/ ./data/channel_user_backup/
```

> sled 数据自动持久化到磁盘，重启后通过 `ChannelManager::new(sled::open(path))` 恢复。

---

## 7. 结算操作

### 7.1 协作关闭（推荐）

双方同意当前状态，共同签名关闭：

```bash
# 通过 HTTP API
curl -X POST http://localhost:3001/v1/channels/{channel_id}/close \
  -H "Content-Type: application/json" \
  -d '{"settle_window": 10000}'
```

```rust
// 通过库调用
let sig_a = sign_state(&channel_id, sequence, &root, &user_keypair);
let sig_b = sign_state(&channel_id, sequence, &root, &provider_keypair);
// 调用链上 cooperative_settle
```

### 7.2 争议关闭

如果对方不响应：

```bash
# 通过 HTTP API 发起争议
curl -X POST http://localhost:3001/v1/channels/{channel_id}/challenge \
  -H "Content-Type: application/json" \
  -d '{"submitted_root": "hex...", "submitted_sequence": 5}'
```

### 7.3 自动关闭

如果通道设置了 `auto_close_offset`：

```rust
let state = manager.open_channel(
    // ...
    Some(current_slot + 100_000),  // auto_close_slot
)?;

// 到期后任何人可以触发结算
manager.auto_settle(&mut state, settle_window)?;
```

---

## 8. 配置参数详解

| 参数 | 类型 | 默认值 | 说明 |
|:-----|:-----|:-------|:-----|
| `server.host` | string | `"0.0.0.0"` | HTTP 监听地址 |
| `server.port` | u16 | `3001` | HTTP 监听端口 |
| `solana.rpc_url` | string | 必填 | Solana JSON RPC 端点 |
| `solana.channel_program_id` | string | 必填 | 链上通道程序 ID (Base58) |
| `solana.keypair_path` | string | `""` | Ed25519 密钥对文件路径，空则自动生成 |
| `channel.default_tree_depth` | u32 | `4` | 默认 Merkle 树深度 |
| `channel.default_challenge_duration` | u64 | `5000` | 默认争议期（slots） |
| `channel.default_min_challenge_delay` | u64 | `1000` | 最短争议延迟（slots） |
| `channel.default_settle_window` | u64 | `10000` | 默认结算窗口（slots） |
| `channel.auto_close_offset` | u64 | `500000` | 自动关闭偏移（slots），0 = 不自动关闭 |
| `channel.db_path` | string | 必填 | sled 数据库存储路径 |
| `compliance` | section | 可选 | 合规配置，不写则禁用合规 |

---

## 9. 安全检查清单

| 检查项 | 说明 | 状态 |
|:-------|:-----|:-----|
| 用户密钥安全存储 | Ed25519 私钥使用安全存储方案 | 必须 |
| 原像保密 | HTLC 原像在受益人确认前不透露 | 必须 |
| sled 数据目录权限 | 限制数据库文件访问权限 | 建议 |
| 序列号连续性 | 确保不签名低于当前序列的 LeafUpdate | 必须 |
| challenge_duration 合理 | 留足够时间响应争议 | 建议 |
| 金额守恒验证 | 拆分树前检查总金额匹配 | 必须 |
| RPC 端点安全 | 生产环境使用私有 RPC 或 HTTPS | 建议 |
| 反向代理 TLS | 生产环境通过 Nginx 启用 HTTPS | 必须 |
