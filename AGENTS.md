# AGENTS.md — Ignite Pay 项目结构

## 仓库概述

Ignite Pay 是一个基于 Solana 的去中心化支付系统，包含链上程序（Anchor）、离链状态通道、DID 身份管理、DIDComm 安全通信、AI Agent 支付编排，以及移动端 SDK。项目采用多 crate 仓库结构，共 15 个 Rust crate + 文档。

---

## 依赖层级

```
Layer 0 — 基础库
  ignite-pay-core              共享类型、DID 身份、DIDComm、VC、审计日志
  <!-- State Channel: 探索阶段，暂不启用
  ignite-pay-state-channel     离链 UTXO Merkle Tree 状态通道引擎
  -->

Layer 1 — Solana 集成
  ignite-pay-solana            Solana RPC 客户端、支付执行、ZK DID 查询、会话密钥

Layer 2 — 链上程序（无仓内依赖）
  <!-- State Channel: 探索阶段，暂不启用
  ignite-pay-program           状态通道链上程序（10 条指令）
  -->
  ignite-pay-did-program       商户 DID 链上程序（ZK Compression，6 条指令）
  ignite-pay-session-program   会话密钥链上程序（4 条指令）

Layer 3 — 服务与应用
  <!-- State Channel: 探索阶段，暂不启用
  ignite-pay-channel-service   状态通道 HTTP 服务（User / Provider / Hub 三个角色）
  -->
  didcomm-router               DIDComm 消息路由与中介服务
  did-registry                 DID 链上注册与查询服务
  ignite-pay-mcp               AI Agent 支付编排 MCP 服务器
  ignite-pay-merchant-mcp      商户侧 MCP 服务器（QR 收款码）
  ignite-pay-skill             Python SDK（PyO3）
  ignite_pay_app               Flutter 移动端（Rust Bridge，含扫码支付）

Layer 4 — 测试
  <!-- State Channel: 探索阶段，暂不启用
  ignite-pay-litesvm-tests     状态通道链上程序测试（litesvm 模拟器）
  ignite-pay-mollusk-tests     状态通道链上程序测试（mollusk 模拟器）
  -->
```

---

## Crate 详细说明

### 1. ignite-pay-core

**位置**: `ignite-pay-core/`
**类型**: 库 (lib)
**用途**: 全项目共享的基础库，提供 DID 身份、DIDComm 加密通信、Verifiable Credential（VC）、白名单/黑名单风控、以及 E2EE 审计日志。

**关键模块**:

| 模块 | 说明 |
|:-----|:-----|
| `identity` | `did:ignite` 去中心化身份生成（Ed25519 + X25519）、DID Document 构建、持久化 |
| `didcomm` | DIDComm 消息创建、JWE 加密封装/解包、DIDComm 协议实现（含扫码支付请求/确认消息） |
| `vc` | Verifiable Credential 签发、验证、IPFS 解析 |
| `ipfs` | IPFS 客户端抽象（trait + Kubo 实现 + Mock） |
| `list_store` | 基于 sled 的白名单/黑名单持久化与风控决策 |
| `types` | 共享类型：`MerchantListEntry`、`RiskControlDecision`、`VerifiableCredential` |
| `audit_merkle` | Merkle 树审计日志（E2EE 日志） |
| `log_crypto` | E2EE 日志加密（HKDF + AES-GCM + zstd 压缩） |
| `log_chunk` / `log_sync` | 日志分块与同步 |
| `solana_did` | Solana DID 链上桥接（需 `solana` feature） |

**可选 features**: `kubo`（IPFS Kubo 客户端）、`solana`（链上 DID 桥接）

---

<!-- State Channel: 探索阶段，暂不启用
### 2. ignite-pay-state-channel

**位置**: `ignite-pay-state-channel/`
**类型**: 库 (lib)
**用途**: 离链 UTXO + Merkle Tree 状态通道引擎，支持流式支付、HTLC、多跳路由、Hub 网络和合规管理。

**关键模块**:

| 模块 | 说明 |
|:-----|:-----|
| `channel` | `ChannelManager` — 通道全生命周期管理（开通、注资、支付、结算） |
| `merkle` | Merkle 树构建与验证 |
| `signing` | Ed25519 签名工具（叶子签名、状态签名、密钥生成） |
| `types` | 通道数据结构（UTXO 叶子、通道元数据、通道状态） |
| `pipeline` | 支付流水线处理 |
| `htlc` | `HtlcManager` — HTLC 原像管理、生命周期追踪 |
| `hub` | `HubManager` — Hub 注册、指标管理 |
| `routing` | `RouteService` — DFS 路由发现、评分、选择 |
| `multihop` | `MultiHopManager` — 多跳 HTLC 支付协调、递减 timelock |
| `compliance` | `ComplianceManager` — 消费限额、滑动窗口、合规标记、审计追踪 |

---
-->

### 3. ignite-pay-solana

**位置**: `ignite-pay-solana/`
**类型**: 库 (lib)
**用途**: Solana 区块链集成层，提供 RPC 客户端封装、链上支付执行、ZK Compression DID 查询和会话密钥管理。<!-- State Channel: 探索阶段，暂不启用、状态通道链上指令构建器 -->

**关键模块**:

| 模块 | 说明 |
|:-----|:-----|
<!-- State Channel: 探索阶段，暂不启用
| `channel` | 10 个链上 Instruction 构建器（open、fund、settle、challenge、claim、HTLC 等） |
-->
| `payment` | `IgnitePayClient` — 链上支付执行（支持 Sponsored 和 SelfFunded 模式） |
| `compression` | ZK Compression DID 查询（Light Protocol `light-sdk`） |
| `session` | 会话密钥管理（`SessionKeypair`、`SessionTokenData`） |
| `session_program` | 会话程序客户端集成 |
| `types` | `PayMode`、`SessionTokenData`、`SplPaymentParams` 等类型 |

---

<!-- State Channel: 探索阶段，暂不启用
### 4. ignite-pay-program

**位置**: `ignite-pay-program/`
**类型**: 链上程序 (cdylib + lib) — Anchor 框架
**Program ID**: `DJBHr35jL3JAGoU7bKMsEFmpeNMrCSK7oYQE4HJ3GBUe`
**用途**: 状态通道的 Solana 链上程序，管理通道账户的完整生命周期和资金安全。

**链上指令（10 条）**:

| 指令 | 说明 |
|:-----|:-----|
| `open_channel` | 开通通道，验证 Ed25519 签名，初始化 Merkle 树 |
| `fund_channel` | Provider 注资（SPL Token 转账） |
| `cooperative_settle` | 双方协作结算（双方签名） |
| `trigger_challenge` | 发起争议挑战 |
| `submit_counter_state` | 提交反状态 |
| `settle_after_timeout` | 超时后结算 |
| `claim` | 认领叶子（Merkle Proof + 签名） |
| `verify_htlc` | 验证 HTLC 原像并认领 |
| `htlc_refund` | 过期 HTLC 退款 |
| `finalize_settlement` | 最终结算，按比例分配未认领余额 |

**关键数据结构**: `ChannelAccount`（通道账户）、`ChannelStatus`（状态枚举）

---
-->

### 5. ignite-pay-did-program

**位置**: `ignite-pay-did-program/`
**类型**: 链上程序 (cdylib + lib) — Anchor 框架
**用途**: 商户 DID 链上身份管理，使用 ZK Compression（Light Protocol）实现低成本链上 DID 注册和验证。

**链上指令（6 条）**:

| 指令 | 说明 |
|:-----|:-----|
| `init_platform` | 平台公钥一次性初始化 |
| `initialize_did` | 创建压缩商户 DID（平台签名验证 + 主体绑定） |
| `update_did_with_vc` | 绑定/更新 VC Hash（仅 controller） |
| `set_recovery_key` | 设置/更换恢复密钥 |
| `recover_controller` | 通过恢复密钥恢复 controller |
| `revoke_vc` | 撤销 VC（仅平台权限） |

**关键数据结构**: `MerchantCompressedDid`、`PlatformConfig`、`RevokedVc`

---

### 6. ignite-pay-session-program

**位置**: `ignite-pay-session-program/`
**类型**: 链上程序 (cdylib + lib) — Anchor 框架
**Program ID**: `6EFvVTh7rEBpHH2JGryjKQmBLRtbYtSEerGNfkHqKiei`
**用途**: 会话密钥链上管理，允许临时密钥在限定范围内执行支付，无需暴露主密钥。

**链上指令（4 条）**:

| 指令 | 说明 |
|:-----|:-----|
| `register_session_key` | 注册临时会话密钥（范围、限额、过期时间） |
| `execute_payment` | 使用会话密钥执行 SOL 转账（检查范围、限额、过期） |
| `revoke_session` | 撤销会话密钥（仅 owner） |
| `close_session` | 关闭并回收租金（需已撤销或已过期） |

---

<!-- State Channel: 探索阶段，暂不启用
### 7. ignite-pay-channel-service

**位置**: `ignite-pay-channel-service/`
**类型**: 三个二进制 (bin) — Axum 0.8 HTTP + WebSocket 服务
**用途**: 状态通道的 REST + WebSocket 服务端，提供 User（用户）、Provider（商户）、Hub（中继路由）三种角色的独立部署二进制。

**二进制目标**:

| 二进制 | 角色 | 默认端口 | 说明 |
|:-------|:-----|:---------|:-----|
| `channel-user` | User | 3001 | 用户端：开通通道、发起支付、HTLC 管理、路由发现 |
| `channel-provider` | Provider | 3002 | 商户端：接收支付、配签确认、结算认领 |
| `channel-hub` | Hub | 3003 | Hub 端：继承 Provider 全部功能 + 路由中继 + 多跳支付 |

**架构**: Hub 继承 Provider 所有端点，Provider 是 Hub 的子集。User 端点与其他两个角色完全不同。

**关键模块**:

| 模块 | 说明 |
|:-----|:-----|
| `config` | TOML 配置加载（`Config`、`Role` 枚举） |
| `state` | `AppState` 共享状态（Arc 包装，含 sled DB、ChannelManager、HubManager 等） |
| `server/router` | 基于角色的 Axum 路由构建器 |
| `ws` | WebSocket 协议定义与认证会话管理 |
| `handlers` | HTTP 请求处理器（channel、payment、settlement、htlc、routing、multihop、compliance） |
| `storage` | sled 存储层（通道索引、节点注册表） |

---
-->

### 8. didcomm-router

**位置**: `didcomm-router/`
**类型**: 二进制 (bin) — Axum HTTP + WebSocket 服务
**用途**: DIDComm 消息路由与中介服务，负责在 DID 对等方之间安全转发加密消息，支持 mediator 协议（mediate-request/grant、keylist-update）。

**关键模块**:

| 模块 | 说明 |
|:-----|:-----|
| `server` | Axum 路由构建（HTTP + WebSocket） |
| `session` | WebSocket 会话管理（认证、消息路由） |
| `protocols` | DIDComm 协议实现（mediate-request、keylist-update、peer-introduction、connection-request） |
| `transport` | WebSocket 传输层 |
| `notification` | 推送通知（FCM 集成） |
| `did` | DID Document 处理 |
| `storage` | 持久化存储（sled） |

---

### 9. did-registry

**位置**: `did-registry/`
**类型**: 二进制 (bin) — Axum HTTP 服务
**用途**: DID 链上注册与查询服务，提供 REST API 将商户 DID 注册到 Solana 链上（支持 ZK Compression），以及查询链上 DID 信息。

**关键模块**:

| 模块 | 说明 |
|:-----|:-----|
| `handlers` | HTTP 请求处理器（注册、查询） |
| `did` | DID 注册与查询逻辑 |
| `server` | Axum 路由构建 |
| `storage` | 持久化存储（sled） |

---

### 10. ignite-pay-mcp

**位置**: `ignite-pay-mcp/`
**类型**: 二进制 (bin) — MCP 服务器
**用途**: AI Agent 支付编排服务器，通过 Model Context Protocol (MCP) 暴露支付工具，处理 x402 HTTP 支付挑战，集成 DIDComm 加密授权和链上 Solana 支付。<!-- State Channel: 探索阶段，暂不启用 V3.0 新增状态通道支付能力，当无活跃会话密钥时自动回退到状态通道支付。 -->

**MCP 工具**:

| 工具 | 说明 |
|:-----|:-----|
| `process_x402_challenge` | 完整 x402 支付流程（解析挑战 → 验证 VC → 链上 DID 检查 → 风控 → 手机认证 → 执行支付），支持会话密钥<!-- State Channel: 探索阶段，暂不启用 和状态通道 -->两种支付方式 |
| `check_authorization` | 检查支付状态 |
| `get_payment_history` | 查询支付历史 |
| `get_identity` | 查看 DID 和连接状态 |
| `generate_pairing_invitation` | 生成手机配对二维码 |
| `create_session` / `get_session_status` / `close_session` | 会话密钥管理 |
| `execute_spl_payment` | 通过会话密钥执行 SPL Token 转账 |
| `add_merchant` / `update_merchant` / `verify_merchant` | 链上 ZK DID 管理 |
<!-- State Channel: 探索阶段，暂不启用
| `open_channel` | 与 Hub 建立状态通道（User 角色） |
| `channel_pay` | 通过状态通道发起支付 |
| `get_channel_status` | 查询通道状态（余额、序列号、叶子数） |
| `close_channel` | 协作关闭状态通道 |
| `settle_channel` | 发起链上结算 |
-->

**关键模块**:

| 模块 | 说明 |
|:-----|:-----|
<!-- State Channel: 探索阶段，暂不启用
| `channel` | 状态通道 User Side 客户端（`ChannelClient`），与 Hub HTTP API 通信 |
-->
| `payment` | 支付存储、待授权存储、支付类型 |
| `mediator` | DIDComm Mediator WebSocket 连接（配对/邀请） |
| `audit` | 支付与列表事件审计存储 |
| `tools` | MCP 工具输入类型定义 |

---

### 11. ignite-pay-skill (ignite_pay_rs)

**位置**: `ignite-pay-skill/`
**类型**: Python 扩展模块 (cdylib) — PyO3
**Python 包名**: `ignite_pay_rs`
**用途**: Web3 Agent Payment SDK 的 Python 绑定，将核心 DID 身份、DIDComm 通信、风控和支付功能暴露给 Python 生态。

**Python 类 `IgnitePayCore` 方法**:

| 方法 | 说明 |
|:-----|:-----|
| `new()` | 生成新 DID 身份 |
| `init_identity(db_path)` | 从 sled 加载/生成持久化身份 |
| `init_list_store(db_path)` | 初始化白名单/黑名单存储 |
| `start_listener(ws_url)` | 启动后台 WebSocket 监听器连接 mediator |
| `check_allowance(merchant_did, amount)` | 查询商户白名单/黑名单 |
| `risk_check(merchant_did, amount)` | 风控决策 |
| `check_and_pay(merchant_did, amount)` | 核心支付流程（含手机授权） |
| `add_to_whitelist` / `remove_from_whitelist` | 白名单管理 |
| `add_to_blacklist` / `remove_from_blacklist` | 黑名单管理 |

---

### 12. ignite_pay_app (rust_lib_ignite_pay_app)

**位置**: `ignite_pay_app/rust/`
**类型**: Flutter Rust Bridge 库 (cdylib + staticlib)
**用途**: Ignite Pay 移动端 Flutter 应用的 Rust 原生层，通过 Flutter Rust Bridge 提供 DID 身份、DIDComm 通信等原生功能。支持扫码支付：解析商户 QR 码 → 确认支付<!-- State Channel: 探索阶段，暂不启用 → 通过状态通道完成付款 -->。

**关键模块**:

| 模块 | 说明 |
|:-----|:-----|
<!-- State Channel: 探索阶段，暂不启用
| `api/channel` | 状态通道 bridge 函数（解析 QR、开通道、支付、关闭、结算） |
| `api/channel_store` | 通道状态 sled 持久化（`ChannelStore`） |
-->
| `api` | Flutter 可调用的 API 函数 |
| `frb_generated` | Flutter Rust Bridge 自动生成绑定 |

**Flutter 层关键文件**:

| 文件 | 说明 |
|:-----|:-----|
<!-- State Channel: 探索阶段，暂不启用
| `lib/services/channel_service.dart` | Dart 通道服务层（`ChannelService`） |
| `lib/qr_payment_screen.dart` | 扫码支付确认 UI（暗色玻璃拟物风格） |
-->

---

### 13. ignite-pay-merchant-mcp

**位置**: `ignite-pay-merchant-mcp/`
**类型**: 二进制 (bin) — MCP 服务器
**用途**: 商户侧 AI Agent MCP 服务器。生成收款二维码<!-- State Channel: 探索阶段，暂不启用、接收状态通道支付 -->、管理订单和收款记录。<!-- State Channel: 探索阶段，暂不启用 商户在状态通道中充当 Provider 角色。 -->

