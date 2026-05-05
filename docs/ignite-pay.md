**AI Agent + 分布式身份 (DID) + MagicBlock 支付通道** 的支付网关设计方案。通过将支付流程与身份认证（DID）深度耦合，结合 MagicBlock 支付通道实现高频低延迟微支付，构建高效、隐私且具备精细化权限管理的系统。

---

## 1. 核心流程架构图

```
Agent → 外部服务商 (402) → Buyer MCP Server → Mediator → 手机 App
                                    ↑                        ↓
                              支付决策引擎                用户授权/拒绝
                        (VC验证+链上DID验证+名单+额度)        ↓
                                    ↑              DIDComm Auth Response
                             IPFS 名单同步 ←—————————————┘
                                    ↓
                       Session Key 链上支付 (SOL/SPL Token)

                ┌───────────────────────────────────────────────┐
                │          MagicBlock 支付通道 (独立流程)         │
                │                                               │
                │  Buyer MCP                    Merchant MCP    │
                │  mb_deposit                   mb_receive_voucher
                │  mb_sign_voucher (off-chain) ──► mb_settle_batch / optimistic_settle
                │  mb_sign_settlement           mb_release_settlement
                │  mb_dispute / resolve_dispute mb_force_release
                │  mb_withdraw                                   │
                └───────────────────────────────────────────────┘
                                    ↓
                           Solana 链上结算
                    (GlobalVault → Escrow → Merchant)
```

---

## 2. 关键环节技术解析

### A. 服务商发现与 X402 协议
X402 协议在此处扮演了"价值交换握手"的角色。
* **触发机制**：当 Agent 请求外部服务商资源而未提供有效凭证时，外部服务商返回 `402 Payment Required` 的扩展版（X402）。MCP Server 解析该响应并启动支付流程。
* **元数据分离**：返回的信息流包含：
  * `accepts[].recipient`：**钱包地址**，用于支付路由（不是 DID）
  * `provider_did`：**商家的 `did:ignite`**（独立字段），用于信誉溯源与黑白名单匹配
  * `accepts[].amount/token/network`：支付金额、代币类型、网络
* **VC 附加**：402 响应可附带平台签发的 Verifiable Credential，用于商家身份背书验证。

### B. 基于 ZK Compression (Light Protocol) 的 DID 管理

使用 Solana 链上的 **ZK Compression (Light Protocol)** 存储商家 DID 账户，实现链上可验证的商家身份管理。压缩账户数据以哈希形式存储在 Light Protocol 状态 Merkle 树中，无需 rent-exemption。

* **架构**：
  * **链上程序**：`ignite-pay-did-program`（Anchor），通过 Light System Program CPI 管理压缩 DID 账户
  * **压缩账户**：`MerchantCompressedDid`，数据以哈希形式存储在 Light 状态树中
  * **账户字段**：`original_pk` (初始公钥), `controller_pk` (当前控制器), `recovery_pk` (恢复密钥), `vc_hash` (平台 VC 哈希), `last_updated`, `nonce` (防重放计数器)
  * **信任链**：平台 Ed25519 签名 `sign(credential_subject_pk || vc_hash)` → 链上 `PlatformConfig` PDA 存储平台公钥 → on-chain 验证
* **VC 撤销注册表**：
  * `RevokedVc` PDA：`seeds = [b"revoked-vc", vc_hash]`，验证者检查 PDA 存在性判断撤销状态
  * 仅平台 authority 可调用 `revoke_vc`
* **操作**：
  * 平台初始化：`init_platform` → 存储平台 Ed25519 公钥到 `[b"platform-config"]` PDA（一次性）
  * 商家入驻：`initialize_did` → 创建压缩 DID（需平台签名 + ZK validity proof）
  * VC 更新：`update_did_with_vc` → 更新 `vc_hash`（需平台签名 + controller 授权 + nonce）
  * 密钥轮换：`set_recovery_key` + `recover_controller` → 通过恢复密钥接管控制器
  * VC 撤销：`revoke_vc` → 创建 `RevokedVc` PDA（仅平台 authority）
