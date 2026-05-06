# 技术架构文档：ZK Compression、DID 上链与 IPFS 存储

## 1. 概述

Ignite-Pay 系统采用三层去中心化存储架构：

| 层次 | 技术 | 用途 |
|------|------|------|
| **链上压缩层** | ZK Compression (Light Protocol) | 商家 DID 压缩存储，链上验证 |
| **链下身份层** | did:ignite 方法 + VC | 去中心化身份标识与可验证凭证 |
| **分布式存储层** | IPFS (Kubo) | VC 存储、策略列表同步、审计日志备份 |

---

## 2. ZK Compression 的作用

### 2.1 核心定位

ZK Compression（基于 Light Protocol）**不是可选的优化层**，而是商家 DID 链上存储的**唯一机制**。商家 DID 数据从不以传统 Solana 账户形式存在，而是以压缩账户哈希存储在 Light Protocol 的 Merkle 树中。

### 2.2 存储模型

```
ConcurrentMerkleTree (State Tree)
└── Leaf: Hash(MerchantCompressedDid)
    ├── original_pk    — 身份锚点（不可变）
    ├── controller_pk  — 控制器密钥（可轮换）
    ├── recovery_pk    — 恢复密钥
    ├── vc_hash        — 平台 VC SHA-256 哈希
    ├── last_updated   — 最后更新时间戳
    └── nonce          — 防重放计数器
```

结构体定义在 `ignite-pay-did-program/src/state.rs`，约 150 字节，以哈希形式存入 Merkle 树叶节点。

### 2.3 关键技术要素

**确定性寻址**：
```
compressed_address = derive_address([b"merchant-did", original_pk], address_tree, program_id)
```
每个商家有且仅有一个压缩地址，由原始公钥推导。

**Light System Program CPI**：所有链上写操作（初始化、更新 VC、设置恢复密钥、恢复控制器）都通过 Light System Program CPI 写入 Merkle 树。

**有效性证明 (Validity Proof)**：每次变异操作需要从 Photon RPC（Light Protocol 索引器服务）获取 ZK 有效性证明，嵌入指令数据中，由 Light System Program 在 CPI 执行时验证。

### 2.4 优势

- **无租金豁免**：压缩账户无需传统 Solana 租金押金
- **大规模扩展**：单棵 Merkle 树可存储数千商家 DID
- **低交易成本**：远低于创建独立链上账户
- **隐私保护**：链上仅存哈希，完整 VC 数据在链下

---

## 3. DID 上链流程

### 3.1 did:ignite DID 方法

DID 本身在本地生成，无需链上注册：
- 格式：`did:ignite:z<multibase-base58btc>`
- 编码内容：`0xed 0x01`（multicodec Ed25519 前缀）+ 32 字节 Ed25519 公钥
- 示例：`did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK`

### 3.2 三层密钥架构

| 密钥 | 用途 | 可变性 |
|------|------|--------|
| **Original Key** (`original_pk`) | 身份锚点，PDA 推导种子 | 不可变 |
| **Controller Key** (`controller_pk`) | 签署日常操作（更新 VC、设置恢复密钥） | 可通过 `rotate-key` 轮换 |
| **Recovery Key** (`recovery_pk`) | 灾难恢复——可重置控制器 | 通过 `set_recovery_key` 设置 |

### 3.3 端到端注册流程

```
步骤 1：商家本地生成身份
    生成 Ed25519 密钥对 → 推导 did:ignite:z... 标识符

步骤 2：向平台申请 VC
    GET /v1/auth/nonce → 获取防重放 nonce（5 分钟 TTL）
    商家签名 "issue_vc:{did}:{merchant_name}:{nonce}"
    POST /v1/vc/issue → 平台验证 DID 所有权，签发 W3C VC

步骤 3：链上注册压缩 DID
    商家签名 "register:{did}:{pubkey}:{vc_hash}:{nonce}"
    POST /v1/merchants/register → 平台签署 (credential_subject_pk || vc_hash)
    平台通过 Light System Program CPI 创建压缩账户

步骤 4（SelfOnchain 模式可选）
    POST /v1/proof → 商家获取 ZK 证明
    商家自行构建、签名、广播交易
    POST /v1/merchants/confirm → 商家通知平台
```

### 3.4 链上验证（Solana 程序内）

`initialize_did` 指令执行三重验证：

