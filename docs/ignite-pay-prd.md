# Ignite Pay — 产品文档

## 一、系统概览

Ignite Pay 是一套基于 Solana 的去中心化支付系统，由三个核心组件构成：

```
┌─────────────┐    DIDComm    ┌──────────┐    DIDComm    ┌──────────────┐
│  用户端 App  │◄────────────►│ Mediator │◄────────────►│  商户端 App   │
│ (Sentinel)  │   加密消息    │  中继服务 │   加密消息    │  (Merchant)  │
└──────┬──────┘               └──────────┘               └──────┬───────┘
       │                                                        │
       │  MB Voucher Signing          MB Voucher Collection      │
       ▼                                                        ▼
┌─────────────────────────────────────────────────────────────────────┐
│                  MagicBlock 支付通道 (链上)                          │
│  init_global · deposit · create_channel · settle_batch · release    │
│  optimistic_settle · dispute · resolve_dispute · withdraw           │
│  off-chain: sign_voucher · receive_voucher · merkle proof           │
└─────────────────────────────────────────────────────────────────────┘
       │
       ▼
┌─────────────────────────────────┐
│  Solana 区块链                    │
│  MagicBlock 支付通道合约           │
│  Session Key 合约                 │
└─────────────────────────────────┘
```

**核心理念：** 用户通过手机 App 掌控自己的 DID 身份和资金，对 AI 代理发起的支付进行实时授权；商户通过手机 App 生成收款码并接收即时支付确认。两端通过 DIDComm 端到端加密通信，经 Mediator 中继。支付基于 MagicBlock 支付通道实现：链上锁定资金 + 链下签名 Voucher + Merkle Sum Tree 批量结算。

---

## 二、用户端 App — Sentinel Dashboard

### 2.1 产品定位

用户的"支付守卫"App。管理 DID 身份、接收并审批 AI 代理发起的支付请求、管理 Session Key、维护白名单/黑名单策略。

### 2.2 技术栈

| 层级 | 技术 |
|------|------|
| UI 框架 | Flutter (Dart) |
| 加密/存储 | Rust (via flutter_rust_bridge) |
| 本地数据库 | sled (身份、Session Key、策略、通道) + SQLite (审计日志) |
| 消息传输 | DIDComm v2 (JWE authcrypt 加密) |
| 推送通道 | FCM (海外) / WebSocket 长连接 (国内) |
| 区块链 | Solana (MagicBlock 支付通道合约 + Session Key 合约) |

### 2.3 核心身份

用户拥有一个 `did:ignite:z<multicodec_base58>` 去中心化身份，包含：
- Ed25519 签名密钥
- X25519 密钥协商密钥（用于 JWE 加解密）
- W3C DID Document

身份存储在 sled 本地数据库，首次启动时生成。

### 2.4 功能模块

#### 2.4.1 引导流程 (OnboardingScreen)

三步引导：欢迎页 → 生成 DID 身份 → 配置 Mediator 连接（可跳过）。

#### 2.4.2 仪表盘 (Dashboard)

- DID 身份卡片（显示 DID、连接状态、待处理消息数）
- 通知铃铛：未读消息数徽章，点击进入通知中心
- 快捷入口：Vault（密钥库）、Policies（策略管理）、Channels（通道拓扑）
- "Scan MCP QR Code" 按钮：扫描商户二维码建立配对（支持 PaymentQrData 和 didcomm:// 配对）
- "Create Channel" 按钮：基于 MagicBlock 支付通道，指定商户公钥和消费上限创建通道
- 信任额度仪表盘：当日消费/限额
- 最近活动列表：实时数据，从 `DidcommService.messages` 获取
- 支付授权横幅：收到 `payment-auth-request` 时弹出

#### 2.4.3 支付授权 (ChallengeScreen — X402 协议)

核心交互界面。收到 MCP 支付请求后弹出全屏授权页：

1. **展示信息**：商户 DID、金额 (SOL)、支付原因
2. **策略配置**：每日限额、每日笔数上限、单笔限额、有效期
3. **名单操作**：仅本次 / 加入白名单 / 加入黑名单 / 移除白名单 / 移除黑名单
4. **签名方式选择**：内置密钥 / Phantom 深链接 / Solflare 深链接 / MWA (Android)
5. **操作**：Approve（创建 Session Key → 链上注册 → 回复 MCP）或 Decline & Block