* **验证模型**：
  * 链下：通过 Light RPC (Photon) 获取 ZK validity proof + `DidService` 客户端验证
  * 链上：平台签名验证 + subject binding 检查 + nonce 防重放
* **注册服务**：`did-registry` 提供 REST API（`/v1/merchants/register`、`/v1/merchants/verify/{did}`、`/v1/vc/issue`、`/v1/vc/revoke` 等），支持 `Sponsored`（平台代付）和 `SelfOnchain`（商家自付）两种模式

### C. Session Keys

临时密钥系统，用于安全执行链上支付：

* **自付模式 (SelfFunded)**：用户预充值 SOL 到临时密钥，临时密钥直接支付
  * 流程：创建 Session → 预充值 SOL → 构建 SOL/SPL 转账 → 签名发送 → 记录花费
* **代付模式 (Sponsored)**：项目方 Relayer 代付 gas
  * 流程：构建交易（fee_payer = relayer）→ 临时密钥部分签名 → 发送到 Relayer `POST /sponsor` → Relayer 追加 fee_payer 签名并广播
* **风控**：
  * 过期时间检查（`expires_at`）
  * 单次花费额度限制（`spending_limit`）
  * 权限范围限定（`scopes`: `["sol:transfer", "spl:transfer"]`）
* **持久化**：Session 数据通过 borsh 序列化存储在 sled 数据库

### D. MagicBlock 支付通道

基于 Solana 链上支付通道的高频微支付系统。Voucher 签发完全 off-chain，无需预先创建 on-chain channel。Channel 仅在商户发起 L1 结算时按需创建。

**三层架构：**

| 层级 | 说明 |
|------|------|
| L1 (Solana) | 通道创建、资金锁定、签名验证、最终结算 |
| ER (MagicBlock) | 高速状态转换（<50ms 延迟、免 Gas），记录每笔 Voucher |
| Off-chain 欺诈层 | 挑战窗口争议解决，基于 Sum-Merkle Proof |

**核心数据结构：**

| 账户 | 大小 | 字段 |
|------|------|------|
| GlobalState | 89 bytes | `buyer`, `token_mint`, `total_deposited`, `total_allocated`, `bump` |
| Channel | 145 bytes | `buyer`, `merchant`, `token_mint`, `spending_cap`, `settled_amount`, `nonce`, `challenge_period`, `dispute_period`, `bump` |
| SettlementEscrow | 164 bytes | `channel`, `merchant`, `token_mint`, `amount`, `merkle_root`, `nonce`, `created_at`, `claimed`, `disputed`, `optimistic`, `bump` |

**稳定币优先支持：**
- 所有账户包含 `token_mint` 字段，用于区分 SOL（`Pubkey::default()`）和 SPL Token（USDC/USDT 等）
- PDA 种子包含 `token_mint`：`[b"global_state", buyer, token_mint]`、`[b"channel", buyer, merchant, token_mint]`
- 同一买家可与同一商户建立多个通道（按 token 类型区分）
- 手机端充值默认选择 USDC，支持 USDC / USDT / SOL 三种代币

**完整支付流程：**

```
1. SETUP (买家)
   mb_init_global    → 创建 GlobalState + GlobalVault PDA（按 token_mint 区分）
   mb_deposit        → 充值到 GlobalVault（手机端发起，支持 USDC/USDT/SOL）

2. 链下微支付 (买家 → 商户，无需 on-chain channel)
   买家: mb_sign_voucher(seq, amount)           → Ed25519 签名，本地存储
   签发前校验: outstanding_vouchers + amount <= total_deposited - total_allocated
   商户: mb_receive_voucher(buyer_sig)           → 验证签名，本地存储

3a. 协作结算 (商户)
   商户: mb_settle_batch(buyer_batch_sig)        → 构建 Merkle Sum Tree，双签名结算
   结算时自动创建 on-chain Channel（若不存在）
   商户: mb_release_settlement                   → 挑战期后释放资金
   争议路径: 买家 mb_dispute → 买家 mb_resolve_dispute (欺诈证明)
             或: 商户 mb_force_release (争议期后)

3b. 乐观结算 (商户，当买家不配合)
   商户: mb_optimistic_settle                    → 仅商户签名
   后续: 同样的挑战/争议路径
```