1. **主体绑定**：`credential_subject_pk == signer.key()` — 确保交易提交者是 VC 主体
2. **平台签名验证**：`verify(platform_pubkey, credential_subject_pk || vc_hash, platform_signature)` — 证明平台授权此绑定
3. **确定性地址推导**：确保每个商家有且仅有一个地址

### 3.5 双重上链模式

| 模式 | 说明 |
|------|------|
| **Sponsored**（默认） | 平台签署并发送交易，记录服务费 |
| **SelfOnchain** | 平台构建未签名交易，商家自行签名广播 |

### 3.6 桥接层

`SolanaDidBridge`（`ignite-pay-core/src/solana_did.rs`）连接核心身份模块与 Solana 压缩层：

```
SolanaDidBridge.quick_verify():
    1. 从 did:ignite:z... 标识符提取 Ed25519 公钥
    2. 通过 DidService::derive_compressed_address 推导压缩 PDA 地址
    3. 查询 Photon API getCompressedAccount 确认压缩账户存在
```

---

## 4. IPFS 的作用

### 4.1 三大用途

IPFS 在系统中承担三个独立但互补的功能：

| 用途 | 数据类型 | 消费者 |
|------|----------|--------|
| VC 存储与解析 | 可验证凭证 JSON | MCP 服务器（支付验证时） |
| 策略列表同步 | 白名单/黑名单 JSON | 手机 App（跨设备同步） |
| 审计日志备份 | 加密日志 protobuf | 手机 App（设备迁移/恢复） |

### 4.2 VC 存储与解析

**存储**：平台签发的 VC 上传至 IPFS，获得 CID。

**引用**：X402 支付请求通过 `vc_ipfs_cid` 字段引用 VC：
```json
{
  "vc_ipfs_cid": "bafyreib4pdl7kg...vfqr3q"
}
```

**解析**：MCP 服务器在支付验证时调用 `resolve_vc_from_ipfs()`，从 IPFS 下载 VC JSON 并验证。

**代码位置**：
- `ignite-pay-core/src/vc.rs` — `resolve_vc_from_ipfs()`
- `ignite-pay-mcp/src/main.rs` — 支付流程中的 IPFS CID 路径（~line 706-764）
- `ignite-pay-mcp/src/tools.rs` — `X402ChallengeInput.vc_ipfs_cid`

### 4.3 策略列表同步

**机制**：用户白名单/黑名单存储为 JSON，上传至 IPFS，CID 记录在 DID 文档的 `serviceEndpoint` 中：

```json
"service": [{
    "id": "did:ignite:z6Mk...#policy-list",
    "type": "IgnitePolicyList",
    "serviceEndpoint": "ipfs://<CID>"
}]
```

**同步流程**：
```
1. MCP 服务器更新本地 sled 缓存（添加/移除商家）
2. 调用 list_store.upload_to_ipfs() 获取新 CID
3. 通过 DIDComm list-sync-notification 通知手机 App 新 CID
4. 手机 App 可通过 CID 拉取最新列表
```

**代码位置**：
- `ignite-pay-core/src/list_store.rs` — `sync_from_ipfs()`, `upload_to_ipfs()`
- `ignite-pay-mcp/src/main.rs` — 列表变更后的 IPFS 上传逻辑
- `ignite-pay-core/src/didcomm.rs` — `build_list_sync_notification()`

### 4.4 审计日志备份

**数据管线**：
```
交易记录 → Merkle 树构建 → protobuf 序列化 → Zstd 压缩 → AES-256-GCM 加密 → 上传 IPFS LogChunk
                                                                              ↓
                                                              ChunkManifest 跟踪所有 chunk CID
```

**备份**：`sync_to_ipfs()` 将未同步的 SQLite 条目上传至 IPFS。

**恢复**：`restore_from_ipfs()` 从单个 manifest CID 恢复所有交易：
```
1. 下载 manifest → 获取所有 chunk CID
2. 逐个下载 chunk → 解密 → 验证哈希链完整性
3. 按 nonce 排序返回所有条目
```

**代码位置**：
- `ignite-pay-core/src/log_sync.rs` — 完整 IPFS 日志管线
- `ignite-pay-core/proto/audit.proto` — protobuf schema（ChunkManifest, LogChunk）
- `ignite_pay_app/rust/src/api/log_store.rs` — 手机端 sync/restore 桥接

### 4.5 IPFS 客户端架构

