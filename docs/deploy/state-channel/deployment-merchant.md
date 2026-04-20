# 状态通道商户端部署配置文档

## 1. 概述

商户端（Party B，收款方）是状态通道的服务提供者，负责接收用户支付、配签确认、管理 HTLC 原像、以及在结算时认领叶子。商户使用 `channel-provider` 二进制部署为持续运行的服务端进程。

支持两种部署方式：
- **方式一（推荐）**：通过 `ignite-pay-channel-service` 的 `channel-provider` 二进制作为独立 HTTP 服务运行
- **方式二**：通过 `ignite-pay-state-channel` 库集成到自有服务中

---

## 2. 服务部署

### 2.1 编译

```bash
cd ignite-pay-channel-service
cargo build --release --bin channel-provider
```

产物：`target/release/channel-provider`

### 2.2 生成密钥

```bash
solana-keygen new --outfile ./keys/provider.key
```

> 如果 `keypair_path` 留空，服务启动时自动生成临时密钥（仅测试用）。

### 2.3 配置文件

创建 `config-provider.toml`：

```toml
[server]
host = "0.0.0.0"        # 监听地址，生产环境建议 "127.0.0.1" + 反向代理
port = 3002              # 监听端口

[solana]
rpc_url = "https://api.devnet.solana.com"
channel_program_id = "DJBHr35jL3JAGoU7bKMsEFmpeNMrCSK7oYQE4HJ3GBUe"
keypair_path = "./keys/provider.key"

[channel]
default_tree_depth = 4
default_challenge_duration = 5000
default_min_challenge_delay = 1000
default_settle_window = 10000
auto_close_offset = 0
db_path = "./data/channel_provider"
```

> Provider 角色无需 `[compliance]` 配置段，合规由 User 端管理。

### 2.4 启动服务

```bash
# 使用默认配置文件 config-provider.toml
./channel-provider

# 指定配置文件
./channel-provider /path/to/config-provider.toml

# 启用 debug 日志
RUST_LOG=debug ./channel-provider
```

### 2.5 API 接口

#### 通用端点

| 方法 | 路径 | 说明 |
|:-----|:-----|:-----|
| GET | `/health` | 健康检查 |
| WS | `/ws` | WebSocket 连接 |

#### 通道管理端点

| 方法 | 路径 | 说明 |
|:-----|:-----|:-----|
| POST | `/v1/channels/{id}/fund` | 注资通道（商户端存款） |
| GET | `/v1/channels` | 列出通道 |
| GET | `/v1/channels/{id}` | 查询通道状态 |

#### 支付处理端点

| 方法 | 路径 | 说明 |
|:-----|:-----|:-----|
| POST | `/v1/channels/{id}/cosign` | Provider 配签 |
| POST | `/v1/channels/{id}/accept-payment` | 接受支付 |
| POST | `/v1/channels/{id}/accept-batch` | 接受批量支付 |

#### 结算端点

| 方法 | 路径 | 说明 |
|:-----|:-----|:-----|
| POST | `/v1/channels/{id}/close` | 协作关闭 |
| POST | `/v1/channels/{id}/challenge` | 发起争议 |
| POST | `/v1/channels/{id}/submit-counter` | 提交反状态 |
| POST | `/v1/channels/{id}/claim` | 认领叶子 |
| POST | `/v1/channels/{id}/finalize` | 最终结算 |

### 2.6 示例请求