**安全模型：**

| 防护 | 说明 |
|------|------|
| 金库余额校验（off-chain） | 签发 voucher 时：`outstanding_vouchers + amount <= total_deposited - total_allocated`（查询链上 GlobalState） |
| 消费上限（on-chain） | 结算时链上检查：`settled_amount + total_amount <= spending_cap` |
| 余额检查（on-chain） | `total_amount <= vault.lamports`（实际余额） |
| 双签名 | Ed25519 指令内省验证买家 + 商户签名 |

**欺诈证明：** Sum-Merkle Tree 设计，买家只需单 Voucher + O(log N) 兄弟节点。128 个 Voucher 的 Proof 仅 280 bytes，远低于 Solana 1232 字节交易限制。

**Global Vault 设计：** 每个 Buyer 一个全局 Vault（GlobalVault PDA），`total_allocated` 追踪所有通道消费上限之和，防止超额分配。Vault 始终由 System Program 拥有，买家通过 `system_instruction::transfer` 存入，程序通过 `invoke_signed` 提取。

### E. 支付决策流程

| 优先级 | 场景 | 判断条件 | 处理动作 |
| :--- | :--- | :--- | :--- |
| 1 | **VC 验证失败** | 附带 VC 签名无效/过期/签发者不匹配 | 拒绝支付，返回验证失败原因 |
| 2 | **链上 DID 验证失败** | 商家 DID 未在链上注册为压缩账户 | 拒绝支付，返回"merchant not found on-chain" |
| 3 | **黑名单阻断** | `provider_did` 在黑名单 | 立即中断，返回 `Security Risk: Provider Blocked` |
| 4 | **白名单自动批准** | `provider_did` 在白名单 && 金额 ≤ `max_amount` | 直接执行链上支付 |
| 5 | **全局阈值自动批准** | 金额 ≤ `auto_approve_max` | 自动执行链上支付，无需手机授权 |
| 6 | **交互式授权** | 以上均不满足 | 触发 DIDComm V2 协议，推送授权请求至用户手机端 |

**支付执行：**
* 若 Solana 已配置：通过 Session Key 执行真实 SOL/SPL Token 转账
* 若 Solana 未配置：使用 mock payment 生成模拟签名（开发模式）

---

## 3. 授权路由：DIDComm V2 与中继器

在这种长链路（Agent → MCP → Mediator → Mobile App）中，**中继器 (Mediator)** 的角色至关重要：

1. **异步处理**：Agent 无法长时间等待用户点击手机。MCP Server 使用 oneshot channel + timeout 机制实现异步等待。
2. **DIDComm V2 协议**：确保了跨端消息的端到端加密。关键区分：
   * **平台 VC**：由平台 DID 签发的商家身份背书凭证，用于验证商家合法性。手机端不签发 VC。
   * **授权响应**：手机端签署的是 `payment-auth-response` 消息（包含 payment_id、authorized、list_action），不是 VC。
3. **名单管理**：用户授权时可选择 `list_action`（whitelist/blacklist/none），授权后自动更新本地 sled 名单并同步（当前 IPFS 同步为 Mock 模式）。

---

## 4. 平台 VC 商家背书流程

```
平台 (Platform DID) → 签发 VC → 附加到 402 响应 → MCP Server 验证
                                          ↑
                                   包含商家 DID、名称、
                                   类别、有效期、Ed25519 签名
```

* **签发者**：平台（使用平台 DID 的 Ed25519 私钥签发）
* **验证内容**：签名有效性、VC 未过期、issuer 匹配配置的平台 DID
* **配置**：MCP Server 的 config.toml 中配置 `[platform]` 节（did + verifying_key_b64）