#### 2.4.4 策略管理 (PolicyScreen)

按商户配置消费规则：
- 自动支付开关
- 单笔限额 (SOL/USD 切换)
- 周消费额度进度条
- 有效期倒计时

#### 2.4.5 密钥库 (VaultScreen)

- DID 身份展示（带动画渐变背景）
- 12 词助记词（可显示/隐藏）
- Mediator 端点配置
- 审计日志查看
- "Erase Key Material" 危险操作

#### 2.4.6 Session Key 管理 (SessionKeysScreen)

链上 Session Key 全生命周期：
- 注册新 Key（5 SOL / 24h 默认参数）
- 查看活跃/已过期 Key 列表
- 撤销（链上）/ 删除（本地）

#### 2.4.7 QR 扫描 (QrScannerScreen)

全屏相机扫描 `didcomm://?_oob=<base64>` 格式的 MCP 配对二维码。扫描后解析 OOB 邀请，连接 Mediator，发送 `connection-request`。

#### 2.4.8 通道支付 (QrPaymentScreen)

扫描商户收款码 `ignite://pay?d=<base64>` 后的确认页：
- 展示商户信息、金额、描述
- Confirm → 签名 Voucher（Ed25519 签名 `SHA256(channel_id || seq || amount)`）→ 发送给商户
- 显示支付结果（sequence, voucher signature）

#### 2.4.9 消息列表 (MessagesScreen)

DIDComm 消息收件箱：
- 筛选标签：全部 / Payment / List Sync / Connection
- 点击 `payment-auth-request` 消息触发授权流程
- 查看消息详情（含 raw body）

#### 2.4.10 连接管理 (ConnectionScreen)

- Mediator 连接状态（WS/FCM 通道显示）
- 已配对的 MCP 代理列表
- 添加新 MCP（打开 QR 扫描器）

#### 2.4.11 设置 (SettingsScreen)

- Solana 网络：Devnet / Mainnet 切换、RPC URL、DAS Endpoint
- SPL 账户压缩参数：Tree Address、Tree Authority
- 程序 ID：MagicBlock 支付通道、DID、Session Key（只读）
- 支付模式：自费 / 赞助
- 存储管理：清除缓存

#### 2.4.12 通知中心 (NotificationCenterScreen)

系统通知与连接更新消息列表：
- 消息列表：从 `DidcommService.messages` 中过滤非 payment-auth-request 类型的消息
- 已读/未读状态：通过 SharedPreferences 存储已读 ID
- 全部标为已读功能
- 通知详情弹窗：显示消息类型、CID、标签、描述、RAW BODY
- 从 Dashboard 通知铃铛图标进入（带未读数徽章）

#### 2.4.13 通道拓扑 (ChannelTopologyScreen)

MagicBlock 支付通道网络可视化与管理：
- 总余额卡片：显示全局 Vault 余额（SOL）+ 存入总额 + 已分配额度
- 本节点卡片：显示用户 DID + MB Buyer Pubkey + 连接状态脉冲动画
- 通道卡片列表：商户公钥、消费上限 (spending_cap)、已结算金额 (settled_amount)、当前 batch nonce、挑战期/争议期
- 操作：调整消费上限 (`update_spending_cap`)、争议 (`dispute`)、解决争议 (`resolve_dispute`)
- 下拉刷新
- 空状态/错误状态/加载中状态

#### 2.4.14 交易历史 (TransactionHistoryScreen)

交易记录浏览：
- 筛选标签：All / Payment / List Sync
- 交易卡片列表：商户 DID、金额 (SOL)、状态徽章（Pending/Processed）
- 交易详情弹窗：类型、Payment ID、商户、金额、描述、RAW BODY
- 下拉刷新（重连 Mediator 拉取最新消息）
- 数据源：`DidcommService.messages` 按类型筛选

#### 2.4.15 个人资料 (ProfileScreen)

用户身份与账户总览：
- DID 头像（前两个字符）
- DID 显示（可复制）
- 编辑显示名称（SharedPreferences 持久化）
- 网络信息：Devnet / Mainnet 切换显示
- 设备状态：连接状态指示灯 + Session Key 激活状态徽章
- 统计卡片：通道数 / 余额 (SOL) / 商户数
- 导出 DID Document（复制到剪贴板）

