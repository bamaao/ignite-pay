# Ignite Pay 日志存储方案

## 现状

| 组件 | 当前做法 | 问题 |
|------|---------|------|
| `ignite-pay-mcp` | `tracing` → stderr，受 `RUST_LOG` 控制 | 进程结束即丢失，无持久化 |
| `ignite-pay-skill` | `tracing` → stderr | 同上 |
| `didcomm-router` | `tracing` → stderr | 同上 |
| `ignite_pay_app` (Dart) | `debugPrint` | release 构建被编译器剥离，零日志 |
| `ignite-pay-state-channel` | 仅 `tracing::warn!` 记录 HTLC 持久化失败 | 无结构化审计日志 |

**结论：系统当前没有任何日志持久化能力。**

---

## 设计目标

1. **金融级审计** — 支付全流程可追溯，满足争议仲裁需求
2. **隐私保护** — 用户交易明细端到端加密，服务端/云端无法读取
3. **抗篡改** — 哈希链 + Merkle 根，任何删除/修改可被检测
4. **跨设备恢复** — 用户换机后可从云端拉取密文日志重建本地记录
5. **低成本** — 热数据本地存储，冷数据压缩后上云，Merkle 根上链

---

## 存储架构：三层分离

```
┌──────────────────────────────────────────────────────┐
│                   L3 冷归档（链上）                      │
│         Merkle Root → Solana Program Log              │
│         或 Celestia DA Layer                          │
│         保留期限：永久                                  │
├──────────────────────────────────────────────────────┤
│                   L2 温存储（云端）                      │
│         E2EE 加密 LogChunk → IPFS (content-addressed)  │
│         索引：ChunkManifest CID（用户只需记住一个 CID）    │
│         保留期限：永久（IPFS pinning）                    │
├──────────────────────────────────────────────────────┤
│                   L1 热存储（本地）                      │
│         Phone: SQLite                                 │
│         MCP/Skill: sled                               │
│         明文/轻量加密                                   │
│         保留期限：7~15 天                               │
└──────────────────────────────────────────────────────┘
```

---

## 数据结构定义

### LogChunk（Protobuf）

```protobuf
syntax = "proto3";

package ignite_pay.audit.v1;

// 存储和同步的最小单位
message LogChunk {
    // 元数据区（明文，云端索引用）
    ChunkMetadata metadata = 1;

    // 数据区（AES-256-GCM 加密后的 EncryptedPayload）
    bytes encrypted_payload = 2;

    // GCM 认证标签
    bytes auth_tag = 3;

    // 该 Chunk 内所有交易的 Merkle 根
    bytes merkle_root = 4;
}

message ChunkMetadata {
    string user_did = 1;           // 用户 DID
    string provider_did = 2;       // 服务商 DID（MCP）
    uint64 chunk_id = 3;           // 单调递增序号
    uint64 start_nonce = 4;        // 起始 Nonce
    uint64 end_nonce = 5;          // 结束 Nonce
    bytes  prev_chunk_hash = 6;    // 前一块 SHA-256，形成哈希链
    int64  timestamp_start = 7;    // 块起始时间戳
    int64  timestamp_end = 8;      // 块结束时间戳
}

// 密文内部结构
message EncryptedPayload {
    repeated TransactionEntry entries = 1;
}

message TransactionEntry {
    uint64 nonce = 1;               // 全局单调递增
    int64  delta_amount = 2;        // 变动额（lamports，正=支出，负=退款）
    uint64 cumulative_amount = 3;   // 累计支出总额
    bytes  signature = 4;           // 服务商签名（仲裁证据）
    int64  timestamp = 5;           // 交易时间戳
    string service_id = 6;          // 服务标识（如 API 路径）
    string payment_id = 7;          // 支付 ID（关联 auth-request）
    string merchant_did = 8;        // 商户 DID
    bytes  memo = 9;                // 可选备注
}
```

### ChunkManifest（Protobuf）— IPFS 索引

```protobuf
// Manifest entry mapping a chunk to its IPFS CID
message ChunkManifestEntry {
    uint64 chunk_id = 1;
    string cid = 2;               // IPFS CID of the LogChunk
    bytes chunk_hash = 3;          // SHA-256 of serialized LogChunk
    bytes merkle_root = 4;         // Allows verification without downloading
}

// Manifest tracking all chunks for a user on IPFS
message ChunkManifest {
    string user_did = 1;
    repeated ChunkManifestEntry entries = 2;
    bytes prev_manifest_hash = 3;  // Hash chain for manifest integrity
}
```

用户只需记住一个 manifest CID，即可定位所有 chunk。

### IPFS 同步流程