---

## 5. 名单管理流程（本地 sled + IPFS 同步）

```
手机授权 → list_action != "none"
         → MCP 更新 sled 本地缓存
         → 上传合并名单到 IPFS → 获取新 CID
         → 发送 list-sync-notification 给手机端
```

* **存储结构**：IPFS 上存储 `MerchantLists`（包含 whitelist + blacklist 数组）
* **本地缓存**：sled 数据库中维护两棵 B-tree（`__whitelist__`、`__blacklist__`）
* **IPFS 客户端**：通过 `[ipfs]` 配置节动态选择客户端：
  * `mode = "mock"`：使用 MockIpfsClient（开发模式，内存存储）
  * `mode = "kubo"`：使用 KuboIpfsClient（生产模式，需运行本地 Kubo 节点，通过 `kubo_url` 指定 RPC 地址）
* **配置**：config.toml 中配置 `[ipfs]` 节（`mode` + `kubo_url`）

---

## 6. Crate 结构

```
ignite-pay-core/                    # 核心协议库
├── src/
│   ├── identity.rs                 # DID 生成、DID Document 构建、身份持久化
│   ├── didcomm.rs                  # DIDComm 消息构造器（17 种消息类型）、JWE 加解密
│   ├── solana_did.rs               # SolanaDidBridge: DID 链上验证桥接层
│   ├── types.rs                    # 共享类型：PaymentRequest, MerchantListEntry 等
│   ├── list_store.rs               # 白名单/黑名单管理 (sled + IPFS 同步)
│   ├── vc.rs                       # Verifiable Credential 签发与验证
│   ├── ipfs.rs                     # IPFS 上传/下载抽象层
│   ├── audit_merkle.rs             # SHA-256 Merkle 树审计日志
│   └── log_*.rs                    # E2EE 审计日志（加密 → Zstd 压缩 → IPFS 同步）

ignite-pay-solana/                  # Solana 链上交互
├── src/
│   ├── lib.rs                      # 模块声明 + re-export solana_sdk
│   ├── types.rs                    # MerchantDidAccount, SessionTokenData, PayMode, PaymentResult
│   ├── error.rs                    # SolanaError 统一错误类型
│   ├── compression.rs              # DidService: ZK Compression DID 操作（initialize_did, update_did_with_vc 等）
│   ├── session.rs                  # SessionManager: 临时密钥创建/持久化/验证
│   ├── session_program.rs          # Session Program 指令构建
│   ├── channel.rs                  # 支付通道交互
│   └── payment.rs                  # IgnitePayClient: SOL/SPL Token 转账 (SelfFunded + Sponsored)

ignite-pay-relayer/                 # Relayer 代付服务
├── config.toml                     # [relayer] keypair, rpc_url, listen_addr, rate_limit
└── src/
    └── main.rs                     # Axum HTTP: GET /info (公钥), POST /sponsor (补签+广播)

ignite-pay-did-program/             # 链上 DID 程序 (Anchor + Light SDK)
├── src/
│   ├── lib.rs                      # 6 个指令: init_platform, initialize_did, update_did_with_vc, set_recovery_key, recover_controller, revoke_vc
│   ├── state.rs                    # MerchantCompressedDid, PlatformConfig, RevokedVc
│   └── error.rs                    # DidError 错误码

did-registry/                       # DID 注册服务 (REST API)
├── src/
│   ├── server.rs                   # Axum 路由: /v1/merchants/*, /v1/did/*, /v1/vc/*, /v1/proof
│   ├── state.rs                    # RegistryState: DidService + LightClient + 平台签名
│   ├── config.rs                   # 服务器、Solana、Light (Photon)、认证、费率配置
│   ├── handlers/                   # register, confirm, verify, status, rotate_key, update_vc, issue_vc, revoke_vc, proof, nonce, fees
│   ├── did/                        # resolver (DID 哈希/签名验证), ignite_store (DID 文档缓存)
│   └── storage/                    # sled_store (MerchantStore: 商家记录、VC、费率、撤销状态)

ignite-pay-mb/sdk/                  # MagicBlock 支付通道 SDK
├── src/
│   ├── lib.rs                      # 模块声明
│   ├── pda.rs                      # PDA 派生: derive_global_state_pda, derive_channel_pda, derive_settlement_pda
│   ├── merkle.rs                   # Sum-Merkle Tree: build_sum_merkle_tree, MerkleProof
│   ├── signing.rs                  # sign_voucher, sign_settlement, verify_signature
│   └── transaction.rs              # 11 个交易构建器

ignite-pay-mcp/                     # Buyer MCP Server (23 tools)
├── config.toml                     # [solana] + [magicblock] 配置
└── src/
    ├── main.rs                     # IgnitePayMcpServer: X402 + Session Key + MB 通道
    ├── lib.rs                      # audit, mediator, payment, tools, voucher_store
    ├── tools.rs                    # 工具输入结构体
    ├── voucher_store.rs            # StoredVoucher + VoucherStore (sled)
    ├── mediator.rs                 # MediatorConnection (DIDComm)
    ├── payment.rs                  # PaymentStore (sled)
    └── audit.rs                    # AuditLogStore (sled)

ignite-pay-merchant-mcp/            # Merchant MCP Server (11 tools)
├── config.toml                     # [solana] + [magicblock] + [merchant] 配置
└── src/
    ├── main.rs                     # MerchantMcpServer: QR + Voucher 收集 + 结算
    ├── lib.rs                      # audit, config, mediator, payment, qr, settlement_store, tools, voucher_store
    ├── tools.rs                    # 工具输入结构体
    ├── config.rs                   # Config, MagicBlockConfig
    ├── voucher_store.rs            # CollectedVoucher + MerchantVoucherStore (sled)
    ├── settlement_store.rs         # SettlementRecord + SettlementStore (sled)
    ├── mediator.rs                 # MerchantMediator (DIDComm)
    ├── payment.rs                  # PaymentOrderStore (sled)
    ├── qr.rs                       # PaymentQrData, generate_payment_qr_text
    └── audit.rs                    # AuditLogStore (sled)
```