```bash
# 健康检查
curl http://localhost:3002/health

# 商户注资通道
curl -X POST http://localhost:3002/v1/channels/{channel_id}/fund \
  -H "Content-Type: application/json" \
  -d '{
    "amount": 500000,
    "token_mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
  }'

# 接受支付（验证并应用用户的 LeafUpdate）
curl -X POST http://localhost:3002/v1/channels/{channel_id}/accept-payment \
  -H "Content-Type: application/json" \
  -d '{
    "leaf_update": {
      "channel_id": "hex...",
      "sequence": 5,
      "leaf_index": 2,
      "prev_leaf_hash": "hex...",
      "new_leaf": { ... },
      "signature": [64 bytes]
    }
  }'

# Provider 配签
curl -X POST http://localhost:3002/v1/channels/{channel_id}/cosign \
  -H "Content-Type: application/json" \
  -d '{
    "sequence": 5,
    "root": "hex..."
  }'

# 接受批量支付
curl -X POST http://localhost:3002/v1/channels/{channel_id}/accept-batch \
  -H "Content-Type: application/json" \
  -d '{
    "updates": [
      { "channel_id": "hex...", "sequence": 5, ... },
      { "channel_id": "hex...", "sequence": 6, ... }
    ]
  }'

# 协作关闭通道
curl -X POST http://localhost:3002/v1/channels/{channel_id}/close \
  -H "Content-Type: application/json" \
  -d '{
    "sequence": 10,
    "root": "hex...",
    "signature_a": [64 bytes],
    "signature_b": [64 bytes]
  }'

# 认领叶子
curl -X POST http://localhost:3002/v1/channels/{channel_id}/claim \
  -H "Content-Type: application/json" \
  -d '{
    "leaf_index": 1,
    "leaf_amount": 500000,
    "leaf_hash": "hex...",
    "leaf_data": "hex...",
    "leaf_owner": "商户Solana公钥",
    "proof": ["hex...", "hex...", "hex...", "hex..."],
    "claimer_signature": [64 bytes]
  }'

# 提交反状态（争议响应）
curl -X POST http://localhost:3002/v1/channels/{channel_id}/submit-counter \
  -H "Content-Type: application/json" \
  -d '{
    "sequence": 10,
    "root": "hex...",
    "signature_a": [64 bytes],
    "signature_b": [64 bytes]
  }'
```

### 2.7 WebSocket 实时通信

商户端支持 WebSocket 连接，用于实时接收用户的 LeafUpdate、配签请求和 HTLC 状态变更。

```javascript
const ws = new WebSocket('ws://localhost:3002/ws');

// 认证
ws.onopen = () => {
  const timestamp = Date.now();
  const message = `channel-ws-auth:${timestamp}`;
  const signature = await ed25519.sign(sha256(message), privateKey);

  ws.send(JSON.stringify({
    type: 'auth',
    pubkey: base58Encode(publicKey),
    signature: Array.from(signature),
    timestamp
  }));
};

ws.onmessage = (event) => {
  const msg = JSON.parse(event.data);
  switch (msg.type) {
    case 'leaf_update':
      // 处理用户的 LeafUpdate
      break;
    case 'cosign_request':
      // 响应配签请求
      break;
    case 'htlc_preimage':
      // 处理 HTLC 原像揭示
      break;
  }
};
```

详细 WebSocket 协议参见 [场景十二：WebSocket 实时通信](scenarios/12-websocket.md)。

### 2.8 systemd 服务

```ini
[Unit]
Description=Ignite Pay Channel Provider Service
After=network.target

[Service]
Type=simple
User=ignite
WorkingDirectory=/opt/ignite-pay
ExecStart=/opt/ignite-pay/channel-provider /opt/ignite-pay/config-provider.toml
Environment=RUST_LOG=info
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

### 2.9 Nginx 反向代理

```nginx
server {
    listen 443 ssl;
    server_name merchant.ignite-pay.example.com;

    ssl_certificate     /etc/ssl/certs/ignite-pay.pem;
    ssl_certificate_key /etc/ssl/private/ignite-pay.key;

    location / {
        proxy_pass http://127.0.0.1:3002;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
    }

    location /ws {
        proxy_pass http://127.0.0.1:3002;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
    }
}
```

---

## 3. 配置参数详解

| 参数 | 类型 | 说明 |
|:-----|:-----|:-----|
| `server.host` | string | HTTP 监听地址 |
| `server.port` | u16 | HTTP 监听端口（默认 3002） |
| `solana.rpc_url` | string | Solana JSON RPC 端点 |
| `solana.channel_program_id` | string | 链上通道程序 ID |
| `solana.keypair_path` | string | Ed25519 密钥对文件路径 |
| `channel.db_path` | string | sled 数据库路径 |
| `channel.default_tree_depth` | u32 | 默认 Merkle 树深度 |
| `channel.auto_close_offset` | u64 | 自动关闭偏移量（0 = 不自动关闭） |

---

## 4. 监控建议

| 指标 | 阈值 | 处理 |
|:-----|:-----|:-----|
| 活跃通道数 | 趋势变化 | 关注业务量变化 |
| 配签延迟 | > 500ms | 优化网络或节点性能 |
| 结算窗口内认领率 | < 100% | 检查认领逻辑是否及时 |
| HTLC 过期率 | > 1% | 检查原像揭示流程 |
| sled 数据库大小 | > 2 GB | 归档历史数据 |
| 支付接受失败率 | > 0.1% | 检查签名验证逻辑 |

---

## 5. DID 数字身份

### 5.1 生成 DID 密钥对

商户使用 `ignite-pay-core` 的 `identity` 模块生成 `did:ignite` 去中心化身份：

```toml
[dependencies]
ignite-pay-core = { path = "../ignite-pay-core" }
ignite-pay-state-channel = { path = "../ignite-pay-state-channel" }
solana-pubkey = "2"
solana-program = "2"
ed25519-dalek = "1"
```

```rust
use ignite_pay_core::identity::{generate_ignite_did, build_did_document, save_identity, load_did};