**MCP 工具**:

| 工具 | 说明 |
|:-----|:-----|
| `generate_payment_qr` | 生成收款二维码（`ignite://pay?d=<base64url>` 格式） |
| `check_payment` | 按 order_id 查询收款状态 |
| `get_payment_history` | 收款历史记录 |
<!-- State Channel: 探索阶段，暂不启用
| `get_channel_status` | 通道状态（余额、序列号、Provider 余额） |
| `open_channel_with_hub` | 提示商户 Provider pubkey 供用户开通道 |
| `close_channel` | 协作关闭通道 |
| `settle_channel` | 链上结算（claim + finalize） |
-->
| `get_identity` | 商户 DID、Hub 连接状态 |

**关键模块**:

| 模块 | 说明 |
|:-----|:-----|
<!-- State Channel: 探索阶段，暂不启用
| `channel` | `MerchantChannelClient` — Provider 角色状态通道客户端（接收支付、配签、结算） |
-->
| `payment` | `PaymentOrderStore` — 订单 sled 持久化（创建、确认、查询、列表） |
| `qr` | QR 码生成与解析（`PaymentQrData` 结构、`ignite://pay` 协议格式） |
| `mediator` | `MerchantMediator` — DIDComm Mediator 连接（发送支付确认消息） |
| `audit` | `AuditLogStore` — 商户操作审计日志 |
| `config` | TOML 配置加载（merchant、mediator、storage、solana、hub） |
| `tools` | MCP 工具输入类型定义 |

**QR 码格式**: `ignite://pay?d=<base64url(JSON)>`，JSON 包含 `type: "ignite-pay-request"`、`merchant_did`、`amount`、`description`、`order_id`<!-- State Channel: 探索阶段，暂不启用、`hub_endpoint` -->、`timestamp`。

---

### 14. ignite-pay-litesvm-tests

**位置**: `tests/svm-litesvm/`
**类型**: 测试库 (lib)
**用途**: 使用 litesvm SVM 模拟器对 `ignite-pay-program` 链上程序进行集成测试。

**覆盖的测试场景**: open_channel 签名验证、trigger_challenge、cooperative_settle、submit_counter_state、settle_after_timeout、challenge 未到期拒绝、settle 状态检查。

---

### 15. ignite-pay-mollusk-tests

**位置**: `tests/svm-mollusk/`
**类型**: 测试库 (lib)
**用途**: 使用 Mollusk SVM 模拟器对 `ignite-pay-program` 链上程序进行集成测试。与 litesvm 测试覆盖相同的测试场景，但使用不同的 SVM 模拟器后端。

---

## 文档

| 目录 | 说明 |
|:-----|:-----|
| `docs/` | 设计文档：<!-- State Channel: 探索阶段，暂不启用 状态通道规范、 -->DID 身份方案、DIDComm 通信协议、会话密钥、审计日志等 |
<!-- State Channel: 探索阶段，暂不启用
| `docs/deploy/` | 部署文档：ZK DID 部署指南、商户 DID 上链演练、状态通道实现细节 |
| `docs/deploy/state-channel/` | 状态通道部署配置：User、Hub、Merchant 三端服务部署文档 |
| `docs/deploy/state-channel/scenarios/` | 12 个业务场景实施文档：通道开通、离链支付、批量流水线、HTLC、协作关闭、争议解决、HTLC 结算、Hub 路由、多跳支付、自动关闭、合规审计、WebSocket 实时通信 |
-->

---

## 技术栈

| 类别 | 技术 |
|:-----|:-----|
| 区块链 | Solana (Anchor 框架) |
| 零知识压缩 | Light Protocol (ZK Compression) |
| 加密通信 | DIDComm v2 (JWE)、Ed25519、X25519 |
| 身份 | `did:ignite`（W3C DID 兼容）、Verifiable Credentials |
| 后端 | Rust、Axum 0.8、sled、tokio |
| 链上测试 | litesvm、mollusk |
| Python 绑定 | PyO3 |
| 移动端 | Flutter + Flutter Rust Bridge |
| AI 集成 | MCP (Model Context Protocol)、x402 支付协议、状态通道扫码支付 |
| 消息中介 | DIDComm Router (WebSocket) |