```
Phone (LocalLogStore)
       │
       ▼
  获取 unsynced entries
       │
       ▼
  build_chunk() → LogChunk (加密)
       │
       ▼
  upload_chunk(ipfs, chunk) → CID
       │
       ▼
  add_manifest_entry(manifest, chunk_id, cid, chunk)
       │
       ▼
  upload_manifest(ipfs, manifest) → manifest_cid
       │
       ▼
  mark_synced(end_nonce)
```

### IPFS 恢复流程

```
新设备                                    IPFS
  │                                        │
  │  1. download_manifest(manifest_cid)    │
  │ ──────────────────────────────────────>│
  │ <──────────────────────────────────────│  ChunkManifest
  │                                        │
  │  2. 按 chunk_id 排序 entries           │
  │                                        │
  │  3. 逐个 download_chunk(cid)           │
  │ ──────────────────────────────────────>│
  │ <──────────────────────────────────────│  LogChunk (加密)
  │                                        │
  │  4. 验证 chunk_hash 与 manifest 一致    │
  │  5. 验证哈希链 prev_chunk_hash          │
  │  6. decrypt_chunk() → TransactionEntry │
  │  7. 写入本地 SQLite                     │
  │  8. 重复 3-7 直到完成                   │
```

```
TransactionEntry[]
       │
       ▼
  构建 Merkle Tree ──→ merkle_root
       │
       ▼
  序列化 EncryptedPayload (Protobuf)
       │
       ▼
  Zstd 压缩（预计 5~10x 压缩率）
       │
       ▼
  AES-256-GCM 加密（密钥由用户 DID 私钥派生）
       │
       ▼
  组装 LogChunk { metadata, encrypted_payload, auth_tag, merkle_root }
```

---

## 各组件实现方案

### 1. Phone 端（Flutter + Rust）

**L1 本地存储：SQLite**

```rust
// ignite_pay_app/rust/src/api/log_store.rs

/// 本地日志存储（SQLite，L1 热层）
pub struct LocalLogStore {
    db: rusqlite::Connection,
    next_nonce: u64,
}

impl LocalLogStore {
    pub fn open(path: &str) -> Result<Self> { ... }

    /// 记录一笔交易
    pub fn record_transaction(&self, entry: &TransactionEntry) -> Result<()> { ... }

    /// 查询最近 N 笔交易
    pub fn recent_transactions(&self, limit: usize) -> Result<Vec<TransactionEntry>> { ... }

    /// 获取当前累计支出
    pub fn cumulative_spending(&self) -> Result<u64> { ... }

    /// 导出未上云的条目，用于构建 LogChunk
    pub fn unsynced_entries(&self, since_chunk_id: u64) -> Result<Vec<TransactionEntry>> { ... }

    /// 标记条目已同步到 L2
    pub fn mark_synced(&self, up_to_nonce: u64) -> Result<()> { ... }
}
```

**日志触发点（在现有代码中插入）：**

| 触发位置 | 文件 | 记录内容 |
|---------|------|---------|
| 授权请求到达 | `didcomm_service.dart` → `_decryptAndProcess` | `payment_id`, `merchant_did`, `amount` |
| 授权响应发送 | `didcomm_service.dart` → `sendAuthResponse` | `authorized`, `list_action` |
| Session key 创建 | `didcomm_service.dart` → `sendAuthResponseWithSessionKey` | `spending_limit`, `duration` |
| 配对完成 | `didcomm_service.dart` → `parseInvitationAndConnect` | `mcp_did`, `mediator_ws_url` |
| WS/FCM 连接/断开 | `didcomm_service.dart` → `connectToMediator` / `disconnect` | 连接状态变更 |

**L2 上传（后台任务）：**

```dart
// 每 100 笔交易或每 1 小时触发一次
Future<void> _syncLogChunk() async {
    final entries = await rust.getUnsyncedLogEntries(limit: 100);
    if (entries.isEmpty) return;

    // Rust 侧完成：构建 Merkle → 压缩 → 加密 → 组装 LogChunk
    final chunk = await rust.buildLogChunk(entries: entries);

    // 上传到云端（仅传密文）
    await _cloudStorage.upload(chunk.metadata, chunk.encryptedPayload);

    // 标记本地已同步
    await rust.markLogSynced(upToNonce: chunk.endNonce);
}
```

### 2. MCP 服务端（Rust）

**L1 本地存储：sled（已有基础设施）**

