# ZK Compression DID 管理系统部署指南

本文档涵盖 ignite-pay-did-program（链上 Solana 程序）和 did-registry（链下 REST 服务）的完整部署流程。

---

## 目录

1. [系统架构概览](#1-系统架构概览)
2. [前置条件](#2-前置条件)
3. [链上程序部署（ignite-pay-did-program）](#3-链上程序部署ignite-pay-did-program)
4. [链下服务部署（did-registry）](#4-链下服务部署did-registry)
5. [配置详解](#5-配置详解)
6. [双模式上链（Sponsored / SelfOnchain）](#6-双模式上链sponsored--selfonchain)
7. [存储架构](#7-存储架构)
8. [API 接口参考](#8-api-接口参考)
9. [安全注意事项](#9-安全注意事项)

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
                        │  │ - 费用记录                 ││
                        │  └──────────────────────────┘│
                        │                              │
                        │  双模式上链:                   │
                        │  ┌──────────┐ ┌────────────┐ │
                        │  │Sponsored │ │SelfOnchain  │ │
                        │  │平台签名   │ │返回未签名TX │ │
                        │  │+发送     │ │商户自签+发送│ │
                        │  │+记录费用 │ │             │ │
                        │  └──────────┘ └────────────┘ │
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
          │  │  - init_platform (一次性)             │         │
          │  │  - initialize_did                    │         │
          │  │  - update_did_with_vc                │         │
          │  │  - set_recovery_key                  │         │
          │  │  - recover_controller                │         │
          │  │  - revoke_vc                         │         │
          │  └─────────────────────────────────────┘         │
          │                                                   │
          │  ┌─────────────────────────────────────┐         │
          │  │  PlatformConfig PDA                  │         │
          │  │  seeds: [b"platform-config"]          │         │
          │  │  存储平台 Ed25519 公钥                 │         │
          │  │  initialize_did / update_did_with_vc │         │
          │  │  验证平台签名后才能写入 vc_hash        │         │
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
          │                                                   │
          │  ┌─────────────────────────────────────┐         │
          │  │  RevokedVc PDA (吊销注册表)           │         │
          │  │  seeds: [b"revoked-vc", vc_hash]      │         │
          │  │  每个 VC 吊销创建一个 PDA             │         │
          │  │  验证方检查 PDA 存在即判定已吊销       │         │
          │  └─────────────────────────────────────┘         │
          └──────────────────────────────────────────────────┘
```

### 数据流

```
0. 部署时: init_platform(platform_ed25519_pubkey) → 写入 PlatformConfig PDA
1. 商户生成 Ed25519 密钥对 → derive did:ignite:z...
2. GET  /v1/auth/nonce              → 获取服务器 nonce
3. 商户签名 "issue_vc:{did}:{merchant_name}:{nonce}"
4. POST /v1/vc/issue + did_signature → 平台校验 DID 所有权 → 签发 VC → 获得 vc_hash
5. 商户签名 "register:{did}:{pubkey}:{vc_hash}:{nonce}"
6. POST /v1/merchants/register      → 平台签名(credential_subject_pk || vc_hash) → 链上创建压缩 DID
   ├── 链上验证: subject_binding + platform_sig_verify
   ├── mode=sponsored (默认): 平台签名+发送, 记录服务费
   └── mode=self_onchain:  返回未签名TX, 商户自行签名+广播
7. POST /v1/merchants/confirm (SelfOnchain 专用) → 商户通知平台交易已上链
8. GET  /v1/did/resolve/{did}       → 解析 DID Document
9. POST /v1/merchants/update-vc     → 更新链上 VC 哈希 (平台签名验证 + Subject Binding, 支持双模式)
10. POST /v1/merchants/rotate-key    → 轮换控制密钥 (同样支持双模式)
11. GET  /v1/fees                    → 查询费用记录
12. POST /v1/proof                   → 获取 ZK proof + platform_config_address（公开端点）
13. POST /v1/vc/revoke               → 吊销 VC（仅平台 authority，创建 RevokedVc PDA）
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
│   ├── lib.rs           # 程序入口，6 条指令 + ed25519 验证
│   ├── state.rs         # MerchantCompressedDid + PlatformConfig + RevokedVc 结构
│   └── error.rs         # DidError 错误码定义（含 PlatformNotInitialized, AlreadyRevoked 等）
└── tests/               # TypeScript 集成测试（可选）
```

### 3.2 账户结构

#### MerchantCompressedDid（压缩账户）

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

#### PlatformConfig（链上 PDA 账户）

```rust
// 链上 PDA，存储平台 Ed25519 公钥，用于验证平台签名
// Seeds: [b"platform-config"]
pub struct PlatformConfig {
    pub platform_ed25519_pubkey: [u8; 32],  // 平台 Ed25519 公钥
    pub authority: Pubkey,                   // 有权更新平台密钥的地址
    pub bump: u8,                            // PDA bump seed
}
```

**空间**: 8（discriminator）+ 32 + 32 + 1 = 73 字节。通过 `init_platform` 指令一次性初始化。

#### RevokedVc（链上 PDA 账户 — 吊销注册表）

```rust
// 链上 PDA，每个被吊销的 VC 创建一个 RevokedVc 账户
// Seeds: [b"revoked-vc", vc_hash]
pub struct RevokedVc {
    pub vc_hash: [u8; 32],              // 被吊销的 VC 哈希
    pub credential_subject_pk: Pubkey,   // VC 主体公钥
    pub revoked_at: i64,                 // 吊销时间戳
    pub reason: u8,                      // 吊销原因 (0=unspecified, 1=violation, 2=expired, etc.)
    pub authority: Pubkey,               // 执行吊销的平台 authority
    pub bump: u8,                        // PDA bump seed
}
```

**空间**: 8（discriminator）+ 32 + 32 + 8 + 1 + 32 + 1 = 114 字节。通过 `revoke_vc` 指令创建。

**吊销检查**: 第三方验证方通过检查 `RevokedVc` PDA 是否存在来判断 VC 是否已被吊销。PDA 地址 = `find_program_address(&[b"revoked-vc", vc_hash], program_id)`。

### 3.3 指令清单

| 指令 | 账户结构 | 功能 |
|---|---|---|
| `init_platform` | `[authority, platform_config, system_program]` | 一次性初始化平台 Ed25519 公钥 |
| `initialize_did` | `[signer, platform_config, ...remaining]` | 创建压缩 DID，需平台签名 |
| `update_did_with_vc` | `[signer, platform_config, ...remaining]` | 绑定/更新 VC 哈希，需平台签名 |
| `set_recovery_key` | `[signer, ...remaining]` | 设置/更换恢复密钥 |
| `recover_controller` | `[signer, ...remaining]` | 通过恢复密钥重置 controller |
| `revoke_vc` | `[authority, platform_config, revoked_vc, system_program]` | 吊销 VC，创建 RevokedVc PDA |

**平台签名验证**（`initialize_did` / `update_did_with_vc`）：

链上程序在 CPI 写入前验证平台对 `(credential_subject_pk || vc_hash)` 的 Ed25519 签名，并强制 `credential_subject_pk == signer.key()`。这同时实现了：
- **Account Binding**: 签名绑定了特定 signer，防止跨账户重放
- **Subject Binding**: 链上强制 signer 必须是 VC 的 subject，防止身份冒充

签名消息格式：`credential_subject_pk (32 bytes) || vc_hash (32 bytes)` = 64 字节

**`initialize_did` 指令数据格式**：
```
[discriminator(8)] [proof(var)] [address_tree_info(borsh)] [output_state_tree_index(1)]
[vc_hash(32)] [platform_signature(64)] [credential_subject_pk(32)]
```

**`update_did_with_vc` 指令数据格式**：
```
[discriminator(8)] [proof(var)] [current_did(borsh)] [account_meta(borsh)]
[vc_hash(32)] [nonce(8)] [platform_signature(64)] [credential_subject_pk(32)]
```

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
| `PlatformNotInitialized` | PlatformConfig PDA 未初始化（需先调用 `init_platform`） |
| `InvalidPlatformSignature` | 平台 Ed25519 签名验证失败 |
| `VcSubjectMismatch` | credential_subject_pk 与 signer 不匹配 |
| `AlreadyRevoked` | 该 VC 已被吊销（RevokedVc PDA 已存在） |
| `UnauthorizedRevocation` | 调用者不是平台 authority，无权吊销 |

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

#### 步骤 6：初始化 PlatformConfig

部署后必须**一次性**调用 `init_platform` 指令，将平台的 Ed25519 公钥写入链上 PDA：

```bash
# 用 Anchor CLI 或 solana-sdk 调用 init_platform
# 参数: platform_ed25519_pubkey (32 字节)
# 账户: [authority(signer), platform_config(PDA), system_program]
# PDA seeds: [b"platform-config"]
```

> 未调用 `init_platform` 前，`initialize_did` 和 `update_did_with_vc` 会因 `PlatformNotInitialized` 错误而拒绝执行。

---

## 4. 链下服务部署（did-registry）

### 4.1 项目结构

```
did-registry/
├── Cargo.toml           # Rust 依赖
├── config.toml          # 服务配置
└── src/
    ├── main.rs          # 入口（tokio + tracing + axum）
    ├── server.rs        # 路由定义（14 条路由）
    ├── config.rs        # Config 结构体（含 FeesConfig）
    ├── state.rs         # RegistryState 共享状态
    ├── error.rs         # RegistryError
    ├── handlers/
    │   ├── mod.rs
    │   ├── nonce.rs     # GET  /v1/auth/nonce
    │   ├── register.rs  # POST /v1/merchants/register (支持 mode 字段)
    │   ├── confirm.rs   # POST /v1/merchants/confirm (SelfOnchain 确认)
    │   ├── resolve.rs   # GET  /v1/did/resolve/{did}
    │   ├── verify.rs    # GET  /v1/merchants/verify/{did}
    │   ├── status.rs    # GET  /v1/merchants/status/{did}
    │   ├── rotate_key.rs# POST /v1/merchants/rotate-key (支持 mode 字段)
    │   ├── update_vc.rs # POST /v1/merchants/update-vc (支持 mode 字段)
    │   ├── issue_vc.rs  # POST /v1/vc/issue (需 DID 签名)
    │   ├── revoke_vc.rs # POST /v1/vc/revoke (仅平台 authority)
    │   ├── proof.rs     # POST /v1/proof (公开，无需认证)
    │   └── fees.rs      # GET  /v1/fees
    ├── did/
    │   ├── resolver.rs  # DID 哈希计算、签名验证
    │   └── ignite_store.rs  # 内存 DID 文档缓存
    └── storage/
        └── sled_store.rs    # sled 持久化存储（含费用记录）
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

[fees]
# Sponsored 模式下的服务费（单位：lamports，1 SOL = 1,000,000,000 lamports）
register_fee_lamports = 5000      # 注册费用
update_vc_fee_lamports = 2000     # 更新 VC 费用
rotate_key_fee_lamports = 2000    # 密钥轮换费用
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
| `fees.register_fee_lamports` | 是 | Sponsored 模式注册服务费（lamports） |
| `fees.update_vc_fee_lamports` | 是 | Sponsored 模式 VC 更新服务费（lamports） |
| `fees.rotate_key_fee_lamports` | 是 | Sponsored 模式密钥轮换服务费（lamports） |

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

## 6. 双模式上链（Sponsored / SelfOnchain）

所有 DID 链上操作（register / update-vc / rotate-key）支持两种上链模式，通过请求体中的 `mode` 字段选择。

### 6.1 OnchainMode 枚举

```rust
pub enum OnchainMode {
    Sponsored,    // 默认值，平台代付
    SelfOnchain,  // 商户自助
}
```

### 6.2 Sponsored 模式（默认，向后兼容）

```
┌──────────┐     ┌──────────────┐     ┌────────────┐
│ Merchant │────▶│ did-registry │────▶│ Solana RPC │
└──────────┘     │              │     └────────────┘
                 │ 1. 构建指令   │
                 │ 2. 平台签名   │
                 │ 3. 发送交易   │
                 │ 4. 记录费用   │
                 └──────────────┘
```

**流程**：
1. did-registry 使用 `payer` keypair 签名并发送交易
2. 交易成功后，在 sled 中记录费用条目
3. 返回 `{ "signature": "..." }`

**费用记录**：每个 Sponsored 操作会写入 sled，供离线结算。

### 6.3 SelfOnchain 模式（商户自助）

```
┌──────────┐     ┌──────────────┐
│ Merchant │────▶│ did-registry │
└────┬─────┘     │              │
     │           │ 1. 构建指令   │
     │◀──────────│ 2. 获取blockhash│
     │           │ 3. 返回未签名TX│
     │           └──────────────┘
     │
     │ 4. 本地签名
     │ 5. 广播交易
     ▼
┌────────────┐
│ Solana RPC │
└────────────┘
```

**流程**：
1. did-registry 构建未签名的 `Transaction`（包含 recent_blockhash）
2. 使用 bincode 序列化，base64 编码返回给商户
3. 商户客户端：反序列化 → 用自己的 keypair 签名 → 通过 RPC 广播

**SelfOnchain 响应格式**：

```json
{
  "transaction": "<base64 bincode 编码的未签名 Transaction>",
  "message": "sign and broadcast within 90 seconds; blockhash expires"
}
```

**商户客户端处理步骤**：

```rust
// 1. 解码 base64
let tx_bytes = base64::decode(&tx_b64)?;
// 2. 反序列化 Transaction
let mut tx: Transaction = bincode::deserialize(&tx_bytes)?;
// 3. 用商户 keypair 签名
tx.sign(&[&merchant_keypair], tx.message.recent_blockhash);
// 4. 广播
let sig = rpc_client.send_and_confirm_transaction(&tx)?;
```

> **注意**：未签名交易包含 `recent_blockhash`，约 90 秒后过期。商户需在过期前完成签名和广播。

### 6.4 各端点 mode 字段

| 端点 | mode 字段 | SelfOnchain signer |
|---|---|---|
| `POST /v1/merchants/register` | `mode`（默认 `sponsored`） | `active_pubkey`（请求中的商户公钥） |
| `POST /v1/merchants/update-vc` | `mode`（默认 `sponsored`） | `controller_pk`（当前链上控制器） |
| `POST /v1/merchants/rotate-key` | `mode`（默认 `sponsored`） | `controller_pk`（当前链上控制器） |

---

## 7. 存储架构

### 7.1 链上（压缩存储）

数据存储在 Light Protocol 的 Merkle Tree 中，不占用传统链上账户空间。每个 `MerchantCompressedDid` 约 150 字节，以哈希形式存在树叶子节点中。

**优势**：
- 无需 rent-exemption
- 单棵树可存储数千个 DID
- 交易成本远低于传统账户

### 7.2 链下（sled 数据库）

did-registry 使用嵌入式 sled 数据库，默认路径 `./did_registry_data`。

| Key 模式 | Value | 用途 |
|---|---|---|
| `merchant:{hex(did_hash)}` | Borsh 序列化 `MerchantDidAccount` | 商户 DID 缓存 |
| `leaf_index:{hex(did_hash)}` | 4 字节 LE u32 | Merkle 树叶子索引 |
| `vc:{vc_hash_hex}` | 原始 JSON | 已签发的 VC 存储 |
| `fee:{operation}:{timestamp_ms}:{did_hash_hex}` | JSON | Sponsored 模式费用记录 |
| `revoked_vc:{vc_hash_hex}` | JSON | VC 吊销记录缓存 |

> `did_hash` = `SHA-256(did_string)`

**费用记录格式**（Sponsored 模式自动写入）：

```json
{
  "merchant_did": "did:ignite:z...",
  "operation": "register",
  "fee_lamports": 5000,
  "timestamp": 1718438400000,
  "mode": "sponsored"
}
```

费用记录通过 `GET /v1/fees` 端点查询，支持按操作类型和时间范围过滤。

---

## 8. API 接口参考

| 方法 | 路由 | 功能 |
|---|---|---|
| GET | `/health` | 健康检查 |
| GET | `/v1/auth/nonce` | 获取防重放 nonce |
| POST | `/v1/vc/issue` | 签发 W3C VC（需 DID 签名验证） |
| POST | `/v1/vc/revoke` | 吊销 VC（仅平台 authority，创建链上 RevokedVc PDA） |
| POST | `/v1/proof` | 获取 ZK proof（公开端点，无需认证） |
| POST | `/v1/merchants/register` | 注册链上压缩 DID（支持 `mode` 字段） |
| POST | `/v1/merchants/confirm` | SelfOnchain 注册确认（商户广播后通知平台） |
| GET | `/v1/did/resolve/{did}` | 解析 DID Document |
| GET | `/v1/merchants/verify/{did}` | 验证商户 DID |
| GET | `/v1/merchants/status/{did}` | 查询商户状态 |
| POST | `/v1/merchants/update-vc` | 更新链上 VC 哈希（支持 `mode` 字段） |
| POST | `/v1/merchants/rotate-key` | 轮换控制密钥（支持 `mode` 字段） |
| GET | `/v1/fees` | 查询费用记录 |

### 8.1 GET /v1/auth/nonce

获取一次性 nonce，5 分钟有效，用于后续请求的防重放保护。

**响应**:
```json
{
  "nonce": "550e8400-e29b-41d4-a716-446655440000",
  "expires_in": 300
}
```

### 8.2 POST /v1/vc/issue

平台签发 W3C Verifiable Credential。需要 DID 签名验证身份所有权。若商户已注册，还会校验签名者是 controller 或 original key。

**请求**:
```json
{
  "merchant_did": "did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK",
  "merchant_name": "示例商店",
  "category": "retail",
  "validity_hours": 8760,
  "nonce": "<server-issued-nonce>",
  "did_signature": "<base64-Ed25519-sig>"
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
    "credentialStatus": {
      "type": "IgniteVcRevocationRegistry",
      "program_id": "<DID程序ID>"
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

### 8.3 POST /v1/vc/revoke

吊销已签发的 VC。仅平台 authority 可调用。在链上创建 `RevokedVc` PDA 并在 sled 中缓存吊销记录。

**请求**:
```json
{
  "vc_hash": "<hex 32字节，被吊销的VC哈希>",
  "credential_subject_pk": "<VC主体公钥(base58)>",
  "reason": 1,
  "platform_signature": "<base64签名，消息: revoke:{vc_hash}:{nonce}>",
  "nonce": "<server-nonce>"
}
```

- `reason`: 吊销原因码（0=unspecified, 1=violation, 2=expired 等）
- `platform_signature`: 平台 Ed25519 签名，消息格式 `revoke:{vc_hash}:{nonce}`

**响应**:
```json
{
  "signature": "<solana-tx-signature>",
  "revoked_vc_pda": "<RevokedVc PDA地址(base58)>"
}
```

**验证方吊销检查**: 第三方使用 `find_program_address(&[b"revoked-vc", vc_hash], program_id)` 推导 PDA 地址，查询该账户是否存在。若存在则 VC 已被吊销。

### 8.4 POST /v1/merchants/register

将商户 DID 注册为链上压缩账户。支持双模式上链。

**请求**:
```json
{
  "merchant_did": "did:ignite:z...",
  "active_pubkey": "<Solana-base58-pubkey>",
  "platform_vc_hash": "<hex-32-bytes>",
  "did_signature": "<base64-Ed25519-sig>",
  "nonce": "<server-nonce>",
  "mode": "sponsored"
}
```

- `mode`: 可选，默认 `"sponsored"`。可选值: `"sponsored"` | `"self_onchain"`

签名消息格式: `register:{merchant_did}:{active_pubkey}:{platform_vc_hash}:{nonce}`

**Sponsored 模式响应**:
```json
{
  "signature": "<solana-tx-signature>"
}
```

**SelfOnchain 模式响应**:
```json
{
  "transaction": "<base64-bincode-编码的未签名Transaction>",
  "message": "sign and broadcast within 90 seconds; blockhash expires"
}
```

### 8.5 GET /v1/did/resolve/{did}

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

### 8.6 POST /v1/merchants/update-vc

更新链上压缩 DID 的 VC 哈希。支持双模式上链。

**请求**:
```json
{
  "merchant_did": "did:ignite:z...",
  "new_vc_hash": "<hex-32-bytes>",
  "platform_signature": "<base64-sig>",
  "nonce": "<server-nonce>",
  "account_meta_b64": "<optional-base64-borsh-CompressedAccountMeta>",
  "mode": "sponsored"
}
```

- `mode`: 可选，默认 `"sponsored"`。SelfOnchain 模式下 signer 为当前 `controller_pk`

签名消息格式: `update-vc:{merchant_did}:{new_vc_hash}:{nonce}`

### 8.7 POST /v1/merchants/rotate-key

轮换商户控制密钥。支持双模式上链。

**请求**:
```json
{
  "merchant_did": "did:ignite:z...",
  "new_active_pubkey": "<base58>",
  "did_signature": "<base64-sig>",
  "nonce": "<server-nonce>",
  "account_meta_b64": "<optional>",
  "mode": "sponsored"
}
```

- `mode`: 可选，默认 `"sponsored"`。SelfOnchain 模式下 signer 为当前 `controller_pk`

签名消息格式: `rotate-key:{merchant_did}:{new_active_pubkey}:{nonce}`

### 8.8 GET /v1/fees

查询 Sponsored 模式产生的费用记录。

**查询参数**:

| 参数 | 类型 | 默认值 | 说明 |
|---|---|---|---|
| `operation` | string | 无（全部） | 过滤操作类型: `register` / `update_vc` / `rotate_key` |
| `since` | int64 | 无（全部） | 仅返回此时间戳之后的记录（Unix 毫秒） |
| `limit` | int | 100 | 最大返回条数 |

**请求示例**:
```bash
curl "http://localhost:8081/v1/fees?operation=register&since=1718438400000&limit=50"
```

**响应**:
```json
{
  "fees": [
    {
      "merchant_did": "did:ignite:z...",
      "operation": "register",
      "fee_lamports": 5000,
      "timestamp": 1718438400000,
      "mode": "sponsored"
    }
  ]
}
```

### 8.9 POST /v1/proof

公开端点，获取 ZK Compression validity proof。无需认证。商户使用返回的 proof 数据在本地构建并签名交易。

**请求**:
```json
{
  "pubkey": "<商户Solana公钥(base58)>",
  "operation": "register",
  "account_hash": "<hex 32字节，update_vc/rotate_key时必填>"
}
```

- `operation`: `"register"` | `"update_vc"` | `"rotate_key"`
- `account_hash`: 仅 `update_vc` 和 `rotate_key` 需要提供（已有压缩账户的哈希）

**响应**:
```json
{
  "proof": "<base64 borsh序列化的ZK proof>",
  "compressed_address": "<base58>",
  "address_seed": "<base58>",
  "address_merkle_tree": "<base58>",
  "address_tree_info": "<base64 borsh序列化>",
  "output_state_tree_index": 0,
  "remaining_accounts": [
    { "pubkey": "...", "is_signer": false, "is_writable": true }
  ],
  "program_id": "DID程序ID(base58)",
  "platform_config_address": "PlatformConfig PDA地址(base58)"
}
```

> `platform_config_address` 需作为 accounts 列表的第二个账户（readonly）传入 `initialize_did` 和 `update_did_with_vc` 指令。

### 8.10 POST /v1/merchants/confirm

SelfOnchain 模式专用。商户广播交易成功后，通知平台缓存商户数据，使后续操作（verify/status/update-vc/rotate-key）可用。

**请求**:
```json
{
  "merchant_did": "did:ignite:z...",
  "tx_signature": "<Solana交易签名(base58)>",
  "active_pubkey": "<商户公钥(base58)>",
  "platform_vc_hash": "<hex 32字节>",
  "did_signature": "<base64签名，消息: confirm:{did}:{tx_signature}:{nonce}>",
  "nonce": "<server-nonce>"
}
```

**响应**:
```json
{ "status": "confirmed" }
```

幂等：如果商户已缓存，返回 `{ "status": "already_confirmed" }`。

---

## 9. 安全注意事项

### 9.0 平台签名验证（防重放 + 防冒充）

链上程序通过 `PlatformConfig` PDA 存储平台 Ed25519 公钥。`initialize_did` 和 `update_did_with_vc` 在写入 vc_hash 前执行两层校验：

1. **Subject Binding**: `credential_subject_pk == signer.key()` — 确保提交者就是 VC 的主体
2. **平台签名验证**: `verify(platform_pubkey, credential_subject_pk || vc_hash, platform_signature)` — 确保平台已授权此绑定

攻击者即使拦截了 `(vc_hash, platform_signature, credential_subject_pk)`，也无法用自己的 signer 提交：
- 若用原始 `credential_subject_pk`，subject binding 检查失败（signer 不匹配）
- 若篡改 `credential_subject_pk`，平台签名验证失败（签名消息不匹配）

### 9.0b VC 吊销机制

平台可通过 `POST /v1/vc/revoke` 吊销已签发的 VC。吊销流程：

1. **链上**: 调用 `revoke_vc` 指令，创建 `RevokedVc` PDA（seeds: `[b"revoked-vc", vc_hash]`）
2. **链下**: 在 sled 中缓存吊销记录（`revoked_vc:{vc_hash_hex}`）
3. **VC 中**: 每个签发的 VC 包含 `credentialStatus` 字段，指向链上吊销注册表

**验证方检查流程**:
1. 验证 VC 的 Ed25519 签名和有效期
2. 从 VC 中提取 `credentialStatus.program_id`（DID 程序地址）
3. 计算 `vc_hash = SHA-256(vc_json)`
4. 推导 PDA: `find_program_address(&[b"revoked-vc", vc_hash], program_id)`
5. 查询该 PDA 是否存在 — 若存在，VC 已被吊销

**权限控制**: 仅 `PlatformConfig.authority` 可以调用 `revoke_vc`，防止未授权吊销。

### 9.1 密钥管理

- `platform_signing.key` 必须妥善保管，泄露意味着任何人可以签发伪造 VC
- `payer_keypair.json` 需要定期补充 SOL 余额
- 建议使用硬件安全模块（HSM）或密钥管理服务（KMS）管理生产密钥

### 9.2 Nonce 机制

- 服务端 nonce 5 分钟有效，单次使用后销毁
- 链上 nonce 为递增计数器，每次 mutation 操作加 1
- 双层 nonce 设计防止跨域重放攻击

### 9.3 ZK Compression 注意

- Photon RPC 是信任假设的一部分——如果索引器提供虚假证明，交易可能失败
- 压缩账户数据不直接存储在链上，需要通过索引器读取
- 建议使用 Helius 等可靠的 Photon RPC 提供商

### 9.4 网络安全

- 生产环境建议在 did-registry 前部署反向代理（nginx/caddy）
- 启用 TLS（HTTPS）
- 考虑添加速率限制
- `jwt_secret` 应使用强随机值

### 9.5 SelfOnchain 模式安全

- 未签名交易包含 `recent_blockhash`，约 90 秒后过期，商户需在窗口内完成签名和广播
- 平台不记录 SelfOnchain 模式的费用（仅 Sponsored 模式记录）
- SelfOnchain 模式下，商户自行承担交易签名和广播的全部责任
- 建议商户客户端实现超时重试机制：若 blockhash 过期，重新请求未签名交易
- `POST /v1/proof` 为公开端点，任何人可获取 ZK proof，但构建交易仍需商户私钥签名
- SelfOnchain 商户广播后**必须**调用 `POST /v1/merchants/confirm` 通知平台，否则后续操作不可用

### 9.6 VC 签发安全

- `POST /v1/vc/issue` 要求 DID 签名（`issue_vc:{did}:{merchant_name}:{nonce}`），确保请求者持有 DID 私钥
- 更新场景：平台会校验签名者是否为当前 controller 或 original key，防止未授权者请求新 VC
- 首次签发（商户未注册）：仅校验 DID 签名，不要求已注册