---

## 7. 配置

### Buyer MCP 配置

```toml
[solana]
rpc_url = "https://api.devnet.solana.com"
pay_mode = "self_funded"   # "self_funded" 或 "sponsored"
relayer_url = "http://localhost:3030"  # 仅 sponsored 模式需要

[ipfs]
mode = "mock"                      # "mock"（开发）或 "kubo"（生产，需本地 Kubo 节点）
kubo_url = "http://127.0.0.1:5001" # Kubo RPC URL（仅 mode = "kubo" 时使用）

[magicblock]
rpc_url = "https://api.devnet.solana.com"
program_id = "6pFXAg1oiV61wVvaJvMHqYdGMe2fscDwmN9UBUSvNuU3"
```

### Relayer 配置

```toml
[relayer]
keypair_b58 = ""                                    # 空=启动时自动生成
rpc_url = "https://api.devnet.solana.com"
listen_addr = "0.0.0.0:3030"
rate_limit = 60
```

### Merchant MCP 配置

```toml
[merchant]
did = ""
hub_endpoint = ""
hub_ws_url = "ws://localhost:3003/ws"
wallet = ""                    # Merchant Solana wallet address (base58)
accept_tokens = ["USDC"]      # Accepted tokens for QR payments

[magicblock]
rpc_url = "https://api.devnet.solana.com"
program_id = "6pFXAg1oiV61wVvaJvMHqYdGMe2fscDwmN9UBUSvNuU3"
```

### 环境变量

| 变量 | 用途 |
|------|------|
| `IGNITE_PAY_CONFIG` | Buyer MCP 配置文件路径（默认 `config.toml`） |
| `IGNITE_MERCHANT_CONFIG` | Merchant MCP 配置文件路径（默认 `config.toml`） |

---

## 8. MCP 工具清单

### Buyer MCP (23 tools)

