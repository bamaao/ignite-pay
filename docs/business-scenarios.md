# Ignite Pay 业务场景操作手册

本文档按业务事件组织，描述 Ignite Pay 系统中所有核心业务流程的操作步骤。每个流程包含多个用例，每个用例包括前置条件、参与角色、详细步骤、预期结果和异常处理。

---

## 目录

1. [用户 DID 身份创建](#业务事件-1-用户-did-身份创建)
2. [App 与 MCP Server 建立连接](#业务事件-2-app-与-mcp-server-建立连接)
3. [DIDComm Mediator 认证](#业务事件-3-didcomm-mediator-认证)
4. [X402 支付授权](#业务事件-4-x402-支付授权)
5. [Session Key 链上支付](#业务事件-5-session-key-链上支付)
<!-- State Channel: 探索阶段，暂不启用
6. [状态通道开通](#业务事件-6-状态通道开通)
7. [状态通道离链支付](#业务事件-7-状态通道离链支付)
-->
8. [QR 码收款](#业务事件-8-qr-码收款)
<!-- State Channel: 探索阶段，暂不启用
9. [状态通道关闭与结算](#业务事件-9-状态通道关闭与结算)
10. [Hub 注册与发现](#业务事件-10-hub-注册与发现)
11. [多跳路由支付](#业务事件-11-多跳路由支付)
-->
12. [商户 DID 入驻](#业务事件-12-商户-did-入驻)
13. [消息推送](#业务事件-13-消息推送)
14. [商户 DID 生命周期管理](#业务事件-14-商户-did-生命周期管理)
<!-- State Channel: 探索阶段，暂不启用
15. [状态通道运维操作](#业务事件-15-状态通道运维操作)
16. [Hub 网络拓扑管理](#业务事件-16-hub-网络拓扑管理)
-->
17. [App 端管理与设置](#业务事件-17-app-端管理与设置)
<!-- State Channel: 探索阶段，暂不启用
18. [合规与风控](#业务事件-18-合规与风控)
-->

> 目录中标题与正文章节标题一一对应。各标题中的冒号（`:`）为正文实际格式，GitHub / 大多数 Markdown 渲染器会自动将标题转为小写并替换空格为 `-` 以生成锚点。

---

## 业务事件 1: 用户 DID 身份创建

### 用例 1.1: 用户 App 首次启动 → 自动生成 DID

**前置条件**：
- Sentinel App 已安装到手机
- 首次启动，本地无已有身份数据

**参与角色**：用户、Sentinel App

**详细步骤**：

1. 用户打开 Sentinel App
2. App 检测本地 sled 数据库中无 DID 身份记录
3. 进入 OnboardingScreen 三步引导流程：
   - 第一步：欢迎页面介绍 App 功能
   - 第二步：点击 "Generate Identity" 按钮
4. App 调用 Rust bridge `initialize_identity()`：
   - 生成 Ed25519 签名密钥对
   - 从公钥推导 X25519 密钥协商密钥
   - 构建 DID 标识符：`did:ignite:z<multibase-base58btc>`
   - 编码内容：`0xed 0x01` (multicodec Ed25519 前缀) + 32 字节 Ed25519 公钥
5. 构建 W3C DID Document：
   - `verificationMethod`: Ed25519VerificationKey2020 (`#key-signing-1`)
   - `keyAgreement`: X25519KeyAgreementKey2020 (`#key-agreement-1`)
   - `service`: IgnitePolicyList 端点（初始 CID 为空）
6. 将密钥对和 DID Document 持久化到 sled 本地数据库
7. 第三步：配置 Mediator 连接（可跳过）
8. 进入 Dashboard 主页

**预期结果**：
- 用户拥有一个 `did:ignite:z...` 去中心化身份
- DID Document 包含签名密钥和加密密钥
- 身份数据安全存储在本地 sled 数据库

**异常处理**：
- sled 数据库写入失败 → 显示错误提示，重试生成
- 密钥对生成失败（系统随机源不可用）→ 提示系统错误

---

### 用例 1.2: 商户 App 首次启动 → 自动生成 DID

**前置条件**：
- Ignite Merchant App 已安装
- 首次启动

**参与角色**：商户、Merchant App

**详细步骤**：

1. 商户打开 Ignite Merchant App
2. 进入 OnboardingScreen：
   - 填写 Hub Endpoint URL（如 `http://hub.example.com:3003`）
   - 填写 Mediator WebSocket URL（可选，如 `ws://mediator.example.com:8080/ws`）
3. App 调用 Rust bridge `initialize_merchant()`：
   - 生成 Ed25519 密钥对
   - <!-- State Channel: 探索阶段，暂不启用 - 原文含"生成状态通道 DID" -->生成商户 DID：`did:ignite:<raw_base58>`
   - 存储到 sled `keypairs` tree
4. App 调用 Rust bridge `initialize_merchant_comm()`：
   - 生成独立的 Ed25519 + X25519 密钥对
   - 生成 DIDComm 通信 DID：`did:ignite:z<multicodec_base58>`
   - 存储到 sled `didcomm_identity` tree
5. 连接 Mediator（如已配置）
6. 进入商户 Dashboard

**预期结果**：
- 商户拥有两个独立 DID：
  - 状态通道 DID（用于 QR 码、通道操作、链上签名）
  - DIDComm 通信 DID（用于 JWE 加解密、Mediator 消息）
- 两个密钥体系完全独立

**异常处理**：
- Hub Endpoint 不可达 → 显示警告，允许继续（后续配置）
- Mediator 连接失败 → 显示离线状态，允许后续重连

---

### 用例 1.3: MCP Server 首次启动 → 自动生成 DID

**前置条件**：
- MCP Server 二进制已编译
- config.toml 已配置

**参与角色**：系统管理员、MCP Server

**详细步骤**：

1. 管理员启动 MCP Server：`cargo run -p ignite-pay-mcp`
2. Server 检查 sled 数据库 (`./data`) 中是否已有 DID 身份
3. 若无已有身份：
   - 自动生成 Ed25519 + X25519 密钥对
   - 推导 `did:ignite` 标识符
   - 持久化到 sled 数据库
4. 若已有身份：
   - 自动加载已有密钥对和 DID
5. 连接 Mediator WebSocket (`ws://127.0.0.1:8080/ws`)
6. 执行 DIDComm Mediator 握手（→ 用例 2.3）
7. 进入消息接收循环

**预期结果**：
- MCP Server 拥有 `did:ignite` 身份
- 已连接到 Mediator 并完成握手
- 可接收/发送 DIDComm 加密消息

**异常处理**：
- Mediator 不可达 → 每 3 秒自动重连
- sled 数据库损坏 → 从备份恢复或重新生成身份

---

## 业务事件 2: App 与 MCP Server 建立连接

### 用例 2.1: 用户 App 扫码配对用户 MCP

**前置条件**：
- 用户已生成 DID 身份（用例 1.1）
- MCP Server 已启动并连接到 Mediator（用例 1.3）
- MCP Server 已生成 OOB 邀请二维码

**参与角色**：用户、Sentinel App、MCP Server、Mediator

**详细步骤**：

1. MCP Server 生成 OOB 邀请二维码，格式：`didcomm://?_oob=<base64>`
2. 用户在 Sentinel Dashboard 点击 "Scan MCP QR Code"
3. 打开全屏 QR 扫描器 (QrScannerScreen)
4. 扫描 MCP Server 的二维码
5. App 调用 Rust bridge `parse_oob_invitation()` 解析邀请：
   - 提取 MCP Server 的 DID
   - 提取 Mediator WebSocket endpoint
6. App 调用 `connect_mediator()` 连接 Mediator WebSocket
7. App 调用 `send_connection_request()` 发送 `connection-request` DIDComm 消息：
   - 消息通过 Mediator 中继到 MCP Server
   - 包含用户 DID Document
8. MCP Server 收到 `connection-request`，注册用户为通信对等方
9. MCP Server 返回 `connection-response`，包含自身 DID Document
10. App 收到响应，注册 MCP Server 为通信对等方
11. 配对完成，MCP Server 出现在连接管理列表 (ConnectionScreen)

**预期结果**：
- 用户 App 与 MCP Server 建立了 DIDComm P2P 加密连接
- 双方互相持有对方的公钥，可进行端到端加密通信
- MCP Server 出现在 App 的连接管理列表中

**异常处理**：
- QR 码格式无效 → 提示 "Invalid QR code"
- Mediator 连接失败 → 提示网络错误
- MCP Server 无响应 → 超时后提示重试

---

### 用例 2.2: 商户 App 扫码配对商户 MCP

**前置条件**：
- 商户已生成双 DID 身份（用例 1.2）
- 商户 MCP Server 已启动并连接到商户侧 Mediator (`:4000`)

**参与角色**：商户、Merchant App、Merchant MCP、Mediator

**详细步骤**：

1. Merchant MCP Server 生成 OOB 邀请二维码
2. 商户在 Merchant App 中扫描二维码
3. App 调用 `parse_oob_invitation()` 解析邀请，提取 MCP Server 的 DID
4. App 连接商户侧 Mediator WebSocket
5. **关键区别**：商户 App 使用 **DIDComm 通信 DID**（`did:ignite:z...`，存储在 `didcomm_identity` tree）进行以下操作：
   - JWE 加密：使用通信 DID 的 X25519 密钥（`#key-agreement-1`）加密消息
   - DIDComm 签名：使用通信 DID 的 Ed25519 密钥（`#key-signing-1`）签名
   - 而**不使用**状态通道 DID（`did:ignite:<raw_base58>`，存储在 `keypairs` tree）
6. App 发送 `connection-request`（JWE 加密），包含通信 DID 的 DID Document
7. Merchant MCP 注册商户 App 的通信 DID 为对等方
8. Merchant MCP 返回 `connection-response`
9. 配对完成，Merchant MCP 出现在商户 App 的连接管理列表

**预期结果**：
- 商户 App 与商户 MCP 建立了 DIDComm 连接
- 双方使用各自的 DIDComm 通信 DID 进行加密通信
- 状态通道 DID 保持独立，仅用于 QR 码生成和链上签名

**异常处理**：
- QR 码格式无效 → 提示 "Invalid QR code"
- Mediator 连接失败 → 提示网络错误
- MCP Server 无响应 → 超时后提示重试

---

### 用例 2.3: MCP Server 连接 DIDComm Mediator (WebSocket 握手 + 认证)

**前置条件**：
- MCP Server 已生成 DID 身份
- Mediator 服务已运行在 `:8080`

**参与角色**：MCP Server、Mediator

**详细步骤**：

1. MCP Server 通过 WebSocket 连接到 `ws://127.0.0.1:8080/ws`
2. 执行三步明文握手：

| 步骤 | 方向 | 消息类型 | 说明 |
|:-----|:-----|:---------|:-----|
| 1 | Client → Mediator | `coordinate-mediation/2.0/mediate-request` | 注册为 Mediator 客户端 |
| 2 | Mediator → Client | `coordinate-mediation/2.0/mediate-grant` | Mediator 确认 |
| 3 | Client → Mediator | `coordinate-mediation/2.0/keylist-update` (add) | 注册接收密钥 `{did}#key-1` |
| 4 | Mediator → Client | `coordinate-mediation/2.0/keylist-update-response` | 确认密钥注册 |
| 5 | Client → Mediator | `peer-did-discovery/1.0/discover` | 发送完整 DID Document |

3. 握手完成后进入加密消息接收循环
4. 后续所有消息通过 JWE authcrypt 加密传输

**预期结果**：
- MCP Server 已注册到 Mediator
- Mediator 可根据 DID 路由消息到 MCP Server
- MCP Server 可接收加密 DIDComm 消息

**异常处理**：
- WebSocket 连接失败 → 每 3 秒自动重连
- 握手超时 → 关闭连接后重连

---

### 用例 2.4: MCP Server 断线重连 Mediator

**前置条件**：
- MCP Server 已完成初始握手（用例 2.3）
- WebSocket 连接意外断开

**参与角色**：MCP Server、Mediator

**详细步骤**：

1. 检测到 WebSocket 连接断开
2. 等待 3 秒
3. 重新建立 WebSocket 连接
4. 重新执行完整握手（mediate-request → mediate-grant → keylist-update → peer-did-discovery）
5. 握手成功后，通过 Message Pickup 3.0 协议拉取离线期间暂存的消息：
   - 发送 `messagepickup/3.0/status-request`
   - 收到 `status`（返回暂存消息计数）
   - 发送 `messagepickup/3.0/batch-pickup` 批量拉取
   - 收到 `batch` 返回批量消息
6. 恢复正常的加密消息接收循环

**预期结果**：
- MCP Server 重新连接到 Mediator
- 离线期间的消息已全部拉取
- 消息处理恢复正常

**异常处理**：
- 重连失败 → 继续每 3 秒重试
- 超过 7 天的离线消息通过 `GET /v1/sync/list` 补全

---

## 业务事件 3: DIDComm Mediator 认证

### 用例 3.1: 用户 App 通过 Challenge-Response 认证 Mediator

**前置条件**：
- 用户 App 已生成 DID 身份
- Mediator 服务运行在 `:8080`

**参与角色**：Sentinel App、Mediator

**详细步骤**：

1. App 调用 `GET /v1/auth/challenge` 获取认证 nonce
2. App 使用 DID Ed25519 私钥对 nonce 进行签名
3. App 调用 `POST /v1/auth/token` 发送签名，换取 JWT：
   - Request Body: `{ "did": "did:ignite:z...", "signature": "<base64>" }`
4. Mediator 验证签名有效性
5. Mediator 签发 JWT（包含 `user_did` 字段）
6. App 获得 Bearer Token，后续 API 调用携带此 Token

**预期结果**：
- App 获得 JWT Bearer Token
- 后续所有 Mediator API 调用携带 `Authorization: Bearer <token>` 头

**异常处理**：
- 签名验证失败 → 返回 401，App 重新发起认证
- Token 过期 → App 自动重新执行 Challenge-Response

---

### 用例 3.2: 商户 App 通过 Challenge-Response 认证 Mediator

**前置条件**：同用例 3.1

**参与角色**：Merchant App、Mediator

**详细步骤**：
- 同用例 3.1，但使用商户的 **DIDComm 通信 DID**（非状态通道 DID）进行签名

**预期结果**：同用例 3.1

**异常处理**：同用例 3.1

---

### 用例 3.3: MCP Server 通过 DID 签名认证 Mediator

**前置条件**：
- MCP Server 已生成 DID 身份

**参与角色**：MCP Server、Mediator

**详细步骤**：

1. MCP Server 在 WebSocket 握手过程中自动完成认证
2. 使用 DIDComm Agent 的 Ed25519 密钥对 Mediator challenge 进行签名
3. Mediator 验证签名后，将 WebSocket 连接与 DID 绑定

**预期结果**：
- WebSocket 连接已与 MCP Server DID 绑定
- Mediator 可根据 DID 路由消息到此连接

**异常处理**：
- 签名验证失败 → Mediator 关闭 WebSocket 连接

---

## 业务事件 4: X402 支付授权

### 用例 4.1: AI Agent 发起支付 → MCP 自动批准 (白名单/低额)

**前置条件**：
- AI Agent 已通过 MCP 协议连接到 User MCP Server
- MCP Server 已配置 `[policy] auto_approve_max` 或用户已将商家加入白名单
- 商家已在链上注册并持有有效 VC

**参与角色**：AI Agent、外部服务商、MCP Server、Solana 区块链

**详细步骤**：

1. AI Agent 向外部服务商发起 HTTP 请求
2. 服务商返回 `402 Payment Required`（X402 协议扩展）
3. Agent 调用 MCP Tool `process_x402_challenge`，传入 `challenge_body`
4. MCP Server 解析 402 响应：
   - 从 `accepts[]` 提取：paymentType, network, token, amount, recipient
   - 从 X402 扩展头提取：`x402-merchant-did`, `x402-payment-address`, `x402-merkle-context`
5. **商家验证**：
   - VC 签名验证：使用内置平台公钥验证 Ed25519Signature2020 proof
   - 链上 Merkle Proof 验证：通过 Helius DAS API 获取 proof，本地验证
   - 一致性校验：VC 中 DID 公钥哈希 == 链上 merchant_did_hash
6. **决策判定**（优先级从高到低）：
   - 商家验证通过
   - 查询 sled 名单缓存：`merchant_did` 在白名单中 && `amount <= list_max_amount`
   - 或 `amount <= auto_approve_max && auto_approve_max > 0`
7. **自动批准**：使用已有 Session Key 执行链上支付（→ 用例 5.2 或 5.3）
8. 返回支付结果和链上交易签名给 Agent

**预期结果**：
- 支付自动执行完成，无需用户手机交互
- Agent 获得支付证明（交易签名）
- Agent 使用支付证明重新请求资源

**异常处理**：
- VC 验证失败 → 拒绝支付，返回验证失败原因
- 链上 Merkle Proof 验证失败 → 拒绝支付
- Session Key 已过期或额度不足 → 降级到交互式授权（→ 用例 4.2）

---

### 用例 4.2: AI Agent 发起支付 → MCP 推送到用户 App → 用户批准

**前置条件**：
- 同用例 4.1
- 商家不在白名单或金额超过 `auto_approve_max`
- 用户 App 与 MCP Server 已配对（→ 用例 2.1）

**参与角色**：AI Agent、MCP Server、Mediator、用户、Sentinel App、Solana 区块链

**详细步骤**：

1. Agent 遇到 402 → 调用 MCP `process_x402_challenge`
2. MCP 解析 402，执行商家验证（通过）
3. **决策判定**：不在白名单且超过阈值 → 需要交互式授权
4. MCP 创建 PaymentRequest（status: PendingAuth），保存到 sled
5. MCP 构建 `payment-auth-request` DIDComm 消息（JWE authcrypt 加密）：
   ```json
   { "payment_id": "uuid-v4", "merchant_did": "did:ignite:z...", "amount": 50000000, "description": "API Service Call" }
   ```
6. MCP 通过 Mediator 发送到用户 App：
   - 海外用户：FCM 信号 → App HTTPS 拉取 → 解密
   - 国内用户：WS 直推 → App 直接解密
7. 用户 App 收到消息，解密后弹出全屏 ChallengeScreen：
   - 展示：商户 DID、金额 (SOL)、支付原因
   - 策略配置：每日限额、单笔限额、有效期
   - 名单操作选择：仅本次 / 加入白名单 / 加入黑名单
   - 签名方式选择：内置密钥 / Phantom 深链接 / Solflare / MWA (Android)
8. 用户审核后点击 Approve
9. App 创建 Session Key：
   - 调用 `create_session_key_for_payment()` 生成 Ed25519 临时密钥对
   - 构建链上注册交易（SessionToken PDA）
   - 用户签名 → 提交到 Solana
   - 链上确认
10. App 构建 `payment-auth-response` JWE 消息：
    ```json
    {
      "payment_id": "uuid-v4",
      "authorized": true,
      "session_key_pubkey": "Base58Pubkey",
      "session_key_tx_signature": "Base58TxSig",
      "session_expires_at": 1713703600,
      "spending_limit": 100000000,
      "scopes": ["sol:transfer"],
      "list_action": "none"
    }
    ```
11. App 通过 Mediator 发送回 MCP Server
12. MCP 收到授权响应：
    - 验证 Session Key 链上状态
    - 使用 Session Key 构建 ExecutePayment 交易
    - 提交到 Solana
    - PaymentRequest 状态 → Executed
13. MCP 返回支付结果（含交易签名）给 Agent

**预期结果**：
- 用户完成授权审批
- Session Key 已在链上注册
- 链上支付执行成功
- Agent 获得支付证明

**异常处理**：
- Mediator 推送失败 → MCP 重试或等待 App 主动拉取
- 用户拒绝 → → 用例 4.3
- 授权超时 (300秒) → → 用例 4.4
- 链上交易失败 → MCP 返回错误给 Agent

---

### 用例 4.3: AI Agent 发起支付 → 用户拒绝 → 名单操作

**前置条件**：同用例 4.2

**参与角色**：AI Agent、MCP Server、用户、Sentinel App

**详细步骤**：

1. 用户在 ChallengeScreen 审核支付请求
2. 用户选择操作：
   - **Decline & Block**：拒绝 + 加入黑名单
   - **Decline**：仅拒绝本次
3. App 发送 `payment-auth-response`：
   ```json
   {
     "payment_id": "uuid-v4",
     "authorized": false,
     "list_action": "add_blacklist",
     "list_label": "可疑商家"
   }
   ```
4. MCP Server 收到拒绝响应：
   - PaymentRequest 状态 → Rejected
   - 解析 `list_action`
5. 若 `list_action = "add_blacklist"`：
   - 构建黑名单条目：`{ did: "merchant_did", label: "可疑商家", expires: null }`
   - 追加到 sled 本地黑名单缓存
   - 异步上传合并名单到 IPFS（获取新 CID）
   - 发送 `list-sync-notification` 给手机端（→ 用例 13.1 或 13.2）
6. MCP 返回拒绝信息给 Agent

**预期结果**：
- 支付请求被拒绝
- 若选 Block，商家被加入黑名单
- 后续该商家的支付请求将被自动阻断

**异常处理**：
- IPFS 上传失败 → sled 本地缓存仍有效，下次启动时重新同步
- 名单同步通知发送失败 → App 下次启动时从 DID Document 获取最新 CID

---

### 用例 4.4: 支付超时 → MCP 返回超时错误给 Agent

**前置条件**：
- MCP 已推送授权请求到用户 App
- 用户未在规定时间内响应

**参与角色**：AI Agent、MCP Server

**详细步骤**：

1. MCP 创建 PaymentRequest 后启动 oneshot channel 等待
2. 等待时间超过 `auth_timeout`（默认 300 秒）
3. MCP 将 PaymentRequest 状态更新为 Expired
4. MCP 返回超时错误给 Agent：
   ```json
   { "status": "expired", "payment_id": "uuid-v4", "error": "Authorization timeout after 300s" }
   ```

**预期结果**：
- Agent 收到超时错误
- PaymentRequest 状态为 Expired

**异常处理**：
- Agent 可选择重试（重新触发 402 流程）

---

## 业务事件 5: Session Key 链上支付

### 用例 5.1: 创建自付模式 Session Key (self_funded)

**前置条件**：
- 用户已在手机端授权支付（用例 4.2 步骤 7-8）
- Solana 网络可达
- 用户拥有足够 SOL 余额

**参与角色**：Sentinel App、Solana 区块链

**详细步骤**：

1. 用户在 ChallengeScreen 点击 Approve（用例 4.2 步骤 8）
2. App 调用 Rust bridge `create_session_key_for_payment()`：
   - 生成 Ed25519 临时密钥对
3. 构建链上 SessionToken PDA 注册交易：
   - `owner`: 用户主钱包公钥
   - `ephemeral_pubkey`: 临时密钥公钥
   - `expiry`: 当前时间 + 有效期
   - `scope`: `["sol:transfer"]` 或 `["spl:transfer"]`
   - `spending_limit`: 授权的花费上限
4. 交易中包含 `system_program::transfer`：从主钱包转入少量 SOL 到临时密钥地址（Gas 费）
5. 用户选择签名方式并签名
6. 提交交易到 Solana RPC
7. 链上确认
8. 返回 `session_key_pubkey` 和 `chain_tx_signature`

**预期结果**：
- Session Key 已在链上注册
- 临时密钥地址有足够 Gas 费
- Session Key 可用于后续链上支付

**异常处理**：
- 链上交易失败 → 提示用户重试
- SOL 余额不足 → 提示充值
- 外接钱包签名失败 → 提示更换签名方式

---

### 用例 5.2: SOL 转账支付

**前置条件**：
- Session Key 已创建且未过期（用例 5.1）
- spending_limit 中有足够余额

**参与角色**：MCP Server、Solana 区块链

**详细步骤**：

1. MCP Server 收到授权响应，提取 session_key_pubkey
2. 验证 Session Key 链上状态：
   - 查询链上 SessionToken PDA
   - 验证未过期：`current_slot < session_expires_at`
   - 验证 `spending_limit >= 本次支付金额`
3. 构建 SOL Transfer 交易：
   - `from`: 临时密钥地址
   - `to`: 商家收款地址
   - `amount`: 支付金额 (lamports)
   - `feePayer`: 临时密钥公钥（自付模式）
4. 使用 Session Key 签名交易
5. 广播到 Solana RPC
6. 链上验证：
   - Session Key 签名有效性
   - 未过期
   - spending_limit 未超限
   - 执行 SOL 转账
   - 更新 Session Key 已花费金额
7. 返回交易签名

**预期结果**：
- SOL 转账成功
- Session Key spending_limit 被扣减
- MCP 获得交易签名

**异常处理**：
- Session Key 已过期 → 返回错误，需要重新授权
- spending_limit 不足 → 返回错误
- 链上确认失败 → 重试或返回错误

---

### 用例 5.3: SPL Token 转账支付

**前置条件**：
- Session Key 已创建，scope 包含 `"spl:transfer"`
- 用户持有对应 SPL Token

**参与角色**：MCP Server、Solana 区块链

**详细步骤**：

1. 同用例 5.2 步骤 1-2
2. 构建 SPL Token Transfer 交易：
   - 使用 `spl_token::instruction::transfer`
   - `source`: 用户 Token Account
   - `destination`: 商家 Token Account
   - `amount`: Token 金额
3. 后续步骤同用例 5.2 步骤 4-7

**预期结果**：SPL Token 转账成功

**异常处理**：同用例 5.2

---

### 用例 5.4: Session Key 过期/额度耗尽处理

**前置条件**：
- Session Key 已创建
- 到达过期时间或 spending_limit 已耗尽

**参与角色**：MCP Server、Solana 区块链

**详细步骤**：

1. MCP Server 尝试使用 Session Key 执行支付
2. 链上验证失败：
   - `current_slot >= session_expires_at`（已过期）
   - 或 `current_usage + payment_amount > spending_limit`（额度耗尽）
3. 链上交易被拒绝
4. MCP 返回错误给 Agent
5. **自付模式回收**（可选）：
   - 用户可执行 `CloseSession` 指令
   - 将临时密钥中剩余 SOL 退还给主钱包

**预期结果**：
- 支付被拒绝
- Session Key 不再可用
- 剩余 Gas 费可通过 CloseSession 退还

**异常处理**：
- 需要重新触发授权流程创建新 Session Key

---

### 用例 5.5: 创建代付模式 Session Key (sponsored)

**前置条件**：
- MCP Server 配置了 `pay_mode = "sponsored"`
- 项目方 Relayer 钱包有足够 SOL 余额
- 用户已在手机端授权支付（用例 4.2 步骤 7-8）

**参与角色**：Sentinel App、MCP Server、Relayer 钱包、Solana 区块链

**详细步骤**：

1. 用户在 ChallengeScreen 点击 Approve（用例 4.2 步骤 8）
2. App 调用 Rust bridge `create_session_key_for_payment()`：
   - 生成 Ed25519 临时密钥对
3. 构建链上 SessionToken PDA 注册交易：
   - `owner`: 用户主钱包公钥
   - `ephemeral_pubkey`: 临时密钥公钥
   - `expiry` / `scope` / `spending_limit`: 同用例 5.1
4. **区别于自付模式**：交易 `feePayer` 为 Relayer 钱包（非临时密钥）
5. Relayer 钱包签名并提交交易到 Solana RPC
6. 链上确认
7. App 返回 `session_key_pubkey` 和 `chain_tx_signature` 给 MCP Server
8. 后续支付由 Session Key 签名，Gas 由 Relayer 承担

**预期结果**：
- Session Key 已在链上注册
- 用户无需预充值 Gas
- 临时密钥地址不需要持有 SOL

**异常处理**：
- Relayer 钱包余额不足 → 链上交易失败，MCP 返回错误
- Relayer 服务不可达 → 降级到自付模式或返回错误

---

<!-- State Channel: 探索阶段，暂不启用
## 业务事件 6: 状态通道开通

### 用例 6.1: 用户 App 选择 Hub → 通过 DIDComm 请求 MCP 创建通道

**前置条件**：
- 用户 App 与 MCP Server 已配对（用例 2.1）
- Hub Registry 服务可用
- 至少一个活跃 Hub 已注册

**参与角色**：用户、Sentinel App、MCP Server、Hub Registry、Channel Hub

**详细步骤**：

1. 用户在 App 中选择 "Open Channel" 操作
2. App 调用 Hub Registry API `GET /v1/hubs?status=active` 获取可用 Hub 列表
3. App 展示 Hub 列表供用户选择（显示名称、延迟、费率、流动性）
4. 用户选择 Hub，配置参数：
   - `deposit`: 存入金额 (lamports)
   - `token_mint`: 代币地址（默认 SOL）
   - `tree_depth`: Merkle 树深度（默认 4）
5. App 构建 `create-channel-request` DIDComm 消息：
   ```json
   {
     "hub_endpoint": "http://hub:3003",
     "provider_pubkey": "Base58SolanaPubkey",
     "token_mint": "So11111111111111111111111111111111",
     "deposit": 1000000000,
     "tree_depth": 8
   }
   ```
6. JWE 加密后通过 Mediator 发送到 MCP Server

**预期结果**：
- App 已发送通道创建请求到 MCP Server

**异常处理**：
- Hub Registry 不可达 → 显示错误，无法获取 Hub 列表
- 无活跃 Hub → 提示暂无可用 Hub

---

### 用例 6.2: 商户 App 选择 Hub → 通过 DIDComm 请求 MCP 创建通道

**前置条件**：
- 商户 App 与商户 MCP Server 已配对
- Hub 可用

**参与角色**：商户、Merchant App、Merchant MCP、Channel Hub

**详细步骤**：
- 同用例 6.1，商户端通过商户 MCP Server 创建通道

**预期结果**：同用例 6.1

**异常处理**：同用例 6.1

---

### 用例 6.3: 通道创建成功

**前置条件**：
- MCP Server 已收到 `create-channel-request`

**参与角色**：MCP Server、Channel Hub

**详细步骤**：

1. MCP Server 解密 `create-channel-request`
2. MCP 调用 Channel Hub HTTP API `POST /v1/channels/open`：
   ```json
   { "provider_pubkey": "...", "token_mint": "...", "deposit": 1000000000, "tree_depth": 8 }
   ```
3. Channel Hub 处理：
   - 生成 channel_id (32 字节随机)
   - 创建 ChannelManager 实例
   - 初始化 Merkle Tree
   - 创建初始 UTXO 叶子（存入金额）
   - 持久化到 sled
4. Hub 返回：
   ```json
   { "channel_id": "hex_encoded_32_bytes", "sequence": 0, "current_root": "hex_encoded_root" }
   ```
5. MCP 构建 `create-channel-response` DIDComm 消息（JWE 加密）：
   ```json
   { "channel_id": "hex_encoded_32_bytes", "sequence": 0, "current_root": "hex_encoded_root", "success": true }
   ```
6. 通过 Mediator 发送回 App
7. App 收到响应，更新 UI 显示新通道

**预期结果**：
- 状态通道创建成功
- App 获得 channel_id 和初始 root
- 通道状态为 Open

**异常处理**：
- Hub 返回错误 → MCP 转发错误到 App

---

### 用例 6.4: 通道创建失败

**前置条件**：同用例 6.3

**参与角色**：MCP Server、Channel Hub、App

**详细步骤**：

1. MCP 调用 Hub API 创建通道
2. Hub 返回错误（如存款不足、参数无效）
3. MCP 构建 `create-channel-response`：
   ```json
   { "channel_id": "", "sequence": 0, "current_root": "", "success": false, "error_message": "Failed to open channel" }
   ```
4. 发送回 App
5. App 显示错误信息

**预期结果**：App 显示通道创建失败原因

**异常处理**：
- Hub 不可达 → MCP 返回网络错误
- 用户可修改参数后重试

---
-->

<!-- State Channel: 探索阶段，暂不启用
## 业务事件 7: 状态通道离链支付

### 用例 7.1: 用户 App 发起支付 → MCP 通过 Hub 执行 LeafUpdate + CoSign

**前置条件**：
- 用户与 Hub 之间已有 Open 状态通道
- 通道余额充足

**参与角色**：Sentinel App、MCP Server、Channel Hub

**详细步骤**：

1. 用户在 App 中发起支付（输入金额和目标）
2. App 构建 `channel-payment-request` DIDComm 消息
3. 通过 Mediator 发送到 MCP Server
4. MCP 调用 Hub API `POST /v1/channels/{id}/pay`：
   ```json
   { "amount": 100000000, "recipient_pubkey": "..." }
   ```
5. Channel Hub 执行支付：
   - 创建 LeafUpdate（类型：Transfer）
   - 从付款方 UTXO 扣除金额
   - 向收款方 UTXO 增加金额
   - 更新 Merkle Tree
   - 生成 SignedState
   - 请求双方 CoSign
6. Hub 返回：
   ```json
   { "sequence": 1, "leaf_index": 2, "new_root": "hex_encoded_root" }
   ```
7. MCP 发送 `channel-payment-confirm` DIDComm 消息到商户 App
8. MCP 将支付结果返回给用户 App

**预期结果**：
- 离链支付执行成功
- Merkle Tree 更新
- 双方签署新状态

**异常处理**：
- 通道余额不足 → 返回 InsufficientBalance 错误
- 通道已关闭 → 返回 ChannelClosed 错误
- CoSign 失败 → 回滚本次更新

---

### 用例 7.2: 批量支付流水线 (Pipeline)

**前置条件**：
- 通道状态为 Open
- 需要执行多个连续操作

**参与角色**：MCP Server、Channel Hub

**详细步骤**：

1. 使用 Pipeline 构建器批量创建多个 LeafUpdate：
   ```rust
   let mut pipeline = Pipeline::new(channel_id);
   pipeline.add_transfer(payer_a, payer_b, 1000)?;
   pipeline.add_transfer(payer_a, payer_b, 2000)?;
   pipeline.add_htlc_create(payer_a, payer_b, 500, hash_lock, timelock)?;
   ```
2. 执行 Pipeline：
   - 按顺序应用所有 LeafUpdate
   - 全部成功 → 更新 Merkle Tree，生成新状态
   - 任一失败 → 自动回滚所有更新
3. 请求双方 CoSign
4. 返回最终的 sequence 和 root

**预期结果**：
- 批量操作原子执行（全部成功或全部回滚）
- Merkle Tree 仅更新一次

**异常处理**：
- Pipeline 中任一操作失败 → 全部回滚

---

### 用例 7.3: HTLC 支付 (条件支付)

**前置条件**：
- 通道状态为 Open（→ 用例 6.3）
- 发起方和接收方在同一通道或有路由路径（→ 用例 11.1）

**参与角色**：MCP Server、Channel Hub

**详细步骤**：

1. **创建 HTLC**：
   - 发起方创建 HTLC Leaf：
     - `hash_lock`: SHA-256(preimage) 的哈希
     - `timelock`: 到期 slot
     - `amount`: 锁定金额
   - 更新 Merkle Tree
   - 双方 CoSign
2. **揭示原像 (Unlock)**：
   - 接收方提供 preimage
   - 验证 `SHA-256(preimage) == hash_lock`
   - 锁定金额转入接收方
3. **超时退款**：
   - 若 timelock 到达但 preimage 未揭示
   - 锁定金额退回给发起方

**预期结果**：
- HTLC 条件支付执行成功
- 或超时后自动退款

**异常处理**：
- preimage 不匹配 → HTLC 无法解锁
- timelock 到期 → 自动退款

---
-->

## 业务事件 8: QR 码收款

### 用例 8.1: 商户 App 生成收款二维码

**前置条件**：
- 商户已生成身份（用例 1.2）
- Hub Endpoint 已配置

**参与角色**：商户、Merchant App

**详细步骤**：

1. 商户打开 Merchant App，进入 QR Generate 页面
2. 输入金额 (USDC) + 可选描述（如 "咖啡"）
3. App 调用 Rust bridge `generate_payment_qr()`：
   - 生成 UUID v4 作为 order_id
   - 创建订单（status: pending）
   - 持久化到 sled
4. 构建 PaymentQrData：
   ```json
   {
     "type": "ignite-pay-request",
     "version": 1,
     "merchant_did": "did:ignite:...",
     "amount": 1000000000,
     "description": "咖啡",
     "order_id": "uuid-v4",
     "hub_endpoint": "http://hub:3003",
     "timestamp": 1713700000
   }
   ```
5. Base64URL 编码后生成 QR 码字符串：`ignite://pay?d=<base64url(JSON)>`
6. 展示 QR 码，进入等待确认状态
7. 启动双通道等待：
   - 主通道：监听 `MerchantPushService.confirmations` 流
   - 兜底轮询：每 5 秒调用 `refreshOrders()` 检查订单状态

**预期结果**：
- QR 码展示在屏幕上
- 订单已创建（pending 状态）
- 等待用户扫描支付

**异常处理**：
- QR 码生成失败 → 提示错误
- 订单创建失败 → 提示重试

---

<!-- State Channel: 探索阶段，暂不启用
### 用例 8.2: 用户 App 扫描商户 QR 码 → 发起状态通道支付

**前置条件**：
- 用户已有 Open 状态通道（通过 Hub）
- 用户 App 的 Sentinel 已打开

**参与角色**：用户、Sentinel App、Channel Hub

**详细步骤**：

1. 用户打开 Sentinel App
2. 扫描商户 QR 码
3. App 调用 Rust bridge `parse_payment_qr()` 解析 PaymentQrData
4. 显示 QrPaymentScreen 确认页：
   - 商户 DID / 名称
   - 金额（USDC 显示）
   - 描述
5. 用户确认支付
6. App 调用 Rust bridge `channel_pay()`：
   - 内部调用 Hub API `POST /v1/channels/{id}/pay`
7. Hub 处理支付（→ 用例 7.1）
8. 返回支付结果：sequence, leaf_index
9. App 显示支付成功结果

**预期结果**：
- 支付执行成功
- 用户看到支付确认

**异常处理**：
- 无可用通道 → 提示先开通通道
- 通道余额不足 → 提示余额不足
- Hub 不可达 → 提示网络错误

---

### 用例 8.3: 商户 App 接收支付确认 → 语音播报

**前置条件**：
- 用户已完成支付（用例 8.2）
- 商户 App 在线（WS 或 FCM）

**参与角色**：Channel Hub、Mediator、Merchant App

**详细步骤**：

1. Hub 处理支付后，构建 `channel-payment-confirm` DIDComm 消息（JWE 加密）：
   ```json
   { "order_id": "uuid-v4", "channel_id": "hex...", "leaf_index": 2, "sequence": 1, "amount": 1000000000 }
   ```
2. Hub 通过 Mediator 发送到商户 App：
   - 国内：WS 直推
   - 海外：FCM 信号 → HTTPS 拉取
3. 商户 App 收到消息：
   - 调用 Rust `decrypt_message()` 解密 JWE
   - 提取 order_id, channel_id, leaf_index, sequence
4. 调用 Rust `confirm_order()` 更新订单状态：pending → confirmed
5. 触发以下操作：
   - QR 页面显示绿色对勾 ✓
   - 触觉反馈（Haptic Feedback）
   - 语音播报："收到收款 1.00 USDC"（中英双语，取决于设置）
   - Dashboard 今日汇总刷新
6. 若主通道未收到确认：
   - 轮询兜底在 5 秒后通过 `refreshOrders()` 检测到状态变更
   - 执行相同的确认流程

**预期结果**：
- 订单状态更新为 confirmed
- 商户收到语音播报
- QR 页面显示成功标识

**异常处理**：
- 消息解密失败 → 忽略消息
- 订单确认失败 → 日志记录，等待下次轮询
- 推送延迟 → 轮询兜底确保不遗漏

---
-->

### 用例 8.4: AI Agent 通过 Merchant MCP 生成收款 QR 码

**前置条件**：
- Merchant MCP Server 已启动
- 商户 Hub Endpoint 已配置

**参与角色**：AI Agent、Merchant MCP、Channel Hub、用户

**详细步骤**：

1. AI Agent（商户侧）调用 Merchant MCP Tool `generate_payment_qr`：
   - 输入：`amount = 1000000000` (1 USDC), `description = "咖啡"`
   - 可选输入：`order_id`（不传则自动生成 UUID）
2. Merchant MCP 生成 PaymentOrder（status: pending），持久化到 sled
3. 构建 PaymentQrData 并编码为 QR 字符串：`ignite://pay?d=<base64url(JSON)>`
4. 返回 QR 文本和 ASCII 二维码给 Agent
5. Agent 将 QR 码展示给用户（终端/网页/打印）
6. 用户使用 Sentinel App 扫描（触发用例 8.2）
7. 商户 App 或 Agent 可调用 `check_payment(order_id)` 轮询订单状态
8. 支付完成后（用例 8.3），Merchant MCP 审计日志记录该笔交易

**预期结果**：
- AI Agent 获得 QR 码，可供用户扫描支付
- 订单已在 Merchant MCP 中创建
- 支付完成后订单状态自动更新

**异常处理**：
- Hub Endpoint 不可达 → QR 仍可生成，但用户扫码时支付会失败
- 订单已存在（重复 order_id）→ 覆盖更新原订单

---

<!-- State Channel: 探索阶段，暂不启用
## 业务事件 9: 状态通道关闭与结算

### 用例 9.1: 协作关闭通道 (Cooperative Close)

**前置条件**：
- 通道状态为 Open
- 双方同意关闭

**参与角色**：用户/商户、Channel Hub、Solana 区块链

**详细步骤**：

1. 任一方发起关闭请求（App 或 MCP）
2. 调用 Hub API `POST /v1/channels/{id}/close`
3. Hub 执行协作关闭：
   - 双方签署最终状态（Latest SignedState）
   - 构建链上结算交易
   - 提交到 Solana
4. 链上程序验证双签
5. 执行资金分配
6. 通道状态变为 Closed

**预期结果**：
- 通道已关闭
- 资金按最终状态分配
- 链上结算确认

**异常处理**：
- 一方拒绝签署 → 可转单方关闭（→ 用例 9.2）
- 链上提交失败 → 重试

---

### 用例 9.2: 单方关闭通道 (Unilateral Close)

**前置条件**：
- 通道状态为 Open
- 一方希望关闭但无法联系对方

**参与角色**：发起方、Channel Hub、Solana 区块链

**详细步骤**：

1. 发起方直接提交链上关闭交易
2. 链上程序记录关闭请求
3. 进入挑战期（`challenge_duration` slots，默认 5000 slots）
4. 挑战期内，另一方可以：
   - 提交争议（用例 9.3）
   - 或不响应
5. 挑战期结束后进入结算

**预期结果**：
- 通道进入关闭流程
- 开始挑战期倒计时

**异常处理**：
- 挑战期内对方提交争议 → 进入争议解决（→ 用例 9.3）

---

### 用例 9.3: 争议解决 (Dispute Resolution)

**前置条件**：
- 通道处于挑战期（用例 9.2）
- 另一方有更新的状态

**参与角色**：争议方、Solana 区块链

**详细步骤**：

1. 争议方在挑战期内提交争议
2. 提交最新的 SignedState 作为证据
3. 链上程序验证：
   - SignedState 包含双方有效签名
   - SignedState 的 sequence 比当前链上状态更新
4. 链上程序选择更优（更高 sequence）的状态
5. 更新链上状态
6. 进入结算

**预期结果**：
- 链上采用最新的双方签署状态
- 公平的资金分配

**异常处理**：
- 证据无效（签名不匹配）→ 争议被拒绝
- sequence 不更高 → 争议被拒绝

---

### 用例 9.4: 链上结算与领取 (Settle + Claim + Finalize)

**前置条件**：
- 挑战期已结束（协作关闭或争议解决后）

**参与角色**：各方、Channel Hub、Solana 区块链

**详细步骤**：

1. **Settle**：调用 Hub API `POST /v1/channels/{id}/settle`
   - 提交链上结算交易
   - 链上程序验证挑战期已过
   - 锁定通道最终状态
2. **Claim**：各方调用 `POST /v1/channels/{id}/claim`
   - 提交各自的 Merkle Proof（证明 UTXO 叶子归属）
   - 链上验证 Proof
   - 将对应资金从 Escrow 转入各方地址
3. **Finalize**：调用 `POST /v1/channels/{id}/finalize`
   - 清理链上 PDA 账户
   - 释放存储空间

**预期结果**：
- 资金已按最终状态分配到各方地址
- 通道 PDA 已清理

**异常处理**：
- Claim 时 Proof 无效 → 领取失败
- 部分叶未被领取 → 可稍后重试

---
-->

<!-- State Channel: 探索阶段，暂不启用
## 业务事件 10: Hub 注册与发现

### 用例 10.1: Channel Hub 启动时自动注册到 Hub Registry

**前置条件**：
- Hub Registry 服务运行在 `:3004`（PostgreSQL 可用）
- Channel Hub 配置了 `[hub_registry]` 节

**参与角色**：Channel Hub、Hub Registry

**详细步骤**：

1. Channel Hub 启动
2. 读取配置中的 `[hub_registry]` 节：
   ```toml
   [hub_registry]
   url = "http://localhost:3004"
   publish_interval_secs = 60
   ```
3. Hub 调用 Hub Registry API `POST /v1/hubs` 注册自身：
   ```json
   {
     "hub_did": "did:ignite:z...",
     "endpoint_url": "http://hub:3003",
     "name": "Hub-ABC12345",
     "description": "Main payment hub",
     "active_pubkey": "Base58SolanaPubkey",
     "collateral": 100000000000,
     "available_liquidity": 50000000000,
     "fee_rate_bps": 10,
     "supported_tokens": ["So11111111111111111111111111111111"]
   }
   ```
4. Registry 返回 `hub_id`（UUID）
5. Hub 保存 `hub_id` 用于后续指标更新

**预期结果**：
- Hub 已注册到 Registry
- Hub 获得 hub_id
- Hub 可被 App 查询发现

**异常处理**：
- Registry 不可达 → Hub 仍可运行，但无法被发现
- hub_did 已存在 → 更新已有记录

---

### 用例 10.2: Hub 定期更新性能指标到 Registry

**前置条件**：
- Hub 已注册（用例 10.1）

**参与角色**：Channel Hub、Hub Registry

**详细步骤**：

1. Hub 每隔 `publish_interval_secs`（默认 60 秒）触发指标更新
2. Hub 收集当前指标：
   - `online_rate`: 在线百分比 (0-100)
   - `success_rate`: 成功率 (0-100)
   - `avg_latency_ms`: 平均延迟
   - `active_channels`: 活跃通道数
   - `available_liquidity`: 可用流动性
   - `fee_rate_bps`: 费率 (基点)
3. 调用 Registry API `PUT /v1/hubs/{hub_id}/metrics`
4. Registry 更新数据库

**预期结果**：
- Hub 指标实时更新
- App 可查询到最新的 Hub 性能数据

**异常处理**：
- Registry 不可达 → 下次重试
- 指标收集失败 → 使用上次数据

---

### 用例 10.3: App 查询 Hub Registry 获取可用 Hub 列表

**前置条件**：
- Hub Registry 服务可用
- 至少一个 Hub 已注册

**参与角色**：App、Hub Registry

**详细步骤**：

1. App 调用 Registry API `GET /v1/hubs?status=active&limit=100&offset=0`
2. 可选过滤参数：
   - `status=active`: 仅返回活跃 Hub
   - `token_mint=So111111...`: 按支持的代币过滤
3. Registry 返回 Hub 列表：
   ```json
   {
     "hubs": [
       {
         "hub_id": "uuid",
         "name": "Hub-ABC12345",
         "endpoint_url": "http://hub:3003",
         "fee_rate_bps": 10,
         "available_liquidity": 50000000000,
         "online_rate": 100,
         "success_rate": 99,
         "avg_latency_ms": 50,
         "active_channels": 42,
         "supported_tokens": ["So111111..."]
       }
     ]
   }
   ```
4. App 展示 Hub 列表，用户可选择

**预期结果**：
- App 展示可用 Hub 列表
- 用户可基于延迟、费率、流动性等选择 Hub

**异常处理**：
- Registry 不可达 → 提示错误
- 无 Hub 返回 → 提示暂无可用 Hub

---

### 用例 10.4: Hub 注销 (下线)

**前置条件**：
- Hub 已注册

**参与角色**：Channel Hub、Hub Registry

**详细步骤**：

1. Hub 调用 Registry API `DELETE /v1/hubs/{hub_id}`
2. Registry 将 Hub 状态设为 `inactive`
3. 后续查询 `status=active` 不会返回此 Hub

**预期结果**：
- Hub 状态变为 inactive
- 不再被 App 发现

**异常处理**：
- Hub 意外下线未注销 → 管理员可通过 Registry API 手动注销

---
-->

<!-- State Channel: 探索阶段，暂不启用
## 业务事件 11: 多跳路由支付

### 用例 11.1: 用户通过 Hub A → Hub B → 商户 的多跳路径支付

**前置条件**：
- 用户与 Hub A 有通道
- Hub A 与 Hub B 有通道
- Hub B 与商户有通道
- 路由路径可用

**参与角色**：用户 App、Hub A、Hub B、商户

**详细步骤**：

1. 用户发起跨 Hub 支付请求
2. RouteService 执行 DFS 路由发现：
   - 从用户所在的 Hub 开始搜索
   - 查找到达商户所在 Hub 的路径
   - 对路径进行评分（基于流动性、费率、延迟）
3. 选择最优路径：User → Hub A → Hub B → Merchant
4. MultiHopManager 构建多跳支付：
   - 每跳递减 timelock（如 Hub B 的 timelock < Hub A 的 timelock）
   - 第一跳：User → Hub A：创建 HTLC（hash_lock, timelock_T1）
   - 第二跳：Hub A → Hub B：创建 HTLC（hash_lock, timelock_T2 < T1）
   - 第三跳：Hub B → Merchant：创建 HTLC（hash_lock, timelock_T3 < T2）
5. 商户揭示 preimage 解锁最后一跳
6. preimage 沿路径反向传播，逐跳解锁
7. 支付完成

**预期结果**：
- 多跳支付成功
- 每个中间 Hub 获得中继费
- 资金安全到达商户

**异常处理**：
- 某一跳 HTLC 创建失败 → 整体支付失败
- 超时未解锁 → → 用例 11.3

---

### 用例 11.2: 路由发现失败 → 降级到直接通道

**前置条件**：
- 无可用多跳路由路径
- 用户与目标有直接通道

**参与角色**：用户 App、Channel Hub

**详细步骤**：

1. RouteService 搜索路径失败（无可达路径）
2. 系统检查是否有直接通道到目标
3. 若有直接通道：
   - 通过直接通道执行标准支付（用例 7.1）
4. 若无直接通道：
   - 返回路由不可达错误

**预期结果**：
- 降级到直接通道支付
- 或返回无可用路径错误

**异常处理**：
- 直接通道余额不足 → 返回错误

---

### 用例 11.3: HTLC 超时 → 自动退款

**前置条件**：
- 多跳支付中某一跳的 HTLC 已创建
- timelock 到达但 preimage 未揭示

**参与角色**：Channel Hub、Solana 区块链

**详细步骤**：

1. timelock slot 到达
2. HTLC 状态变为 Expired
3. 锁定金额自动退回给发起方
4. 外层 HTLC 也因内层退款而超时，逐层退款
5. 最终所有锁定资金退回给原始发起方

**预期结果**：
- 所有 HTLC 锁定资金退回
- 不存在资金损失

**异常处理**：
- 退款过程中链上失败 → 重试

---
-->

## 业务事件 12: 商户 DID 入驻

### 用例 12.1: 平台为商户签发 Verifiable Credential

**前置条件**：
- 商户已生成 DID 身份
- 商户已提交身份证明资料

**参与角色**：商户、平台

**详细步骤**：

1. 商户向平台提交：
   - `did:ignite` 标识符
   - Solana 收款公钥
   - 身份证明资料
   - 服务元数据（名称、类型、描述）
2. 商户使用 DID 私钥对请求签名：`issue_vc:{did}:{merchant_name}:{nonce}`
3. 平台验证签名（确保持有 DID）
4. 平台审核商户资料
5. 审核通过后，平台签发 VC：
   ```json
   {
     "@context": ["https://www.w3.org/2018/credentials/v1"],
     "type": ["VerifiableCredential", "IgniteMerchantCredential"],
     "issuer": "did:ignite:z6Mk...<platform_did>",
     "issuanceDate": "2025-01-01T00:00:00Z",
     "credentialSubject": {
       "id": "did:ignite:z6Mk...<merchant_did>",
       "service_type": "api-service",
       "merchant_name": "Example API Service"
     },
     "expirationDate": "2026-01-01T00:00:00Z",
     "proof": {
       "type": "Ed25519Signature2020",
       "verificationMethod": "did:ignite:z6Mk...<platform_did>#key-signing-1",
       "proofPurpose": "assertionMethod",
       "proofValue": "<ed25519_signature_base58>"
     }
   }
   ```
6. 平台将 VC 返回给商户

**预期结果**：
- 商户获得平台签发的有效 VC
- VC 包含 Ed25519Signature2020 proof

**异常处理**：
- 审核拒绝 → 平台返回拒绝原因
- 签名验证失败 → 要求重新签名提交

---

### 用例 12.2: 商户 DID 注册到链上 Merkle Tree

**前置条件**：
- 商户已获得平台签发的 VC（用例 12.1）
- Concurrent Merkle Tree 已部署

**参与角色**：平台、Solana 区块链

**详细步骤**：

1. 平台计算 MerchantLeaf：
   - `merchant_did_hash = SHA-256(DID 公钥)`
   - `active_pubkey = Solana 收款地址`
   - `platform_vc_hash = SHA-256(canonical_json(VC))`
   - `status = 0` (active)
2. 平台计算 PDA 索引：`Index = Hash(Program_ID + Original_PK)`
3. 平台调用 `append` 指令将叶子插入 Concurrent Merkle Tree：
   - Tree 参数：maxDepth=14, maxBufferSize=64
   - 支持 ~16K 商家
4. 链上程序验证：
   - 平台签名有效性（PlatformConfig PDA，seeds: `[b"platform-config"]`）
   - Subject Binding：`credential_subject_pk == signer.key()`
5. 索引器生成 Merkle Proof
6. 叶子节点可在链上查询验证

**预期结果**：
- 商户身份已上链
- 可通过 Merkle Proof 验证
- MerchantLeaf status = 0 (active)

**异常处理**：
- 叶子已存在 → 使用 `replace_leaf` 更新
- 链上交易失败 → 重试

---

### 用例 12.3: MCP Server 验证商户链上身份

**前置条件**：
- MCP Server 收到 X402 支付请求
- 商户已上链（用例 12.2）

**参与角色**：MCP Server、Solana 区块链

**详细步骤**：

1. MCP Server 从 402 响应提取 `merchant_did`
2. **链上 Merkle Proof 验证**：
   - 通过 Helius DAS API (IndexerClient) 获取 Merkle Proof
   - 本地 `verify_proof_locally()`：计算 Proof + Leaf == Root
   - 检查 `MerchantLeaf.status == 0` (active)
3. **VC 签名验证**：
   - 从 402 响应提取商家 VC
   - 使用内置平台公钥验证 Ed25519Signature2020 proof
   - 检查 `expirationDate` 未过期
4. **一致性校验**：
   - VC 中 `credentialSubject.id` 的 DID 公钥哈希 == 链上 `merchant_did_hash`
5. 全部通过 → 进入支付决策流程
6. 任一失败 → 拒绝支付

**预期结果**：
- 商家身份验证通过
- 确认商家已上链且状态为 active
- VC 有效且与链上数据一致

**异常处理**：
- Merkle Proof 获取失败 → 拒绝支付
- VC 签名无效 → 拒绝支付
- 一致性不匹配 → 拒绝支付（可能身份被冒充）
- 链上 status != 0 → 拒绝（商家已吊销）

---

## 业务事件 13: 消息推送

### 用例 13.1: 海外用户 → FCM 推送信号 + HTTPS 拉取

**前置条件**：
- 用户 App 已注册 FCM token（`push_channel: "fcm"`）
- MCP Server 与 Mediator 的 WebSocket 连接正常

**参与角色**：MCP Server、Mediator、Google FCM、Sentinel App

**详细步骤**：

**上行（MCP → 手机）**：

1. MCP Server 构建支付授权请求 `payment-auth-request`（JWE 加密）
2. 通过 WebSocket 发送到 Mediator
3. Mediator 接收并存入消息队列，生成 `msg_id`
4. Mediator 查询用户的 `push_channel` 偏好：`"fcm"`
5. Mediator 调用 FCM 发送 Data Message：
   ```json
   { "type": "SIGNAL", "msg_id": "uuid-123" }
   ```
6. FCM 推送到用户手机
7. 手机收到 FCM 消息：
   - 前台：`FirebaseMessaging.onMessage` 触发
   - 后台：`FirebaseMessaging.onBackgroundMessage` 触发
8. App 调用 `GET /v1/sync/messages/{msg_id}` 拉取完整 JWE
9. App 执行 DIDComm Unpack（解密）
10. 展示支付授权界面

**下行（手机 → MCP）**：

1. 用户授权后，App 构建响应 JWE
2. 通过 `POST /v1/agents/{agent_id}/command` 提交到 Mediator
3. Mediator 通过 WebSocket 转发到 MCP Server

**预期结果**：
- 消息通过 FCM 信号 + HTTPS 拉取成功送达
- 用户可实时收到支付授权请求

**异常处理**：
- FCM 信号丢失 → App 回到前台时触发 `GET /v1/sync/list` 兜底同步
- iOS Force Quit → 推送可能延迟，回到前台后同步补全

---

### 用例 13.2: 国内用户 → WebSocket 直推

**前置条件**：
- 用户 App 注册了 `push_channel: "websocket"`
- App 与 Mediator 的 WebSocket 连接在线

**参与角色**：MCP Server、Mediator、Sentinel App

**详细步骤**：

**在线推送**：

1. MCP Server 构建支付授权请求（JWE 加密）
2. 通过 WebSocket 发送到 Mediator
3. Mediator 查询用户的 `push_channel` 偏好：`"websocket"`
4. Mediator 检查用户 WebSocket session 是否在线
5. **在线**：直接通过 WebSocket 将 JWE 推送到手机
6. 手机 App 通过 `onWebSocketMessage` 实时接收
7. DIDComm Unpack 解密 → 展示

**离线暂存**：

1. 若步骤 4 检测到 WS 离线：
2. Mediator 将消息暂存到消息队列
3. 手机 App 恢复在线后：
   - 执行 Mediator 握手（`mediate-request` → `keylist-update`）
   - 发送 `messagepickup/3.0/status-request`
   - 收到 `status`（返回暂存消息计数）
   - 发送 `messagepickup/3.0/batch-pickup` 批量拉取
   - 收到 `batch` 返回批量消息
4. 逐条解密处理

**预期结果**：
- 在线时消息实时推送
- 离线时消息暂存，重连后批量拉取

**异常处理**：
- WS 连接不稳定 → 自动重连（3 秒延迟）
- 批量拉取失败 → 重试

---

### 用例 13.3: 离线消息 → 重连后 Pickup 拉取

**前置条件**：
- App 曾离线一段时间
- Mediator 有暂存的离线消息

**参与角色**：App、Mediator

**详细步骤**：

1. App 回到前台或网络恢复
2. App 自动触发同步：
   - 优先通过 Message Pickup 3.0 协议拉取
   - 兜底调用 `GET /v1/sync/list?after={last_read_id}&limit=100`
3. 获取离线期间所有未读消息
4. 逐条解密处理
5. 更新 `last_read_id` 游标

**完整数据丢失恢复**：

1. 若 App 丢失本地数据（重装/换设备）
2. 使用 `GET /v1/sync/list?after=&limit=100`（不传 after 参数）
3. 从最早消息开始同步
4. 服务端按 `user_did` 过滤，确保只返回该用户的消息

**预期结果**：
- 所有离线消息已拉取并处理
- 不遗漏任何消息

**异常处理**：
- 超过 7 天的离线消息可能已被清理 → 依赖业务层重试
- 游标丢失 → 从头同步

---

### 用例 13.4: App 切回前台 → 兜底同步

**前置条件**：
- App 曾处于后台或锁屏状态
- 可能存在未读的 DIDComm 消息

**参与角色**：App、Mediator

**详细步骤**：

1. App 从后台切回前台（AppLifecycleState.resumed）
2. App 自动触发兜底同步（与用例 13.3 类似，但触发原因不同）：
   - WebSocket 仍在线：发送 `messagepickup/3.0/status-request` 检查是否有未读消息
   - WebSocket 已断开：先重连（3 秒延迟），再执行完整握手 + Pickup 拉取
3. 若 Pickup 协议不可用，降级为 HTTPS 拉取：
   - 调用 `GET /v1/sync/list?after={last_read_id}&limit=100`
4. 收到消息后执行去重：
   - 每条消息有唯一 `id`（DIDComm Message ID）
   - 检查本地是否已处理过该 `id`
   - 已处理 → 跳过
   - 未处理 → 解密、处理、更新游标
5. 处理完毕，UI 刷新（如待处理的支付授权弹窗）

**去重保证**：

| 层级 | 机制 | 说明 |
|:-----|:-----|:-----|
| 消息层 | DIDComm Message `id` | 全局唯一，防重放 |
| 同步层 | `last_read_id` 游标 | 防止重复拉取已处理消息 |
| App 层 | 本地已处理消息缓存 | Pick up 协议和 HTTPS 拉取可能返回重叠消息 |

**预期结果**：
- App 恢复前台后立即同步所有未读消息
- 不遗漏、不重复处理

**异常处理**：
- 同步失败 → 下次前台切换时重试
- Mediator 不可达 → 保持离线状态，等待网络恢复

## 业务事件 14: 商户 DID 生命周期管理

### 用例 14.1: 商户更新链上 VC 哈希

**前置条件**：
- 商户已完成链上注册（用例 12.2）
- 平台已签发新 VC（如经营范围变更、年审更新）

**参与角色**：商户、did-registry、Solana 区块链

**详细步骤**：

1. 商户获取 did-registry nonce：`GET /v1/auth/nonce`
2. 商户使用 Controller Key 签名消息：`update-vc:{merchant_did}:{new_vc_hash}:{nonce}`
3. 商户调用 `POST /v1/merchants/update-vc`：
   ```json
   { "merchant_did": "did:ignite:z...", "new_vc_hash": "SHA-256-hash", "signature": "base64", "nonce": "...", "mode": "sponsored" }
   ```
4. did-registry 验证签名和 nonce
5. 调用链上 `update_did_with_vc` 指令：
   - 验证 Controller Key 授权
   - 验证平台签名
   - 更新 ZK Compression 树中叶子的 `vc_hash` 字段
6. 链上确认
7. 后续支付验证使用新的 `platform_vc_hash`

**预期结果**：
- 链上 `MerchantLeaf.platform_vc_hash` 已更新
- 新 VC 对后续支付验证生效

**异常处理**：
- 签名不匹配 → 拒绝（非 Controller Key 签名）
- nonce 不匹配 → 拒绝（防重放）
- 链上交易失败 → 重试

---

### 用例 14.2: 商户轮换 Controller Key

**前置条件**：
- 商户已完成链上注册
- 商户持有当前 Controller Key
- 已生成新的 Ed25519 Controller Key

**参与角色**：商户、did-registry、Solana 区块链

**详细步骤**：

1. 商户在本地生成新的 Controller Key (Ed25519)
2. 获取 nonce：`GET /v1/auth/nonce`
3. 使用**当前 Controller Key**签名：`rotate-key:{merchant_did}:{new_controller_pubkey}:{nonce}`
4. 调用 `POST /v1/merchants/rotate-key`：
   ```json
   { "merchant_did": "did:ignite:z...", "new_active_pubkey": "Base58Pubkey", "signature": "base64", "nonce": "..." }
   ```
5. did-registry 验证当前 Controller Key 签名
6. 调用链上指令更新 `controller_pk` 字段
7. 商户安全存储新 Controller Key，销毁旧密钥

**预期结果**：
- 链上 `MerchantCompressedDid.controller_pk` 已更新
- 旧 Controller Key 不再有效
- DID 标识符不变

**异常处理**：
- 签名验证失败（非当前 Controller Key）→ 拒绝
- 新密钥与已有密钥冲突 → 拒绝

---

### 用例 14.3: 使用 Recovery Key 恢复 Controller

**前置条件**：
- 商户已设置 Recovery Key（链上 `recovery_pk != 11111...`）
- Controller Key 已丢失或泄露

**参与角色**：商户、did-registry、Solana 区块链

**详细步骤**：

1. 商户从冷存储取出 Recovery Key
2. 生成新的 Controller Key
3. 获取 nonce：`GET /v1/auth/nonce`
4. 使用 Recovery Key 签名恢复消息
5. 调用链上 `recover_controller` 指令：
   - 验证 Recovery Key 签名
   - 更新 `controller_pk` 为新密钥
   - 递增 `nonce`（防重放）
6. 链上确认
7. 商户使用新 Controller Key 进行后续操作

**预期结果**：
- Controller Key 已通过 Recovery Key 重置
- 商户可使用新 Controller Key 管理身份

**异常处理**：
- Recovery Key 不匹配 → 拒绝
- Recovery Key 也丢失 → 身份不可恢复，需联系平台

---

### 用例 14.4: 平台吊销商户 VC

**前置条件**：
- 商户持有有效 VC
- 平台决定吊销（如违规、关闭）

**参与角色**：平台管理员、did-registry、Solana 区块链

**详细步骤**：

1. 平台管理员确定需要吊销的 VC（通过 vc_hash）
2. 获取 nonce：`GET /v1/auth/nonce`
3. 使用平台签名密钥签名：`revoke:{vc_hash}:{nonce}`
4. 调用 `POST /v1/vc/revoke`：
   ```json
   { "vc_hash": "SHA-256-hash", "reason": "违规操作", "nonce": "..." }
   ```
5. did-registry 验证平台权限
6. 调用链上 `revoke_vc` 指令：
   - 创建 `RevokedVc` PDA（seeds: `[b"revoked-vc", vc_hash]`）
   - 记录吊销时间、原因
7. 链上确认
8. 后续支付验证中，MCP Server 检测到 VC 已吊销 → 拒绝支付

**预期结果**：
- VC 已被链上标记为吊销
- 商户后续支付请求被拒绝
- `RevokedVc` PDA 记录了吊销信息

**异常处理**：
- 非平台管理员调用 → 拒绝（403）
- VC 已吊销 → 返回 AlreadyRevoked 错误
- 链上创建 PDA 失败 → 重试

---

### 用例 14.5: 商户自助上链 (SelfOnchain 模式)

**前置条件**：
- 商户已获得平台签发的 VC
- 商户拥有 Solana 钱包和 SOL 余额
- 商户选择自助上链（非平台代付）

**参与角色**：商户、did-registry、Solana 区块链

**详细步骤**：

1. 商户获取 ZK Proof：`POST /v1/proof`：
   ```json
   { "merchant_did": "did:ignite:z...", "active_pubkey": "Base58", "vc_hash": "SHA-256-hash" }
   ```
2. did-registry 返回未签名的链上交易
3. 商户使用自己的 Solana 私钥签名交易
4. 商户自行广播交易到 Solana RPC
5. 链上确认
6. 商户调用确认端点：`POST /v1/merchants/confirm`：
   ```json
   { "did": "did:ignite:z...", "tx_signature": "Base58Sig", "nonce": "..." }
   ```
7. did-registry 验证链上交易
8. 更新本地 sled 记录

**预期结果**：
- 商户身份已自助上链
- did-registry 同步了上链状态

**异常处理**：
- 链上交易失败 → 商户重试广播
- 确认端点找不到交易 → 提示商户检查交易状态
- 超时未确认 → 平台不记录，商户需重新确认

---

### 用例 14.6: 查询商户状态与 DID 解析

**前置条件**：
- 商户已注册（链上或待确认）

**参与角色**：任意客户端、did-registry

**详细步骤**：

1. **查询商户状态**：`GET /v1/merchants/status/{did}`
   - 返回：注册状态、VC 哈希、last_updated、链上 slot
2. **验证商户 DID**：`GET /v1/merchants/verify/{did}`
   - 执行完整的链上验证：获取 Merkle Proof → 本地验证 → 检查 status
   - 返回：verified (bool), leaf_data, proof_valid
3. **解析 DID Document**：`GET /v1/did/resolve/{did}`
   - 从链上和 sled 数据构建 W3C DID Document
   - 返回标准 DID Document JSON

**预期结果**：
- 状态查询返回商户当前注册状态
- 验证端点返回链上验证结果
- DID 解析返回完整 DID Document

**异常处理**：
- DID 不存在 → 返回 404
- 链上查询失败 → 返回错误信息

---

### 用例 14.7: 查询 DID Registry 费用记录

**前置条件**：
- did-registry 服务可用

**参与角色**：平台管理员、did-registry

**详细步骤**：

1. 调用 `GET /v1/fees?operation=register&since=1700000000000&limit=50`
2. did-registry 查询 sled 中 `fee:{operation}:{timestamp_ms}:{did_hash_hex}` 记录
3. 返回费用列表：
   ```json
   { "fees": [{ "operation": "register", "did": "...", "amount_lamports": 5000, "timestamp_ms": 1700000001000 }] }
   ```

**预期结果**：返回指定操作类型的费用记录列表

**异常处理**：无记录 → 返回空列表

---

<!-- State Channel: 探索阶段，暂不启用
## 业务事件 15: 状态通道运维操作

### 用例 15.1: 通道充值 (Fund)

**前置条件**：
- 通道已创建且状态为 Open
- 用户需要增加通道余额

**参与角色**：用户 App、Channel User 服务 (:3001)、Solana 区块链

**详细步骤**：

1. 用户在 App 中选择需要充值的通道
2. 输入充值金额
3. App 调用 Channel User API `POST /v1/channels/{id}/fund`：
   ```json
   { "amount": 2000000000 }
   ```
4. Channel User 服务处理：
   - 创建新的 UTXO 叶子（类型: Standard）
   - 更新 Merkle Tree
   - 生成新的 SignedState
   - 请求双方 CoSign
5. 更新 sled 中的通道状态
6. 返回新的 sequence 和 root

**预期结果**：
- 通道余额已增加
- Merkle Tree 更新
- 新的 SignedState 已双签

**异常处理**：
- 通道已关闭 → 返回 ChannelClosed 错误
- 充值金额无效 → 返回 InvalidAmount 错误

---

### 用例 15.2: UTXO 拆分 (Split Tree)

**前置条件**：
- 通道已开通
- 现有 UTXO 面额不适合后续微额支付

**参与角色**：Channel User 服务

**详细步骤**：

1. 调用 `POST /v1/channels/{id}/split`：
   ```json
   { "leaf_index": 0, "split_amounts": [100000000, 200000000, 700000000] }
   ```
2. Channel User 服务执行拆分：
   - 选择目标 UTXO 叶子
   - 创建多个新的子面额叶子
   - 验证面额总和等于原始叶子余额
   - 更新 Merkle Tree
3. 返回新的叶子索引列表

**预期结果**：
- 原始 UTXO 被拆分为指定面额的多个 UTXO
- 后续可使用不同面额的叶子进行支付

**异常处理**：
- 面额总和不匹配 → 返回 ConservationError
- 叶子不存在 → 返回 LeafNotFound

---

### 用例 15.3: 通道服务 WebSocket 认证

**前置条件**：
- Channel User/Provider/Hub 服务已启动
- 客户端需要实时接收通道事件

**参与角色**：客户端（App/MCP）、Channel 服务

**详细步骤**：

1. 客户端连接 WebSocket：`ws://localhost:3001/ws`
2. 发送认证消息：
   ```json
   { "type": "auth", "pubkey": "<base58>", "signature": [64 bytes], "timestamp": 1713700000 }
   ```
3. 签名内容：`SHA-256("channel-ws-auth:{timestamp}")`
4. 服务端验证 Ed25519 签名
5. 认证成功，建立 WebSocket 会话
6. 后续接收实时 `leaf_update` 推送：
   ```json
   { "type": "leaf_update", "channel_id": "hex", "sequence": 5, "leaf_index": 2 }
   ```
7. 客户端返回 ack 确认

**预期结果**：
- WebSocket 认证成功
- 客户端可实时接收通道状态变更

**异常处理**：
- 签名验证失败 → 服务端关闭 WS 连接
- 认证超时 → 服务端关闭连接

---

### 用例 15.4: 合规状态查询

**前置条件**：
- 通道已开通
- 合规配置已设置

**参与角色**：Channel User 服务 (:3001)

**详细步骤**：

1. 调用 `GET /v1/compliance/{channel_id}`
2. ComplianceManager 返回合规状态：
   ```json
   {
     "channel_id": "hex",
     "window_spending": 500000000,
     "spending_threshold": 1000000000,
     "per_channel_limit": 100000000,
     "travel_rule_triggered": false,
     "window_slots": 100000,
     "current_slot": 250000000
   }
   ```
3. 展示当前滑动窗口内的消费总额与阈值

**预期结果**：返回通道合规状态详情

**异常处理**：通道不存在 → 返回 404

---

### 用例 15.5: 通道自动关闭 (Auto Close)

**前置条件**：
- 通道配置了 `auto_close_offset`（Channel Hub: 500000 slots）
- 通道达到自动关闭条件

**参与角色**：Channel Hub 服务、Solana 区块链

**详细步骤**：

1. Hub 监控所有通道的 `auto_close_slot`（= 开通 slot + auto_close_offset）
2. 当前 slot >= `auto_close_slot` 时触发自动关闭
3. Hub 发起协作关闭流程（类似用例 9.1）：
   - 双方签署最终状态
   - 提交链上结算交易
4. 若协作关闭失败（对方不响应）：
   - 转为单方关闭（用例 9.2）

**预期结果**：
- 长期不活跃的通道被自动关闭
- 资金结算退还各方

**异常处理**：
- 链上提交失败 → 重试
- 对方不响应 → 单方关闭

---
-->

<!-- State Channel: 探索阶段，暂不启用
## 业务事件 16: Hub 网络拓扑管理

### 用例 16.1: Hub 本地注册与信息查询

**前置条件**：
- Channel Hub 已启动

**参与角色**：Channel Hub、其他 Hub/客户端

**详细步骤**：

1. Hub 调用 `POST /v1/hub/register` 自注册：
   ```json
   {
     "hub_did": "did:ignite:z...",
     "endpoint_url": "http://hub:3003",
     "active_pubkey": "Base58Pubkey",
     "collateral": 100000000000,
     "supported_tokens": ["So11111111111111111111111111111111"]
   }
   ```
2. Hub 存储 HubLeaf 到 sled：
   - `hub_did_hash`: SHA-256(Hub DID)
   - `active_pubkey`: 收款公钥
   - `endpoint_hash`: SHA-256(endpoint URL)
   - `collateral`: 抵押金额
   - `platform_vc_hash`: 平台 VC 哈希
3. 任何客户端可调用 `GET /v1/hub/info` 查询 Hub 自身信息
4. 调用 `GET /v1/hub/list` 列出所有已注册 Hub

**预期结果**：Hub 已在本地注册，可被其他节点查询

**异常处理**：DID 重复注册 → 更新已有记录

---

### 用例 16.2: 路由边管理与图刷新

**前置条件**：
- 多个 Hub 已互相发现
- Hub 之间已建立通道

**参与角色**：Channel Hub 管理员、Channel Hub

**详细步骤**：

1. 管理员添加路由边：`POST /v1/routes/add-edge`：
   ```json
   {
     "from_hub_did": "did:ignite:z...A",
     "to_hub_did": "did:ignite:z...B",
     "channel_id": "hex",
     "capacity": 5000000000,
     "fee_rate_bps": 5
   }
   ```
2. Hub 将边添加到路由图（sled 存储）
3. 刷新路由图：`POST /v1/routes/refresh`：
   - 重新扫描所有通道状态
   - 更新可用容量
   - 移除已关闭通道的边
4. 路由图可用于后续路径发现（用例 11.1）

**预期结果**：
- 路由图已更新
- 多跳路径发现可使用最新拓扑

**异常处理**：
- 通道不存在 → 拒绝添加边
- Hub DID 不存在 → 拒绝

---

### 用例 16.3: 路由发现查询

**前置条件**：
- 路由图已有可用边

**参与角色**：Channel Hub

**详细步骤**：

1. 调用 `POST /v1/routes/find`：
   ```json
   { "from_hub_did": "did:ignite:z...A", "to_hub_did": "did:ignite:z...C", "amount": 100000000 }
   ```
2. RouteService 执行 DFS 搜索：
   - 从起始 Hub 开始遍历
   - 过滤掉容量不足的边
   - 对找到的路径评分：`score = 0.3 * fee_score + 0.3 * latency_score + 0.4 * reliability_score`
3. 返回最优路径：
   ```json
   { "path": ["hub_A", "hub_B", "hub_C"], "total_fee_bps": 15, "estimated_latency_ms": 120, "score": 0.85 }
   ```

**预期结果**：返回最优路由路径及其评分

**异常处理**：无可达路径 → 返回空路径列表

---

### 用例 16.4: Hub 中继多跳支付

**前置条件**：
- 多跳路径已确定（用例 16.3）
- 每个 Hop 的 HTLC 已计算

**参与角色**：Channel Hub (中间节点)

**详细步骤**：

1. 收到上游 Hub 的中继请求：`POST /v1/multihop/relay`：
   ```json
   {
     "payment_id": "uuid",
     "from_hub_did": "...",
     "to_hub_did": "...",
     "amount": 100000000,
     "hash_lock": "SHA-256-hash",
     "timelock": 2501000000,
     "hop_index": 2
   }
   ```
2. Hub 验证请求有效性
3. 在本地通道创建 HTLC（锁定资金）
4. 转发到下一跳 Hub
5. 等待 preimage 揭示
6. 收到 preimage → 解锁本地 HTLC → 资金到账
7. 中继费自动计入

**预期结果**：
- 中继支付执行成功
- Hub 获得中继费

**异常处理**：
- 通道余额不足 → 返回 InsufficientCapacity
- Timelock 不合理 → 返回 InvalidTimelock
- 下游 Hub 不响应 → HTLC 超时后退款

---

### 用例 16.5: Hub 接收支付

**前置条件**：
- Hub 作为 Provider 角色
- 用户通过通道向 Hub 发起支付

**参与角色**：Channel Hub、用户

**详细步骤**：

1. 收到支付请求：`POST /v1/channels/{id}/accept-payment`：
   ```json
   { "leaf_update": { ... }, "signature": "base64" }
   ```
2. Hub 验证：
   - LeafUpdate 格式正确
   - 签名有效
   - 金额守恒（转账前后总额不变）
3. 接受支付，更新 Merkle Tree
4. 生成 CoSign 返回

**预期结果**：Hub 接受支付并完成联合签名

**异常处理**：
- 金额不守恒 → 返回 ConservationError
- 签名无效 → 返回 InvalidSignature

---

### 用例 16.6: Hub 批量接收支付

**前置条件**：同用例 16.5，但有多个支付需要处理

**参与角色**：Channel Hub

**详细步骤**：

1. 收到批量支付请求：`POST /v1/channels/{id}/accept-batch`：
   ```json
   { "updates": [ { ... }, { ... } ] }
   ```
2. Hub 逐个验证每个 LeafUpdate
3. 全部有效 → 批量更新 Merkle Tree
4. 任一无效 → 全部拒绝（原子性）
5. 返回批量 CoSign

**预期结果**：批量支付原子处理成功

**异常处理**：任一更新无效 → 全部回滚

---

### 用例 16.7: Hub 提交争议反证

**前置条件**：
- 通道处于挑战期
- Hub 有更新的状态作为反证

**参与角色**：Channel Hub、Solana 区块链

**详细步骤**：

1. Hub 收到挑战通知
2. 从 sled 中检索最新 SignedState
3. 调用 `POST /v1/channels/{id}/submit-counter`：
   ```json
   { "sequence": 15, "root": "hex_root", "signature_a": "base64", "signature_b": "base64" }
   ```
4. 提交链上 `submit_counter_state` 指令
5. 链上验证双签、sequence 更高
6. 更新链上状态

**预期结果**：Hub 的反证被链上采纳，通道状态更新

**异常处理**：
- sequence 不更高 → 反证被拒绝
- 签名不完整 → 反证无效

---
-->

## 业务事件 17: App 端管理与设置

### 用例 17.1: Session Key 管理与撤销

**前置条件**：
- 用户已创建至少一个 Session Key

**参与角色**：用户、Sentinel App、Solana 区块链

**详细步骤**：

1. 用户打开 SessionKeysScreen
2. App 调用 Rust bridge 查询活跃 Session Key 列表：
   - 显示每个 Key 的公钥、创建时间、过期时间、spending_limit、已用额度
   - 活跃 Key 显示绿色标记，已过期显示灰色
3. 用户选择要撤销的 Session Key
4. 确认撤销操作
5. App 调用 Rust bridge `revoke_session_key_onchain()`：
   - 构建链上撤销交易
   - 用户签名
   - 提交到 Solana
6. 链上确认后，Session Key 状态变为 Revoked
7. MCP Server 后续使用该 Key 的支付将被拒绝

**预期结果**：
- Session Key 已在链上撤销
- App 列表更新显示已撤销状态

**异常处理**：
- 链上交易失败 → 提示重试
- Key 已过期 → 无需撤销，提示已过期

---

### 用例 17.2: 助记词导出与身份恢复

**前置条件**：
- 用户已生成 DID 身份

**参与角色**：用户、Sentinel App

**详细步骤**：

**导出助记词**：

1. 用户打开 VaultScreen
2. 点击 "Show Mnemonic Phrase"
3. App 要求二次确认（安全提示）
4. App 调用 Rust bridge `exportMnemonicPhrase()`
5. 显示 12 个助记词（带动画渐变背景）
6. 用户手写备份后点击 "I've Saved It"
7. 助记词从屏幕消失

**从助记词恢复身份**：

1. 用户在新设备上安装 Sentinel App
2. 选择 "Restore Identity"
3. 输入 12 个助记词
4. App 调用 Rust bridge `importMnemonicPhrase()`
5. 从助记词恢复 Ed25519 密钥对
6. 重新推导 DID 标识符
7. 连接 Mediator，拉取离线消息

**预期结果**：
- 导出：助记词安全展示后自动隐藏
- 恢复：用户在新设备上恢复原有 DID 身份

**异常处理**：
- 助记词输入错误 → 提示 "Invalid mnemonic"
- 恢复后 DID 与原设备不同 → 提示密钥不匹配

---

### 用例 17.3: 清除密钥材料

**前置条件**：
- 用户已生成 DID 身份

**参与角色**：用户、Sentinel App

**详细步骤**：

1. 用户打开 VaultScreen
2. 点击 "Erase Key Material"（红色危险按钮）
3. App 弹出确认对话框，要求输入 "ERASE" 确认
4. 用户输入确认文字
5. App 调用 Rust bridge `eraseAllKeyMaterial()`：
   - 删除 sled 数据库中的密钥对
   - 删除 DID Document
   - 删除 Session Key 缓存
   - 删除白名单/黑名单缓存
   - 断开 Mediator 连接
6. App 返回 OnboardingScreen

**预期结果**：
- 所有密钥材料已被安全擦除
- App 回到初始状态

**异常处理**：
- 此操作不可逆 → 确保用户已备份助记词

---

### 用例 17.4: Solana 网络与程序配置

**前置条件**：
- Sentinel App 已初始化

**参与角色**：用户、Sentinel App

**详细步骤**：

1. 用户打开 SettingsScreen
2. 配置 Solana 网络参数：
   - **网络选择**：Devnet / Mainnet 切换
   - **RPC URL**：Solana RPC 端点（可编辑）
   - **DAS Endpoint**：Helius DAS API 端点
3. 配置 SPL 账户压缩参数：
   - **Tree Address**：Concurrent Merkle Tree 地址
   - **Tree Authority**：树管理者公钥
4. 配置程序 ID（只读显示）：
   <!-- State Channel: 探索阶段，暂不启用
   - State Channel Program ID
   -->
   - DID Program ID
   - Session Key Program ID
5. 选择支付模式：自费 (self_funded) / 赞助 (sponsored)
6. App 调用 Rust bridge 保存配置到 sled

**预期结果**：
- 网络配置已更新
- 后续操作使用新配置

**异常处理**：
- RPC URL 无效 → 连接测试失败提示
- Tree Address 格式错误 → 提示格式错误

---

### 用例 17.5: 审计日志查看与 IPFS 同步

**前置条件**：
- 用户已有支付历史

**参与角色**：用户、Sentinel App、IPFS

**详细步骤**：

1. 用户打开 VaultScreen，点击 "Audit Logs"
2. App 从 `LocalLogStore` (SQLite) 加载审计日志列表：
   - 每条记录包含：时间、操作类型、商户 DID、金额、状态
3. App 后台执行 IPFS 同步：
   - `sync_to_ipfs()`：加密（E2EE）→ Zstd 压缩 → 上传到 IPFS
   - `restore_from_ipfs()`：从 IPFS 下载 → 解压 → 解密 → 合并到本地
4. 显示同步状态（已同步/待同步）

**预期结果**：
- 审计日志可查看
- 日志已通过 E2EE 加密同步到 IPFS

**异常处理**：
- IPFS 不可达 → 日志仅保存在本地，标记为待同步
- 解密失败 → 日志可能被篡改，标记警告

---

### 用例 17.6: 商户端订单列表与详情

**前置条件**：
- 商户已有订单记录

**参与角色**：商户、Merchant App

**详细步骤**：

1. 商户打开 PaymentListScreen
2. App 调用 Rust bridge `list_orders()` 加载订单列表
3. 支持筛选标签：全部 / 待确认 / 已确认
4. 下拉刷新
5. 商户点击某个订单进入 PaymentDetailScreen
6. App 调用 `get_order(order_id)` 获取详情：
   - 金额（大字体 USDC 显示）
   - 状态徽章：confirmed=绿 / pending=琥珀 / failed=红 / expired=灰
   - 订单号（可复制）
   - 描述、Hub endpoint、创建时间、确认时间
   - 通道信息（仅 confirmed）：Channel ID、Leaf Index、Sequence

**预期结果**：
- 商户可浏览和筛选所有订单
- 可查看每笔订单的完整详情

**异常处理**：
- 无订单 → 显示空状态提示

---

<!-- State Channel: 探索阶段，暂不启用
### 用例 17.7: 商户端通道列表与操作

**前置条件**：
- 商户已开通至少一个状态通道

**参与角色**：商户、Merchant App、Channel Hub

**详细步骤**：

1. 商户打开 ChannelScreen
2. App 调用 Rust bridge `merchant_list_channels()` 获取通道 ID 列表
3. 对每个通道调用 `merchant_get_channel_status()` 获取详情：
   - Channel ID、状态、Sequence、叶子数、余额、总存入
4. 显示通道卡片列表，顶部汇总：通道总数 + 总余额
5. 商户点击某个通道进入 ChannelDetailScreen
6. 展示完整通道信息
7. 可执行操作：
   - **关闭通道**：确认对话框 → `merchant_close_channel()` → Hub API `/v1/channels/{id}/close`
   - **结算**：`merchant_claim_leaf()` → `merchant_finalize()`

**预期结果**：
- 商户可查看所有通道状态
- 可从商户端发起关闭和结算操作

**异常处理**：
- Hub 不可达 → 提示网络错误
- 通道已关闭 → 显示 Closed 状态

---
-->

### 用例 17.8: 商户语音播报配置

**前置条件**：
- Merchant App 已安装

**参与角色**：商户、Merchant App

**详细步骤**：

1. 商户打开 SettingsScreen，找到 "语音播报" 区域
2. 配置选项：
   - **开关**：启用/禁用语音播报
   - **语言**：中文 / English 切换
   - **音量**：滑块调节 (0-100%)
   - **测试按钮**：点击播放 "收到收款 1.00 USDC" 测试
3. App 调用 Flutter TTS (`flutter_tts`) 服务
4. 配置持久化到 sled

**预期结果**：
- 语音播报按配置执行
- 收到支付确认时播报对应语言的金额

**异常处理**：
- TTS 引擎不可用 → 降级为仅震动提醒

---

### 用例 17.9: 管理 MCP 连接列表

**前置条件**：
- 用户已配对至少一个 MCP Server

**参与角色**：用户、Sentinel App

**详细步骤**：

1. 用户打开 ConnectionScreen
2. App 调用 Rust bridge `getBoundAgents()` 获取已配对 MCP 列表
3. 展示每个 MCP 连接：
   - MCP DID、标签名、连接时间、最后活跃时间
   - Mediator 连接状态（WS/FCM 通道显示）
4. 用户可操作：
   - **添加新 MCP**：打开 QR 扫描器（用例 2.1）
   - **移除 MCP**：调用 `removeBoundAgent(agent_did)` → 从本地缓存删除对等方公钥
5. 配对关系更新

**预期结果**：
- 用户可查看和管理所有已配对的 MCP 连接
- 可添加或移除 MCP

**异常处理**：
- 移除后 MCP 仍可发送消息（直到 Mediator 侧也解绑）→ 提示用户

---

<!-- State Channel: 探索阶段，暂不启用
## 业务事件 18: 合规与风控

### 用例 18.1: 滑动窗口消费阈值追踪

**前置条件**：
- Channel User 服务配置了 `[compliance]` 节
- 通道已有支付记录

**参与角色**：Channel User 服务、ComplianceManager

**详细步骤**：

1. 每次通道支付时，ComplianceManager 自动检查：
   - 计算当前滑动窗口内的总消费：`window_slots` 范围内的所有 LeafUpdate 金额之和
   - 比较 `window_spending + new_amount` 是否超过 `spending_threshold`
2. 若超过阈值：
   - 拒绝本次支付
   - 返回 `SpendingThresholdExceeded` 错误
3. 若未超过：
   - 记录本次消费到 compliance sled 记录
   - 允许支付继续

**预期结果**：
- 用户在滑动窗口内的消费不会超过 `spending_threshold`（1 SOL）
- 超额支付被自动拒绝

**异常处理**：
- 窗口消费记录损坏 → 使用保守估计（拒绝支付）

---

### 用例 18.2: 单通道支付限额

**前置条件**：
- Channel User 服务配置了 `per_channel_limit`

**参与角色**：Channel User 服务、ComplianceManager

**详细步骤**：

1. 用户发起通道支付
2. ComplianceManager 检查：
   - 当前通道累计支付金额是否超过 `per_channel_limit`（0.1 SOL）
   - 本次支付金额是否使累计值超限
3. 超限 → 拒绝支付
4. 未超限 → 允许并记录

**预期结果**：单个通道的累计支付不超过限额

**异常处理**：限额为 0 → 禁用此检查

---

### 用例 18.3: Travel Rule 数据收集

**前置条件**：
- 支付金额超过 `travel_rule_threshold`（0.5 SOL）
- 交易双方的身份信息可用

**参与角色**：Channel User 服务、ComplianceManager

**详细步骤**：

1. 支付金额 > `travel_rule_threshold`
2. ComplianceManager 自动创建 Compliance 叶子（UTXO 类型: Compliance）：
   - 记录发起方 DID 和身份信息
   - 记录接收方 DID 和身份信息
   - 记录金额、时间、通道 ID
3. Compliance 叶子存入 Merkle Tree（不可篡改）
4. 管理员可查询合规记录

**预期结果**：
- 超过阈值的支付自动记录 Travel Rule 数据
- 数据以 Compliance 叶子形式存储在 Merkle Tree 中

**异常处理**：
- 身份信息不完整 → 标记为待补充

---

### 用例 18.4: 金额守恒验证

**前置条件**：
- 通道正在进行 LeafUpdate 操作

**参与角色**：Channel User/Hub 服务

**详细步骤**：

1. 每次 LeafUpdate 执行前，服务端自动验证金额守恒：
   - 遍历所有叶子余额之和（更新前）
   - 遍历所有叶子余额之和（更新后）
   - 两者必须相等
2. 守恒验证通过 → 允许更新
3. 守恒验证失败 → 拒绝更新，返回 `ConservationError`

**预期结果**：
- 所有通道操作保证金额守恒
- 不存在凭空创造或销毁资金的可能

**异常处理**：
- 守恒失败 → 交易拒绝 + 记录审计日志
- 可能指示 Merkle Tree 损坏 → 触发通道关闭

---

### 用例 18.5: Pipeline 回滚机制

**前置条件**：
- Pipeline 批量操作正在构建中
- 某个操作失败

**参与角色**：Channel User/Hub 服务

**详细步骤**：

1. 构建 Pipeline：
   ```rust
   let mut pipeline = Pipeline::new(channel_id);
   pipeline.transfer_leaf(0, 1, 1000)?;  // 成功
   pipeline.transfer_leaf(0, 2, 500)?;   // 成功
   pipeline.create_htlc(0, 3, 200, hash, timelock)?;  // 失败（余额不足）
   ```
2. 第三步返回错误
3. Pipeline 自动回滚前两步的 LeafUpdate
4. 通道状态恢复到 Pipeline 开始前
5. 调用方收到错误信息

**预期结果**：
- Pipeline 操作原子性保证
- 失败时通道状态完全回滚

**异常处理**：
- Pipeline 未调用 `build()` 就被 drop → 自动调用 `abort()` 回滚
-->