#### 2.4.16 MB 支付通道管理 (MbChannelScreen)

MagicBlock 支付通道配置：
- 配置 MB RPC URL 和 Program ID
- 全局状态初始化 (`init_global`)：创建 GlobalState + GlobalVault PDA
- 充值 (`deposit`)：SOL 转入 GlobalVault
- 创建通道：指定商户公钥、消费上限 (spending_cap)、挑战期 (challenge_period)、争议期 (dispute_period)
- 提取未分配资金 (`withdraw`)
- 从 Dashboard 的 "Create Channel" 按钮进入

### 2.5 核心服务

| 服务 | 职责 |
|------|------|
| `DidcommService` | DID 身份管理、Mediator 连接、消息收发、认证、推送编排 |
| `SessionKeyService` | Session Key 创建/注册/撤销/查询，支持内置密钥和外接钱包 |
| `ChannelService` | MagicBlock 支付通道操作（初始化全局状态、充值、创建通道、签名 Voucher、争议/解决争议、提取） |
| `FcmService` | Firebase 推送通知 |
| `MediatorApi` | Mediator REST API HTTP 客户端 |
| `WalletDeepLinkService` | Phantom / Solflare 钱包深度链接构建 |
| `WalletMwaService` | Mobile Wallet Adapter（Android，桩实现） |

### 2.6 Rust Bridge 函数清单

| 函数 | 用途 |
|------|------|
| `initialize_identity` | 生成/加载 DID 身份 |
| `connect_mediator` | 连接 Mediator WebSocket |
| `disconnect_mediator` | 断开连接 |
| `authenticate_with_mediator` | 挑战-响应认证获取 JWT |
| `pull_messages` | 从 Mediator 拉取 JWE 消息 |
| `decrypt_message` | 解密 JWE 提取支付字段 |
| `send_auth_response` | 发送支付授权响应（含 Session Key） |
| `register_device_token` | 注册 FCM 设备令牌 |
| `parse_oob_invitation` | 解析 OOB 邀请二维码 |
| `send_connection_request` | 发送连接请求到 MCP |
| `create_session_key_for_payment` | 创建临时 Session Key |
| `build_unsigned_register_tx` | 构建未签名注册交易（外接钱包） |
| `complete_register_with_signature` | 完成注册 |
| `revoke_session_key_onchain` | 链上撤销 Session Key |
| `save_merchant_policy` / `load_merchant_policy` | 商户策略持久化 |
| `parse_payment_qr` | 解析收款二维码 |
| `mb_init_global` | 初始化全局状态（创建 GlobalState + GlobalVault PDA） |
| `mb_deposit` | 向 GlobalVault 充值 SOL |
| `mb_create_channel` | 创建支付通道（指定商户、消费上限、挑战期、争议期） |
| `mb_update_spending_cap` | 调整通道消费上限 |
| `mb_get_channel` / `mb_get_global_state` | 查询通道/全局状态 |
| `mb_sign_voucher` | 签名 Voucher（Ed25519 签名 `SHA256(channel_id \|\| seq \|\| amount)`） |
| `mb_sign_settlement` | 签名结算消息（验证 Merkle Root 后签名） |
| `mb_dispute` / `mb_resolve_dispute` | 争议/解决争议（提交 Merkle Proof 欺诈证明） |
| `mb_withdraw` | 提取未分配资金 |

### 2.7 支付授权完整流程

```
1. MCP 代理发起 payment-auth-request
2. Mediator 推送到用户 App（FCM 或 WS）
3. App 拉取 JWE → 解密 → 展示授权请求
4. 用户在 ChallengeScreen 审核：
   a. 确认金额、商户
   b. 可选：调整策略参数
   c. 可选：添加白名单/黑名单
   d. 选择签名方式
5. App 创建 Session Key → 链上注册交易 → 确认
6. 发送 payment-auth-response（含 Session Key 数据）回 MCP
7. MCP 使用 Session Key 执行支付
```

---

## 三、商户端 App — Ignite Merchant

### 3.1 产品定位

商户的收款工具。生成收款二维码、接收即时支付确认、管理 MagicBlock 支付通道、语音播报到账。