**X402 支付工具：**

| 工具 | 用途 |
|------|------|
| `process_x402_challenge` | 处理 HTTP 402 支付挑战：解析 x402、验证商家、风控、授权、执行支付 |
| `check_authorization` | 查询支付授权状态 |
| `get_payment_history` | 获取支付历史 |

**身份与配对：**

| 工具 | 用途 |
|------|------|
| `get_identity` | 获取买家 DID、Mediator 状态、Solana 状态、MB Buyer Pubkey/Program ID |
| `generate_pairing_invitation` | 生成 DIDComm 配对 QR 码 |

**Session Key 管理：**

| 工具 | 用途 |
|------|------|
| `create_session` | 创建 Session Key（SOL 或 SPL Token），可选链上注册 |
| `get_session_status` | 查询 Session Key 状态（余额、有效期） |
| `close_session` | 关闭 Session Key，可选退还 SOL |
| `execute_spl_payment` | 使用 Session Key 执行 SPL Token 转账 |

**链上 DID 管理：**

| 工具 | 用途 |
|------|------|
| `add_merchant` | 添加商家 ZK 压缩 DID 账户 |
| `update_merchant` | 更新商家 ZK 压缩 DID 数据 |
| `verify_merchant` | 验证商家链上身份 |

**MagicBlock 支付通道（11 tools）：**

| 工具 | 用途 |
|------|------|
| `mb_init_global` | 初始化全局状态（创建 GlobalState + GlobalVault PDA） |
| `mb_deposit` | 向 GlobalVault 充值 SOL |
| `mb_create_channel` | 创建支付通道（指定商户、消费上限、挑战期、争议期） |
| `mb_update_spending_cap` | 调整通道消费上限 |
| `mb_get_channel` | 查询通道状态 |
| `mb_get_global_state` | 查询全局状态 |
| `mb_sign_voucher` | 签名 Voucher（Ed25519 签名 `SHA256(channel_id \|\| seq \|\| amount)`） |
| `mb_sign_settlement` | 签名结算消息（重建 Merkle Tree，验证后签名） |
| `mb_dispute` | 争议结算（冻结 Escrow） |
| `mb_resolve_dispute` | 解决争议（提交 Sum-Merkle Proof 欺诈证明） |
| `mb_withdraw` | 提取未分配资金 |

### Merchant MCP (11 tools)

**订单管理：**

| 工具 | 用途 |
|------|------|
| `generate_payment_qr` | 生成收款二维码（含商户 MB Pubkey） |
| `check_payment` | 查询订单状态 |
| `get_payment_history` | 获取订单历史 |
| `get_identity` | 获取商户 DID、MB Merchant Pubkey、Program ID |

**MagicBlock 支付通道（7 tools）：**

| 工具 | 用途 |
|------|------|
| `mb_get_channel` | 查询与买家的通道状态 |
| `mb_receive_voucher` | 接收买家 Voucher：验证签名、存储 |
| `mb_settle_batch` | 批量结算：构建 Merkle Sum Tree、商户签名、双签名提交 |
| `mb_optimistic_settle` | 乐观结算：仅商户签名（需 challenge_period > 0） |
| `mb_get_settlement` | 查询结算 Escrow 状态 |
| `mb_release_settlement` | 释放结算（挑战期后，资金转入商户） |
| `mb_force_release` | 强制释放（争议期后） |

---

## 9. DIDComm 消息类型