let db = sled::open("./merchant_data")?;

// 检查是否已有身份
let existing_did = load_did(&db)?;

let (identity, merchant_did) = match existing_did {
    Some(did) => {
        // 已有 DID，用相同的 DID 重新生成身份
        // 注意：密钥会不同，但 DID 标识符保持一致
        let identity = PrivateIdentity::generate(&did);
        (identity, did)
    }
    None => {
        // 首次生成
        let (identity, did) = generate_ignite_did();
        save_identity(&db, &identity, &did)?;
        (identity, did)
    }
};

// 构建 W3C DID Document
let did_doc = build_did_document(&merchant_did, &identity);

println!("商户 DID: {}", merchant_did);
// 输出类似: did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK
```

**DID 编码规则**：

`did:ignite:z` + Base58(`0xed 0x01` + Ed25519 公钥)

其中 `0xed 0x01` 是 multicodec 中 Ed25519 公钥的标识前缀。

**生成后获得**：
- **DID 标识符**: `did:ignite:z6Mk...`
- **Ed25519 签名私钥**: 用于签署支付请求和 DIDComm 消息（安全存储）
- **X25519 密钥协商密钥**: 用于 DIDComm 加密通信（由 Ed25519 派生）
- **Solana 收款密钥对**: 独立的 Solana 密钥对，用于接收支付

> **重要**：DID 签名密钥和 Solana 收款密钥是分离的。DID 密钥用于身份认证，Solana 收款密钥用于接收资金。

### 5.2 DID Document 结构

```json
{
  "@context": ["https://www.w3.org/ns/did/v1"],
  "id": "did:ignite:z6MkhaXgBZ...",
  "verificationMethod": [{
    "id": "did:ignite:z6MkhaXgBZ...#key-signing-1",
    "type": "Ed25519VerificationKey2020",
    "controller": "did:ignite:z6MkhaXgBZ...",
    "publicKeyMultibase": "z6MkhaXgBZ..."
  }],
  "keyAgreement": [{
    "id": "did:ignite:z6MkhaXgBZ...#key-agreement-1",
    "type": "X25519KeyAgreementKey2020",
    "controller": "did:ignite:z6MkhaXgBZ...",
    "publicKeyBase64": "base64-encoded-x25519-public-key"
  }]
}
```

### 5.3 申请平台背书 (VC)

向 Ignite Pay 平台提交以下信息：

| 字段 | 说明 |
|:-----|:-----|
| `merchant_did` | 商户 DID 标识符 |
| `name` | 商户名称 |
| `category` | 商户类别（如 SaaS, API, Content） |
| `service_endpoint` | 商户服务 URL |
| `solana_pubkey` | Solana 收款公钥 |

平台审核后使用 `ignite-pay-core` 的 `vc` 模块签发 Verifiable Credential：

```rust
use ignite_pay_core::vc::VerifiableCredential;
use ed25519_dalek::SigningKey;