### 3.2 技术栈

| 层级 | 技术 |
|------|------|
| UI 框架 | Flutter (Dart) |
| 加密/存储 | Rust (via flutter_rust_bridge) |
| 本地数据库 | sled (订单、密钥、通道、DIDComm 身份) |
| 消息传输 | DIDComm v2 (JWE authcrypt 加密) |
| 推送通道 | FCM (海外) / WebSocket 长连接 (国内) |
| 语音播报 | flutter_tts (中英双语) |

### 3.3 双 DID 架构

商户 App 管理两个独立身份：

| 身份 | DID 格式 | 用途 | 存储位置 |
|------|----------|------|----------|
| MagicBlock 通道 DID | `did:ignite:<raw_base58>` | QR 码生成、Voucher 收集、链上结算签名 | sled `mb_keys` tree |
| DIDComm 通信 DID | `did:ignite:z<multicodec_base58>` | JWE 加解密、Mediator 消息收发 | sled `didcomm_identity` tree |

两者密钥体系完全独立，互不干扰。

### 3.4 功能模块

#### 3.4.1 引导流程 (OnboardingScreen)

1. 填写 MagicBlock RPC URL 和 Program ID
2. 填写 Mediator WebSocket URL（可选）
3. 生成商户身份（Ed25519 密钥对 → MB Merchant Keypair）
4. 初始化推送服务（连接 Mediator）

#### 3.4.2 仪表盘 (DashboardScreen)

- "Ignite Merchant" 头部 + 在线状态指示灯（"在线"）
- 通知铃铛：未读订单数徽章，点击进入通知中心
- 今日汇总卡片：已收款总额 (USDC) + 笔数（仅计 confirmed）
- 快捷操作：生成收款码 / 通道管理 / MB 配置
- 最近订单列表：点击进入订单详情

#### 3.4.3 生成收款码 (QrGenerateScreen)

核心收款界面：

1. 输入金额 (USDC) + 可选描述
2. 生成 QR 码：格式 `ignite://pay?d=<base64url(JSON)>`
3. QR 码展示，进入等待确认状态
4. **双通道等待**：
   - 推送确认（主通道）：监听 `MerchantPushService.confirmations` 流
   - 轮询兜底（5 秒间隔）：调用 `refreshOrders()` 检查订单状态
5. 确认后：绿色对勾 + 触觉反馈 + 语音播报

**QR 码载荷结构 (PaymentQrData)**：
```json
{
  "type": "ignite-pay-request",
  "version": 1,
  "merchant_did": "did:ignite:...",
  "merchant_pubkey": "MB merchant Ed25519 base58 pubkey",
  "amount": 1000000000,
  "description": "咖啡",
  "order_id": "uuid-v4",
  "timestamp": 1713700000
}
```

#### 3.4.4 收款明细 (PaymentListScreen)

- 筛选：全部 / 待确认 / 已确认
- 下拉刷新
- 订单卡片列表（金额、状态徽章、描述、时间）

#### 3.4.5 订单详情 (PaymentDetailScreen)

- 金额展示（大字体 USDC）
- 状态徽章：confirmed=绿 / pending=琥珀 / failed=红 / expired=灰
- 订单信息：订单号（可复制）、描述、创建时间、确认时间
- 通道信息（仅 confirmed）：Channel ID（可复制）、Voucher Seq、Buyer Signature

#### 3.4.6 通道管理 (ChannelScreen)

- 汇总：通道总数 + 累计收款 (USDC)
- 通道卡片列表（Buyer Pubkey、Spending Cap、Settled Amount、Nonce）
- 下拉刷新

#### 3.4.7 通道详情 (ChannelDetailScreen)

- 通道信息展示：Buyer Pubkey、Spending Cap、Settled Amount、Nonce、Challenge Period、Dispute Period
- 操作按钮：
  - **批量结算**：`settle_batch`（构建 Merkle Sum Tree + 双签名）或 `optimistic_settle`（仅商户签名）
  - **释放结算**：`release_settlement`（挑战期后）
  - **强制释放**：`force_release`（争议期后）

#### 3.4.8 设置 (SettingsScreen)

