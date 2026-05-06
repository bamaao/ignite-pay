# Ignite Pay 系统架构与实施文档

本文档描述 Ignite Pay 系统的整体架构、组件交互、数据流、API 参考和安全设计。

---

## 目录

1. [架构概览](#1-架构概览)
2. [组件清单](#2-组件清单)
3. [核心库模块](#3-核心库模块)
4. [数据流图](#4-数据流图)
5. [DIDComm 消息协议](#5-didcomm-消息协议)
6. [存储架构](#6-存储架构)
7. [身份体系](#7-身份体系)
8. [安全设计](#8-安全设计)
9. [API 参考](#9-api-参考)
10. [推送通知架构](#10-推送通知架构)
11. [合规配置](#11-合规配置)
12. [状态通道程序](#12-状态通道程序)
13. [错误处理与重试策略](#13-错误处理与重试策略)
14. [监控与可观测性](#14-监控与可观测性)
15. [部署概览](#15-部署概览)
16. [性能与容量规划](#16-性能与容量规划)

---

## 1. 架构概览

Ignite Pay 系统采用四层架构：

```
┌───────────────────────────────────────────────────────────────────┐
│                          应用层 (Application)                      │
│    Sentinel (用户 App)  │  Ignite Merchant (商户 App)  │  AI Agent │
├───────────────────────────────────────────────────────────────────┤
│                        通信层 (Communication)                      │
│         DIDComm V2 (JWE authcrypt)  │  Mediator (中继路由)         │
├───────────────────────────────────────────────────────────────────┤
│                         服务层 (Service)                           │
│  User MCP │ Merchant MCP │ Channel Service │ Hub Registry │ DID  │
├───────────────────────────────────────────────────────────────────┤
│                          链上层 (On-chain)                         │
│    ignite-pay-program │ DID Program │ Session Key │ ZK Compression│
└───────────────────────────────────────────────────────────────────┘
```

### 全局数据流

```
AI Agent ──X402──> MCP Server ──DIDComm JWE──> Mediator ──push──> Phone App
                                       │                                  │
                                       │                          FCM (海外) / WebSocket (国内)
                                       │                                  │
                                       │                         HTTPS Pull (消息收取)
                                       │                                  │
                                  支付决策引擎 <────────────────── 用户授权/拒绝
                            (VC验证+链上DID验证+名单+额度)
                                       │
                                       ↓
                          Session Key 链上支付 (SOL/SPL Token)
                                       │
                                       ↓
                                 Solana 区块链
```

---

## 2. 组件清单

### 2.1 服务组件

| 组件 | 二进制/目录 | 端口 | 传输协议 | 存储 | 说明 |
|:-----|:-----------|:-----|:---------|:-----|:-----|
| PostgreSQL | 外部依赖 | 5432 | TCP | PostgreSQL | Hub Registry 数据库 |
| Hub Registry | `ignite-pay-hub-registry` | 3004 | HTTP | PostgreSQL | Hub 注册发现服务 |
| DIDComm Router | `didcomm-router` | 8080 | HTTP + WS | sled | DIDComm 消息路由器/中继 |
| DID Registry | `did-registry` | 8081 | HTTP | sled | DID 注册服务 |
| Channel Hub | `ignite-pay-channel-service --config config-hub.toml` | 3003 | HTTP | sled | Hub 路由节点，支持多跳 |
| Channel Provider | `ignite-pay-channel-service --config config-provider.toml` | 3002 | HTTP | sled | 商户端通道服务 |
| Channel User | `ignite-pay-channel-service --config config.toml` | 3001 | HTTP | sled | 用户端通道服务 |
| User MCP | `ignite-pay-mcp` | stdio | MCP (JSON-RPC 2.0) | sled | 用户端 MCP 代理 |
| Merchant MCP | `ignite-pay-merchant-mcp` | stdio | MCP (JSON-RPC 2.0) | sled | 商户端 MCP 代理 |

### 2.2 移动端应用

| 应用 | 目录 | 平台 | 存储 | 说明 |
|:-----|:-----|:-----|:-----|:-----|
| Sentinel | 用户端 Flutter App | iOS / Android | sled + SQLite | 用户支付授权守卫 |
| Ignite Merchant | 商户端 Flutter App | iOS / Android | sled | 商户收款工具 |

### 2.3 链上程序

| 程序 | Program ID | 框架 | 说明 |
|:-----|:-----------|:-----|:-----|
| 状态通道程序 | `DJBHr35jL3JAGoU7bKMsEFmpeNMrCSK7oYQE4HJ3GBUe` | Anchor 1.0.0 | 通道结算/争议处理 |
| DID 程序 | 配置于 `did-registry` | Anchor + Light SDK | ZK Compression 压缩账户 |

### 2.4 组件依赖关系

```
                    ┌─────────────────┐
                    │   AI Agent      │
                    └────────┬────────┘
                             │ stdio (JSON-RPC)
                    ┌────────▼────────┐
                    │    User MCP     │
                    │    (stdio)      │
                    └──┬──────────┬───┘
                       │          │
              WS (:8080)          │ HTTP (:3003)
                       │          │
              ┌────────▼──┐  ┌───▼────────────┐
              │ Mediator  │  │  Channel Hub    │
              │ (:8080)   │  │  (:3003)        │
              │           │  └───┬────────┬────┘
              │           │      │        │
              └──┬───┬────┘      │        │
                 │   │           │        │
           WS/FCM│   │HTTPS      │        │
                 │   │           │        │
          ┌──────▼─┐ │     ┌────▼─────────▼───┐
          │Sentinel│ │     │  Hub Registry     │
          │(用户App)│ │     │  (:3004)          │
          └────────┘ │     └────┬──────────────┘
                     │          │ PostgreSQL
                     │     ┌────▼─────────┐
                     │     │ PostgreSQL   │
                     │     │ (:5432)      │
                     │     └──────────────┘
                     │
              ┌──────▼──────┐     ┌──────────────┐
              │ Merchant    │     │  Channel     │
              │ App         │     │  Provider    │
              │ (商户端)     │     │  (:3002)     │
              └──────┬──────┘     └──────────────┘
                     │
              ┌──────▼──────┐
              │ Merchant    │
              │ MCP (stdio) │
              └─────────────┘
```

---

## 3. 核心库模块

### 3.1 ignite-pay-core

核心协议库，提供 DID 身份管理、DIDComm 通信、名单管理、VC 签发验证等基础能力。

| 模块 | 功能 |
|------|------|
| `identity` | DID 生成、DID Document 构建、身份持久化、DID 签名验证 |
| `didcomm` | DIDComm 消息构造器（20 种消息类型）、JWE 加解密、Agent 创建 |
| `types` | 共享类型：PaymentRequest, MerchantListEntry, VerifiableCredential, RiskControlDecision |
| `list_store` | 白名单/黑名单管理（sled + IPFS 同步），风控决策 |
| `vc` | Verifiable Credential 签发与验证 |
| `ipfs` | IPFS 上传/下载抽象层 |
| `audit_merkle` | SHA-256 Merkle 树审计日志 |
| `log_crypto` / `log_chunk` / `log_sync` | E2EE 审计日志（加密 → Zstd 压缩 → IPFS 同步） |
| `solana_did` | SolanaDidBridge: DID 链上验证桥接层 (feature gate: `solana`) |

### 3.2 ignite-pay-state-channel

状态通道协议库，提供通道管理、Merkle 树、HTLC、路由、合规等状态通道核心能力。

| 模块 | 文件 | 说明 |
|:-----|:-----|:-----|
| `channel` | `channel.rs` | ChannelManager — 通道生命周期管理，sled 持久化 |
| `merkle` | `merkle.rs` | MerkleTree — 排序对哈希二叉树（sorted-pair hashing，匹配链上程序） |
| `types` | `types.rs` | UTXOLeaf (Standard/HTLC/Compliance), LeafUpdate, SignedState, ChannelMetadata |
| `signing` | `signing.rs` | Ed25519 签名/验证，消息构造 |
| `pipeline` | `pipeline.rs` | Pipeline — 批量 LeafUpdate 构建器，自动回滚 |
| `htlc` | `htlc.rs` | HtlcManager — HTLC 原像/生命周期管理 |
| `hub` | `hub.rs` | HubManager — Hub 注册/指标，sled 持久化 |
| `routing` | `routing.rs` | RouteService — DFS 路由发现/评分 |
| `multihop` | `multihop.rs` | MultiHopManager — 多跳支付，递减 timelock |
| `compliance` | `compliance.rs` | ComplianceManager — 消费限额/审计 |
| `error` | `error.rs` | StateChannelError 统一错误类型 |
| `helpers` | `helpers.rs` | 辅助工具函数 |

**关键依赖**：

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
```

### 3.3 ignite-pay-solana

Solana 链上交互库，提供商家身份验证、Session Key 管理、链上支付等能力。

```
ignite-pay-solana/
├── src/
│   ├── lib.rs              # 模块声明 + re-export solana_sdk
│   ├── types.rs            # MerchantLeaf, SessionTokenData, PayMode, PaymentResult
│   ├── error.rs            # SolanaError 统一错误类型
│   ├── compression.rs      # CompressionService: Merkle Tree 操作
│   ├── indexer.rs          # IndexerClient: Helius DAS API 查询
│   ├── session.rs          # SessionManager: 临时密钥创建/持久化/验证
│   └── payment.rs          # IgnitePayClient: SOL/SPL Token 真实转账
```

**核心类型**：

- `MerchantLeaf`: 链上商户身份叶子节点（merchant_did_hash, active_pubkey, platform_vc_hash, status）
- `SessionTokenData`: Session Key 链上 PDA 数据（owner, ephemeral_pubkey, expiry, scopes, spending_limit）
- `PayMode`: 支付模式枚举（SelfFunded / Sponsored）
- `PaymentResult`: 支付执行结果

---

## 4. 数据流图

### 4.1 X402 支付授权流

```
AI Agent              MCP Server           Mediator           用户 App (Sentinel)
   │                      │                    │                     │
   │  HTTP Request        │                    │                     │
   ├─────────────────────>│                    │                     │
   │                      │                    │                     │
   │  402 Payment Req     │                    │                     │
   │<─────────────────────┤                    │                     │
   │                      │                    │                     │
   │  process_x402        │                    │                     │
   ├─────────────────────>│                    │                     │
   │                      │                    │                     │
   │                      │  商家验证           │                     │
   │                      │  (VC + Merkle)     │                     │
   │                      │                    │                     │
   │                      │  名单/额度检查      │                     │
   │                      │                    │                     │
   │                      │  payment-auth-req  │                     │
   │                      │  (JWE encrypted)   │                     │
   │                      ├───────────────────>│                     │
   │                      │                    │  FCM/WS push        │
   │                      │                    ├────────────────────>│
   │                      │                    │                     │
   │                      │                    │  HTTPS Pull (JWE)   │
   │                      │                    │<────────────────────┤
   │                      │                    │                     │
   │                      │                    │  用户审核+创建       │
   │                      │                    │  Session Key        │
   │                      │                    │                     │
   │                      │                    │  auth-response      │
   │                      │                    │  (JWE encrypted)    │
   │                      │                    │<────────────────────┤
   │                      │                    │                     │
   │                      │  auth-response     │                     │
   │                      │<───────────────────┤                     │
   │                      │                    │                     │
   │                      │  Session Key 支付   │                     │
   │                      │  (SOL/SPL Token)   │                     │
   │                      │                    │                     │
   │  支付结果+tx签名      │                    │                     │
   │<─────────────────────┤                    │                     │
```

### 4.2 状态通道支付流

```
用户 App (A)                    Hub (B)                     Solana
    │                              │                           │
    │  LeafUpdate (Transfer)       │                           │
    ├─────────────────────────────>│                           │
    │                              │                           │
    │                              │  更新 Merkle Tree          │
    │                              │  创建 SignedState          │
    │                              │                           │
    │  CoSign Request              │                           │
    │<─────────────────────────────┤                           │
    │                              │                           │
    │  CoSign Response             │                           │
    ├─────────────────────────────>│                           │
    │                              │                           │
    │  Payment Result              │                           │
    │  (sequence, leaf_index)      │                           │
    │<─────────────────────────────┤                           │
    │                              │                           │
    │           ── 关闭通道时 ──     │                           │
    │                              │                           │
    │  Close Channel               │                           │
    ├─────────────────────────────>│                           │
    │                              │  Settle TX                │
    │                              ├──────────────────────────>│
    │                              │                           │
    │                              │  Settlement Confirmed     │
    │                              │<──────────────────────────┤
```

### 4.3 QR 收款支付流

```
商户 App                用户 App (Sentinel)         Hub                Mediator
   │                         │                       │                    │
   │  生成 QR 码              │                       │                    │
   │  (ignite://pay?d=...)    │                       │                    │
   │                         │                       │                    │
   │     ───── QR 扫描 ─────>│                       │                    │
   │                         │                       │                    │
   │                         │  解析 PaymentQrData    │                    │
   │                         │  确认支付               │                    │
   │                         │                       │                    │
   │                         │  POST /v1/channels/   │                    │
   │                         │  {id}/pay             │                    │
   │                         ├──────────────────────>│                    │
   │                         │                       │                    │
   │                         │  Payment Result       │                    │
   │                         │<──────────────────────┤                    │
   │                         │                       │                    │
   │                         │                       │  payment-confirm   │
   │                         │                       │  (JWE)             │
   │                         │                       ├───────────────────>│
   │                         │                       │                    │
   │                         │                       │  WS/FCM push       │
   │  <──────────────────────────────────────────────────────────────────┤
   │                         │                       │                    │
   │  确认订单                │                       │                    │
   │  语音播报                │                       │                    │
```

### 4.4 多跳支付流

多跳支付允许资金通过多个 Hub 中转到达目标方，使用递减 timelock 和共享 hash_lock 保证原子性。

**关键常量**：

| 常量 | 值 | 说明 |
|:-----|:---|:-----|
| `HOP_MARGIN` | 1000 slots (~6.7 min) | 相邻跳之间的 timelock 差值 |
| `HTLC_SAFETY_MARGIN` | 1000 slots | HTLC 安全余量 |
| `min_timelock` | `challenge_duration + 3 * HOP_MARGIN` | 单跳最小 timelock |

**Timelock 递减公式**：

```
base_timelock = current_slot + min_timelock(challenge_duration) + (num_hops - 1) * HOP_MARGIN
hop[i].timelock = base_timelock - i * HOP_MARGIN
```

首跳 timelock 最长，末跳最短。若末跳超时退款，上游每跳仍有 `HOP_MARGIN` 的窗口完成退款。

**路由发现**：

1. Hub 向 `RouteService` 注册自身（fee、延迟、流动性、成功率）
2. 调用 `discover_routes(RouteRequest)` 执行 DFS 搜索候选路由
3. 路由评分公式：`score = 0.3 * fee_score + 0.3 * latency_score + 0.4 * reliability_score`
4. `select_best_route()` 选取最高分路由

**费用计算**：从末跳向前推导，每跳在下游金额基础上加收路由费：

```
amounts[last] = destination_amount
amounts[i]    = amounts[i+1] + amounts[i+1] * fee_rate_bps[i] / 10000
```

**完整序列图**：

```
Sender(A)           Hub_1(B)            Hub_2(C)            Receiver(D)         Solana
   │                   │                   │                   │                   │
   │  1. discover_routes                   │                   │                   │
   │──(RouteService)──>│                   │                   │                   │
   │  routes           │                   │                   │                   │
   │<──────────────────┤                   │                   │                   │
   │                   │                   │                   │                   │
   │  2. create_payment(preimage, hash_lock, hops_metadata)    │                   │
   │──(MultiHopManager)                                                      │                   │
   │  payment_id, status=Pending                                             │                   │
   │                   │                   │                   │                   │
   │  3. HTLC Lock: hop[0]                 │                   │                   │
   │  LeafUpdate(Standard→HTLC)            │                   │                   │
   ├──────────────────>│                   │                   │                   │
   │                   │  HTLC Lock: hop[1]│                   │                   │
   │                   │  LeafUpdate       │                   │                   │
   │                   ├──────────────────>│                   │                   │
   │                   │                   │  HTLC Lock: hop[2]│                   │
   │                   │                   │  LeafUpdate       │                   │
   │                   │                   ├──────────────────>│                   │
   │                   │                   │                   │                   │
   │  status = Locked  │                   │                   │                   │
   │                   │                   │                   │                   │
   │  ── 原像揭示阶段 (反向传播) ──         │                   │                   │
   │                   │                   │                   │                   │
   │                   │                   │  4. reveal_preimage                   │
   │                   │                   │<──────────────────┤                   │
   │                   │                   │  SHA-256(preimage)==hash_lock ✓       │
   │                   │                   │                   │                   │
   │  status = Resolving                   │                   │                   │
   │                   │                   │                   │                   │
   │                   │  5. resolve_hop[1]│                   │                   │
   │                   │<──────────────────┤                   │                   │
   │                   │                   │                   │                   │
   │  6. resolve_hop[0]│                   │                   │                   │
   │<──────────────────┤                   │                   │                   │
   │                   │                   │                   │                   │
   │  status = Completed                   │                   │                   │
   │                   │                   │                   │                   │
   │           ── 结算阶段 (各跳独立上链) ── │                   │                   │
   │                   │                   │                   │                   │
   │  settle hop[0]    │                   │                   │                   │
   ├──────────────────>│  settle hop[1]    │                   │                   │
   │                   ├──────────────────>│  settle hop[2]    │                   │
   │                   │                   ├──────────────────>│                   │
   │                   │                   │                   │  Settle TX        │
   │                   │                   │                   ├──────────────────>│
   │                   │                   │                   │                   │
   │           ── 超时失败路径 ──           │                   │                   │
   │                   │                   │                   │                   │
   │  check_expiry(current_slot)           │                   │                   │
   │  hop[i].timelock_slot < current_slot  │                   │                   │
   │  → status = Failed │                  │                   │                   │
   │                   │                   │                   │                   │
   │  各跳 HTLC: Expired → Refunded        │                   │                   │
```

**多跳支付状态机**：

```
Pending → Locked → Resolving → Completed
                    │
                    └→ Failed (超时: hop.timelock_slot < current_slot)
```

**HTTP API 端点**（Channel Service 多跳处理器）：

| 端点 | 方法 | 用途 |
|------|------|------|
| `/v1/multihop/payments` | POST | 创建多跳支付 |
| `/v1/multihop/payments/{id}/resolve` | POST | 解析指定跳 |
| `/v1/multihop/payments/{id}/relay` | POST | Hub 中继解析 |
| `/v1/multihop/payments/{id}` | GET | 查询支付状态 |
| `/v1/routing/hubs` | POST | 注册 Hub 路由信息 |
| `/v1/routing/edges` | POST | 添加通道边 |
| `/v1/routing/find` | POST | 查找路由 |
| `/v1/routing/refresh` | POST | 刷新路由图 |

---

### 4.5 DIDComm 通道创建流

```
用户 App            MCP Server          Hub Registry          Channel Hub
   │                    │                    │                    │
   │  GET /v1/hubs      │                    │                    │
   ├───────────────────>│───────────────────>│                    │
   │                    │                    │                    │
   │  Hub 列表          │                    │                    │
   │<───────────────────┤<───────────────────┤                    │
   │                    │                    │                    │
   │  选择 Hub          │                    │                    │
   │  create-channel-   │                    │                    │
   │  request (JWE)     │                    │                    │
   ├───────────────────>│                    │                    │
   │                    │                    │                    │
   │                    │  POST /v1/channels/open                │
   │                    ├────────────────────────────────────────>│
   │                    │                    │                    │
   │                    │  channel_id, root  │                    │
   │                    │<────────────────────────────────────────┤
   │                    │                    │                    │
   │  create-channel-   │                    │                    │
   │  response (JWE)    │                    │                    │
   │<───────────────────┤                    │                    │
```

### 4.6 Hub 注册发现流

```
Channel Hub              Hub Registry              App
    │                        │                      │
    │  POST /v1/hubs         │                      │
    │  (注册自身)             │                      │
    ├───────────────────────>│                      │
    │                        │                      │
    │  hub_id                │                      │
    │<───────────────────────┤                      │
    │                        │                      │
    │  PUT /v1/hubs/{id}/    │                      │
    │  metrics (每 N 秒)     │                      │
    ├───────────────────────>│                      │
    │                        │                      │
    │                        │  GET /v1/hubs        │
    │                        │<─────────────────────┤
    │                        │                      │
    │                        │  Hub 列表             │
    │                        ├─────────────────────>│
```

---

## 5. DIDComm 消息协议

### 5.1 消息类型汇总表

| 消息 | 类型 URI | 方向 | 用途 |
|------|----------|------|------|
| OOB 邀请 | `https://didcomm.org/out-of-band/2.0/invitation` | MCP → 用户 | QR 配对邀请 |
| 连接请求 | `https://didcomm.org/ignite-pay/1.0/connection-request` | 用户 → MCP | 建立连接 |
| 连接响应 | `https://didcomm.org/ignite-pay/1.0/connection-response` | MCP → 用户 | 连接确认 |
| 支付授权请求 | `https://didcomm.org/ignite-pay/1.0/payment-auth-request` | MCP → 用户 | 请求支付授权 |
| 支付授权响应 | `https://didcomm.org/ignite-pay/1.0/payment-auth-response` | 用户 → MCP | 授权响应（含 Session Key） |
| 通道支付请求 | `https://didcomm.org/ignite-pay/1.0/channel-payment-request` | App → MCP | 状态通道支付请求 |
| 通道支付确认 | `https://didcomm.org/ignite-pay/1.0/channel-payment-confirm` | Hub → 商户 | 支付确认推送 |
| 名单同步通知 | `https://didcomm.org/ignite-pay/1.0/list-sync-notification` | MCP → 用户 | 白名单/黑名单更新 |
| 通道创建请求 | `https://didcomm.org/ignite-pay/1.0/create-channel-request` | App → MCP | 创建通道请求 |
| 通道创建响应 | `https://didcomm.org/ignite-pay/1.0/create-channel-response` | MCP → App | 创建通道响应 |
| Mediation | `https://didcomm.org/coordinate-mediation/2.0/*` | 双向 | Mediator 协议 |
| WS 认证 | `https://didcomm.org/ignite-pay/1.0/ws-challenge-response` | 双向 | WS 认证挑战 |
| 消息拾取 | `https://didcomm.org/messagepickup/3.0/*` | 双向 | 消息拾取协议 |
| 会话充值请求 | `https://didcomm.org/ignite-pay/1.0/session-fund-request` | MCP → 用户 | 会话密钥余额不足时请求充值 |
| 会话充值响应 | `https://didcomm.org/ignite-pay/1.0/session-fund-response` | 用户 → MCP | 手机充值后回复 |
| 余额通知 | `https://didcomm.org/ignite-pay/1.0/balance-notification` | MCP → 用户 | 余额低于阈值时主动通知 |
| 会话续期请求 | `https://didcomm.org/ignite-pay/1.0/session-renew-request` | MCP → 用户 | 会话密钥即将过期时请求续期 |
| 会话续期响应 | `https://didcomm.org/ignite-pay/1.0/session-renew-response` | 用户 → MCP | 手机注册新密钥后回复 |

### 5.2 支付授权请求消息体

`payment-auth-request`：

| 字段 | 类型 | 说明 |
|:-----|:-----|:-----|
| `payment_id` | string | UUID |
| `merchant_did` | string | 收款方 DID |
| `amount` | number | 金额 (最小单位) |
| `description` | string | 人类可读描述 |

### 5.3 支付授权响应消息体

`payment-auth-response`：

| 字段 | 类型 | 必填 | 说明 |
|:-----|:-----|:-----|:-----|
| `payment_id` | string | 是 | 支付请求 UUID |
| `authorized` | bool | 是 | 是否授权 |
| `session_key_pubkey` | string | 授权时 | Session Key Base58 公钥 |
| `session_key_tx_signature` | string | 授权时 | 注册交易签名 |
| `session_expires_at` | number | 授权时 | Session Key 过期时间 (Unix) |
| `spending_limit` | number | 授权时 | 花费上限 (lamports) |
| `scopes` | string[] | 授权时 | 权限范围 |
| `list_action` | string | 是 | 名单操作 |
| `list_label` | string | 否 | 自定义备注 |
| `list_max_amount` | number | 否 | 白名单自动批准上限 |

### 5.4 名单同步通知消息体

`list-sync-notification`：

| 字段 | 类型 | 说明 |
|:-----|:-----|:-----|
| `list_cid` | string | IPFS 新名单 CID |
| `action` | string | 执行的操作 |
| `target_did` | string | 目标商家 DID |
| `timestamp` | string | 同步时间戳 (ISO 8601) |

### 5.5 通道创建消息体

`create-channel-request`：

```json
{
  "hub_endpoint": "http://hub:3003",
  "provider_pubkey": "Base58SolanaPubkey",
  "token_mint": "Base58MintAddress",
  "deposit": 1000000000,
  "tree_depth": 8
}
```

`create-channel-response`：

```json
{
  "channel_id": "hex_encoded_32_bytes",
  "sequence": 0,
  "current_root": "hex_encoded_root",
  "success": true
}
```

### 5.6 Mediator 支持的协议

| 协议 | 版本 | 消息类型 |
|:-----|:-----|:---------|
| Coordinate Mediation | 2.0 | `mediate-request`, `mediate-grant`, `keylist-update`, `keylist-update-response` |
| Routing | 2.0 | `forward` |
| Message Pickup | 3.0 | `status-request`, `status`, `batch-pickup`, `batch`, `live-delivery-request` |
| Peer DID Discovery | 1.0 | `discover` |

### 5.7 消息加密

- **加密方式**：JWE authcrypt（DIDComm V2 标准）
- **签名密钥**：Ed25519 (`#key-signing-1`)
- **加密密钥**：X25519 (`#key-agreement-1`)
- **安全性**：Mediator 无法读取消息体，仅做路由转发

---

## 6. 存储架构

| 服务 | 存储技术 | 数据内容 | 持久化路径 |
|:-----|:---------|:---------|:-----------|
| Channel User (:3001) | sled | 通道数据、Merkle 树、签名 | `./data/channel_user` |
| Channel Provider (:3002) | sled | 通道数据、Merkle 树、签名 | `./data/channel_provider` |
| Channel Hub (:3003) | sled | 通道数据、Merkle 树、签名、Hub 指标 | `./data/channel_hub` |
| Hub Registry (:3004) | PostgreSQL | Hub 注册信息、性能指标 | PostgreSQL 数据库 |
| DIDComm Router (:8080) | sled | 消息队列、路由表、已知对等方 | `./data` |
| DID Registry (:8081) | sled | DID 文档、VC 记录 | sled |
| User MCP | sled | 支付请求、Session Key、名单缓存 | `./data` |
| Merchant MCP | sled | 商户身份、订单、通道 | `./data/merchant-mcp` |
| Sentinel (Flutter) | sled + SQLite | DID 身份、Session Key、策略、审计日志 | 本地存储 |
| Ignite Merchant (Flutter) | sled | 密钥对、订单、通道、DIDComm 身份 | 本地存储 |

### 存储层级

| 层级 | 技术实现 | 存储内容 | 生命周期 |
|:-----|:---------|:---------|:---------|
| 身份层 | 内存 (DIDCommAgent) | `did:ignite` 密钥对、对等方公钥 | 进程生命周期 |
| 支付层 | sled (嵌入式 KV) | PaymentRequest 记录、状态、交易签名 | 持久化至磁盘 |
| 授权层 | DashMap (内存) | PendingAuthStore（oneshot channel 映射） | 进程生命周期 |
| 策略层 | IPFS + sled (本地缓存) | 黑白名单、商家 VC | IPFS 持久化 + sled 缓存 |
| 信任层 | 平台 DID (内置) | 平台签名公钥、VC 验证逻辑 | 随版本发布 |

---

## 7. 身份体系

### 7.1 DID 方法: `did:ignite`

**标识符格式**：

```
did:ignite:z<multibase-base58btc>
```

- **前缀**：`did:ignite:`
- **多碱基指示符**：`z`（base58btc 编码）
- **编码内容**：`0xed 0x01`（multicodec Ed25519 公钥前缀）+ 32 字节 Ed25519 公钥

**示例**：`did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK`

### 7.2 密钥体系

| 用途 | 算法 | 密钥尺寸 | DID Document 片段 ID |
|:-----|:-----|:---------|:---------------------|
| 签名/验证 | Ed25519 | 32 字节 | `#key-signing-1` |
| 密钥协商 (加密) | X25519 | 32 字节 | `#key-agreement-1` |

### 7.3 DID Document 结构

```json
{
  "@context": ["https://www.w3.org/ns/did/v1"],
  "id": "did:ignite:z6Mk...",
  "verificationMethod": [{
    "id": "did:ignite:z6Mk...#key-signing-1",
    "type": "Ed25519VerificationKey2020",
    "controller": "did:ignite:z6Mk...",
    "publicKeyMultibase": "z<multibase-base58btc(0xed+0x01+ed25519_pubkey)>"
  }],
  "keyAgreement": [{
    "id": "did:ignite:z6Mk...#key-agreement-1",
    "type": "X25519KeyAgreementKey2020",
    "controller": "did:ignite:z6Mk...",
    "publicKeyBase64": "<base64-nopad(x25519_pubkey)>"
  }],
  "service": [{
    "id": "did:ignite:z6Mk...#policy-list",
    "type": "IgnitePolicyList",
    "serviceEndpoint": "ipfs://<CID>"
  }]
}
```

### 7.4 身份生命周期

1. **生成**：调用 `generate_ignite_did()` 生成 Ed25519 密钥对，从公钥推导 DID 标识符
2. **注册**：通过 DIDComm Agent 注册密钥，用于后续签名与加密
3. **发布**：在 Mediator 握手阶段通过 `peer-introduction` 发送完整 DID Document
4. **解析**：接收方通过 `parse_did_document()` 从 DID Document 提取公钥并注册为通信对等方

### 7.5 商户双 DID 架构

商户 App 管理两个独立身份：

| 身份 | DID 格式 | 用途 | 存储位置 |
|------|----------|------|----------|
| 状态通道 DID | `did:ignite:<raw_base58>` | QR 码生成、通道操作、链上签名 | sled `keypairs` tree |
| DIDComm 通信 DID | `did:ignite:z<multicodec_base58>` | JWE 加解密、Mediator 消息收发 | sled `didcomm_identity` tree |

两者密钥体系完全独立，互不干扰。

### 7.6 商户 DID 三层密钥结构

| 密钥类型 | 定义 | 存储位置 | 作用 |
|:---------|:-----|:---------|:-----|
| Original Public Key (Root) | 商户注册时的 Solana 地址 | 链上永久 ID | DID 锚点，不可更改 |
| Controller Key | 纯 Ed25519 密钥对 | 商户本地/离线 | DID 文档修改权 |
| Recovery Key | 备份 Ed25519 密钥对 | 离线冷存储 | Controller Key 丢失时重置 |

### 7.7 ZK Compression 身份

**链上结构**：

- **存储**：`MerchantLeaf` 叶子节点存储在 Concurrent Merkle Tree 中
- **树参数**：maxDepth=14, maxBufferSize=64（支持 ~16K 商家）
- **叶子字段**：
  - `merchant_did`: SHA-256 哈希
  - `active_pubkey`: Solana 收款公钥
  - `platform_vc_hash`: 平台 VC 哈希
  - `slot_updated`: 更新 slot
  - `status`: 0=active

**两层验证**：

1. **链下快速过滤**：通过 Helius DAS API 获取 Merkle Proof，本地 `verify_proof_locally()` 验证
2. **链上强制验证**：提交 `verify_leaf` 指令到 Solana，由链上程序验证 proof

---

## 8. 安全设计

### 8.1 传输安全

| 安全措施 | 实现方式 | 说明 |
|:---------|:---------|:-----|
| 端到端加密 | DIDComm V2 JWE authcrypt | Mediator 无法读取消息体 |
| 双层认证 | JWT (传输层) + DIDComm 签名 (消息层) | 传输层验证"谁在调用 API"，消息层验证"消息是谁发的" |
| 外层保护 | TLS 1.3 | 保护外层元数据 |
| 防重放 | 检查 DIDComm Message `id` (Unique Message ID) | 防止消息重复提交 |
| 时效校验 | 检查 `expires_time` | 丢弃过时指令 |

### 8.2 Session Key 风控

| 安全措施 | 说明 |
|:---------|:-----|
| 过期时间检查 | `expires_at` 字段，到期后 Session Key 失效 |
| spending_limit | 单次/累计额度限制，超限交易被拒绝 |
| scopes | 权限范围限定 (`["sol:transfer", "spl:transfer"]`) |
| 禁止指令 | Session Key 不可执行 UpdateState / CloseAccount 等控制权指令 |
| CloseSession | 自付模式下，过期后可退还剩余 Gas 给主钱包 |
| F15 原子支付 | 通过 payment_mutex（tokio::sync::Mutex）实现原子化支付执行，防止并发请求超过消费限额。execute_payment_atomic 方法在互斥锁保护下依次执行：余额检查 → 支付执行 → 消费记录 |

**两种支付模式**：

| 特性 | 自付模式 (SelfFunded) | 代付模式 (Sponsored) |
|:-----|:-----|:-----|
| Gas 来源 | 临时密钥账户 (预充值) | 项目方 Relayer 钱包 |
| 用户感知 | 需确认一次"充值"交易 | 零感知 |
| 中心化程度 | 完全去中心化 | 依赖 Relayer 服务 |
| 适用场景 | 大额、低频结算 | 高频、微额 Agent 自动支付 |

### 8.3 HTLC 安全余量

| 参数 | 默认值 | 说明 |
|:-----|:-------|:-----|
| default_challenge_duration | 5000 slots | 挑战持续时间 |
| default_min_challenge_delay | 1000 slots | 最小挑战延迟 |
| default_settle_window | 10000 slots | 结算窗口 |
| auto_close_offset | 500000 slots | 自动关闭偏移 |
| default_tree_depth | 4 | 默认 Merkle 树深度（支持到 12） |

### 8.4 支付决策引擎

收到 X402 待支付请求后，按以下 6 级优先级依次判定：

| 优先级 | 场景 | 判断条件 | 处理动作 |
|:-------|:-----|:---------|:---------|
| 1 | VC 验证失败 | 附带 VC 签名无效/过期/签发者不匹配 | 拒绝支付，返回验证失败原因 |
| 2 | 链上 DID 验证失败 | 商家 DID 未在 Merkle Tree 注册 | 拒绝支付，返回 "merchant not found on-chain" |
| 3 | 黑名单阻断 | `provider_did` 在黑名单 | 立即中断，返回 `Security Risk: Provider Blocked` |
| 4 | 白名单自动批准 | `provider_did` 在白名单 && 金额 <= `max_amount` | 直接执行链上支付 |
| 5 | 全局阈值自动批准 | 金额 <= `auto_approve_max` && `auto_approve_max > 0` | 自动执行链上支付 |
| 6 | 交互式授权 | 以上均不满足 | 触发 DIDComm 推送授权请求至用户手机 |

### 8.5 商家验证流程

```
收到 X402 待支付请求
  │
  ├─ 1. VC 签名验证
  │    ├─ 从 402 响应提取商家 VC
  │    ├─ 使用内置平台公钥验证 Ed25519Signature2020 proof
  │    ├─ 检查 VC expirationDate 未过期
  │    └─ 失败 → 拒绝支付
  │
  ├─ 2. 链上 Merkle Proof 验证
  │    ├─ 从索引器获取商家叶子节点 Merkle Proof
  │    ├─ 本地验证: Proof + Leaf == Root
  │    ├─ 检查 MerchantLeaf.status == 0 (active)
  │    └─ 失败 → 拒绝支付
  │
  ├─ 3. 一致性校验
  │    ├─ VC 中 credentialSubject.id 的 DID 公钥哈希 == 链上 merchant_did_hash
  │    └─ 不一致 → 拒绝支付
  │
  └─ 全部通过 → 进入决策流程
```

---

## 9. API 参考

### 9.1 Hub REST API (:3003)

| 端点 | 方法 | 用途 |
|------|------|------|
| `/v1/channels/open` | POST | 开通状态通道 |
| `/v1/channels/{id}/pay` | POST | 通道支付 |
| `/v1/channels/{id}/close` | POST | 协作关闭通道 |
| `/v1/channels/{id}/settle` | POST | 发起结算 |
| `/v1/channels/{id}/claim` | POST | 认领叶子 |
| `/v1/channels/{id}/finalize` | POST | 完成结算 |

### 9.2 Hub Registry REST API (:3004)

| 端点 | 方法 | 用途 |
|------|------|------|
| `/v1/hubs` | POST | 注册 Hub |
| `/v1/hubs` | GET | 列出 Hub（支持 status, token_mint, limit, offset 参数） |
| `/v1/hubs/{hub_id}` | GET | 获取 Hub 详情 |
| `/v1/hubs/{hub_id}` | PUT | 更新 Hub |
| `/v1/hubs/{hub_id}` | DELETE | 注销 Hub（设为 inactive） |
| `/v1/hubs/{hub_id}/metrics` | GET | 获取 Hub 性能指标 |
| `/v1/hubs/{hub_id}/metrics` | PUT | 更新 Hub 性能指标 |

### 9.3 Mediator REST API (:8080)

| 端点 | 方法 | 用途 |
|------|------|------|
| `/v1/auth/challenge` | GET | 获取认证 nonce |
| `/v1/auth/token` | POST | 签名换 JWT |
| `/v1/sync/list` | GET | 拉取消息列表（游标分页） |
| `/v1/sync/messages/{id}` | GET | 获取单条消息 |
| `/v1/agents/{id}/command` | POST | 发送加密命令 |
| `/v1/agents/bind` | POST | 绑定 Agent DID |
| `/v1/devices/register-token` | POST | 注册推送通道 (FCM token 或 websocket) |

### 9.4 DID Registry REST API (:8081)

| 端点 | 方法 | 用途 |
|------|------|------|
| `/health` | GET | 健康检查 |
| `/v1/did/resolve/{did}` | GET | 解析 DID Document |
| `/v1/auth/nonce` | GET | 获取认证 nonce |
| `/v1/merchants/register` | POST | 注册商户（链上 ZK Compression） |
| `/v1/merchants/confirm` | POST | 确认商户注册 |
| `/v1/merchants/verify/{did}` | GET | 验证商户身份 |
| `/v1/merchants/status/{did}` | GET | 查询商户状态 |
| `/v1/merchants/rotate-key` | POST | 轮换商户密钥 |
| `/v1/merchants/update-vc` | POST | 更新商户 VC |
| `/v1/vc/issue` | POST | 签发 VC |
| `/v1/vc/revoke` | POST | 撤销 VC |
| `/v1/proof` | POST | 获取 ZK Compression Proof（公开，无需认证） |
| `/v1/fees` | GET | 列出费用记录 |

**费用**：register=5000, update_vc=2000, rotate_key=2000 lamports

### 9.5 User MCP 工具接口

| 工具名 | 输入 | 输出 |
|:-------|:-----|:-----|
| `process_x402_challenge` | `challenge_body`, `phone_did` | 支付结果 + tx 签名 / 错误 |
| `check_authorization` | `payment_id` | 支付状态、金额、时间、tx 签名 |
| `get_payment_history` | `limit` (默认 10) | 最近 N 条支付记录 |
| `get_identity` | (无) | 当前 `did:ignite`、Mediator 连接状态 |

### 9.6 Merchant MCP 工具接口

| 工具名 | 必填参数 | 可选参数 | 输出 |
|:-------|:---------|:---------|:-----|
| `generate_payment_qr` | `amount` | `description`, `order_id` | QR 文本 (`ignite://pay?d=...`) + ASCII 二维码 |
| `check_payment` | `order_id` | — | 订单状态、金额、通道 ID、确认时间 |
| `get_payment_history` | — | `limit` (默认 20) | 最近 N 条订单列表 |
| `get_channel_status` | — | `channel_id` | 单通道详情或全部通道列表 |
| `open_channel_with_hub` | `hub_endpoint` | `deposit` (默认 0), `tree_depth` (默认 8) | 提示信息（商户侧由用户发起开通） |
| `close_channel` | `channel_id` | — | 关闭确认 |
| `settle_channel` | `channel_id` | — | claim + finalize 结果 |
| `get_identity` | (无) | — | 商户 DID、Hub 连接状态 |

---

## 10. 推送通知架构

```
                    ┌───────────────────────┐
                    │      Mediator         │
                    │   (消息中继 + 推送)    │
                    └─────┬──────────┬──────┘
                          │          │
              ┌───────────┘          └───────────┐
              │                                  │
        zh_CN 用户                          非 zh_CN 用户
              │                                  │
     WebSocket 长连接                      FCM 推送通知
     (直接接收 JWE)                     (SIGNAL → 拉取 JWE)
              │                                  │
              └──────────┬───────────────────────┘
                         │
                    pull_messages()
                    decrypt_message()
                    确认订单 / 授权支付
```

| 用户区域 | 推送方式 | 上行路径 | 下行路径 |
|:---------|:---------|:---------|:---------|
| 海外 | FCM 信号 + HTTPS 拉取 | MCP → WS → Mediator → FCM Signal → App HTTPS 拉取 | App → HTTPS → Mediator → WS → MCP |
| 中国大陆 | WebSocket 直推 | MCP → WS → Mediator → WS 直推 → App | App → HTTPS → Mediator → WS → MCP |

**共同点**：
- 首次连接时 authenticate → pull 离线消息
- WS 断线后先拉离线消息再重连（3 秒延迟）
- App 回到前台时触发 `GET /v1/sync/list` 兜底同步

---

## 11. 合规配置

Channel User (:3001) 和 Channel Hub (:3003) 支持合规配置：

```toml
[compliance]
spending_threshold = 1000000000    # 消费阈值: 1 SOL
per_channel_limit = 100000000      # 单通道限额: 0.1 SOL
window_slots = 100000              # 滑动窗口: ~100000 slots (~1-2 天)
travel_rule_threshold = 500000000  # Travel Rule 阈值: 0.5 SOL
```

合规模块 (`ComplianceManager`) 提供：
- **滑动窗口消费阈值**：追踪用户在指定 slot 窗口内的总消费
- **单通道限额**：限制单个通道的最大支付金额
- **Travel Rule 数据**：超过阈值时记录交易双方身份信息

---

## 12. 状态通道程序

### 12.1 链上程序

- **Program ID**：`DJBHr35jL3JAGoU7bKMsEFmpeNMrCSK7oYQE4HJ3GBUe`
- **框架**：Anchor 1.0.0
- **PDA 账户**：
  - `channel`: 通道状态账户
  - `escrow`: 托管账户

### 12.2 UTXO 叶子类型

| 类型 | 说明 |
|:-----|:-----|
| Standard | 标准转账叶子 |
| HTLC | 哈希时间锁叶子（条件支付） |
| Compliance | 合规标记叶子 |

### 12.3 配置文件参考

**Channel User (`config.toml`)**：

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

**Channel Hub (`config-hub.toml`)**：

```toml
[server]
host = "0.0.0.0"
port = 3003

[solana]
rpc_url = "https://api.devnet.solana.com"
channel_program_id = "DJBHr35jL3JAGoU7bKMsEFmpeNMrCSK7oYQE4HJ3GBUe"
keypair_path = "./keys/hub.key"

[channel]
default_tree_depth = 4
default_challenge_duration = 5000
default_min_challenge_delay = 1000
default_settle_window = 10000
auto_close_offset = 500000
db_path = "./data/channel_hub"

[compliance]
spending_threshold = 1000000000
per_channel_limit = 100000000
window_slots = 100000
travel_rule_threshold = 500000000
```

**DIDComm Router (`didcomm-router/config.toml`)**：

```toml
[server]
host = "0.0.0.0"
port = 8080

[router]
did = "did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK"
max_queued_messages = 1000
max_message_age_seconds = 86400

[storage]
path = "./data"
```

**User MCP (`ignite-pay-mcp/config.toml`)**：

```toml
[mediator]
ws_url = "ws://127.0.0.1:8080/ws"
phone_did = ""

[storage]
path = "./data"

[policy]
auto_approve_max = 0
auth_timeout = 300

[platform]
did = "did:ignite:zPlatformDIDPlaceholder"
verifying_key_b64 = ""

[ipfs]
mode = "mock"

[solana]
rpc_url = "https://api.devnet.solana.com"
tree_address = ""
tree_authority = ""
das_endpoint = ""
pay_mode = "self_funded"
default_owner = ""
tree_authority_keypair_b58 = ""
```

**Merchant MCP (`ignite-pay-merchant-mcp/config.toml`)**：

```toml
[merchant]
did = ""
hub_endpoint = "http://localhost:3003"
hub_ws_url = "ws://localhost:3003/ws"

[mediator]
ws_url = "ws://localhost:4000/ws"

[storage]
path = "./data/merchant-mcp"

[solana]
rpc_url = "https://api.devnet.solana.com"
program_id = ""

[hub]
token_mint = ""
provider_pubkey = ""
```

---

## 13. 错误处理与重试策略

### 13.1 错误类型体系

各服务使用 `thiserror` 定义统一错误枚举，并通过 Axum `IntoResponse` 映射为 HTTP 状态码。

**DIDComm Router** (`didcomm-router/src/error.rs`)：

| 错误变体 | HTTP 状态码 | 说明 |
|:---------|:-----------|:-----|
| `Unauthorized` | 401 | JWT/DIDComm 签名验证失败 |
| `SessionNotFound` | 404 | WebSocket 会话不存在 |
| `Didcomm` / `DidResolution` / `Storage` / `Protocol` | 500 | 内部错误 |

**DID Registry** (`did-registry/src/error.rs`)：

| 错误变体 | HTTP 状态码 | 说明 |
|:---------|:-----------|:-----|
| `BadRequest` | 400 | 请求参数错误 |
| `Unauthorized` | 401 | JWT 验证失败 |
| `MerchantNotFound` | 404 | 商户未找到 |
| `ProofVerificationFailed` | 500 | ZK Proof 验证失败 |
| 其他 | 500 | 链上/存储/序列化错误 |

**Channel Service** (`ignite-pay-channel-service/src/error.rs`)：

| 错误变体 | HTTP 状态码 | 说明 |
|:---------|:-----------|:-----|
| `BadRequest` | 400 | 请求参数错误 |
| `Unauthorized` | 401 | 签名验证失败 |
| `ChannelNotFound` | 404 | 通道不存在 |
| `ComplianceHold` | 403 | 合规冻结 |
| `StateChannel` | 422 | 状态通道协议错误（签名/序列/Merkle 等） |
| `PeerUnreachable` | 502 | 对端不可达 |
| `OnChain` / `SolanaRpc` / `Storage` / `Internal` | 500 | 内部错误 |

**Hub Registry** (`ignite-pay-hub-registry/src/error.rs`)：

| 错误变体 | HTTP 状态码 | 说明 |
|:---------|:-----------|:-----|
| `BadRequest` | 400 | 参数错误 |
| `NotFound` | 404 | Hub 不存在 |
| `Database` / `Internal` | 500 | 数据库/内部错误 |

**状态通道库** (`ignite-pay-state-channel/src/error.rs`)：

| 错误变体 | 说明 |
|:---------|:-----|
| `InvalidSequence { expected, actual }` | 序列号不连续 |
| `PrevHashMismatch` | 前一叶子哈希不匹配 |
| `InvalidSignature` | Ed25519 签名验证失败 |
| `InsufficientBalance { required, available }` | 余额不足 |

**VC 验证** (`ignite-pay-core/src/vc.rs`)：

| 错误变体 | 说明 |
|:---------|:-----|
| `InvalidSignature` | VC Ed25519 签名无效 |
| `Expired` | VC 已过期 |
| `IssuerMismatch` | 签发者不匹配 |
| `MissingProof` | 缺少 proof 字段 |

### 13.2 WebSocket 重连策略

所有需要持久 WebSocket 连接的组件（User MCP、Merchant MCP、User App、Skill）采用相同的重连模式：

```
loop {
    match connect_and_run(...).await {
        Ok(()) => warn!("Mediator disconnected, reconnecting..."),
        Err(e) => error!("WS error: {}, reconnecting in 3s...", e),
    }
    tokio::time::sleep(Duration::from_secs(3)).await;
}
```

| 特性 | 当前实现 |
|:-----|:---------|
| 重连间隔 | 固定 3 秒 |
| 最大重试次数 | 无限制 |
| 退避策略 | 无（固定间隔） |
| 抖动 (jitter) | 无 |
| 涉及组件 | User MCP、Merchant MCP、User App (Flutter/Rust)、Skill |

### 13.3 超时配置

| 场景 | 超时时间 | 组件 | 位置 |
|:-----|:---------|:-----|:-----|
| WS 认证挑战响应 | 10s | DIDComm Router (服务端) | `transport/ws.rs` |
| WS 认证结果 | 5s | User MCP / User App | `mediator.rs` / `ws_client.rs` |
| WS 消息状态查询 | 5s | User MCP | `mediator.rs` |
| WS 消息批量拾取 | 10s | User MCP | `mediator.rs` |
| 支付授权等待 | 可配置（默认 300s） | User MCP / Skill | `PolicyConfig.auth_timeout` |
| Nginx WS 空闲超时 | 3600s (1h) | Nginx 反代 | `nginx.conf` |
| HTTP 客户端 | 无超时 | 所有服务 (reqwest 默认) | — |

### 13.4 降级与容错策略

**消息投递降级链**：

```
WebSocket 实时投递
    │ 失败或用户离线
    ▼
消息持久化队列 (sled) → 用户下次上线时拉取
    │ 配置了 FCM
    ▼
FCM 推送通知 → App 收到信号后 HTTPS 拉取消息
    │ FCM 未配置
    ▼
NoopNotificationSender（静默，消息仍保留在队列中）
```

**JWE 解包降级**：

```
JWE authcrypt 解密
    │ 失败
    ▼
明文 JSON 解析（兼容未加密消息）
```

**Session Key 降级**：

```
使用本次授权响应中的 Session Key
    │ 解析失败
    ▼
使用本地缓存的活跃 Session Key
```

**通道状态持久化容错**：

本地状态更新失败时静默忽略（`let _ = persist_state()`），不阻塞远程操作成功返回。

### 13.5 已知局限

| 局限 | 说明 |
|:-----|:-----|
| 无指数退避 | 所有 WS 重连均为固定 3s 间隔，持续故障时可能产生大量重连请求 |
| 无熔断器 | 对 Solana RPC、Hub 等外部依赖无熔断保护 |
| 无限流 | 未实现 API 速率限制 |
| 无优雅关闭 | 所有服务未处理 SIGTERM/SIGINT，tokio 任务会被强制终止 |
| HTTP 客户端无超时 | 所有 `reqwest::Client::new()` 使用默认值（无超时） |
| MCP 服务使用 anyhow | User MCP 和 Merchant MCP 未使用结构化错误类型 |

---

## 14. 监控与可观测性

### 14.1 健康检查端点

| 服务 | 端点 | 响应格式 | 检查深度 |
|:-----|:-----|:---------|:---------|
| DIDComm Router (:8080) | `GET /health` | `"ok"` (纯文本) | 仅 HTTP 存活 |
| DID Registry (:8081) | `GET /health` | `"ok"` (纯文本) | 仅 HTTP 存活 |
| Channel User (:3001) | `GET /health` | `{"status":"ok"}` (JSON) | 仅 HTTP 存活 |
| Channel Provider (:3002) | `GET /health` | `{"status":"ok"}` (JSON) | 仅 HTTP 存活 |
| Channel Hub (:3003) | `GET /health` | `{"status":"ok"}` (JSON) | 仅 HTTP 存活 |
| Hub Registry (:3004) | 无 `/health` 端点 | — | — |

> 所有健康检查端点仅验证 HTTP 服务是否响应，不检查数据库连接、Solana RPC 可达性或 sled 状态。

### 14.2 日志体系

**统一框架**：所有服务使用 `tracing` + `tracing-subscriber`（带 `env-filter` feature）。

**日志配置方式**：通过 `RUST_LOG` 环境变量控制。

```bash
# 全局级别
RUST_LOG=info ./target/release/channel-hub ./config-hub.toml

# 按模块过滤
RUST_LOG=ignite_pay_channel_service=debug,ignite_pay_state_channel=trace ./channel-hub ./config-hub.toml
```

| 服务 | 默认过滤 | 输出目标 | 文件日志 |
|:-----|:---------|:---------|:---------|
| DIDComm Router | `didcomm_router=info` | stderr + `logs/router.log`（按天滚动） | 是 |
| DID Registry | `did_registry=info` | stderr | 否 |
| Channel User / Provider / Hub | `info` | stderr | 否 |
| Hub Registry | `info` | stderr | 否 |
| User MCP | `ignite_pay_mcp=info` | stderr + 可选审计日志文件 | 条件（`AUDIT_LOG_DIR` 环境变量） |
| Merchant MCP | `info` | stderr + 可选审计日志文件 | 条件（`AUDIT_LOG_DIR` 环境变量） |

**MCP 审计日志启用**：

```bash
AUDIT_LOG_DIR=/var/log/ignite-pay ./target/release/ignite-pay-mcp ./config.toml
# 日志文件: /var/log/ignite-pay/ignite-pay-mcp.log（按天滚动）
```

**关键日志点**：

| 服务 | 日志事件 | 级别 |
|:-----|:---------|:-----|
| DIDComm Router | WS 连接认证成功/失败 | info / warn |
| DIDComm Router | 收到 HTTP DIDComm 消息（含字节数） | info |
| DIDComm Router | WS 会话注销 | info |
| DIDComm Router | FCM 推送失败 | warn |
| Channel Service | LeafUpdate / CoSignRequest / HtlcPreimage | info |
| Channel Service | 多跳支付初始化 | info |
| Hub Registry | Hub 注册、指标更新 | info |
| MCP 服务 | Mediator 断连/重连 | warn / error |
| MCP 服务 | 排队消息数量 | info |

### 14.3 Hub 性能指标系统

系统实现了一套领域级指标收集，用于路由评分而非通用监控。

**HubMetrics 结构** (`ignite-pay-state-channel/src/hub.rs`)：

| 字段 | 类型 | 说明 |
|:-----|:-----|:-----|
| `online_rate` | u16 (基点) | 在线率（10000 = 100%） |
| `success_rate` | u16 (基点) | 交易成功率 |
| `avg_latency_ms` | u32 | 平均延迟（毫秒） |
| `total_routed` | u64 | 累计路由笔数 |
| `total_transactions` | u64 | 累计交易数 |
| `active_channels` | u32 | 活跃通道数 |
| `available_liquidity` | u64 | 可用流动性 |
| `fee_rate_bps` | u16 | 费率（基点） |

**指标流转**：

```
Channel Hub ──(PUT /v1/hubs/{id}/metrics, 周期推送)──> Hub Registry
                                                          │
RouteService ──(GET /v1/hubs/{id}/metrics)───────────────>│
    │                                                     │
    └─ score_route_from_metrics() 用于路由评分             │
```

**指标完整性**：`compute_metrics_hash()` 对所有字段做 SHA-256 哈希，可用于链上验证指标真实性。

### 14.4 诊断端点

| 端点 | 服务 | 说明 |
|:-----|:-----|:-----|
| `GET /v1/merchants/status/{did}` | DID Registry | 商户链上状态查询 |
| `GET /v1/hub/info` | Channel Hub | 当前 Hub 注册信息与状态 |
| `GET /v1/compliance/{channel_id}` | Channel User | 通道合规状态 |
| `GET /v1/hubs/{id}/metrics` | Hub Registry | Hub 性能指标查询 |

### 14.5 F13 余额监控

MCP 后台每 60 秒检查会话密钥余额，当余额低于消费限额的 10% 时，通过 DIDComm 向手机发送 balance-notification。每会话最多每 5 分钟通知一次。

### 14.6 已知局限

| 局限 | 说明 |
|:-----|:-----|
| 无 Prometheus/OpenMetrics | 未暴露 `/metrics` 端点，无法对接 Prometheus 生态 |
| 无分布式追踪 | 未使用 `#[tracing::instrument]` 或 span，日志为扁平事件 |
| 无请求关联 ID | 跨服务请求无 correlation ID 串联 |
| 无 WebSocket 心跳 | 未实现应用层 ping-pong，依赖 Nginx 1 小时超时 |
| 健康检查无依赖检测 | `/health` 不验证 sled/PostgreSQL/Solana RPC 连通性 |
| Hub Registry 无 `/health` | 唯一缺失健康检查端点的服务 |

---

## 15. 部署概览

> 完整部署步骤、配置详解、Docker 编排和故障排查请参阅 [部署指南](deploy/system-deployment.md)。

### 15.1 启动顺序

服务间存在依赖关系，须按以下顺序启动：

```
1. PostgreSQL          ← 外部依赖，Hub Registry 的数据库
       │
2. DIDComm Router      ← 无外部依赖
   DID Registry        ← 依赖 Solana RPC + 链上程序
       │
3. Channel User        ← 依赖 Solana RPC
   Channel Provider    ← 依赖 Solana RPC
   Channel Hub         ← 依赖 Solana RPC + Hub Registry
       │
4. Hub Registry        ← 依赖 PostgreSQL（schema 自动初始化）
       │
5. User MCP            ← 依赖 DIDComm Router (WS) + Channel User (HTTP)
   Merchant MCP        ← 依赖 DIDComm Router (WS) + Channel Hub (HTTP+WS)
       │
6. 移动端 App          ← 依赖 DIDComm Router + MCP
```

### 15.2 环境变量

| 变量 | 适用服务 | 说明 |
|:-----|:---------|:-----|
| `RUST_LOG` | 所有服务 | 日志过滤级别（默认 `info`） |
| `AUDIT_LOG_DIR` | User MCP, Merchant MCP | 审计日志文件目录（设置后启用文件日志） |

### 15.3 密钥管理摘要

| 密钥类型 | 格式 | 生成方式 | 安全要求 |
|:---------|:-----|:---------|:---------|
| Solana Keypair | JSON 数组 (64 bytes) | `solana-keygen new` | `chmod 400`，生产用 HSM/KMS |
| Platform Signing Key | 32 bytes 原始二进制 | `openssl rand -out file 32` | 离线备份，`chmod 400` |
| DID Identity | Ed25519 密钥对 | `ignite-pay-core::identity` | sled 持久化 |
| FCM Service Account | JSON | Firebase Console | 仅 DIDComm Router 需要 |

---

## 16. 性能与容量规划

### 16.1 容量限制总览

| 维度 | 限制值 | 来源 | 说明 |
|:-----|:-------|:-----|:-----|
| 消息队列（每用户） | 1000 条 | `max_queued_messages` 配置 | FIFO 淘汰最旧消息 |
| 消息 TTL | 86400s (24h) | `max_message_age_seconds` 配置 | 过期消息静默丢弃 |
| 消息同步分页 | 默认 100，硬上限 1000 | `GET /v1/sync/list` | 单次拉取上限 |
| Merkle 树深度 | 最大 12 | 链上程序校验 | 单通道最多 4096 叶子 |
| 通道默认树深度 | 4 | 配置文件 | 16 叶子，~350 bytes 链上空间 |
| Hub 列表分页 | 默认 100，硬上限 500 | Hub Registry API | 单次查询上限 |
| PostgreSQL 连接池 | 10 | `max_connections(10)` | Hub Registry |
| Channel WS 发送缓冲 | 256 条/连接 | `mpsc::channel(256)` | 背压控制 |
| Hub 指标推送间隔 | 60s | `publish_interval_secs` 配置 | Hub → Registry |
| 多跳默认最大跳数 | 3 | `max_hops.unwrap_or(3)` | 路由发现默认值 |

### 16.2 Merkle 树容量

| tree_depth | 最大叶子数 | 链上账户空间 | 离链存储（Vec\<UTXOLeaf\>） |
|:-----------|:-----------|:-------------|:---------------------------|
| 4 (默认) | 16 | ~350 B | ~1-3 KB |
| 6 | 64 | ~800 B | ~5-11 KB |
| 8 (MCP 默认) | 256 | ~1.4 KB | ~18-35 KB |
| 10 | 1024 | ~4.3 KB | ~75-140 KB |
| 12 (最大) | 4096 | ~16.6 KB | ~300-560 KB |

> 每次通道状态更新会重写完整的 `Vec<UTXOLeaf>`。tree_depth=12 时单次写入 ~300-560 KB。

### 16.3 数据增长模型

#### 按事件类型的增长

| 事件类型 | 涉及服务 | 每事件增长 | 是否有清理 |
|:---------|:---------|:-----------|:-----------|
| 支付请求 (X402) | User MCP | ~300-600 B (PaymentRequest) | 无 |
| 支付订单 (QR) | Merchant MCP, Merchant App | ~400-600 B (PaymentOrder) | 无 |
| 审计日志 | User MCP, Merchant MCP | ~200-1000 B (AuditEntry) | **无，纯追加** |
| 通道状态更新 | Channel Service | 重写 ~1-560 KB (取决于 tree_depth) | 关闭通道后保留 |
| 合规审计 | Channel Service | ~170-230 B (LeafUpdate) | **无，纯追加** |
| DIDComm 消息 | Router | ~1-10 KB (含加密信封) | 有（TTL + 容量淘汰） |
| HTLC 记录 | Channel Service | ~176 B/条，整 Vec 重写 | cleanup() 清除已完成 |
| 多跳支付 | Channel Service | ~150 B/跳 | **无** |
| 链上操作费用 | DID Registry | ~200-300 B | **无，纯追加** |
| 名单变更 | User MCP (IPFS + sled) | ~200-400 B/商户 | IPFS 全量重建，本地覆盖 |

#### 增长公式估算

```
DIDComm Router 磁盘 ≈ Σ(每用户 min(消息数, 1000) × 5 KB)
Channel Service 磁盘 ≈ Σ(每通道 2^depth × ~100 B) + Σ(每 LeafUpdate ~200 B)
User MCP 磁盘 ≈ 支付记录数 × 500 B + 审计条目数 × 600 B
Merchant MCP 磁盘 ≈ 订单数 × 500 B + 审计条目数 × 300 B
```

### 16.4 sled 存储详情

#### 各服务 sled 树清单

**DIDComm Router** (`./data`)：

| 树名 | 增长维度 | 说明 |
|:-----|:---------|:-----|
| `msg:{recipient_did}` | 每用户独立树，上限 1000 条 | 加密消息队列 |
| `keylist` | 每 (session, recipient) 对 | 路由转发映射 |
| `keylist_reverse` | 每接收者 DID | 反向路由映射 |
| `device_tokens` | 每用户 | FCM 设备令牌 |
| `push_channels` | 每用户 | 推送通道偏好 (fcm/websocket) |
| `agent_to_user` | 每 Agent DID | Agent → 用户绑定 |
| `user_to_agents` | 每 (用户, Agent) 对 | 用户 → Agent 索引 |

**Channel Service** (`{db_path}`)：

| Key 模式 | 增长维度 | 说明 |
|:---------|:---------|:-----|
| `channel:{id}:meta` | 每通道 | 通道元数据 (~220 B) |
| `channel:{id}:leaves` | 每通道 | Merkle 叶子全量 (1-560 KB) |
| `channel:{id}:cosign` | 每通道 | 联合签名 (0 或 65 B) |
| `compliance:{id}` | 每通道 | 合规状态 (~200 B+) |
| `audit:{id}:{seq}` | 每通道每更新 | 审计条目 (~200 B, **纯追加**) |
| `htlc:{id}` | 每通道 | HTLC 记录 Vec |
| `multihop:{id}` | 每多跳支付 | 多跳支付状态 |
| `hub:{hash}` | 每 Hub | Hub 注册 (~228 B) |
| `hub_metrics:{hash}` | 每 Hub | Hub 指标 (~52 B, 原地更新) |

**User MCP** (`./data`)：

| Key 模式 | 增长维度 | 说明 |
|:---------|:---------|:-----|
| `{payment_id}` (默认树) | 每支付 | PaymentRequest (~300-600 B) |
| `__identity__` | 单条 | DID 身份 (~200 B) |
| `__audit_log__:{ts}:{uuid}` | 每事件 | 审计条目 (**纯追加**) |
| `session:{pubkey}` | 每 Session Key | 临时密钥+元数据 (~200 B) |
| `__merchant_spending__` | 每商户 | 商户累计消费追踪 (key: merchant_did, value: u64 LE bytes) |

**Merchant MCP** (`./data/merchant-mcp`)：

| 树名 | 增长维度 | 说明 |
|:-----|:---------|:-----|
| `orders` | 每订单 | PaymentOrder (~400-600 B) |
| `merchant_audit` | 每事件 | 审计条目 (**纯追加**) |

**DID Registry** (`./did_registry_data`)：

| Key 模式 | 增长维度 | 说明 |
|:---------|:---------|:-----|
| `merchant:{hash}` | 每商户 | 链上商户记录 |
| `leaf_index:{hash}` | 每商户 | 叶子索引 (4 B) |
| `vc:{hash}` | 每 VC | 可验证凭证 JSON |
| `fee:{op}:{ts}:{hash}` | 每链上操作 | 费用记录 (**纯追加**) |
| `revoked_vc:{hash}` | 每撤销 | 撤销记录 |

#### sled 配置

所有服务使用 `sled::open(path)` 默认配置，无自定义调优：

| 参数 | 默认值 | 说明 |
|:-----|:-------|:-----|
| 页面缓存 | 256 MB | sled 默认 |
| 后台刷盘 | 500ms | sled 默认 |
| 显式 flush | 每次关键写入后调用 | 33 处 `.flush()` 调用 |

### 16.5 数据生命周期

#### 有清理机制

| 数据 | 清理方式 | 触发条件 |
|:-----|:---------|:---------|
| DIDComm 消息 | FIFO 淘汰 | `len > max_queued_messages` |
| DIDComm 消息 | TTL 过期 | `age > max_message_age_seconds` |
| HTLC 记录 | `cleanup()` | 已 fulfilled/refunded/expired |
| 合规窗口支付 | `retain()` | 超出 slot 窗口 |
| Pending Session Key | `db.remove()` | 链上注册完成后 |
| 名单 (IPFS 同步) | `tree.clear()` + 重建 | IPFS CID 变更时 |

#### 软过期但不删除

| 数据 | 过期检查方式 | 实际删除 |
|:-----|:------------|:---------|
| 白名单/黑名单条目 | 读取时检查 `expires` 字段 | 需手动移除 |
| Session Key (链上) | `is_expired()` 检查 | 需调用 `close_session()` |
| HTLC (未清理) | `check_expiry()` 标记 Expired | 需调用 `cleanup()` |

#### 纯追加，无清理

- User MCP 审计日志 (`__audit_log__`)
- Merchant MCP 审计日志 (`merchant_audit`)
- 合规审计条目 (`audit:{channel_id}:{seq}`)
- DID Registry 费用记录 (`fee:...`)
- 多跳支付记录 (`multihop:...`)
- DID Registry 商户记录和 VC（逻辑上不应删除）
- Hub 注册和指标记录

### 16.6 性能特征与瓶颈

#### 写入放大

通道叶子数据（`Vec<UTXOLeaf>`）在每次状态更新时全量重写。对于 tree_depth=12 的通道，每笔支付产生 ~300-560 KB 的 sled 写入。这在大树深 + 高频支付场景下会成为主要瓶颈。

同样，HTLC 记录（`Vec<HtlcRecord>`）也是全量重写模式。

#### 无连接/通道数限制

| 资源 | 限制 | 说明 |
|:-----|:-----|:-----|
| WebSocket 连接数 | 无上限 | `DashMap` 无容量限制 |
| 单用户通道数 | 无上限 | 应用层未限制 |
| 单 Hub 通道数 | 无上限 | 应用层未限制 |
| 并发 HTLC/通道 | 隐式：2^tree_depth - 1 | 受叶子数量约束 |

#### 性能基准

当前代码库无性能基准测试或压力测试。无 `criterion` 或类似基准框架。

### 16.7 容量规划建议

| 场景 | 日活用户 | 通道数 | 日均支付 | 预估月磁盘增长 |
|:-----|:---------|:-------|:---------|:---------------|
| 小规模试点 | 100 | 50 | 500 | ~50 MB |
| 中等规模 | 1,000 | 500 | 5,000 | ~500 MB |
| 大规模 | 10,000 | 5,000 | 50,000 | ~5 GB |

> 以上估算基于 tree_depth=8、审计日志和消息队列的中位数大小。实际增长取决于 tree_depth 配置和交易频率。

**关键运维建议**：

- 审计日志（MCP + 合规）为纯追加增长，需定期归档或实现 TTL 清理
- tree_depth > 8 的通道应控制数量，避免写入放大影响延迟
- DIDComm Router 的 `max_queued_messages` 和 `max_message_age_seconds` 是控制磁盘增长的关键旋钮
- 生产环境建议监控 sled 数据目录大小，设置告警阈值
- 所有 sled 数据库默认缓存 256 MB，大规模部署需评估是否充足

---

## 附录：设计系统

两个 Flutter App 共享同一套 Dark Glassmorphism 设计语言：

---

## 文档变更记录

| 版本 | 日期 | 变更内容 |
|:-----|:-----|:---------|
| v0.5 | 2026-04-22 | 补充性能与容量规划（§16）、优化组件依赖关系图（§2.4） |
| v0.4 | 2026-04-22 | 补充错误处理与重试策略（§13）、监控与可观测性（§14）、部署概览（§15） |
| v0.3 | 2026-04-22 | 补充多跳支付流（§4.4）、DID Registry API 完整路径（§9.4）、Merchant MCP 工具接口（§9.6） |
| v0.2 | 2026-04-21 | 初始完整版本：架构、组件、数据流、协议、存储、身份、安全、API、推送、合规、链上程序 |

两个 Flutter App 共享同一套 Dark Glassmorphism 设计语言：

| Token | 值 | 用途 |
|-------|-----|------|
| Background | `#0A0A14` | 页面背景 |
| Surface | `#12121F` ~ `#22223A` | 卡片、输入框、边框 |
| Text Primary | `#F0F0F8` | 标题、金额 |
| Text Secondary | `#7A7A96` | 描述 |
| Neon Cyan | `#00F5FF` | 主强调色、按钮渐变 |
| Purple | `#8B5CF6` | 次强调色 |
| Success | `#00E676` | 已确认、已连接 |
| Pending | `#FFB300` | 待处理 |
| Danger | `#FF5252` | 失败、关闭、断开 |

字体：Inter（正文）+ JetBrains Mono（数值、DID、代码）。