// 平台侧签发 VC
let vc = VerifiableCredential::sign(
    vec!["https://www.w3.org/2018/credentials/v1".to_string()],
    "vc:ignite:merchant:001".to_string(),
    vec!["VerifiableCredential".to_string(), "MerchantAttestation".to_string()],
    platform_did.clone(),                          // 签发者：平台 DID
    chrono::Utc::now() - chrono::Duration::hours(1),
    chrono::Utc::now() + chrono::Duration::days(365),
    merchant_did.clone(),                          // 主体：商户 DID
    "Example Merchant".to_string(),
    Some("SaaS".to_string()),
    &platform_signing_key,                         // 平台签名私钥
    &format!("{}#key-signing-1", platform_did),
);
```

生成的 VC 格式：

```json
{
  "@context": ["https://www.w3.org/2018/credentials/v1"],
  "type": ["VerifiableCredential", "MerchantAttestation"],
  "issuer": "did:ignite:zPlatformDID...",
  "issuanceDate": "2025-01-01T00:00:00Z",
  "expirationDate": "2026-01-01T00:00:00Z",
  "credentialSubject": {
    "id": "did:ignite:z6MkMerchant...",
    "name": "Example Merchant",
    "category": "SaaS"
  },
  "proof": {
    "type": "Ed25519Signature2020",
    "verificationMethod": "did:ignite:zPlatformDID...#key-signing-1",
    "proofValue": "base64-signature..."
  }
}
```

### 5.4 链上注册（SPL Account Compression）

平台将商户信息压缩上链，使用 `ignite-pay-core` 的 `SolanaDidBridge`（需启用 `solana` feature）：

```rust
use ignite_pay_core::solana_did::SolanaDidBridge;

// 平台侧：将商户注册到链上 Merkle Tree
// MerchantLeaf {
//     merchant_did_hash: SHA-256(商户 DID),
//     active_pubkey: Solana 收款地址,
//     platform_vc_hash: SHA-256(canonical_json(VC)),
//     status: 0,  // 0=active
//     slot_updated: current_slot
// }
```

链上参数（由平台一次性部署）：

| 参数 | 值 | 说明 |
|:-----|:---|:-----|
| Merkle Tree 地址 | Solana Pubkey | ConcurrentMerkleTree 账户 |
| Tree Authority | Solana Pubkey | 平台控制密钥 |
| maxDepth | 14 | 支持 ~16K 商家 |
| maxBufferSize | 64 | 并发更新缓冲区 |
| DAS API | Helius 端点 | 用于查询 Merkle Proof |

### 5.5 DID 持久化

```rust
use ignite_pay_core::identity::{save_identity, load_did};

// 保存到 sled
save_identity(&db, &identity, &merchant_did)?;

// 重启后加载
let did = load_did(&db)?;
```

> **注意**：当前 `PrivateIdentity` 的种子无法直接提取，因此 `load_did` 仅恢复 DID 字符串。密钥会在重启时重新生成（DID 不变）。生产环境需额外实现密钥的安全持久化。

---

## 6. 角色与职责

| 职责 | 说明 |
|:-----|:-----|
| DID 身份管理 | 生成/持久化 did:ignite 身份，维护 DID Document |
| 接收支付 | 接收用户签名的 LeafUpdate，配签确认 |
| HTLC 管理 | 生成原像、在服务交付后揭示原像 |
| Provider 配签 | 对用户的 LeafUpdate 和 SignedState 进行 Ed25519 签名 |
| 结算认领 | 在结算窗口内提交 Merkle Proof 认领自己的叶子 |
| 争议响应 | 收到 challenge 后在窗口内提交 counter_state |
| VC 续签 | 定期检查 VC 有效期，到期前向平台申请续签 |

---

## 7. 通道集成（库集成模式）

### 7.1 初始化 ChannelManager

```rust
use ignite_pay_state_channel::channel::ChannelManager;
use ignite_pay_state_channel::signing::{generate_keypair, to_pubkey};

let db = sled::open("./merchant_channel_data")?;
let manager = ChannelManager::new(db)?;

// 商户通道密钥对（独立于 DID 密钥）
let provider_keypair = generate_keypair();
let provider_pubkey = to_pubkey(&provider_keypair);
```

### 7.2 加载通道

当用户开通通道后，商户从链上事件或通信协议获取 `channel_id`，加载通道状态：

```rust
let channel_id: [u8; 32] = /* 从链上或通信获取 */;
let state = manager.load_state(&channel_id)?;

println!("通道状态: {:?}", state.metadata.status);
println!("用户存款: {}", state.metadata.deposit_a);
```

### 7.3 Provider 配资（可选）

如果通道需要双端注资，商户可以注入资金：

```rust
let update = manager.fund_channel(
    &mut state,
    &provider_keypair,
    500_000,        // 商户注资金额
    None,           // 自动选择空槽位
)?;