- 商户身份：MB Merchant Pubkey（可复制）、MB Program ID（只读）
- 连接配置：MB RPC URL（可编辑）、Mediator WS（状态指示灯）
- 推送服务：DIDComm DID（可复制）、Mediator 连接状态、推送通道类型
- 语音播报：开关、语言切换（中/英）、音量滑块、测试按钮
- 关于：版本 1.0.0

#### 3.4.9 通知中心 (NotificationCenterScreen)

商户端通知列表（中文界面）：
- 从 `MerchantService.orders` 转换为通知（收款成功/待确认）
- 已读/未读状态管理（SharedPreferences `merchant_read_notification_ids`）
- 全部标为已读功能
- 通知详情弹窗：订单号、金额、描述、状态、通道
- 从 Dashboard 通知铃铛图标进入（带未读数徽章）

#### 3.4.10 个人资料 (ProfileScreen)

商户身份与账户总览（中文界面）：
- DID 头像（前两个字符或 "M"）
- DID 显示（可复制）+ DID 文档导出
- 编辑商户名称（SharedPreferences 持久化）
- 网络信息：Devnet / Mainnet 显示
- 连接状态：推送服务连接指示灯 + MB RPC URL 显示
- 统计卡片：通道数 / 余额 (SOL) / 已确认订单数
- 从 Dashboard 个人资料入口进入

#### 3.4.11 MB 配置 (MbConfigScreen)

MagicBlock 支付通道配置：
- MB RPC URL 和 Program ID 配置
- 查看商户 MB Keypair
- 从 Dashboard 快捷操作进入

### 3.5 核心服务

| 服务 | 职责 |
|------|------|
| `MerchantService` | 商户身份、订单管理、QR 生成、配置持久化 |
| `MerchantPushService` | 双通道推送编排（WS/FCM）、消息解密、订单确认 |
| `ChannelService` | MagicBlock 支付通道：Voucher 收集、批量结算、乐观结算、释放 |
| `VoiceService` | 支付到账语音播报（中英双语） |
| `FcmService` | Firebase 推送通知 |
| `MediatorApi` | Mediator REST API HTTP 客户端 |

### 3.6 Rust Bridge 函数清单

**merchant.rs — MagicBlock 支付通道与订单：**

| 函数 | 用途 |
|------|------|
| `initialize_merchant` | 生成/加载商户 MB 密钥对 |
| `generate_merchant_keypair` | 生成 Ed25519 密钥对 |
| `get_merchant_pubkey` | 获取 base58 公钥 |
| `generate_payment_qr` | 创建订单 + 生成 QR 字符串 |
| `list_orders` / `get_order` / `get_pending_orders` | 订单查询 |
| `confirm_order` | 订单状态 pending → confirmed |
| `mb_get_channel` | 查询通道状态（Buyer/merchant 通道 PDA） |
| `mb_receive_voucher` | 验证买家 Voucher 签名并存储 |
| `mb_settle_batch` | 构建 Merkle Sum Tree，双签名批量结算 |
| `mb_optimistic_settle` | 仅商户签名的乐观结算 |
| `mb_get_settlement` | 查询结算 Escrow 账户 |
| `mb_release_settlement` | 释放结算资金到商户 |
| `mb_force_release` | 争议期后强制释放 |

**merchant_didcomm.rs — DIDComm 通信：**

| 函数 | 用途 |
|------|------|
| `initialize_merchant_comm` | 生成/加载 DIDComm 身份（独立于 MB 通道 Keypair） |
| `connect_mediator` | 连接 Mediator |
| `disconnect_mediator` | 断开连接 |
| `authenticate_with_mediator` | 挑战-响应认证获取 JWT |
| `pull_messages` | 从 Mediator 拉取 JWE 消息 |
| `decrypt_message` | 解密 JWE 提取支付确认字段 |
| `register_device_token` | 注册 FCM 设备令牌 |

### 3.7 收款完整流程