```rust
// ignite-pay-mcp/src/audit.rs

/// 服务端审计日志（sled，L1 热层）
pub struct AuditLogStore {
    db: sled::Db,
}

impl AuditLogStore {
    /// 记录支付事件（不记录用户敏感信息，仅记录通道级 State Diff）
    pub fn record_state_diff(
        &self,
        channel_id: &str,
        batch_id: &str,
        delta: u64,
        merkle_root: &[u8; 32],
    ) -> Result<()> { ... }

    /// 记录授权请求/响应事件
    pub fn record_auth_event(
        &self,
        payment_id: &str,
        event_type: &str,  // "auth_request_sent" | "auth_response_received"
        metadata: &Value,
    ) -> Result<()> { ... }

    /// 查询支付历史
    pub fn query_payments(&self, from: i64, to: i64, limit: usize) -> Result<Vec<AuditEntry>> { ... }
}
```

**日志触发点：**

| 触发位置 | 文件 | 记录内容 |
|---------|------|---------|
| 收到 402 挑战 | `main.rs` → `process_x402_challenge` | `merchant_did`, `amount`, `challenge_body` hash |
| 发送授权请求 | `mediator.rs` → `send_auth_request` | `payment_id`, `phone_did`, `merchant_did` |
| 收到授权响应 | `mediator.rs` → `process_inner_message` | `authorized`, `list_action`, `session_key` presence |
| 执行链上支付 | `main.rs` → `execute_solana_payment` | `tx_signature`, `slot`, `amount` |
| 白名单变更 | `main.rs` → `handle_list_action` | `list_type`, `action`, `merchant_did` |
| 配对请求 | `mediator.rs` → `process_inner_message` | `phone_did`, `push_channel` |

**日志格式（结构化 JSON）：**

```json
{
  "ts": "2025-01-15T10:30:00Z",
  "level": "info",
  "event": "auth_request_sent",
  "payment_id": "pay_abc123",
  "merchant_did": "did:ignite:zMerchant...",
  "phone_did": "did:ignite:zPhone...",
  "amount": 500000000,
  "correlation_id": "req_xyz"
}
```

### 3. DIDComm Router（Mediator）

Router 不记录消息内容（仅做密文中继），记录操作日志：

```rust
// 结构化 tracing 日志，输出到 stderr + 可选文件
tracing::info!(
    recipient = %did,
    msg_count = messages.len(),
    "messages_queued"
);
```

建议增加文件输出：

```rust
// didcomm-router/src/main.rs
let log_file = std::fs::File::create("logs/router.log")?;
tracing_subscriber::fmt()
    .with_writer(std::io::stderr.and(log_file))
    .with_rolling_file_appender(Rotation::DAILY, "logs", "router.log")
    .init();
```

---

## 哈希链与 Merkle 对账

### 哈希链（防删除）

每个 `LogChunk` 的 `metadata.prev_chunk_hash` 指向前一块的 SHA-256。

```
Chunk#0 ← prev_hash=0x00..00
Chunk#1 ← prev_hash=SHA256(Chunk#0)
Chunk#2 ← prev_hash=SHA256(Chunk#1)
...
```

同步时校验：如果 `SHA256(Chunk#N) != Chunk#N+1.prev_chunk_hash`，则检测到数据被篡改或删除。

### Merkle 对账（服务端 vs 用户端）

```
用户端:  Transaction[] → Merkle Tree → merkle_root_user
服务端:  StateDiff[]   → Merkle Tree → merkle_root_server

验证: merkle_root_user == merkle_root_server
```

现有代码基础：`ignite-pay-state-channel/src/merkle.rs` 已实现完整的 Merkle 树（构造、增量更新、证明生成/验证），可直接复用。

### 链上锚定（L3）

将 `merkle_root` 作为 Solana 自定义指令的一个账户数据字段写入，或发布到 Celestia DA 层。

现有代码基础：`ignite-pay-solana/src/compression.rs` 已有 SPL Concurrent Merkle Tree 交互能力。

---

## 密钥派生方案

用户端加密密钥由 DID 私钥派生，无需额外密钥管理：

```
Ed25519 Signing Private Key (32 bytes)
    │
    ▼  HKDF-SHA256(salt="ignite-pay-log-v1", info=user_did)
    │
    ▼
AES-256-GCM Key (32 bytes)
```

```rust
// ignite-pay-core/src/log_crypto.rs (新增)

use hkdf::Hkdf;
use sha2::Sha256;

/// 从 Ed25519 签名私钥派生 AES-256-GCM 加密密钥
pub fn derive_log_key(signing_private: &[u8; 32], user_did: &str) -> [u8; 32] {
    let hk = Hkdf::<Sha256>::new(
        Some(b"ignite-pay-log-v1"),
        signing_private,
    );
    let mut key = [0u8; 32];
    hk.expand(user_did.as_bytes(), &mut key).expect("32 bytes");
    key
}
```

---

## 跨设备恢复流程