// update 是签名的 LeafUpdate，需提交到链上 fund_channel 指令
```

---

## 8. 处理用户支付（库集成模式）

### 8.1 接收 LeafUpdate

商户接收用户发送的 LeafUpdate，验证签名后应用：

```rust
use ignite_pay_state_channel::signing::verify_leaf_update_signature;

// 验证签名
if !verify_leaf_update_signature(&leaf_update, &state.metadata.user_pubkey) {
    return Err("Invalid user signature");
}

// 应用更新
manager.apply_leaf_update(&mut state, &leaf_update, &state.metadata.user_pubkey)?;
```

### 8.2 Provider 配签

商户对更新后的状态进行配签，表示同意新状态：

```rust
let cosignature = manager.provider_cosign_state(
    &mut state,
    &provider_keypair,
)?;

// cosignature 是商户的 Ed25519 签名
// 返回给用户作为确认
```

### 8.3 批量更新处理

```rust
// 用户可能一次发送多个 LeafUpdate
let updates: Vec<LeafUpdate> = /* 从通信获取 */;

let result = manager.apply_leaf_update_batch(
    &mut state,
    &updates,
    &state.metadata.user_pubkey,
)?;

// 如果批量中间某条失败，result 为 Err(BatchFailureInfo)
// 已经应用的更新不会自动回滚（需要协作处理）
```

---

## 9. HTLC 管理（库集成模式）

### 9.1 服务完成后揭示原像

当商户提供完服务后，需要揭示 HTLC 原像来完成支付：

```rust
use ignite_pay_state_channel::htlc::HtlcManager;

let mut htlc_mgr = HtlcManager::with_db(db.clone(), channel_id);

// 商户持有原像（从用户处获取 hash_lock，服务完成后用户揭示原像）
// 或者：商户生成原像

// 方式 A：用户创建 HTLC，商户等待原像
// 用户发送 hash_lock → 商户验证 → 服务完成后用户揭示 preimage

// 方式 B：商户生成原像
let (hash_lock, preimage) = htlc_mgr.create_htlc(
    100_000,           // 金额
    leaf_index,        // 叶子索引
    user_pubkey,       // 所有者（用户锁定资金）
    provider_pubkey,   // 受益人（商户）
    current_slot,
    500,               // 持续时间
);

// 将 hash_lock 发送给用户（用户创建 HTLC 叶子）
// 服务完成后揭示原像
htlc_mgr.reveal_preimage(&hash_lock, &preimage)?;
```

### 9.2 检查过期

```rust
// 定期检查 HTLC 是否过期
let expired = htlc_mgr.check_expiry(current_slot);
for hash_lock in &expired {
    htlc_mgr.mark_refunded(hash_lock)?;
}
```

### 9.3 HTLC 生命周期

```
Pending → (原像揭示) → Revealed → (链上解决) → Fulfilled
Pending → (超时) → Expired → (退款) → Refunded
```

---

## 10. 结算操作（库集成模式）

### 10.1 认领叶子

在结算窗口内，商户提交 Merkle Proof 认领属于自己的 UTXO：

```rust
use ignite_pay_state_channel::signing::claim_message;

// 获取商户拥有的叶子
let leaf_index = 1;  // 商户拥有的叶子索引
let leaf = state.tree.get_leaf(leaf_index)?;

// 生成 Merkle Proof
let proof = state.tree.proof(leaf_index)?;

// 构造链上 claim 调用参数
let claim_amount = leaf.amount;
let leaf_hash = leaf.hash();
let leaf_data = borsh::to_vec(leaf)?;
let leaf_owner = leaf.owner;  // 应为 provider_pubkey