```rust
trait IpfsClient {
    async fn upload(&self, data: &[u8]) -> Result<String>;  // 返回 CID
    async fn download(&self, cid: &str) -> Result<Vec<u8>>;  // 通过 CID 下载
}
```

| 实现 | 用途 | 状态 |
|------|------|------|
| `MockIpfsClient` | 开发/测试（内存 HashMap） | 默认启用 |
| `KuboIpfsClient` | 生产环境（本地 Kubo 节点） | `kubo` feature 启用 |

当前配置默认 `mode = "mock"`，生产部署需配置 `mode = "kubo"` 并运行本地 Kubo 节点。

---

## 5. 三层协作关系

### 5.1 数据流全景

```
商家 (本地 Ed25519 密钥对)
    │
    │ 生成 did:ignite:z...
    │
    ▼
did-registry (REST API)
    ├── 签发 W3C VC（平台 Ed25519 签名）    ──→ IPFS 存储 → CID
    ├── 签署 VC 绑定: sign(subject_pk || vc_hash)
    ├── 从 Photon RPC 获取 ZK 有效性证明
    └── 构建 Light System Program CPI 指令
         │
         ▼
    Solana 区块链
    ├── ignite-pay-did-program (Anchor + Light SDK)
    │       ├── initialize_did    — 压缩账户创建
    │       ├── update_did_with_vc — 压缩账户更新
    │       └── revoke_vc         — VC 撤销 PDA
    │
    ├── Light Protocol 状态树 (Merkle Tree)
    │       └── MerchantCompressedDid 叶节点（哈希）
    │
    ├── PlatformConfig PDA（平台 Ed25519 公钥）
    └── RevokedVc PDA（按 VC 的撤销条目）
```

### 5.2 支付验证的双层模型

**第一层（链下快速过滤）**：
1. 从 X402 响应提取商家 VC（内联或 IPFS CID）
2. 用平台公钥验证 VC 签名
3. 从索引器获取 Merkle Proof
4. 本地验证 `Proof + Leaf == Root`
5. 检查 `MerchantLeaf.status == 0 (active)`

**第二层（链上强制）**：
1. Agent 调用结算合约
2. 合约使用 `spl_account_compression::verify_leaf` 确认商家受平台背书
3. 合约验证 Session Key 有效性
4. 任一验证失败，交易回滚

### 5.3 职责划分总结

| 组件 | 职责 |
|------|------|
| **ZK Compression** | 链上存储：商家 DID 压缩账户、Merkle 证明、链上验证 |
| **IPFS** | 链下存储：VC 完整数据、策略列表、加密审计日志 |
| **did:ignite** | 身份标识：本地密钥对生成、DIDComm V2 通信、VC 主体绑定 |

三者关系：DID 是身份层（本地生成），ZK Compression 是链上身份数据层（链上证明），IPFS 是链下数据层（完整数据引用）。在支付验证时，MCP 服务器从 IPFS 获取完整 VC 验证签名，同时从 Merkle 树获取压缩证明验证链上身份，两层共同确保商家可信。

---

## 6. 相关代码索引

| 文件 | 内容 |
|------|------|
| `ignite-pay-did-program/src/state.rs` | `MerchantCompressedDid` 结构体 |
| `ignite-pay-did-program/src/lib.rs` | 链上指令（initialize_did, update_did 等） |
| `ignite-pay-core/src/identity.rs` | `did:ignite` DID 方法定义 |
| `ignite-pay-core/src/solana_did.rs` | `SolanaDidBridge` 桥接层 |
| `ignite-pay-core/src/ipfs.rs` | `IpfsClient` trait + Kubo/Mock 实现 |
| `ignite-pay-core/src/vc.rs` | `resolve_vc_from_ipfs()` |
| `ignite-pay-core/src/list_store.rs` | 策略列表 IPFS 同步 |
| `ignite-pay-core/src/log_sync.rs` | 审计日志 IPFS 管线 |
| `ignite-pay-core/proto/audit.proto` | 审计日志 protobuf schema |
| `did-registry/src/handlers/register.rs` | DID 注册 + ZK 证明获取 |
| `did-registry/src/handlers/proof.rs` | ZK 有效性证明端点 |
| `ignite-pay-solana/Cargo.toml` | Light SDK 依赖 |
| `ignite-pay-mcp/src/main.rs` | IPFS 客户端初始化、VC 解析、列表上传 |
| `ignite-pay-mcp/config.toml` | IPFS 配置（mode, kubo_url） |
