# Ignite Pay 系统部署指南

本文档涵盖 Ignite Pay 全部服务的完整部署流程，包括基础设施依赖、链上程序、链下微服务、移动端应用和 MCP 代理服务。

---

## 目录

1. [系统概述](#1-系统概述)
2. [环境要求](#2-环境要求)
3. [网络拓扑图](#3-网络拓扑图)
4. [部署步骤（按依赖顺序）](#4-部署步骤按依赖顺序)
5. [配置文件详解](#5-配置文件详解)
6. [密钥管理](#6-密钥管理)
7. [Docker 部署](#7-docker-部署)
8. [生产环境注意事项](#8-生产环境注意事项)
9. [健康检查](#9-健康检查)
10. [故障排查](#10-故障排查)
11. [备份与恢复](#11-备份与恢复)
12. [升级与回滚](#12-升级与回滚)
13. [环境变量参考](#13-环境变量参考)

---

## 1. 系统概述

Ignite Pay 是基于 Solana 区块链的离链支付系统，采用 UTXO + Merkle Tree 状态通道架构，支持单跳和多跳支付、HTLC 条件支付、合规审计和 DID 去中心化身份。

### 1.1 核心组件

| 组件 | 说明 |
|:-----|:-----|
| **ignite-pay-program** | 链上状态通道程序（Anchor 1.0.0），Program ID: `DJBHr35jL3JAGoU7bKMsEFmpeNMrCSK7oYQE4HJ3GBUe` |
| **ignite-pay-did-program** | 链上 DID 程序（Anchor + Light SDK），ZK Compression 压缩账户 |
| **didcomm-router** | DIDComm 消息路由器，为移动端提供消息中继和 FCM 推送 |
| **did-registry** | DID 注册服务，管理商户链上身份、VC 签发/吊销 |
| **channel-user** | 用户端状态通道服务（Party A，付款方） |
| **channel-provider** | 商户端状态通道服务（Party B，收款方） |
| **channel-hub** | Hub 路由节点，继承 Provider 功能并支持多跳路由 |
| **ignite-pay-hub-registry** | Hub 注册发现服务，PostgreSQL 后端 |
| **ignite-pay-mcp** | 用户端 MCP 代理服务，桥接移动端与状态通道 |
| **ignite-pay-merchant-mcp** | 商户端 MCP 代理服务，桥接商户系统与状态通道 |
| **Sentinel (Flutter)** | 用户移动端 App |
| **Ignite Merchant (Flutter)** | 商户端 App |

### 1.2 服务与端口总览

| 服务 | 二进制/目录 | 端口 | 传输协议 | 存储 |
|:-----|:-----------|:-----|:---------|:-----|
| PostgreSQL | 外部依赖 | 5432 | TCP | PostgreSQL |
| Hub Registry | `ignite-pay-hub-registry` | 3004 | HTTP | PostgreSQL |
| DIDComm Router | `didcomm-router` | 8080 | HTTP + WS | sled |
| DIDComm Router (商户侧) | `didcomm-router` (同二进制，不同配置) | 4000 | HTTP + WS | sled |
| DID Registry | `did-registry` | 8081 | HTTP | sled |
| Channel Hub | `channel-hub` | 3003 | HTTP + WS | sled |
| Channel Provider | `channel-provider` | 3002 | HTTP + WS | sled |
| Channel User | `channel-user` | 3001 | HTTP + WS | sled |
| User MCP | `ignite-pay-mcp` | stdio | JSON-RPC + WS -> :8080 | sled |
| Merchant MCP | `ignite-pay-merchant-mcp` | stdio | JSON-RPC + WS -> :4000 (商户侧 Router) | sled |

### 1.3 依赖关系图

```mermaid
graph TD
    PG[PostgreSQL :5432] --> HR[Hub Registry :3004]
    SOL[Solana Blockchain] --> DU[Channel User :3001]
    SOL --> DP[Channel Provider :3002]
    SOL --> DH[Channel Hub :3003]
    SOL --> DR[DID Registry :8081]

    DR --> SOL
    DH --> HR

    DCR[DIDComm Router :8080<br/>实例 1] --> UMCP[User MCP stdio]
    DCR2[DIDComm Router :4000<br/>实例 2，同二进制] --> MMCP[Merchant MCP stdio]

    UMCP --> DCR
    UMCP --> DU
    MMCP --> DCR2
    MMCP --> DH

    APP[Sentinel App] --> UMCP
    APP --> DCR
    MAPP[Merchant App] --> MMCP
    MAPP --> DCR2
```

---

## 2. 环境要求

### 2.1 工具链

| 工具 | 版本要求 | 用途 |
|:-----|:---------|:-----|
| Rust | 1.75+ | 编译所有 Rust crate |
| Solana CLI | 1.18+ | 部署链上程序、生成密钥 |
| Anchor CLI | 0.31.1+ | 构建链上程序 |
| PostgreSQL | 14+ | Hub Registry 数据库 |
| Flutter | 3.x | 编译移动端 App（可选） |

### 2.2 运行时依赖

| 依赖 | 用途 |
|:-----|:-----|
| Solana RPC | 链上交易提交和状态查询 |
| Photon RPC (Helius) | ZK Compression 证明服务（DID 程序） |
| Firebase (可选) | FCM 推送通知（DIDComm Router） |

### 2.3 操作系统

- **生产环境**：Linux (Ubuntu 22.04 LTS / Debian 12 推荐)
- **开发环境**：Linux / macOS / Windows (WSL2)

### 2.4 硬件建议

| 组件 | 最低配置 | 推荐配置 |
|:-----|:---------|:---------|
| CPU | 2 核 | 4 核 |
| 内存 | 4 GB | 8 GB |
| 磁盘 | 40 GB SSD | 100 GB SSD |
| 网络 | 10 Mbps | 50 Mbps |

---

## 3. 网络拓扑图

```
                              ┌─────────────────────┐
                              │   Solana Blockchain  │
                              │   (Devnet/Mainnet)   │
                              └──────────┬───────────┘
                                         │ RPC
                 ┌───────────────────────┼───────────────────────┐
                 │                       │                       │
        ┌────────▼────────┐    ┌─────────▼─────────┐   ┌────────▼────────┐
        │  DID Registry   │    │  Channel Services  │   │  DID Program    │
        │    :8081         │    │                    │   │  (链上)          │
        │  sled DB         │    │  User   :3001      │   │  State Channel  │
        └────────┬─────────┘    │  Provider :3002    │   │  Program        │
                 │              │  Hub     :3003     │   └────────────────┘
                 │              │  (各含 sled DB)    │
                 │              └────────┬───────────┘
                 │                       │
        ┌────────▼───────────────────────▼───────────────────────┐
        │              Nginx / Reverse Proxy (TLS)               │
        │     :443 → :3001  :3002  :3003  :8080  :8081          │
        └────────┬───────────────────────────────────────────────┘
                 │
      ┌──────────┼──────────────────────────┐
      │          │                          │
┌─────▼─────┐  ┌─▼──────────┐  ┌───────────▼──────────┐
│ DIDComm   │  │ DIDComm    │  │  Hub Registry        │
│ Router    │  │ Router     │  │    :3004              │
│ :8080     │  │ :4000      │  │  PostgreSQL :5432     │
│ sled DB   │  │ (商户侧)   │  └──────────────────────┘
│ +FCM(可选) │  │ sled DB    │
└─────┬─────┘  └─────┬──────┘
      │               │
      │  WebSocket    │  WebSocket
      ▼               ▼
┌───────────┐  ┌──────────────┐
│ User MCP  │  │ Merchant MCP │
│ (stdio)   │  │ (stdio)      │
│ sled DB   │  │ sled DB      │
└─────┬─────┘  └──────┬───────┘
      │               │
      │ DIDComm       │ DIDComm
      ▼               ▼
┌───────────┐  ┌──────────────┐
│ Sentinel  │  │ Ignite       │
│ (Flutter) │  │ Merchant     │
│ 用户 App  │  │ (Flutter)    │
└───────────┘  └──────────────┘
```

### 端口连接矩阵

| 源服务 | 目标服务 | 目标端口 | 协议 |
|:-------|:---------|:---------|:-----|
| 所有通道服务 | Solana RPC | 443 | HTTPS |
| Channel Hub | Hub Registry | 3004 | HTTP |
| Channel Hub/Provider/User | Solana RPC | 443 | HTTPS |
| User MCP | DIDComm Router | 8080 | WS |
| Merchant MCP | DIDComm Router (商户侧实例) | 4000 | WS |
| User MCP | Channel User | 3001 | HTTP |
| Merchant MCP | Channel Hub | 3003 | HTTP+WS |
| DID Registry | Solana RPC + Photon | 443 | HTTPS |

---

## 4. 部署步骤（按依赖顺序）

> 以下步骤按依赖关系排列。每个步骤完成后应进行验证再继续。

### 步骤 1：部署 PostgreSQL

```bash
# Ubuntu/Debian
sudo apt update
sudo apt install postgresql postgresql-contrib

# 创建数据库和用户
sudo -u postgres psql <<EOF
CREATE USER ignite WITH PASSWORD 'ignite';
CREATE DATABASE hub_registry OWNER ignite;
EOF

# 验证
psql -U ignite -d hub_registry -h localhost -c "SELECT 1;"
```

**验证**: 连接成功即可。

---

### 步骤 2：部署链上 Solana 程序

#### 2a. 部署状态通道程序 (ignite-pay-program)

```bash
cd ignite-pay-program

# 编译
anchor build

# 部署到 Devnet
anchor deploy --provider.cluster devnet

# 记录 Program ID
# 当前: DJBHr35jL3JAGoU7bKMsEFmpeNMrCSK7oYQE4HJ3GBUe
```

#### 2b. 部署 DID 程序 (ignite-pay-did-program)

```bash
cd ignite-pay-did-program

# 编译
anchor build

# 部署到 Devnet
anchor deploy --provider.cluster devnet

# 记录 Program ID
# 更新 did-registry config.toml 中的 did_program_id
```

#### 2c. 初始化 PlatformConfig

部署后必须**一次性**调用 `init_platform` 指令，将平台 Ed25519 公钥写入链上 PDA。

```bash
# 使用 Anchor CLI 或 SDK 调用
# 参数: platform_ed25519_pubkey (32 字节)
# PDA seeds: [b"platform-config"]
```

**验证**: `solana program show <PROGRAM_ID> --url devnet`

---

### 步骤 3：部署 DIDComm Router

```bash
cd didcomm-router
cargo build --release

# 创建数据目录
mkdir -p ./data

# 编辑 config.toml（按需修改端口、FCM、TLS 等）
# 路由器不需要 DID，启动即可使用

# 启动
RUST_LOG=info ./target/release/didcomm-router ./config.toml
```

> **说明**：路由器不持有 DID 身份，仅做消息中继。WS 客户端通过 Ed25519 签名验证身份，无需为路由器预先配置密钥。

**验证**: `curl http://localhost:8080/health`

#### 3b. 部署商户侧 DIDComm Router (端口 4000)

商户侧 Router 是同一 `didcomm-router` 二进制的第二个实例，使用独立配置。

```bash
# 复用已编译的二进制
# 创建商户侧配置文件
mkdir -p ./config/merchant-router
cat > ./config/merchant-router/config.toml <<'EOF'
[server]
host = "0.0.0.0"
port = 4000

[router]
max_queued_messages = 1000
max_message_age_seconds = 86400

[storage]
path = "./data/merchant-router"
EOF

# 创建独立数据目录
mkdir -p ./data/merchant-router

# 启动第二个实例
RUST_LOG=info ./target/release/didcomm-router ./config/merchant-router/config.toml
```

> **说明**：两个 Router 实例使用不同的端口（8080 / 4000）和不同的 sled 数据目录。用户侧 MCP 连接 :8080，商户侧 MCP 连接 :4000。

**验证**: `curl http://localhost:4000/health`

---

### 步骤 4：部署 DID Registry

```bash
cd did-registry
cargo build --release --bin did-registry

# 准备密钥文件
solana-keygen new -o /path/to/payer-keypair.json
solana airdrop 2 <PAYER_ADDRESS> --url devnet

openssl rand -out /path/to/platform_signing.key 32
chmod 400 /path/to/platform_signing.key

# 编辑配置文件 config.toml
# 更新 did_program_id, payer_keypair_path, platform_signing_key_path 等

# 启动
RUST_LOG=info ./target/release/did-registry ./config.toml
```

**验证**: `curl http://localhost:8081/health` 返回 `ok`

---

### 步骤 5：部署 Channel User 服务

```bash
cd ignite-pay-channel-service
cargo build --release --bin channel-user

# 生成密钥
solana-keygen new --outfile ./keys/user.key

# 创建数据目录
mkdir -p ./data/channel_user

# 启动
RUST_LOG=info ./target/release/channel-user ./config.toml
```

**验证**: `curl http://localhost:3001/health`

---

### 步骤 6：部署 Channel Provider 服务

```bash
cd ignite-pay-channel-service
cargo build --release --bin channel-provider

# 生成密钥
solana-keygen new --outfile ./keys/provider.key

# 创建数据目录
mkdir -p ./data/channel_provider

# 启动
RUST_LOG=info ./target/release/channel-provider ./config-provider.toml
```

**验证**: `curl http://localhost:3002/health`

---

### 步骤 7：部署 Channel Hub 服务

```bash
cd ignite-pay-channel-service
cargo build --release --bin channel-hub

# 生成密钥
solana-keygen new --outfile ./keys/hub.key

# 创建数据目录
mkdir -p ./data/channel_hub

# 启动
RUST_LOG=info ./target/release/channel-hub ./config-hub.toml
```

**验证**: `curl http://localhost:3003/health`

---

### 步骤 8：部署 Hub Registry

```bash
cd ignite-pay-hub-registry
cargo build --release --bin ignite-pay-hub-registry

# 确保 PostgreSQL 已运行，hub_registry 数据库已创建

# 启动（数据库 schema 自动初始化）
RUST_LOG=info ./target/release/ignite-pay-hub-registry ./hub-registry.toml
```

**验证**: `curl http://localhost:3004/health`

---

### 步骤 9：部署 MCP 服务

#### 9a. User MCP

```bash
cd ignite-pay-mcp
cargo build --release --bin ignite-pay-mcp

# 编辑 config.toml
# 设置 mediator.ws_url, platform.did, solana 参数等

# MCP 通过 stdio 通信，通常由宿主进程启动
echo '{"jsonrpc":"2.0","method":"tools/list","id":1}' | ./target/release/ignite-pay-mcp ./config.toml
```

**验证**: JSON-RPC 响应正常。

#### 9b. Merchant MCP

```bash
cd ignite-pay-merchant-mcp
cargo build --release --bin ignite-pay-merchant-mcp

# 编辑 config.toml
# 设置 merchant.hub_endpoint, mediator.ws_url 等

echo '{"jsonrpc":"2.0","method":"tools/list","id":1}' | ./target/release/ignite-pay-merchant-mcp ./config.toml
```

**验证**: JSON-RPC 响应正常。

---

### 步骤 10：编译移动端应用（可选）

```bash
# 用户端 App
cd ignite_pay_app
flutter pub get
flutter run

# 商户端 App
cd ignite_pay_merchant_app
flutter pub get
flutter run
```

---

## 5. 配置文件详解

### 5.1 DIDComm Router (`didcomm-router/config.toml`)

```toml
[server]
host = "0.0.0.0"       # 监听地址
port = 8080             # 监听端口

[router]
# 内存中最大排队消息数
max_queued_messages = 1000
# 消息最大存活时间（秒）
max_message_age_seconds = 86400

# 可选：预配置对等节点
# [[router.known_peers]]
# did = "did:ignite:z6Mk..."
# key_agreement_kid = "did:ignite:z6Mk...#key-agreement-1"
# key_agreement_public_base64 = "..."

# 可选：内置 TLS（使用 nginx 反代时无需启用）
# [tls]
# cert_path = "./certs/tls.crt"
# key_path = "./certs/tls.key"

# 可选：FCM 推送
# [fcm]
# service_account_json = "./firebase-service-account.json"
# project_id = "ignite-pay-d1217"

[storage]
path = "./data"         # sled 持久化存储路径
```

> **说明**：路由器不使用 DID 身份。WS 客户端连接时，路由器发送 nonce 挑战，客户端用 Ed25519 私钥签名响应，路由器从客户端的 `did:ignite` 中提取公钥验证签名。

### 5.2 DID Registry (`did-registry/config.toml`)

```toml
[server]
host = "0.0.0.0"
port = 8081

[solana]
rpc_url = "https://api.devnet.solana.com"        # Solana RPC 端点
did_program_id = "<DEPLOYED_PROGRAM_ID>"           # 链上 DID 程序 ID
payer_keypair_path = ""                            # 交易支付方密钥路径（空=临时密钥）

[light]
photon_url = ""                                    # Photon RPC URL（ZK Compression）

[auth]
jwt_secret = "did-registry-secret"                 # JWT 签名密钥
platform_public_key = ""                           # 平台 Ed25519 公钥（Base64）
platform_signing_key_path = ""                     # 平台签名私钥文件路径

[fees]
register_fee_lamports = 5000                       # Sponsored 注册服务费
update_vc_fee_lamports = 2000                      # Sponsored VC 更新费
rotate_key_fee_lamports = 2000                     # Sponsored 密钥轮换费
```

| 字段 | 必填 | 说明 |
|:-----|:-----|:-----|
| `solana.did_program_id` | 是 | 部署后的 ignite-pay-did-program ID |
| `solana.payer_keypair_path` | 生产必填 | 空=临时密钥，仅开发用 |
| `light.photon_url` | 生产必填 | Helius Photon RPC URL |
| `auth.platform_signing_key_path` | 生产必填 | 32 字节 Ed25519 私钥 |

### 5.3 Channel Hub (`ignite-pay-channel-service/config-hub.toml`)

```toml
[server]
host = "0.0.0.0"
port = 3003

[solana]
rpc_url = "https://api.devnet.solana.com"
channel_program_id = "DJBHr35jL3JAGoU7bKMsEFmpeNMrCSK7oYQE4HJ3GBUe"
keypair_path = "./keys/hub.key"

[channel]
default_tree_depth = 4               # 默认 Merkle 树深度 (2^4=16 叶子)
default_challenge_duration = 5000    # 争议期 (slots，约 33 分钟)
default_min_challenge_delay = 1000   # 最短争议延迟 (slots)
default_settle_window = 10000        # 结算窗口 (slots)
auto_close_offset = 500000           # 自动关闭偏移 (slots)
db_path = "./data/channel_hub"       # sled 数据库路径

[compliance]
spending_threshold = 1000000000      # 累计消费阈值
per_channel_limit = 100000000        # 单通道最大支付
window_slots = 100000                # 滑动窗口 (slots)
travel_rule_threshold = 500000000    # Travel Rule 触发金额
```

### 5.4 Channel Provider (`ignite-pay-channel-service/config-provider.toml`)

```toml
[server]
host = "0.0.0.0"
port = 3002

[solana]
rpc_url = "https://api.devnet.solana.com"
channel_program_id = "DJBHr35jL3JAGoU7bKMsEFmpeNMrCSK7oYQE4HJ3GBUe"
keypair_path = "./keys/provider.key"

[channel]
default_tree_depth = 4
default_challenge_duration = 5000
default_min_challenge_delay = 1000
default_settle_window = 10000
auto_close_offset = 500000
db_path = "./data/channel_provider"
```

> Provider 角色无需 `[compliance]` 配置段，合规由 User 端管理。

### 5.5 Channel User (`ignite-pay-channel-service/config.toml`)

```toml
[server]
host = "0.0.0.0"
port = 3001

[solana]
rpc_url = "https://api.devnet.solana.com"
channel_program_id = "DJBHr35jL3JAGoU7bKMsEFmpeNMrCSK7oYQE4HJ3GBUe"
keypair_path = "./keys/user.key"

[channel]
default_tree_depth = 4
default_challenge_duration = 5000
default_min_challenge_delay = 1000
default_settle_window = 10000
auto_close_offset = 500000
db_path = "./data/channel_user"

[compliance]
spending_threshold = 1000000000
per_channel_limit = 100000000
window_slots = 100000
travel_rule_threshold = 500000000
```

### 5.6 User MCP (`ignite-pay-mcp/config.toml`)

```toml
[mediator]
ws_url = "ws://127.0.0.1:8080/ws"     # DIDComm Router WebSocket
phone_did = ""                          # 手机 App DID

[storage]
path = "./data"                         # sled 数据库路径

[policy]
auto_approve_max = 0                    # 自动批准的最大金额
auth_timeout = 300                      # 授权超时（秒）

[platform]
did = "did:ignite:zPlatformDIDPlaceholder"  # 平台 DID
verifying_key_b64 = ""                       # 平台 Ed25519 验证密钥

[ipfs]
mode = "mock"                           # IPFS 模式（mock/kubo）

[solana]
rpc_url = "https://api.devnet.solana.com"
tree_address = ""                       # Concurrent Merkle Tree 地址
tree_authority = ""                     # Tree 控制密钥
das_endpoint = ""                       # Helius DAS API 端点
pay_mode = "self_funded"                # 支付模式
default_owner = ""                      # 默认 owner 公钥
tree_authority_keypair_b58 = ""         # Tree authority 密钥（Base58）
```

### 5.7 Merchant MCP (`ignite-pay-merchant-mcp/config.toml`)

```toml
[merchant]
did = ""                                    # 商户 DID（首次运行自动生成）
hub_endpoint = "http://localhost:3003"       # Hub HTTP 端点
hub_ws_url = "ws://localhost:3003/ws"        # Hub WebSocket 端点

[mediator]
ws_url = "ws://localhost:4000/ws"            # DIDComm Router WebSocket（商户侧）

[storage]
path = "./data/merchant-mcp"                 # sled 数据库路径

[solana]
rpc_url = "https://api.devnet.solana.com"
program_id = ""                              # 状态通道程序 ID

[hub]
token_mint = ""                              # 默认 Token Mint
provider_pubkey = ""                         # Provider (Hub) 公钥
```

### 5.8 Hub Registry (`hub-registry.toml`)

```toml
[server]
host = "0.0.0.0"
port = 3004

[database]
url = "postgres://ignite:ignite@localhost:5432/hub_registry"
```

数据库 schema 在服务启动时自动初始化。

### 5.9 DIDComm Router — 商户侧 (`deploy/config/didcomm-router-merchant.toml`)

商户侧 DIDComm Router 是同一 `didcomm-router` 二进制的第二个实例，使用不同的配置文件：

```toml
[server]
host = "0.0.0.0"
port = 4000                  # 商户侧端口（区别于用户侧 :8080）

[router]
max_queued_messages = 1000
max_message_age_seconds = 86400

[storage]
path = "./data/merchant-router"   # 独立的 sled 数据目录
```

| 字段 | 说明 |
|:-----|:-----|
| `server.port` | 必须为 `4000`，与 Merchant MCP 的 `mediator.ws_url` 对应 |
| `storage.path` | 独立的 sled 数据目录，不能与用户侧 Router 共享 |

> 两个 Router 实例共享相同的源码（`didcomm-router`），仅配置不同。可选启用 FCM 推送和 TLS 配置，与用户侧配置方式相同。

---

## 6. 密钥管理

### 6.1 密钥类型总览

| 密钥 | 格式 | 用途 | 生成方式 |
|:-----|:-----|:-----|:---------|
| Solana Keypair | JSON 数组 (64 字节) | 通道服务签名、交易付费 | `solana-keygen new` |
| Platform Signing Key | 32 字节原始二进制 | DID Registry 平台签名、VC 签发 | `openssl rand -out file 32` |
| DID Identity | Ed25519 | `did:ignite` 去中心化身份 | `ignite-pay-core::identity` |
| FCM Service Account | JSON | Firebase 推送通知 | Firebase Console |

### 6.2 Solana Keypair 生成

```bash
# 创建密钥目录
mkdir -p ./keys

# 用户端密钥
solana-keygen new --outfile ./keys/user.key

# 商户端密钥
solana-keygen new --outfile ./keys/provider.key

# Hub 密钥
solana-keygen new --outfile ./keys/hub.key

# DID Registry 付费密钥
solana-keygen new --outfile ./keys/payer.json

# Devnet 空投
solana airdrop 2 $(solana-keygen pubkey ./keys/user.key) --url devnet
solana airdrop 2 $(solana-keygen pubkey ./keys/provider.key) --url devnet
solana airdrop 2 $(solana-keygen pubkey ./keys/hub.key) --url devnet
solana airdrop 2 $(solana-keygen pubkey ./keys/payer.json) --url devnet
```

> 如果 `keypair_path` 留空（`""`），服务启动时自动生成临时密钥（每次重启会变，仅测试用）。

### 6.3 DID 身份初始化

#### 移动端和 MCP DID

移动端 App（消费者/商户）和 MCP 服务的 DID 通过 `ignite-pay-core` 的 `identity` 模块在首次运行时自动生成：

```rust
use ignite_pay_core::identity::{generate_ignite_did, build_did_document, save_identity};

// 首次生成
let (identity, did) = generate_ignite_did();
save_identity(&db, &identity, &did)?;

// DID 格式: did:ignite:z + Base58(0xed 0x01 + Ed25519_PublicKey)
println!("DID: {}", did);
```

**DID 编码规则**：`did:ignite:z` + Base58(`0xed 0x01` + Ed25519 公钥)，其中 `0xed 0x01` 是 multicodec 中 Ed25519 公钥的标识前缀。

### 6.4 Platform Signing Key 生成

```bash
# 生成 32 字节随机私钥
openssl rand -out /path/to/platform_signing.key 32

# 设置权限（仅 owner 可读）
chmod 400 /path/to/platform_signing.key
```

### 6.5 密钥安全建议

| 级别 | 措施 |
|:-----|:-----|
| 必须 | 所有私钥文件设置 `chmod 400` |
| 必须 | 生产环境使用 HSM 或 KMS 管理签名密钥 |
| 必须 | Platform Signing Key 离线备份 |
| 建议 | Solana 收款密钥定期转出到冷钱包 |
| 建议 | 定期轮换 DID Controller Key |

---

## 7. Docker 部署

### 7.1 概述

所有服务均可容器化部署。推荐使用 docker-compose 编排多服务。以下描述各服务的 Docker 化方法。

### 7.2 各服务容器化要点

**DIDComm Router**

- 暴露端口 `8080`（用户侧）和 `4000`（商户侧）—— 同一二进制的两个实例
- 挂载 `config.toml` 和 `./data` 目录
- 无外部数据库依赖

**DID Registry**

- 暴露端口 `8081`
- 挂载密钥文件（payer keypair、platform signing key）
- 挂载 sled 数据目录

**Channel Services (User/Provider/Hub)**

- 分别暴露端口 `3001` / `3002` / `3003`
- 各自挂载独立配置文件和 sled 数据目录
- 各自挂载对应 keypair 文件

**Hub Registry**

- 暴露端口 `3004`
- 依赖 PostgreSQL 容器
- 挂载配置文件

**MCP Services**

- stdio 模式运行，不需要暴露端口
- 挂载配置文件和 sled 数据目录

### 7.3 Dockerfile 模板

```dockerfile
FROM rust:1.82-slim AS builder
WORKDIR /app
COPY . .
RUN cargo build --release -p <crate-name>

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/<binary> /usr/local/bin/
COPY <config-dir>/config.toml /etc/ignite-pay/config.toml
EXPOSE <port>
ENTRYPOINT ["<binary>", "/etc/ignite-pay/config.toml"]
```

### 7.4 docker-compose 完整配置

```yaml
# docker-compose.yml
# 使用: docker compose up -d

services:
  # ─── 基础设施层 ───
  postgres:
    image: postgres:16-bookworm
    restart: unless-stopped
    environment:
      POSTGRES_USER: ignite
      POSTGRES_PASSWORD: ${PG_PASSWORD:-ignite}
      POSTGRES_DB: hub_registry
    volumes:
      - pg_data:/var/lib/postgresql/data
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U ignite -d hub_registry"]
      interval: 10s
      timeout: 5s
      retries: 5
    networks:
      - backend

  # ─── 身份层 ───
  didcomm-router:
    build:
      context: .
      dockerfile: didcomm-router/Dockerfile
    restart: unless-stopped
    environment:
      - RUST_LOG=didcomm_router=info
      - JWT_SECRET=${JWT_SECRET:-change-me-in-production}
    volumes:
      - router_data:/app/data
      - ./didcomm-router/config.toml:/app/config.toml:ro
    expose:
      - "8080"
    networks:
      - backend

  didcomm-router-merchant:
    build:
      context: .
      dockerfile: didcomm-router/Dockerfile
    restart: unless-stopped
    environment:
      - RUST_LOG=didcomm_router=info
      - JWT_SECRET=${JWT_SECRET:-change-me-in-production}
    volumes:
      - router_merchant_data:/app/data
      - ./deploy/config/didcomm-router-merchant.toml:/app/config.toml:ro
    expose:
      - "4000"
    networks:
      - backend

  did-registry:
    build:
      context: .
      dockerfile: did-registry/Dockerfile
    restart: unless-stopped
    environment:
      - RUST_LOG=did_registry=info
    volumes:
      - did_registry_data:/app/data
      - ./did-registry/config.toml:/app/config.toml:ro
      - ./keys:/app/keys:ro
    expose:
      - "8081"
    networks:
      - backend

  # ─── 通道层 ───
  channel-user:
    build:
      context: .
      dockerfile: ignite-pay-channel-service/Dockerfile
    restart: unless-stopped
    environment:
      - RUST_LOG=info
    volumes:
      - channel_user_data:/app/data
      - ./ignite-pay-channel-service/config.toml:/app/config.toml:ro
      - ./keys:/app/keys:ro
    expose:
      - "3001"
    networks:
      - backend

  channel-provider:
    build:
      context: .
      dockerfile: ignite-pay-channel-service/Dockerfile
    restart: unless-stopped
    environment:
      - RUST_LOG=info
    volumes:
      - channel_provider_data:/app/data
      - ./ignite-pay-channel-service/config-provider.toml:/app/config.toml:ro
      - ./keys:/app/keys:ro
    expose:
      - "3002"
    networks:
      - backend

  channel-hub:
    build:
      context: .
      dockerfile: ignite-pay-channel-service/Dockerfile
    restart: unless-stopped
    environment:
      - RUST_LOG=info
    volumes:
      - channel_hub_data:/app/data
      - ./ignite-pay-channel-service/config-hub.toml:/app/config.toml:ro
      - ./keys:/app/keys:ro
    expose:
      - "3003"
    networks:
      - backend

  # ─── 注册层 ───
  hub-registry:
    build:
      context: .
      dockerfile: ignite-pay-hub-registry/Dockerfile
    restart: unless-stopped
    environment:
      - RUST_LOG=info
    volumes:
      - ./ignite-pay-hub-registry/hub-registry.toml:/app/hub-registry.toml:ro
    expose:
      - "3004"
    depends_on:
      postgres:
        condition: service_healthy
    networks:
      - backend

  # ─── 代理层（stdio 模式，由宿主进程管理）───
  # User MCP 和 Merchant MCP 通过 stdio 通信，不适合直接放入 docker-compose。
  # 如需容器化运行，建议使用 supervisord 或自定义 entrypoint 包装。

  # ─── Nginx 反向代理 ───
  nginx:
    image: nginx:1.27-bookworm
    restart: unless-stopped
    ports:
      - "80:80"
      - "443:443"
    volumes:
      - ./deploy/nginx/nginx.conf:/etc/nginx/conf.d/default.conf:ro
      - ./deploy/certs:/etc/nginx/certs:ro
    depends_on:
      - didcomm-router
      - did-registry
      - channel-user
      - channel-provider
      - channel-hub
      - hub-registry
    networks:
      - backend

volumes:
  pg_data:
  router_data:
  router_merchant_data:
  did_registry_data:
  channel_user_data:
  channel_provider_data:
  channel_hub_data:

networks:
  backend:
    driver: bridge
```

> **配置要点**：
> - MCP 服务（User MCP、Merchant MCP）使用 stdio 通信，不适合直接容器化。如需容器化，需自定义 entrypoint 包装或使用 supervisord
> - `didcomm-router-merchant` 需要独立的配置文件 `deploy/config/didcomm-router-merchant.toml`（见 5.9 节）
> - 密钥文件通过 `./keys` 目录只读挂载
> - 使用 named volumes 持久化 sled 和 PostgreSQL 数据
> - 所有后端服务仅在内网 `backend` 网络中通信，仅 Nginx 对外暴露端口

---

## 8. 生产环境注意事项

### 8.1 TLS 与 Nginx 反向代理

所有对外暴露的服务均应通过 Nginx 反向代理启用 TLS。

#### Nginx 主配置框架

```nginx
# 通用 SSL 配置
ssl_protocols TLSv1.2 TLSv1.3;
ssl_ciphers HIGH:!aNULL:!MD5;
ssl_prefer_server_ciphers on;

# WebSocket 通用 location 模板
# 用于所有需要 WS 的服务
```

#### Channel User 反向代理

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

#### Channel Provider 反向代理

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

#### Channel Hub 反向代理

```nginx
server {
    listen 443 ssl;
    server_name hub.ignite-pay.example.com;

    ssl_certificate     /etc/ssl/certs/ignite-pay.pem;
    ssl_certificate_key /etc/ssl/private/ignite-pay.key;

    location / {
        proxy_pass http://127.0.0.1:3003;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
    }

    location /ws {
        proxy_pass http://127.0.0.1:3003;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
    }
}
```

#### DIDComm Router 反向代理

```nginx
server {
    listen 443 ssl;
    server_name didcomm.ignite-pay.example.com;

    ssl_certificate     /etc/ssl/certs/ignite-pay.pem;
    ssl_certificate_key /etc/ssl/private/ignite-pay.key;

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
    }

    location /ws {
        proxy_pass http://127.0.0.1:8080;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
        # WebSocket 长连接超时
        proxy_read_timeout 3600s;
        proxy_send_timeout 3600s;
    }
}
```

#### DID Registry 反向代理

```nginx
server {
    listen 443 ssl;
    server_name did-registry.ignite-pay.example.com;

    ssl_certificate     /etc/ssl/certs/ignite-pay.pem;
    ssl_certificate_key /etc/ssl/private/ignite-pay.key;

    location / {
        proxy_pass http://127.0.0.1:8081;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
    }
}
```

#### Hub Registry 反向代理

```nginx
server {
    listen 443 ssl;
    server_name hub-registry.ignite-pay.example.com;

    ssl_certificate     /etc/ssl/certs/ignite-pay.pem;
    ssl_certificate_key /etc/ssl/private/ignite-pay.key;

    location / {
        proxy_pass http://127.0.0.1:3004;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
    }
}
```

> **说明**：DID Registry 和 Hub Registry 无 WebSocket 端点，仅配置 HTTP 反向代理即可。如果这两个服务仅在内网使用（不对外暴露），可以省略 Nginx 配置，直接通过内网地址访问。
```

### 8.2 systemd 服务配置

#### Channel User

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

#### Channel Provider

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

#### Channel Hub

```ini
[Unit]
Description=Ignite Pay Channel Hub Service
After=network.target

[Service]
Type=simple
User=ignite
WorkingDirectory=/opt/ignite-pay
ExecStart=/opt/ignite-pay/channel-hub /opt/ignite-pay/config-hub.toml
Environment=RUST_LOG=info
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

#### DIDComm Router

```ini
[Unit]
Description=Ignite Pay DIDComm Router
After=network.target

[Service]
Type=simple
User=ignite
WorkingDirectory=/opt/ignite-pay
ExecStart=/opt/ignite-pay/didcomm-router /opt/ignite-pay/didcomm-router-config.toml
Environment=RUST_LOG=info
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

#### DID Registry

```ini
[Unit]
Description=Ignite Pay DID Registry
After=network.target

[Service]
Type=simple
User=ignite
WorkingDirectory=/opt/ignite-pay
ExecStart=/opt/ignite-pay/did-registry /opt/ignite-pay/did-registry-config.toml
Environment=RUST_LOG=info
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

#### Hub Registry

```ini
[Unit]
Description=Ignite Pay Hub Registry
After=network.target postgresql.service
Requires=postgresql.service

[Service]
Type=simple
User=ignite
WorkingDirectory=/opt/ignite-pay
ExecStart=/opt/ignite-pay/ignite-pay-hub-registry /opt/ignite-pay/hub-registry.toml
Environment=RUST_LOG=info
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

#### DIDComm Router — 商户侧

```ini
[Unit]
Description=Ignite Pay DIDComm Router (Merchant)
After=network.target

[Service]
Type=simple
User=ignite
WorkingDirectory=/opt/ignite-pay
ExecStart=/opt/ignite-pay/didcomm-router /opt/ignite-pay/didcomm-router-merchant-config.toml
Environment=RUST_LOG=info
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

#### 启用和启动服务

```bash
sudo systemctl daemon-reload
sudo systemctl enable ignite-channel-user ignite-channel-provider ignite-channel-hub \
                     ignite-didcomm-router ignite-didcomm-router-merchant \
                     ignite-did-registry ignite-hub-registry
sudo systemctl start ignite-didcomm-router ignite-didcomm-router-merchant \
                      ignite-did-registry ignite-channel-user \
                      ignite-channel-provider ignite-channel-hub ignite-hub-registry
```

### 8.3 日志管理

```bash
# 查看 systemd 服务日志
sudo journalctl -u ignite-channel-hub -f

# 按时间范围查看
sudo journalctl -u ignite-channel-hub --since "2025-01-01 00:00:00" --until "2025-01-02 00:00:00"

# 按日志级别
RUST_LOG=debug  # trace / debug / info / warn / error
```

建议配置 journald 或 logrotate 进行日志轮转。

### 8.4 监控建议

| 指标 | 监控对象 | 告警阈值 | 处理措施 |
|:-----|:---------|:---------|:---------|
| 可用流动性 | Channel Hub | < 2x 平均路由量 | 补充流动性 |
| 通道成功率 | Channel Hub | < 95% | 检查通道状态 |
| 平均延迟 | 所有通道服务 | > 200ms | 优化网络/节点 |
| sled 数据库大小 | 所有通道服务 | > 2 GB | 归档历史数据 |
| 配签延迟 | Provider | > 500ms | 优化节点性能 |
| HTLC 过期率 | Provider | > 1% | 检查原像揭示流程 |
| 过期多跳支付 | Hub | > 5% | 调整 timelock |
| PostgreSQL 连接数 | Hub Registry | > 80% max | 扩容 |
| 活跃通道数趋势 | Hub/Provider | 持续下降 | 检查服务质量 |

### 8.5 Solana RPC 端点

| 环境 | 推荐方案 |
|:-----|:---------|
| 开发 | `https://api.devnet.solana.com`（免费，有限速） |
| 生产 | 私有 RPC 节点或 Helius/QuickNode/Alchemy 等付费服务 |

生产环境务必使用私有 RPC 端点，避免公共 RPC 的速率限制影响交易提交。

### 8.6 防火墙与安全组规则

#### 网络分区设计

```
┌─────────────────────────────────────────────────────────────┐
│                     公网 (Internet)                          │
│             仅 Nginx :443 对外暴露                           │
└───────────────────────────┬─────────────────────────────────┘
                            │
┌───────────────────────────▼─────────────────────────────────┐
│                   DMZ / 前端网络                             │
│  Nginx 反向代理 :443 → 各后端服务                           │
└───────────────────────────┬─────────────────────────────────┘
                            │
┌───────────────────────────▼─────────────────────────────────┐
│                   后端服务网络                               │
│  Channel User :3001    Channel Provider :3002               │
│  Channel Hub :3003     DIDComm Router :8080 / :4000         │
│  DID Registry :8081    Hub Registry :3004                   │
└───────────────────────────┬─────────────────────────────────┘
                            │
┌───────────────────────────▼─────────────────────────────────┐
│                   数据层网络                                 │
│  PostgreSQL :5432 (仅 Hub Registry 可访问)                   │
│  sled 数据目录 (各服务本地)                                  │
│  Solana RPC :443 (出站 HTTPS)                               │
└─────────────────────────────────────────────────────────────┘
```

#### iptables 规则示例

```bash
#!/bin/bash
# firewall-rules.sh

# 默认策略
iptables -P INPUT DROP
iptables -P FORWARD DROP
iptables -P OUTPUT ACCEPT

# 允许已建立的连接
iptables -A INPUT -m state --state ESTABLISHED,RELATED -j ACCEPT

# 允许本地回环
iptables -A INPUT -i lo -j ACCEPT

# SSH 管理（限制来源 IP）
iptables -A INPUT -p tcp --dport 22 -s <ADMIN_CIDR> -j ACCEPT

# Nginx 对外暴露
iptables -A INPUT -p tcp --dport 443 -j ACCEPT
iptables -A INPUT -p tcp --dport 80 -j ACCEPT    # HTTP→HTTPS 重定向

# 后端服务端口 — 仅允许本机和内网访问
# DIDComm Router
iptables -A INPUT -p tcp --dport 8080 -s 127.0.0.1 -j ACCEPT
iptables -A INPUT -p tcp --dport 4000 -s 127.0.0.1 -j ACCEPT
# DID Registry
iptables -A INPUT -p tcp --dport 8081 -s 127.0.0.1 -j ACCEPT
# Channel Services
iptables -A INPUT -p tcp --dport 3001 -s 127.0.0.1 -j ACCEPT
iptables -A INPUT -p tcp --dport 3002 -s 127.0.0.1 -j ACCEPT
iptables -A INPUT -p tcp --dport 3003 -s 127.0.0.1 -j ACCEPT
# Hub Registry
iptables -A INPUT -p tcp --dport 3004 -s 127.0.0.1 -j ACCEPT
# PostgreSQL
iptables -A INPUT -p tcp --dport 5432 -s 127.0.0.1 -j ACCEPT

# 记录被拒绝的连接
iptables -A INPUT -j LOG --log-prefix "DROPPED: " --log-level 4
```

#### 端口暴露策略总览

| 端口 | 服务 | 对外暴露 | 访问范围 |
|:-----|:-----|:---------|:---------|
| 443 | Nginx (TLS) | **是** | 公网 |
| 80 | Nginx (HTTP→HTTPS) | **是** | 公网 |
| 22 | SSH | 限制 | 管理网段 |
| 8080 | DIDComm Router | 否 | localhost / 内网 |
| 4000 | DIDComm Router (商户侧) | 否 | localhost / 内网 |
| 8081 | DID Registry | 否 | localhost / 内网 |
| 3001 | Channel User | 否 | localhost / 内网 |
| 3002 | Channel Provider | 否 | localhost / 内网 |
| 3003 | Channel Hub | 否 | localhost / 内网 |
| 3004 | Hub Registry | 否 | localhost / 内网 |
| 5432 | PostgreSQL | 否 | localhost only |

---

## 9. 健康检查

### 9.1 各服务健康检查端点

| 服务 | 端点 | 预期响应 |
|:-----|:-----|:---------|
| DIDComm Router | `GET http://localhost:8080/health` | HTTP 200 |
| DIDComm Router (商户侧) | `GET http://localhost:4000/health` | HTTP 200 |
| DID Registry | `GET http://localhost:8081/health` | `ok` |
| Channel User | `GET http://localhost:3001/health` | HTTP 200 |
| Channel Provider | `GET http://localhost:3002/health` | HTTP 200 |
| Channel Hub | `GET http://localhost:3003/health` | HTTP 200 |
| Hub Registry | `GET http://localhost:3004/health` | HTTP 200 |

### 9.2 验证脚本

```bash
#!/bin/bash
# ignite-pay-healthcheck.sh

check() {
    local name=$1
    local url=$2
    local expected=$3

    response=$(curl -s -o /dev/null -w "%{http_code}" "$url" 2>/dev/null)
    if [ "$response" = "200" ] || [ "$response" = "$expected" ]; then
        echo "[OK] $name ($url)"
    else
        echo "[FAIL] $name ($url) - HTTP $response"
    fi
}

check "DIDComm Router"           "http://localhost:8080/health"
check "DIDComm Router (Merchant)" "http://localhost:4000/health"
check "DID Registry"             "http://localhost:8081/health"
check "Channel User"     "http://localhost:3001/health"
check "Channel Provider" "http://localhost:3002/health"
check "Channel Hub"      "http://localhost:3003/health"
check "Hub Registry"     "http://localhost:3004/health"

# PostgreSQL 使用 pg_isready 检查（curl 无法检测原始 TCP 端口）
if pg_isready -h localhost -p 5432 -U ignite > /dev/null 2>&1; then
    echo "[OK] PostgreSQL (localhost:5432)"
else
    echo "[FAIL] PostgreSQL (localhost:5432)"
fi
```

### 9.3 systemd 健康检查配置

可在 systemd unit 中添加健康检查：

```ini
[Service]
# ... 其他配置 ...

ExecStartPost=/bin/sleep 2
ExecStartPost=/usr/bin/curl -sf http://localhost:3003/health

# 健康检查失败时自动重启
WatchdogSec=30
```

### 9.4 验证通道服务 API 功能

```bash
# 查询通道列表
curl http://localhost:3001/v1/channels

# Hub 注册信息
curl http://localhost:3003/v1/hub/info

# 路由发现测试
curl -X POST http://localhost:3003/v1/routes/find \
  -H "Content-Type: application/json" \
  -d '{
    "from_did_hash": "hex...",
    "to_did_hash": "hex...",
    "amount": 1000000,
    "token_mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
    "max_hops": 3
  }'

# DID 解析
curl http://localhost:8081/v1/did/resolve/did:ignite:z6Mk...

# Hub 列表
curl http://localhost:3004/v1/hubs?status=active
```

---

## 10. 故障排查

### 10.1 常见启动错误

#### 错误：端口被占用

```
Error: Address already in use (os error 98)
```

**排查**：
```bash
# 查找占用端口的进程
sudo lsof -i :3003
# 或
sudo ss -tlnp | grep 3003

# 终止进程
kill <PID>
```

#### 错误：sled 数据库锁冲突

```
Error: database is already open in another process
```

**排查**：
- sled 不支持多进程同时打开同一数据库
- 确保没有残留进程
- 异常退出后删除 `*.lock` 文件：
```bash
rm ./data/channel_hub/*.lock
```

#### 错误：Solana RPC 连接失败

```
Error: Failed to connect to Solana RPC
```

**排查**：
```bash
# 测试 RPC 连通性
curl -s -X POST https://api.devnet.solana.com \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"getHealth","id":1}'

# 检查 RPC 响应
# 正常: {"jsonrpc":"2.0","result":"ok","id":1}
```

#### 错误：PostgreSQL 连接失败 (Hub Registry)

```
Error: connection refused (os error 111)
```

**排查**：
```bash
# 检查 PostgreSQL 状态
sudo systemctl status postgresql

# 测试连接
psql -U ignite -d hub_registry -h localhost -c "SELECT 1;"

# 检查 pg_hba.conf 是否允许密码认证
sudo cat /etc/postgresql/14/main/pg_hba.conf | grep ignite
```

#### 错误：密钥文件不存在

```
Error: No such file or directory (keypair_path)
```

**排查**：
- 检查配置文件中的路径是否正确
- 使用绝对路径而非相对路径
- 确认文件权限：`ls -la ./keys/`

### 10.2 签名验证失败

**现象**：`verify_leaf_update_signature` 或链上 `InvalidSignature`

**排查**：
1. 检查签名者公钥是否正确（User 或 Provider）
2. 检查 `prev_leaf_hash` 是否匹配当前叶子
3. 检查 sequence 是否连续
4. 确认使用相同的 `channel_id`
5. 链上验证使用不同的消息格式（三族签名消息），确认使用正确族

### 10.3 金额守恒错误

**现象**：`AmountConservation { expected, actual }`

**排查**：
1. 拆分树时确保所有叶子金额之和 = `total_deposited`
2. Pipeline 操作中 partial_transfer 的金额不超过源叶子
3. 检查是否有并发修改

### 10.4 Merkle Proof 验证失败

**现象**：链上 `ProofVerificationFailed`

**排查**：
1. 确认离链 `MerkleTree` 使用排序对哈希：`hashv(&[min, max])`
2. 检查叶子是否在正确的索引位置
3. 确认 `current_root` 是最新的

### 10.5 HTLC 超时问题

**现象**：`HtlcNotExpired` 或 `HtlcExpired`

**排查**：
1. Solana slot 时间：1 slot 约 400ms（正常），devnet 可能更慢
2. 检查 `timelock_slot` 满足约束：`> current_slot + challenge_duration + HTLC_SAFETY_MARGIN`
3. 多跳时检查 timelock 递减是否正确
4. `HTLC_SAFETY_MARGIN` = 1000 slots（约 6.7 分钟）

### 10.6 DIDComm Router WebSocket 断连

**现象**：MCP 服务与 Router 的 WebSocket 频繁断开

**排查**：
1. 检查 Nginx WebSocket 超时配置（`proxy_read_timeout`）
2. 检查网络稳定性
3. 查看 Router 日志中是否有 `max_queued_messages` 触发
4. 确认 `max_message_age_seconds` 配置合理

### 10.7 Hub 路由发现无结果

**现象**：`POST /v1/routes/find` 返回空路由

**排查**：
1. 确认 Hub 已注册：`GET /v1/hub/info`
2. 确认路由图有边：`POST /v1/routes/refresh`
3. 确认 Hub 有足够流动性
4. 检查 `from_did_hash` 和 `to_did_hash` 是否正确

### 10.8 日志级别调整

```bash
# 临时调整（重启失效）
RUST_LOG=debug ./channel-hub ./config-hub.toml

# 按模块过滤
RUST_LOG=ignite_pay_channel_service=debug,info ./channel-hub ./config-hub.toml

# systemd 服务调整
sudo systemctl edit ignite-channel-hub
# 添加:
# [Service]
# Environment=RUST_LOG=debug
sudo systemctl restart ignite-channel-hub
```

---

## 11. 备份与恢复

### 11.1 备份范围

| 数据类型 | 存储位置 | 备份方式 | 频率建议 |
|:---------|:---------|:---------|:---------|
| 通道 sled 数据 | `./data/channel_user/`, `./data/channel_provider/`, `./data/channel_hub/` | 文件系统快照 | 每日 |
| DID Registry sled | `./did_registry_data/`（硬编码路径） | 文件系统快照 | 每日 |
| DIDComm Router sled | `./data/` | 文件系统快照 | 每日 |
| MCP sled 数据 | `./data/`, `./data/merchant-mcp/` | 文件系统快照 | 每日 |
| PostgreSQL (Hub Registry) | PostgreSQL 数据目录 | `pg_dump` | 每日 |
| 密钥文件 | `./keys/` | 离线备份 | 变更时 |
| 配置文件 | 各服务 `config.toml` | 版本控制 | 变更时 |

### 11.2 sled 数据库备份

sled 不支持热备份 API（无 `sled::export`），备份时必须停止写入或使用文件系统快照。

**方法一：停服务备份（推荐用于小规模部署）**

```bash
#!/bin/bash
# backup-sled.sh
BACKUP_DIR="/opt/ignite-pay/backups/$(date +%Y%m%d_%H%M%S)"
mkdir -p "$BACKUP_DIR"

# 停止所有服务
sudo systemctl stop ignite-channel-hub ignite-channel-provider ignite-channel-user \
                     ignite-didcomm-router ignite-did-registry

# 复制 sled 数据目录
cp -r /opt/ignite-pay/data/channel_hub   "$BACKUP_DIR/channel_hub"
cp -r /opt/ignite-pay/data/channel_provider "$BACKUP_DIR/channel_provider"
cp -r /opt/ignite-pay/data/channel_user  "$BACKUP_DIR/channel_user"
cp -r /opt/ignite-pay/did_registry_data  "$BACKUP_DIR/did_registry_data"

# 重启服务
sudo systemctl start ignite-didcomm-router ignite-did-registry ignite-channel-user \
                      ignite-channel-provider ignite-channel-hub

echo "Backup completed: $BACKUP_DIR"
```

**方法二：文件系统快照（推荐用于生产环境）**

如果数据目录位于 LVM 或 ZFS 卷上，使用快照可在不停止服务的情况下备份：

```bash
# LVM 快照
lvcreate -L 1G -s -n ignite_data_snap /dev/vg0/ignite_data
mkdir -p /mnt/snap && mount /dev/vg0/ignite_data_snap /mnt/snap
cp -r /mnt/snap/ /opt/ignite-pay/backups/$(date +%Y%m%d_%H%M%S)/
umount /mnt/snap && lvremove -f /dev/vg0/ignite_data_snap

# ZFS 快照
zfs snapshot pool0/ignite-data@backup-$(date +%Y%m%d_%H%M%S)
zfs send pool0/ignite-data@backup-... | gzip > /opt/ignite-pay/backups/snap-$(date +%Y%m%d).zfs.gz
```

> **注意**：sled 在写入时可能存在内存中未刷盘的数据。33 个 `.flush()` 调用确保关键写入后立即持久化，但快照仍可能丢失最后一次未 flush 的写入。生产环境建议在低峰期执行备份。

### 11.3 PostgreSQL 备份

```bash
#!/bin/bash
# backup-postgres.sh
BACKUP_DIR="/opt/ignite-pay/backups/postgres"
mkdir -p "$BACKUP_DIR"

# 全库逻辑备份
pg_dump -U ignite -d hub_registry -F c \
  -f "$BACKUP_DIR/hub_registry_$(date +%Y%m%d_%H%M%S).dump"

# 保留最近 7 天的备份
find "$BACKUP_DIR" -name "*.dump" -mtime +7 -delete
```

恢复：

```bash
# 恢复到新数据库
pg_restore -U ignite -d hub_registry_restored \
  "$BACKUP_DIR/hub_registry_20250101_120000.dump"
```

### 11.4 密钥备份

```bash
#!/bin/bash
# backup-keys.sh — 必须离线存储，加密保护
tar czf - /opt/ignite-pay/keys/ | \
  gpg --symmetric --cipher-algo AES256 \
  -o "/opt/ignite-pay/backups/keys_$(date +%Y%m%d).tar.gz.gpg"
```

> 密钥备份必须存储在独立于服务器的离线介质上（如 USB 加密盘、离线 HSM 备份）。

### 11.5 灾难恢复流程

| 场景 | 恢复步骤 |
|:-----|:---------|
| **sled 数据损坏** | 1. 停止对应服务 → 2. 删除损坏目录 → 3. 从备份恢复 → 4. 重启服务 |
| **PostgreSQL 数据丢失** | 1. 确认 PostgreSQL 运行 → 2. `pg_restore` 恢复 → 3. 重启 Hub Registry |
| **整台服务器故障** | 1. 新服务器安装环境 → 2. 恢复配置文件和密钥 → 3. 启动 PostgreSQL 并恢复 → 4. 恢复 sled 数据 → 5. 按依赖顺序启动所有服务 |
| **密钥泄露** | 1. 生成新密钥 → 2. 更新配置文件 → 3. 通道服务需重新开通道（旧通道无法恢复） → 4. DID Registry 需执行密钥轮换 |

### 11.6 已知限制

| 限制 | 说明 |
|:-----|:-----|
| sled 无热备份 API | 无法在不停止写入的情况下获得一致性快照 |
| 无自动化备份机制 | 项目中无备份脚本，需运维手动配置（见上方脚本模板） |
| 审计日志恢复 | 支持 `restore_from_ipfs()` 从 IPFS 恢复审计日志，但依赖 IPFS 数据可用性 |
| 通道状态不可重建 | sled 数据丢失意味着通道状态丢失，只能等待链上争议期过期后结算 |

---

## 12. 升级与回滚

### 12.1 升级原则

- **滚动升级**：服务可独立升级，无需全系统停机
- **向后兼容**：sled 数据格式变更需支持自动迁移或提供迁移工具
- **链上程序不可变**：Solana 程序部署后不可修改，只能部署新版本（新 Program ID）

### 12.2 链下服务升级流程

```bash
# 1. 构建新版本
cd ignite-pay-channel-service
git pull origin master
cargo build --release --bin channel-hub

# 2. 备份当前二进制和数据
cp /opt/ignite-pay/channel-hub /opt/ignite-pay/channel-hub.bak.$(date +%Y%m%d)

# 3. 替换二进制
cp target/release/channel-hub /opt/ignite-pay/channel-hub

# 4. 重启服务
sudo systemctl restart ignite-channel-hub

# 5. 验证
sleep 3
curl -sf http://localhost:3003/health || {
  # 回滚
  cp /opt/ignite-pay/channel-hub.bak.$(date +%Y%m%d) /opt/ignite-pay/channel-hub
  sudo systemctl restart ignite-channel-hub
  echo "Rollback executed!"
}
```

### 12.3 PostgreSQL Schema 迁移

当前 Hub Registry 使用 `include_str!()` 在启动时执行 `migrations/001_init.sql`，**无版本追踪**。`CREATE TABLE` 语句在表已存在时会失败。

升级策略：

1. **新增迁移文件**：按序号创建 `002_xxx.sql`，使用 `CREATE TABLE IF NOT EXISTS` 或 `ALTER TABLE`
2. **代码中顺序执行**：在 `main.rs` 中依次 `include_str!` 所有迁移文件
3. **幂等设计**：每个迁移文件应可重复执行不报错

```rust
// 建议的 main.rs 迁移执行模式
let migrations = [
    include_str!("../migrations/001_init.sql"),
    include_str!("../migrations/002_xxx.sql"),  // 新增迁移
];
for sql in &migrations {
    sqlx::query(sql).execute(&pool).await?;
}
```

### 12.4 链上程序升级

Solana 链上程序升级流程：

```bash
# 1. 构建新版本
cd ignite-pay-program
anchor build

# 2. 部署到同一 Program ID（需要升级权限）
anchor deploy --provider.cluster devnet \
  --program-name ignite_pay_program \
  --program-keypair ./keys/program-keypair.json

# 3. 验证
solana program show <PROGRAM_ID> --url devnet
```

> **注意**：链上程序升级需要拥有升级权限的密钥。生产环境建议使用多签治理（如 Squads Protocol）管理升级权限。

### 12.5 回滚策略

| 组件 | 回滚方式 | 注意事项 |
|:-----|:---------|:---------|
| 链下服务 | 替换二进制 + 重启 | sled 数据格式需向后兼容 |
| PostgreSQL | `pg_restore` 恢复备份 | 会丢失上次备份后的数据 |
| 链上程序 | 部署旧版本 `.so` | 需要升级权限密钥 |
| 配置文件 | 替换配置 + 重启 | 密钥变更需同步更新相关服务 |

---

## 13. 环境变量参考

### 13.1 运行时环境变量

| 变量 | 适用服务 | 默认值 | 说明 |
|:-----|:---------|:-------|:-----|
| `RUST_LOG` | 所有服务 | 见下表 | `tracing_subscriber` 日志级别过滤器 |
| `JWT_SECRET` | DIDComm Router | 随机 UUID v4（每次启动生成） | JWT 签名密钥，也可通过 `config.toml` `[router] jwt_secret` 设置 |
| `IGNITE_PAY_CONFIG` | User MCP | `"config.toml"` | 配置文件路径（覆盖 CLI 参数） |
| `IGNITE_MERCHANT_CONFIG` | Merchant MCP | `"config.toml"` | 配置文件路径（覆盖 CLI 参数） |
| `AUDIT_LOG_DIR` | User MCP, Merchant MCP | （未设置 = 仅输出到 stderr） | 设置后启用按日滚动审计日志文件 |

### 13.2 各服务 RUST_LOG 默认值

| 服务 | 默认值 | 示例覆盖 |
|:-----|:-------|:---------|
| DIDComm Router | `didcomm_router=info` | `didcomm_router=debug` |
| DID Registry | `did_registry=info` | `did_registry=debug,sqlx=warn` |
| Channel User | `info` | `ignite_pay_channel_service=debug` |
| Channel Provider | `info` | `ignite_pay_channel_service=debug` |
| Channel Hub | `info` | `ignite_pay_channel_service=debug` |
| Hub Registry | `info` | `ignite_pay_hub_registry=debug,sqlx=debug` |
| User MCP | `ignite_pay_mcp=info` | `ignite_pay_mcp=debug` |
| Merchant MCP | `info` | `info,ignite_pay_core=debug` |

### 13.3 日志级别语法

```
# 全局级别
RUST_LOG=debug

# 按 crate 过滤
RUST_LOG=ignite_pay_channel_service=debug,sqlx=warn

# 按模块路径过滤
RUST_LOG=ignite_pay_channel_service::handlers::payment=trace

# 混合
RUST_LOG=info,ignite_pay_channel_service=debug,sqlx=warn
```

### 13.4 systemd 环境变量配置

在 systemd unit 中通过 `Environment` 或 `EnvironmentFile` 设置：

```ini
[Service]
# 单个变量
Environment=RUST_LOG=debug

# 多个变量
Environment=RUST_LOG=debug
Environment=AUDIT_LOG_DIR=/var/log/ignite-pay/audit

# 或使用文件
EnvironmentFile=/opt/ignite-pay/env
```

`/opt/ignite-pay/env` 文件格式：

```bash
RUST_LOG=info
JWT_SECRET=your-production-jwt-secret
AUDIT_LOG_DIR=/var/log/ignite-pay/audit
```

---

## 变更记录

| 版本 | 日期 | 变更内容 |
|:-----|:-----|:---------|
| v0.1 | 2025-06-01 | 初始版本：涵盖部署步骤、配置、Docker、生产注意事项、健康检查、故障排查 |
| v0.2 | 2025-06-15 | 补充商户侧 DIDComm Router 部署步骤；新增备份与恢复、升级与回滚、环境变量参考章节；添加防火墙规则、完整 docker-compose.yml、DID Registry / Hub Registry Nginx 配置；修复健康检查脚本 PostgreSQL 检测 |
| v0.3 | 2026-04-26 | 移除路由器 DID 要求，WS 认证改为 Ed25519 签名验证；移除 `didcomm-router-init` 工具；更新步骤 3、5.1、5.9、6.3 节 |