// 签名（链下辅助函数，链上验证使用 channel_id || current_slot || current_root）
let claim_msg = claim_message(&channel_id, leaf_index as u32, claim_amount, current_slot);
let signature = provider_keypair.sign(&claim_msg);
```

### 10.2 HTLC 认领

如果叶子是 HTLC 类型，使用链上 `verify_htlc` 指令：

```rust
// 需要提供：
// - leaf_index
// - preimage（32 字节）
// - hash_lock
// - leaf_amount
// - beneficiary（应为 provider_pubkey）
// - leaf_hash + Merkle proof
// - timelock_slot（必须 >= current_slot）
// - leaf_data
// - claimer_signature
```

> **截止时间**：在 `Challenged` 状态下，截止时间为 `challenge_slot + challenge_duration`（`settle_deadline` 为 None）；在 `Settling` 状态下，使用 `settle_deadline`。

### 10.3 HTLC 退款

如果 HTLC 过期，商户不认领（资金退回用户），或用户使用 `htlc_refund` 指令：

```rust
// 需要：timelock_slot < current_slot
// 用户提交 htlc_refund 指令，资金返回 leaf.owner
```

### 10.4 最终结算

结算窗口结束后，任何一方调用 `finalize_settlement`：

```rust
// 链上操作：
// - 计算未认领余额
// - 按 deposit_a / deposit_b 比例分配
// - 将剩余资金分别转入 vault_a 和 vault_b
// - 关闭通道
```

---

## 11. 合规支持（库集成模式）

### 11.1 消费限额

如果通道启用了合规管理：

```rust
use ignite_pay_state_channel::compliance::{ComplianceManager, SpendingLimit};

let compliance = ComplianceManager::new(db.clone())?;

compliance.init_channel_compliance(channel_id, SpendingLimit {
    threshold: 1_000_000,     // 累计消费阈值
    per_channel: 2_000_000,   // 单通道最大支付
    window_slots: 1000,       // 滑动窗口（slots）
})?;

// 每次支付后记录
let action = compliance.record_payment(
    channel_id,
    payment_amount,
    current_slot,
    user_pubkey,
    provider_pubkey,
)?;

match action {
    ComplianceAction::None => { /* 正常 */ }
    ComplianceAction::InsertMarker { compliance_hash, threshold } => {
        // 触发合规审查，需要插入合规标记叶子
    }
}
```

### 11.2 审计追踪

```rust
// 记录每次 LeafUpdate
compliance.record_audit(&leaf_update)?;

// 查询通道完整审计
let trail = compliance.get_audit_trail(channel_id)?;
for update in &trail {
    println!("seq={} leaf_idx={} amount={}",
        update.sequence, update.leaf_index, update.new_leaf.amount);
}
```

---

## 12. 密钥轮换

当商户需要更换 Solana 收款地址时：

1. 使用 DID 签名私钥签署新收款地址声明
2. 提交给平台
3. 平台验证 DID 签名后，调用 `replace_leaf` 更新链上叶子节点
4. `merchant_did_hash` 不变，DID 标识符不变
5. 新的收款地址在下一个 slot 生效

> DID 标识符和签名密钥不变，仅更换收款地址，确保业务连续性。

---

## 13. 多通道管理

商户通常同时维护与多个用户的通道：

```rust
// 使用 sled 前缀管理多通道
let db = sled::open("./merchant_channels")?;
let manager = ChannelManager::new(db)?;

// 加载特定通道
let channel_id = /* ... */;
let state = manager.load_state(&channel_id)?;

// 管理多个 HTLC（每个通道独立）
let htlc_mgr = HtlcManager::with_db(db.clone(), channel_id_1);
let htlc_mgr_2 = HtlcManager::with_db(db.clone(), channel_id_2);
```

---

## 14. 安全检查清单

| 检查项 | 说明 | 状态 |
|:-------|:-----|:-----|
| DID 签名私钥安全存储 | 使用 HSM 或密钥管理服务 | 必须 |
| Solana 收款私钥安全存储 | 定期检查余额并转出到冷钱包 | 建议 |
| VC 有效期检查 | 定期续签平台 VC | 运维注意 |
| 原像管理 | 原像仅在服务确认后揭示 | 必须 |
| LeafUpdate 验证 | 每次配签前验证用户签名 | 必须 |
| 序列号检查 | 只接受 sequence > 当前值的更新 | 必须 |
| 金额守恒 | 验证每次更新后总金额不变 | 必须 |
| 结算窗口监控 | 及时在窗口内认领叶子 | 必须 |
| HTLC 超时处理 | 定期检查过期 HTLC 并退款 | 建议 |
| 审计完整性 | 记录所有 LeafUpdate | 建议 |
| 收款地址监控 | 监控链上收款地址，及时发现异常交易 | 建议 |