```
1. 商户在 QR Generate 页面输入金额 + 描述
2. App 调用 Rust generate_payment_qr()
   → 创建 UUID 订单 (status=pending)
   → 返回 ignite://pay?d=... 二维码字符串（含商户 MB Pubkey）
3. 用户 App 扫码 → 解析 PaymentQrData
4. 用户确认支付 → 用户 App 签名 Voucher（Ed25519 签名 SHA256(channel_id || seq || amount)）
5. Voucher 通过 DIDComm 发送给商户 → Mediator 推送到商户 App
6. 商户 App 调用 mb_receive_voucher() 验证买家签名并存储
7. 商户 App confirm_order() 更新订单状态为 confirmed
8. 触发：
   - QR 页面绿色对勾
   - 触觉反馈
   - 语音播报（"收到收款 X.XX USDC"）
   - Dashboard 今日汇总刷新
9. 后续结算流程（商户主动触发）：
   a. mb_settle_batch()：构建 Merkle Sum Tree，商户签名，提交链上结算
   b. 或 mb_optimistic_settle()：仅商户签名（需买家配合提供结算签名时用 settle_batch）
   c. 挑战期过后 mb_release_settlement()：资金释放到商户
   d. 如有争议：买家可 mb_dispute()，商户可 mb_force_release() 或买家 mb_resolve_dispute() 提交欺诈证明
```

---

## 四、共享基础设施

### 4.1 ignite-pay-core

核心协议库，提供：

| 模块 | 功能 |
|------|------|
| `identity` | DID 生成、DID Document 构建、身份持久化、DID 签名验证 |
| `didcomm` | DIDComm 消息构造器（15 种消息类型）、JWE 加解密、Agent 创建 |
| `types` | 共享类型：PaymentRequest, MerchantListEntry, VerifiableCredential, RiskControlDecision |
| `list_store` | 白名单/黑名单管理（sled + IPFS 同步），风控决策 |
| `vc` | Verifiable Credential 签发与验证 |
| `ipfs` | IPFS 上传/下载抽象层 |
| `audit_merkle` | SHA-256 Merkle 树审计日志 |
| `log_crypto` / `log_chunk` / `log_sync` | E2EE 审计日志（加密 → Zstd 压缩 → IPFS 同步） |

### 4.2 ignite-pay-mb-sdk

MagicBlock 支付通道 SDK，提供：

| 模块 | 功能 |
|------|------|
| `pda` | PDA 派生：`derive_global_state_pda`、`derive_global_vault_pda`、`derive_channel_pda`、`derive_settlement_pda` |
| `merkle` | Sum-Merkle Tree（每个节点存储 hash + sum）：`build_sum_merkle_tree`、`MerkleProof`（兄弟 hashes + sums） |
| `signing` | Voucher 签名：`sign_voucher(channel_id, seq, amount, sk)` → `(msg_hash, sig)`；结算签名：`sign_settlement`、`build_settlement_message`；签名验证：`verify_signature` |
| `transaction` | 11 个交易构建器：`build_initialize_global_tx`、`build_deposit_tx`、`build_initialize_channel_tx`、`build_update_spending_cap_tx`、`build_settle_batch_tx`、`build_optimistic_settle_tx`、`build_dispute_tx`、`build_resolve_dispute_tx`、`build_release_settlement_tx`、`build_force_release_tx`、`build_withdraw_tx` |

**链上账户结构：**

| 账户 | 大小 | 字段 |
|------|------|------|
| GlobalState | 57 bytes | `buyer`, `total_deposited`, `total_allocated`, `bump` |
| Channel | 113 bytes | `buyer`, `merchant`, `spending_cap`, `settled_amount`, `nonce`, `challenge_period`, `dispute_period`, `bump` |
| SettlementEscrow | 132 bytes | `channel`, `merchant`, `amount`, `merkle_root`, `nonce`, `created_at`, `claimed`, `disputed`, `optimistic`, `bump` |

**链上指令：**

| 指令 | 签名者 | 说明 |
|------|--------|------|
| `initialize_global` | buyer | 创建 GlobalState + GlobalVault PDA |
| `deposit` | buyer | SOL 转入 GlobalVault |
| `initialize_channel` | buyer | 创建支付通道，锁定 spending_cap |
| `update_spending_cap` | buyer | 调整通道消费上限 |
| `settle_batch` | merchant | 双签名批量结算（Ed25519 指令内省） |
| `optimistic_settle` | merchant | 仅商户签名乐观结算（需 challenge_period > 0） |
| `release_settlement` | merchant | 挑战期后释放资金 |
| `dispute` | buyer | 冻结 Escrow（挑战窗口内） |
| `force_release` | merchant | 争议期后强制释放 |
| `resolve_dispute` | buyer | 欺诈证明（Sum-Merkle Proof） |
| `withdraw` | buyer | 提取未分配资金 |