| 消息类型 URI | 方向 | 用途 |
|------|------|------|
| `ignite-pay/1.0/connection-request` | Phone → MCP | 手机发起配对 |
| `ignite-pay/1.0/connection-response` | MCP → Phone | MCP 接受配对 |
| `ignite-pay/1.0/connection-confirm` | Phone → MCP | 手机确认配对 |
| `ignite-pay/1.0/payment-auth-request` | MCP → Phone | 支付授权请求 |
| `ignite-pay/1.0/payment-auth-response` | Phone → MCP | 手机授权/拒绝支付 |
| `ignite-pay/1.0/list-sync-notification` | MCP → Phone | 名单同步通知 |
| `ignite-pay/1.0/qr-payment-request` | Phone → MCP | 手机扫描商户 QR 发起支付 |
| `ignite-pay/1.0/qr-payment-response` | MCP → Phone | QR 支付结果 |
| `ignite-pay/1.0/qr-payment-notify` | MCP → Merchant MCP | 支付成功通知商户 |
| `ignite-pay/1.0/mb-deposit-request` | Phone → MCP | 手机发起 MB 共享金库充值（含 `token`: USDC/USDT/SOL） |
| `ignite-pay/1.0/mb-deposit-response` | MCP → Phone | MB 充值结果（含 total_deposited, tx_signature, token） |

---

## 10. 优化建议与潜在挑战

### 1. 状态同步问题
* **挑战**：IPFS 上的黑白名单更新可能有延迟。
* **建议**：在 MCP Server 本地 sled 缓存确保即时查询，IPFS 仅用于跨设备同步。

### 2. 隐私保护
* **建议**：在向中继器发送支付意图时，可使用隐身地址或对交易金额进行混淆，防止中继器掌握消费画像。

### 3. Agent 的重试逻辑
* **流程**：Agent 拿到支付信息后，在 HTTP Header（如 `Authorization: Bearer <Payment_Proof>`）中带上该信息再次请求。
* **容错**：如果支付成功但服务商未返回资源，系统需要基于 `provider_did` 的仲裁或申诉机制。

### 4. 性能考量
* **ZK Compression DID**：压缩账户无需 rent-exemption，通过 Light RPC 获取 validity proof，链下验证为毫秒级
* **链上 DID 操作**：平台签名验证 + nonce 防重放，交易大小可控
* **Session 管理**：sled 持久化，重启后自动恢复活跃 Session
* **MB 支付通道**：纯 off-chain Voucher 签发（毫秒级，仅查询链上 GlobalState 余额），批量结算将多笔支付合并为一次链上交易，Channel 按需创建
* **MB Keypair 持久化**：sled 存储，重启后自动恢复

---

## 11. 阶段规划

| 阶段 | 功能 | 状态 |
| :--- | :--- | :--- |
| **V0.1** | 基础 MCP + DIDComm 加密 + Mediator + Mock 支付 | ✅ 已完成 |
| **V1.0** | 手机端授权闭环（Flutter Rust Bridge + WS 双向通信） | ✅ 已完成 |
| **V1.1** | VC 验证 + IPFS 黑白名单 + 名单同步 | ✅ 已完成 |
| **V2.0** | ZK Compression (Light Protocol) DID + Session Keys + 链上支付 | ✅ 已完成 |
| **V2.1** | MagicBlock 支付通道（链下 Voucher + Merkle 结算 + 争议机制） | ✅ 已完成 |
| **V2.2** | 代付模式 (Sponsored) + Relayer 服务 | ✅ 已完成 |
| **V2.3** | 手机端发起 MB 共享金库充值 + 纯 off-chain voucher 签发 + 稳定币优先支持 | ✅ 已完成 |
| **V2.4** | QR 支付完善：收款地址 + 币种选择 + SPL Token 支持（所有支付方式支持 USDC/USDT） | ✅ 已完成 |

---

## 总结

该系统为 **"Agent Economy" (智能体经济)** 提供完整支付基础设施。通过 X402 实现按需付费，通过 VC 验证实现信任体系，通过 DIDComm V2 保证用户的最终控制权（Self-Sovereignty）。链上支付通过 Session Keys 实现安全便捷的 SOL/SPL Token 转账（支持 SelfFunded 自付和 Sponsored 代付两种模式），MagicBlock 支付通道实现高频微支付场景（纯 off-chain 签发 Voucher，金库余额校验，商户按需 L1 批量结算 + 欺诈证明争议机制）。Relayer 服务为 Sponsored 模式提供 gas 代付能力，用户无需持有 SOL 即可完成支付。