```
新设备                        IPFS                         旧设备
  │                            │                             │
  │  1. 用户输入 DID + 私钥     │                             │
  │    + manifest CID          │                             │
  │                            │                             │
  │  2. download_manifest(cid) │                             │
  │ ──────────────────────────>│                             │
  │ <──────────────────────────│                             │
  │  (ChunkManifest)           │                             │
  │                            │                             │
  │  3. 按 chunk_id 排序       │                             │
  │                            │                             │
  │  4. download_chunk(cid)    │                             │
  │ ──────────────────────────>│                             │
  │  5. 返回加密 Chunk          │                             │
  │ <──────────────────────────│                             │
  │                            │                             │
  │  6. 验证 chunk_hash        │                             │
  │  7. 派生密钥 → 解密         │                             │
  │  8. 校验哈希链完整性        │                             │
  │  9. 校验 Merkle 根          │                             │
  │ 10. 写入本地 SQLite         │                             │
  │ 11. 重复 4-10 直到完成      │                             │
```

---

## 需要新增的依赖

### ignite-pay-core / Cargo.toml

```toml
# 日志存储
prost = "0.13"              # Protobuf 运行时
prost-types = "0.13"        # protobuf well-known types
hkdf = "0.12"               # 密钥派生
aes-gcm = "0.10"            # AES-256-GCM 加密
zstd = "0.13"               # 压缩
sha2 = "0.10"               # SHA-256（已有，确认版本）
```

### ignite_pay_app / rust / Cargo.toml

```toml
rusqlite = { version = "0.31", features = ["bundled"] }  # SQLite
```

### ignite-pay-mcp / Cargo.toml

```toml
tracing-appender = "0.2"    # 文件日志轮转
```

---

## 实施路线

### Phase 1：基础日志持久化

1. **MCP 审计日志** — 新增 `audit.rs`，sled 存储，在现有 `tracing::info!` 触发点旁插入结构化写入
2. **Phone 本地日志** — 新增 `log_store.rs` (SQLite)，在 Dart 侧关键操作点插入 Rust 桥接调用
3. **Router 文件日志** — 添加 `tracing-appender` 文件输出

### Phase 2：E2EE 日志流

4. **Protobuf 定义** — 新增 `proto/audit.proto`，`build.rs` 生成 Rust 代码
5. **密钥派生** — 新增 `log_crypto.rs`，HKDF 从 DID 私钥派生 AES 密钥
6. **Chunk 构建** — 新增 `log_chunk.rs`，实现 Merkle 构建 → Zstd 压缩 → AES 加密 → 哈希链

### Phase 3：IPFS 云端同步

7. **IPFS 上传** — 加密 LogChunk 上传 IPFS 获得 CID，用 `ChunkManifest` 记录映射
8. **跨设备恢复** — 从 manifest CID → 拉取 manifest → 逐个拉取 chunk → 解密 → 验证哈希链
9. **链上锚定** — MCP 定期将 Merkle 根写入 Solana 程序日志或 Celestia

---

## 与现有代码的关系

| 设计要素 | 现有代码基础 | 需新增 |
|---------|------------|--------|
| 本地存储 | `sled` (MCP), `SharedPreferences` (Phone) | `rusqlite` (Phone SQLite) |
| Merkle 树 | `state-channel/src/merkle.rs` 完整实现 | 复用，封装为 `audit_merkle` |
| 加密 | DIDComm authcrypt (`affinidi_messaging_didcomm`) | AES-256-GCM (独立于 DIDComm) |
| 哈希链 | 无 | `prev_chunk_hash` 逻辑 |
| 压缩 | 无 | `zstd` crate |
| IPFS | `ipfs.rs` trait + `KuboIpfsClient` | 可选：L2 走 IPFS 而非 S3 |
| 链上写入 | `compression.rs` SPL Merkle Tree | Merkle 根锚定逻辑 |
| 合规审计 | `compliance.rs` audit trail | 扩展为全局日志格式 |
| 白名单/黑名单 | `list_store.rs` sled 实现 | 日志记录白名单变更事件 |
| Protobuf | 无 | `prost` + `.proto` 文件 |

---

## 安全考量

1. **密钥隔离** — 日志加密密钥与 DIDComm 通信密钥分离（通过 HKDF salt 区分）
2. **云端零知识** — 云端仅存储 `{metadata, encrypted_payload}`，无能力解密
3. **前向安全** — 每个 Chunk 使用独立 IV/nonce，即使单个密钥泄露也不影响其他 Chunk
4. **Release 构建日志** — Dart 侧替换 `debugPrint` 为持久化日志（`debugPrint` 在 release 被剥离）
5. **日志自动清理** — L1 达到 500MB 上限后，将已同步数据替换为 Merkle 根摘要，释放空间