### 4.3 DIDComm 消息类型

| 消息 | 方向 | 用途 |
|------|------|------|
| `out-of-band/2.0/invitation` | MCP → 用户 | QR 配对邀请 |
| `ignite-pay/1.0/connection-request` | 用户 → MCP | 建立连接 |
| `ignite-pay/1.0/connection-response` | MCP → 用户 | 连接确认 |
| `ignite-pay/1.0/payment-auth-request` | MCP → 用户 | 请求支付授权 |
| `ignite-pay/1.0/payment-auth-response` | 用户 → MCP | 授权响应（含 Session Key） |
| `ignite-pay/1.0/channel-payment-request` | — | MagicBlock Voucher 支付请求 |
| `ignite-pay/1.0/channel-payment-confirm` | — | Voucher 支付确认（含签名）→ 推送给商户 |
| `ignite-pay/1.0/list-sync-notification` | MCP → 用户 | 白名单/黑名单更新 |
| `coordinate-mediation/2.0/*` | 双向 | Mediator 协议（mediate-request, keylist-update） |
| `ignite-pay/1.0/ws-challenge-response` | 双向 | WS 认证挑战 |
| `messagepickup/3.0/*` | 双向 | 消息拾取协议 |

### 4.4 Mediator REST API

| 端点 | 方法 | 用途 |
|------|------|------|
| `/v1/auth/challenge` | GET | 获取认证 nonce |
| `/v1/auth/token` | POST | 签名换 JWT |
| `/v1/sync/list` | GET | 拉取消息列表（游标分页） |
| `/v1/sync/messages/{id}` | GET | 获取单条消息 |
| `/v1/agents/{id}/command` | POST | 发送加密命令 |
| `/v1/agents/bind` | POST | 绑定 Agent DID |
| `/v1/devices/register-token` | POST | 注册推送通道（FCM token 或 websocket） |

### 4.5 MagicBlock 支付通道架构

**三层架构：**

| 层级 | 说明 |
|------|------|
| L1 (Solana) | 通道创建、资金锁定、签名验证、最终结算 |
| ER (MagicBlock) | 高速状态转换（<50ms 延迟、免 Gas），记录每笔 Voucher |
| Off-chain 欺诈层 | 挑战窗口争议解决，基于 Merkle Proof |

**安全模型（三重防护）：**

| 防护 | 说明 |
|------|------|
| 消费上限 | `settled_amount + total_amount <= spending_cap`（链上检查） |
| 余额检查 | `total_amount <= vault.lamports`（实际余额） |
| 双签名 | Ed25519 指令内省验证买家 + 商户签名 |

**Global Vault 设计：** 每个 Buyer 一个全局 Vault（`GlobalVault PDA`），`total_allocated` 追踪所有通道消费上限之和，防止超额分配。

---

## 五、双端对比

| 维度 | 用户端 (Sentinel) | 商户端 (Merchant) |
|------|-------------------|-------------------|
| **核心角色** | 支付授权守卫 | 收款工具 |
| **DID 数量** | 1 个（通信 + 交易共用） | 2 个（MB 通道 Keypair + DIDComm DID） |
| **消息方向** | 收 auth-request → 发 auth-response | 收 payment-confirm → 确认订单 |
| **QR 交互** | 扫码（配对 MCP / 支付） | 生成码（收款） |
| **链上操作** | Session Key 注册/撤销、MB 通道管理 | MB 结算/释放/争议处理 |
| **推送触发** | MCP 支付请求 | 支付确认通知 |
| **特色功能** | 白名单/黑名单策略、外接钱包签名 | 语音播报、订单管理 |
| **UI 语言** | 英文 | 中文 |
| **屏幕数** | 16 | 11 |
| **Rust 模块** | simple + identity + auth + session + ws_client + voucher_store + log_store (7) | merchant + merchant_didcomm + voucher_store + settlement_store (4) |
| **Bridge 函数** | 30+ | 24 |

---

## 六、设计系统

两个 App 共享同一套 Dark Glassmorphism 设计语言：

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

