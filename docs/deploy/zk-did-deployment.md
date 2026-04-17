# ZK Compression DID 管理系统部署指南

本文档涵盖 ignite-pay-did-program（链上 Solana 程序）和 did-registry（链下 REST 服务）的完整部署流程。

---

## 目录

1. [系统架构概览](#1-系统架构概览)
2. [前置条件](#2-前置条件)
3. [链上程序部署（ignite-pay-did-program）](#3-链上程序部署ignite-pay-did-program)
4. [链下服务部署（did-registry）](#4-链下服务部署did-registry)
5. [配置详解](#5-配置详解)
6. [存储架构](#6-存储架构)
7. [API 接口参考](#7-api-接口参考)
8. [安全注意事项](#8-安全注意事项)

---

## 1. 系统架构概览

```
                        ┌─────────────────────────────┐
                        │     Merchant (Client)        │
                        │  Ed25519 Keypair (本地)       │
                        │  did:ignite:z<multibase>     │
                        └──────────┬──────────────────┘
                                   │ HTTP REST
                                   ▼
                        ┌─────────────────────────────┐
                        │      did-registry            │
                        │   (Axum HTTP Server)         │
                        │                              │
                        │  ┌─────────┐  ┌───────────┐ │
                        │  │ VC 签发 │  │ Nonce 管理 │ │
                        │  └─────────┘  └───────────┘ │
                        │  ┌──────────────────────────┐│
                        │  │ Platform Signing Key      ││
                        │  │ (Ed25519, 32 bytes)       ││
                        │  └──────────────────────────┘│
                        │  ┌──────────────────────────┐│
                        │  │ sled (持久化存储)          ││
                        │  │ - 商户 DID 缓存           ││
                        │  │ - VC 存储                 ││
                        │  │ - Leaf Index 映射         ││
                        │  └──────────────────────────┘│
                        └──────┬──────────┬────────────┘
                               │          │
                    ┌──────────┘          └──────────┐
                    ▼                                ▼
          ┌──────────────┐               ┌─────────────────────┐
          │ Solana RPC    │               │ Photon RPC          │
          │ (RPC URL)     │               │ (ZK Compression     │
          │               │               │  Indexer)            │
          └──────┬────────┘               └──────────┬──────────┘
                 │                                   │
                 ▼                                   ▼
          ┌──────────────────────────────────────────────────┐
          │         Solana Blockchain (Devnet/Mainnet)       │
          │                                                   │
          │  ┌─────────────────────────────────────┐         │
          │  │  ignite-pay-did-program              │         │
          │  │  (Anchor + Light SDK)                │         │
          │  │                                      │         │
          │  │  Instructions:                       │         │
          │  │  - initialize_did                    │         │
          │  │  - update_did_with_vc                │         │
          │  │  - set_recovery_key                  │         │
          │  │  - recover_controller                │         │
          │  └─────────────────────────────────────┘         │
          │                                                   │
          │  ┌─────────────────────────────────────┐         │
          │  │  Light Protocol State Trees          │         │
          │  │  (Merkle Tree — 压缩账户存储)        │         │
          │  │                                      │         │
          │  │  MerchantCompressedDid:               │         │
          │  │  - original_pk   (不可变)            │         │
          │  │  - controller_pk (可轮换)            │         │
          │  │  - recovery_pk   (恢复密钥)          │         │
          │  │  - vc_hash       (VC 哈希)           │         │
          │  │  - last_updated  (时间戳)            │         │
          │  │  - nonce         (防重放计数器)       │         │
          │  └─────────────────────────────────────┘         │
          └──────────────────────────────────────────────────┘
```

### 数据流

```
1. 商户生成 Ed25519 密钥对 → derive did:ignite:z...
2. GET  /v1/auth/nonce              → 获取服务器 nonce
3. POST /v1/vc/issue                → 平台签发 VC → 获得 vc_hash
4. 商户签名 "register:{did}:{pubkey}:{vc_hash}:{nonce}"
5. POST /v1/merchants/register      → 链上创建压缩 DID
6. GET  /v1/did/resolve/{did}       → 解析 DID Document
7. POST /v1/merchants/update-vc     → 更新链上 VC 哈希
8. POST /v1/merchants/rotate-key    → 轮换控制密钥
```

---

## 2. 前置条件

### 2.1 工具链

| 工具 | 版本 | 用途 |
|---|---|---|
| Rust | 1.75+ | 编译所有 Rust crate |
| Solana CLI | 1.18+ | 部署链上程序 |
| Anchor CLI | 0.31.1 | 构建 ignite-pay-did-program |
| cargo-build-sgx (或 solana bpf) | — | 编译 BPF/SBF 程序 |
| Node.js / Yarn | 可选 | 运行 Anchor 测试脚本 |

### 2.2 账户准备

- **Payer Keypair**: 用于支付链上交易费用，需要足够 SOL（devnet 可通过 `solana airdrop` 获取）
- **Platform Signing Key**: 32 字节 Ed25519 私钥，用于签发 VC（建议通过安全方式生成）
- **Photon RPC API Key**: Helius 或其他 Light Protocol 索引器提供的 ZK Compression 证明服务

### 2.3 生成 Platform Signing Key

```bash
# 生成 32 字节随机私钥文件
openssl rand -out platform_signing.key 32

# 如果需要查看对应的公钥和 DID（调试用）
# 可使用项目中的 identity 模块 derive
```

### 2.4 网络

| 网络 | Solana RPC | Photon RPC |
|---|---|---|
| Localnet | `http://127.0.0.1:8899` | 本地 Photon (需单独部署) |
| Devnet | `https://api.devnet.solana.com` | `https://photon.helius.com?api-key=<KEY>` |
| Mainnet | `https://api.mainnet-beta.solana.com` | `https://photon.helius.com?api-key=<KEY>` |

---

## 3. 链上程序部署（ignite-pay-did-program）

### 3.1 项目结构

```
ignite-pay-did-program/
├── Anchor.toml          # Anchor 配置（program ID, cluster, wallet）
├── Cargo.toml           # Rust 依赖
├── src/
│   ├── lib.rs           # 程序入口，4 条指令
│   ├── state.rs         # MerchantCompressedDid 压缩账户结构
│   └── error.rs         # DidError 错误码定义
└── tests/               # TypeScript 集成测试（可选）
```

### 3.2 压缩账户结构

```rust
// 存储在 Light Protocol Merkle Tree 中（不是传统链上账户）
pub struct MerchantCompressedDid {
    pub original_pk: Pubkey,      // 初始公钥（不可变锚点）
    pub controller_pk: Pubkey,    // 当前控制器（可轮换）
    pub recovery_pk: Pubkey,      // 恢复密钥
    pub vc_hash: [u8; 32],        // 平台签发的 VC SHA-256 哈希
    pub last_updated: i64,        // 最后更新 Unix 时间戳
    pub nonce: u64,               // 防重放计数器
}
```

**PDA 派生**: `seeds = [b"merchant-did", original_pk]`，在 Address Tree 中确定地址。

### 3.3 指令清单

| 指令 | 签名者 | 功能 |
|---|---|---|
| `initialize_did` | 任意（成为 original_pk + controller_pk） | 创建新的压缩 DID |
| `update_did_with_vc` | controller_pk | 绑定/更新 VC 哈希 |
| `set_recovery_key` | controller_pk | 设置/更换恢复密钥 |
| `recover_controller` | recovery_pk | 通过恢复密钥重置 controller |

所有指令均通过 Light System Program CPI 写入压缩状态树，需要：
- **Validity Proof**: 由 Photon RPC 提供，证明账户存在/不存在
- **Remaining Accounts**: CPI 所需的 Light Protocol 账户（树账户、系统程序等）
- **Anti-replay Nonce**: 链上计数器，每次 mutation 必须递增

### 3.4 错误码

| 错误码 | 含义 |
|---|---|
| `AlreadyInitialized` | DID 已存在 |
| `NotInitialized` | DID 不存在 |
| `InvalidControllerKey` | 签名者不是当前 controller |
| `NonceMismatch` | 提供的 nonce 不匹配当前值 |
| `InvalidRecoveryKey` | 签名者不是恢复密钥 |
| `ArithmeticOverflow` | Nonce 溢出 |
| `InsufficientCpiAccounts` | CPI 账户不足 |

### 3.5 编译与部署

#### 步骤 1：配置 Anchor.toml

```toml
[features]
seeds = false
skip-lint = false

[programs.devnet]  # 或 mainnet
ignite_pay_did_program = "<YOUR_PROGRAM_ID>"

[registry]
url = "https://api.apr.dev"

[provider]
cluster = "devnet"  # 或 mainnet
wallet = "~/.config/solana/id.json"
```

将 `YOUR_PROGRAM_ID` 替换为实际程序 ID。生成新 keypair：

```bash
solana-keygen new -o target/deploy/ignite_pay_did_program-keypair.json
```

程序 ID 会自动从 keypair 推导，更新到 `Anchor.toml` 和 `declare_id!` 宏中。

#### 步骤 2：编译

```bash
cd ignite-pay-did-program

# 编译（debug）
anchor build

# 或使用 cargo 直接编译
cargo build-sbf
```

编译产物位于 `target/deploy/ignite_pay_did_program.so`。

#### 步骤 3：部署

```bash
# Devnet 部署
anchor deploy --provider.cluster devnet

# 或手动部署
solana program deploy \
  target/deploy/ignite_pay_did_program.so \
  --program-id target/deploy/ignite_pay_did_program-keypair.json \
  --url devnet
```

#### 步骤 4：验证部署

```bash
solana program show <PROGRAM_ID> --url devnet
```

确认程序已部署且不可变（或可升级，取决于部署策略）。

#### 步骤 5：记录 Program ID

部署后将 Program ID 记录下来，供 did-registry 配置使用：

```
did_program_id = "<DEPLOYED_PROGRAM_ID>"
```

---

## 4. 链下服务部署（did-registry）

### 4.1 项目结构

```
did-registry/
├── Cargo.toml           # Rust 依赖
├── config.toml          # 服务配置
└── src/
    ├── main.rs          # 入口（tokio + tracing + axum）
    ├── server.rs        # 路由定义（9 条路由）
    ├── config.rs        # Config 结构体
    ├── state.rs         # RegistryState 共享状态
    ├── error.rs         # RegistryError
    ├── handlers/
    │   ├── mod.rs
    │   ├── nonce.rs     # GET  /v1/auth/nonce
    │   ├── register.rs  # POST /v1/merchants/register
    │   ├── resolve.rs   # GET  /v1/did/resolve/{did}
    │   ├── verify.rs    # GET  /v1/merchants/verify/{did}
    │   ├── status.rs    # GET  /v1/merchants/status/{did}
    │   ├── rotate_key.rs# POST /v1/merchants/rotate-key
    │   ├── update_vc.rs # POST /v1/merchants/update-vc
    │   └── issue_vc.rs  # POST /v1/vc/issue
    ├── did/
    │   ├── resolver.rs  # DID 哈希计算、签名验证
    │   └── ignite_store.rs  # 内存 DID 文档缓存
    └── storage/
        └── sled_store.rs    # sled 持久化存储
```

### 4.2 编译

```bash
cd did-registry
cargo build --release
```

编译产物: `target/release/did-registry`

### 4.3 配置 config.toml

```toml
[server]
host = "0.0.0.0"
port = 8081

[solana]
# Solana RPC 端点
rpc_url = "https://api.devnet.solana.com"
# 部署后的 ignite-pay-did-program ID
did_program_id = "<DEPLOYED_PROGRAM_ID>"
# 交易支付方 keypair 文件路径（Solana JSON keypair 格式）
payer_keypair_path = "/path/to/payer-keypair.json"

[light]
# Photon RPC URL（ZK Compression 索引器）
# 格式：https://photon.helius.com?api-key=<YOUR_API_KEY>
photon_url = "https://photon.helius.com?api-key=<YOUR_API_KEY>"

[auth]
# JWT 签名密钥
jwt_secret = "<随机强密钥>"
# 平台 Ed25519 公钥（Base64 编码），用于验证 update-vc 请求中的平台签名
platform_public_key = "<BASE64_PUBLIC_KEY>"
# 平台 Ed25519 私钥文件路径（32 字节原始二进制）
platform_signing_key_path = "/path/to/platform_signing.key"
```

#### 配置字段说明

| 字段 | 必填 | 说明 |
|---|---|---|
| `server.host` | 是 | 监听地址 |
| `server.port` | 是 | 监听端口 |
| `solana.rpc_url` | 是 | Solana RPC 端点 URL |
| `solana.did_program_id` | 是 | 部署后的 ignite-pay-did-program 程序 ID |
| `solana.payer_keypair_path` | 生产必填 | 交易支付方 Keypair（空字符串 = 临时 keypair，仅开发用） |
| `light.photon_url` | 生产必填 | ZK Compression Photon RPC URL（空字符串将导致证明获取失败） |
| `auth.jwt_secret` | 是 | JWT 签名密钥 |
| `auth.platform_public_key` | 生产必填 | 平台 Ed25519 公钥（Base64），用于验证 update-vc 的平台签名 |
| `auth.platform_signing_key_path` | 生产必填 | 32 字节 Ed25519 私钥文件路径（空 = 临时密钥，仅开发用） |

### 4.4 准备密钥文件

#### Payer Keypair

```bash
# 生成 Solana keypair（标准 JSON 格式）
solana-keygen new -o /path/to/payer-keypair.json

# Devnet 空投 SOL
solana airdrop 2 <PAYER_ADDRESS> --url devnet
```

#### Platform Signing Key

```bash
# 生成 32 字节随机私钥
openssl rand -out /path/to/platform_signing.key 32

# 设置权限（仅 owner 可读）
chmod 400 /path/to/platform_signing.key
```

获取对应的 `platform_public_key`（供 `config.toml` 和 `update-vc` 验证使用）需要通过工具从私钥文件推导公钥再 Base64 编码。

### 4.5 启动服务

```bash
# 直接运行
./target/release/did-registry /path/to/config.toml

# 或使用默认 config.toml（当前目录）
./target/release/did-registry

# 通过环境变量覆盖日志级别
RUST_LOG=did_registry=debug ./target/release/did-registry
```

启动后会输出：

```
INFO did_registry: Starting DID Registry on 0.0.0.0:8081
INFO did_registry::state: Registry payer pubkey: <PAYER_PUBKEY>
INFO did_registry::state: Platform DID: did:ignite:z<...>
INFO did_registry: Listening on 0.0.0.0:8081
```

### 4.6 Docker 部署（推荐）

```dockerfile
FROM rust:1.82-slim AS builder
WORKDIR /app
COPY . .
RUN cargo build --release -p did-registry

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/did-registry /usr/local/bin/
COPY did-registry/config.toml /etc/did-registry/config.toml
EXPOSE 8081
ENTRYPOINT ["did-registry", "/etc/did-registry/config.toml"]
```

```bash
docker build -t did-registry .
docker run -d \
  -p 8081:8081 \
  -v /path/to/config.toml:/etc/did-registry/config.toml \
  -v /path/to/payer-keypair.json:/secrets/payer.json:ro \
  -v /path/to/platform_signing.key:/secrets/platform.key:ro \
  -v did-registry-data:/var/lib/did-registry \
  --name did-registry \
  did-registry
```

> 注意：sled 数据库默认写入 `./did_registry_data` 目录。Docker 中需挂载 volume 以持久化。

### 4.7 健康检查

```bash
curl http://localhost:8081/health
# 预期响应: ok
```

---

## 5. 配置详解

### 5.1 状态树（State Tree）

ZK Compression 依赖 Light Protocol 的 Merkle 树存储压缩账户。每个压缩 DID 实际上是树中的一个叶子节点哈希。Photon RPC 负责索引这些叶子并提供 Merkle 证明。

- **Address Tree**: 用于寻址，`derive_address([b"merchant-did", original_pk], address_tree, program_id)` 确定性推导地址
- **State Tree**: 存储实际的压缩账户数据哈希

did-registry 启动时通过 `LightClient::new(config)` 自动连接 Photon RPC，获取可用的树信息。

### 5.2 密钥架构

```
商户密钥三层架构:
┌─────────────────────────────────────┐
│ Original Key (original_pk)          │  不可变，身份锚点
│ - 注册时确定，永远不会改变           │
│ - 用于 PDA 派生：[b"merchant-did", pk]│
└──────────────┬──────────────────────┘
               │ 可通过恢复流程转移控制权
               ▼
┌─────────────────────────────────────┐
│ Controller Key (controller_pk)      │  可轮换，日常操作
│ - 签名 update-vc, set-recovery 操作 │
│ - 通过 rotate-key 端点轮换         │
└──────────────┬──────────────────────┘
               │ 灾备恢复
               ▼
┌─────────────────────────────────────┐
│ Recovery Key (recovery_pk)          │  灾备恢复
│ - 可重置 controller_pk              │
│ - recovery_pk 持有者签名 recover    │
│ - 安全离线保管                       │
└─────────────────────────────────────┘
```

### 5.3 Platform DID

Platform DID 由 `platform_signing_key` 的公钥推导而来：

```
公钥 → multicodec(0xed, 0x01) → Base58 → "did:ignite:z" + encoded
```

该 DID 作为所有签发 VC 的 `issuer` 字段，以及 VC proof 中的 `verification_method` 前缀。

---

## 6. 存储架构

### 6.1 链上（压缩存储）

数据存储在 Light Protocol 的 Merkle Tree 中，不占用传统链上账户空间。每个 `MerchantCompressedDid` 约 150 字节，以哈希形式存在树叶子节点中。

**优势**：
- 无需 rent-exemption
- 单棵树可存储数千个 DID
- 交易成本远低于传统账户

### 6.2 链下（sled 数据库）

did-registry 使用嵌入式 sled 数据库，默认路径 `./did_registry_data`。

| Key 模式 | Value | 用途 |
|---|---|---|
| `merchant:{hex(did_hash)}` | Borsh 序列化 `MerchantDidAccount` | 商户 DID 缓存 |
| `leaf_index:{hex(did_hash)}` | 4 字节 LE u32 | Merkle 树叶子索引 |
| `vc:{vc_hash_hex}` | 原始 JSON | 已签发的 VC 存储 |

> `did_hash` = `SHA-256(did_string)`

---

## 7. API 接口参考

| 方法 | 路由 | 功能 |
|---|---|---|
| GET | `/health` | 健康检查 |
| GET | `/v1/auth/nonce` | 获取防重放 nonce |
| POST | `/v1/vc/issue` | 签发 W3C VC |
| POST | `/v1/merchants/register` | 注册链上压缩 DID |
| GET | `/v1/did/resolve/{did}` | 解析 DID Document |
| GET | `/v1/merchants/verify/{did}` | 验证商户 DID |
| GET | `/v1/merchants/status/{did}` | 查询商户状态 |
| POST | `/v1/merchants/update-vc` | 更新链上 VC 哈希 |
| POST | `/v1/merchants/rotate-key` | 轮换控制密钥 |

### 7.1 GET /v1/auth/nonce

获取一次性 nonce，5 分钟有效，用于后续请求的防重放保护。

**响应**:
```json
{
  "nonce": "550e8400-e29b-41d4-a716-446655440000",
  "expires_in": 300
}
```

### 7.2 POST /v1/vc/issue

平台签发 W3C Verifiable Credential。

**请求**:
```json
{
  "merchant_did": "did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK",
  "merchant_name": "示例商店",
  "category": "retail",
  "validity_hours": 8760,
  "nonce": "<server-issued-nonce>"
}
```

**响应**:
```json
{
  "verifiable_credential": {
    "@context": [
      "https://www.w3.org/2018/credentials/v1",
      "https://ignite-pay.com/credentials/v1"
    ],
    "id": "urn:uuid:...",
    "type": ["VerifiableCredential", "MerchantAttestation"],
    "issuer": "did:ignite:z<platform>",
    "issuanceDate": "2025-01-01T00:00:00Z",
    "expirationDate": "2026-01-01T00:00:00Z",
    "credentialSubject": {
      "id": "did:ignite:z<merchant>",
      "name": "示例商店",
      "category": "retail"
    },
    "proof": {
      "type": "Ed25519Signature2020",
      "created": "2025-01-01T00:00:00Z",
      "proofPurpose": "assertionMethod",
      "verificationMethod": "did:ignite:z<platform>#key-signing-1",
      "proofValue": "<base64-signature>"
    }
  },
  "vc_hash": "<sha256-hex-of-vc-json>"
}
```

### 7.3 POST /v1/merchants/register

将商户 DID 注册为链上压缩账户。

**请求**:
```json
{
  "merchant_did": "did:ignite:z...",
  "active_pubkey": "<Solana-base58-pubkey>",
  "platform_vc_hash": "<hex-32-bytes>",
  "did_signature": "<base64-Ed25519-sig>",
  "nonce": "<server-nonce>"
}
```

签名消息格式: `register:{merchant_did}:{active_pubkey}:{platform_vc_hash}:{nonce}`

**响应**:
```json
{
  "signature": "<solana-tx-signature>"
}
```

### 7.4 GET /v1/did/resolve/{did}

解析 DID 为 W3C DID Document。

**响应**:
```json
{
  "@context": ["https://www.w3.org/ns/did/v1"],
  "id": "did:ignite:z...",
  "verificationMethod": [{
    "id": "did:ignite:z...#key-signing-1",
    "type": "Ed25519VerificationKey2020",
    "controller": "did:ignite:z...",
    "publicKeyMultibase": "z..."
  }],
  "controller_pubkey": "<base58>",
  "original_pubkey": "<base58>",
  "last_updated": 1700000000
}
```

### 7.5 POST /v1/merchants/update-vc

更新链上压缩 DID 的 VC 哈希。

**请求**:
```json
{
  "merchant_did": "did:ignite:z...",
  "new_vc_hash": "<hex-32-bytes>",
  "platform_signature": "<base64-sig>",
  "nonce": "<server-nonce>",
  "account_meta_b64": "<optional-base64-borsh-CompressedAccountMeta>"
}
```

签名消息格式: `update-vc:{merchant_did}:{new_vc_hash}:{nonce}`

### 7.6 POST /v1/merchants/rotate-key

轮换商户控制密钥。

**请求**:
```json
{
  "merchant_did": "did:ignite:z...",
  "new_active_pubkey": "<base58>",
  "did_signature": "<base64-sig>",
  "nonce": "<server-nonce>",
  "account_meta_b64": "<optional>"
}
```

签名消息格式: `rotate-key:{merchant_did}:{new_active_pubkey}:{nonce}`

---

## 8. 安全注意事项

### 8.1 密钥管理

- `platform_signing.key` 必须妥善保管，泄露意味着任何人可以签发伪造 VC
- `payer_keypair.json` 需要定期补充 SOL 余额
- 建议使用硬件安全模块（HSM）或密钥管理服务（KMS）管理生产密钥

### 8.2 Nonce 机制

- 服务端 nonce 5 分钟有效，单次使用后销毁
- 链上 nonce 为递增计数器，每次 mutation 操作加 1
- 双层 nonce 设计防止跨域重放攻击

### 8.3 ZK Compression 注意

- Photon RPC 是信任假设的一部分——如果索引器提供虚假证明，交易可能失败
- 压缩账户数据不直接存储在链上，需要通过索引器读取
- 建议使用 Helius 等可靠的 Photon RPC 提供商

### 8.4 网络安全

- 生产环境建议在 did-registry 前部署反向代理（nginx/caddy）
- 启用 TLS（HTTPS）
- 考虑添加速率限制
- `jwt_secret` 应使用强随机值