共享组件：`BackButtonGlass`, `PageHeader`, `SettingsTile`, `SectionLabel`, `glassDecoration()`。

---

## 七、数据模型

### 7.1 订单 (PaymentOrder)

```
状态流转：pending → confirmed / failed / expired

字段：
  orderId        String      UUID v4
  merchantDid    String      did:ignite:...
  amount         BigInt      lamports (1 USDC = 1_000_000_000)
  description    String      可选描述
  merchantPubkey String      商户 MB Ed25519 公钥 (base58)
  status         String      "pending" | "confirmed" | "failed" | "expired"
  createdAt      int         Unix 秒
  confirmedAt    int?        Unix 秒（仅 confirmed）
  channelId      String?     通道 PDA（仅 confirmed）
  voucherSeq     BigInt?     Voucher 序列号（仅 confirmed）
  buyerSig       String?     买家 Voucher 签名（仅 confirmed）
```

### 7.2 通道 (ChannelAccount)

```
字段：
  buyer            Pubkey      买家公钥
  merchant         Pubkey      商户公钥
  spending_cap     u64         消费上限 (lamports)
  settled_amount   u64         已结算金额 (lamports)
  nonce            u64         当前 batch nonce
  challenge_period i64         挑战期（秒）
  dispute_period   i64         争议期（秒）
```

### 7.3 全局状态 (GlobalStateAccount)

```
字段：
  buyer            Pubkey      买家公钥
  total_deposited  u64         总充值金额 (lamports)
  total_allocated  u64         总分配额度（所有通道 spending_cap 之和）
```

### 7.4 结算 Escrow (SettlementEscrowAccount)

```
字段：
  channel          Pubkey      通道 PDA
  merchant         Pubkey      商户公钥
  amount           u64         结算金额
  merkle_root      [u8; 32]    Merkle Sum Tree 根哈希
  nonce            u64         Batch nonce
  created_at       i64         创建时间戳
  claimed          bool        是否已释放
  disputed         bool        是否有争议
  optimistic       bool        是否乐观结算
```

### 7.5 DIDComm 消息 (DecryptedMessage)

```
商户端解密后字段：
  msgType      String      消息类型 URI
  orderId      String?     关联订单 ID
  channelId    String?     通道 PDA
  voucherSeq   BigInt?     Voucher 序列号
  amount       BigInt?     确认金额
  buyerSig     String?     买家 Voucher 签名 (base58)
  authorized   bool?       授权状态
  rawBody      String      原始 JSON
```

---

## 八、推送通知架构

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

**WS 流程**（国内用户）：连接 → identify → 持续监听 → 收到 JWE → 直接解密处理

**FCM 流程**（海外用户）：SIGNAL 通知 → `onSignalReceived` → pull_messages 拉取 → 解密处理

**共同点**：
- 首次连接时 authenticate → pull 离线消息
- WS 断线后先拉离线消息再重连（3 秒延迟）
- FCM 前台收到时显示本地通知（标题 "Payment Received"）

---

## 九、安全模型

| 安全措施 | 说明 |
|----------|------|
| DID 身份 | Ed25519 签名密钥 + X25519 密钥协商，本地 sled 加密存储 |
| 消息加密 | DIDComm authcrypt (JWE)，端到端加密，Mediator 无法读取明文 |
| Mediator 认证 | 挑战-响应：nonce → Ed25519 签名 → JWT 令牌 |
| Session Key | 临时密钥链上注册，有效期和额度限制，可撤销 |
| 白名单/黑名单 | IPFS 同步的商户名单，风控决策 (Blocked/AutoApproved/NeedsAuth) |
| 审计日志 | Merkle 树 + E2EE 加密 + IPFS 同步，防篡改 |
| 双 DID 隔离 | MB 通道 Keypair 和通信 DID 分离，互不影响 |
| MagicBlock 安全 | 三重防护：消费上限检查 + Vault 余额检查 + Ed25519 双签名内省 |
| 欺诈证明 | Sum-Merkle Tree 欺诈证明，单 Voucher + O(log N) 兄弟节点即可证明 |
| 挑战窗口 | 结算后进入 challenge_period，买家可 dispute 冻结资金，提交 Merkle Proof 解决争议 |
