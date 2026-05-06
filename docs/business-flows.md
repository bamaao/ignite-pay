# Ignite Pay — 业务流程总览

## 1. 流程清单

| # | 流程名称 | 状态 | 涉及参与者 |
|---|---------|------|-----------|
| F1 | 手机配对（DIDComm 3-step handshake） | ✅ 已实现 | MCP ↔ Phone |
| F2 | Session Key 创建（嵌入支付流程，MCP 按需创建 + 手机注册充值授权一步完成） | ✅ 已实现 | MCP → Phone → Solana |
| F3 | Session Key 充值（余额不足时请求充值） | ✅ 已实现 | MCP ↔ Phone → Solana |
| F4 | x402 支付授权（含支付方式选择） | ✅ 已实现 | Agent → MCP ↔ Phone |
| F5 | 支付执行（Session Key 链上转账） | ✅ 已实现 | MCP → Solana |
| F6 | 支付执行（MagicBlock Voucher） | ✅ 已实现 | MCP → VoucherStore |
| F7 | 额度不足 → 充值请求 | ✅ 已实现 | MCP ↔ Phone |
| F8 | 商户未授权 / 授权超额 → 追加授权 | ✅ 已实现 | MCP ↔ Phone |
| F9 | 商户白名单/黑名单管理 | ✅ 已实现 | MCP / Phone → MCP |
| F10 | MagicBlock 全局金库充值 | ✅ 已实现 | MCP → Solana |
| F11 | MagicBlock 批量结算 | ✅ 已实现 | MCP / Merchant → Solana |
| F12 | 争议与仲裁 | ✅ 已实现 | MCP / Merchant → Solana |
| F13 | 余额查询与通知 | ✅ 已实现 | MCP ↔ Phone |
| F14 | Session Key 续期 / 替换 | ✅ 已实现 | MCP ↔ Phone |
| F15 | 多商户并发支付 | ✅ 已实现 | Agent → MCP → Solana |
| F16 | 支付方式选择（Session Key / MagicBlock / Relayer） | ✅ 已实现 | MCP ↔ Phone |
| F17 | 用户扫码商家支付（QR → Phone → MCP → 执行支付） | ✅ 已实现 | Phone → MCP → Solana |
| F18 | 商家语音播报（QR 支付成功后通知商家 MCP → 商家 App 播报） | ✅ 已实现 | Buyer MCP → Merchant MCP → Merchant App |

---

## 2. 各流程详细说明

### F1: 手机配对（DIDComm 3-step handshake）

**状态**: ✅ 已实现

用户扫码配对，MCP 和手机建立 DIDComm 加密通道，后续所有消息都通过此通道加密传输。

```
Phone                           MCP                           Mediator
  |                               |                               |
  | scan QR (OOB invitation)      |                               |
  |                               |                               |
  |--- connection-request --------|------------------------------>|
  |                               |                               |
  |<-- connection-response -------|<------------------------------|
  |   (MCP 签名 nonce)            |                               |
  |                               |                               |
  |--- connection-confirm ------->|------------------------------>|
  |   (Phone 签名 nonce)          |                               |
  |                               |                               |
  |<-- connection-confirm-resp ---|<------------------------------|
  |   (双向签名验证完成)           |                               |
```

**代码位置**:
- MCP 端处理: `ignite-pay-mcp/src/mediator.rs:858-1235`
- DIDComm 消息构建: `ignite-pay-core/src/didcomm.rs` — `build_connection_request`, `build_connection_response`, `build_connection_confirm`, `build_connection_confirm_response`
- 手机端: `ignite_pay_app/rust/src/api/simple.rs:549` — `send_connection_request`

**DIDComm 消息类型**:
- `ignite-pay/1.0/connection-request`
- `ignite-pay/1.0/connection-response`
- `ignite-pay/1.0/connection-confirm`
- `ignite-pay/1.0/connection-confirm-response`

---

### F2: Session Key 创建（嵌入支付流程中）

**状态**: ✅ 已实现

Session key **只能由 MCP 本地创建**（MCP 独占私钥），但 MCP 不会主动/预先创建。**只有当需要支付且没有可用的 session key 时**，MCP 才会本地生成 ephemeral keypair，然后**将 session key 信息和支付授权请求一起**通过 DIDComm 发给手机。手机端**同时处理**三件事：链上注册 session key account、充值（SOL gas + 稳定币）、用户授权支付。

**完整目标流程**:

```
MCP                              Phone                          Solana
  |                                |                               |
  | [x402 支付请求到达]            |                               |
  | 检查: 没有可用的 session key   |                               |
  |                                |                               |
  | 1. 本地生成 ephemeral keypair  |                               |
  |    (MCP 独占私钥)              |                               |
  |                                |                               |
  | 2. DIDComm: payment-auth-req ->|                               |
  |   + 支付信息:                  |                               |
  |     merchant_did, amount, ...  |                               |
  |   + session key 信息:          |                               |
  |     ephemeral_pubkey           |                               |
  |     spending_limit             |                               |
  |     建议充值金额               |                               |
  |                                |                               |
  |                                | 3. 手机同时处理:               |
  |                                |                                |
  |                                | 3a. 链上注册 session key       |
  |                                |--- register_session ---------->|
  |                                |    (owner + ephemeral 签名)     |
  |                                |<-- tx sig ---------------------|
  |                                |                                |
  |                                | 3b. 充值                       |
  |                                |--- transfer SOL ------------->|
  |                                |    (owner → ephemeral)         |
  |                                |--- transfer USDC ------------>|
  |                                |    (owner_ATA → ephemeral_ATA) |
  |                                |                                |
  |                                | 3c. 用户授权支付               |
  |                                |    (展示支付详情，用户确认)    |
  |                                |                                |
  |<-- payment-auth-response ------|                               |
  |   + authorized: true           |                               |
  |   + session_key_pubkey         |                               |
  |   + session_key_tx_sig (注册)  |                               |
  |   + 充值 tx sigs               |                               |
  |   + spending_limit, expires_at |                               |
  |                                |                               |
  | 4. MCP 本地保存 session key    |                               |
  |    (keypair 已有, 记录授权信息)|                               |
  |                                |                               |
  | 5. execute_payment ----------------------------------------->|
  |    (使用 session key 签名)     |                               |
```

**当前实现 vs 目标差距**:

| 环节 | 状态 |
|------|------|
| MCP 自动创建 ephemeral keypair（支付流程中） | ✅ `process_x402_challenge` → `create_session_key_for_request` |
| payment-auth-request 含 session key + secret key | ✅ `new_session_key` 对象含 pubkey、secret_key、spending_limit、suggested_funding |
| 手机解析 new_session_key 字段 | ✅ `DecryptedMessage` 扩展 + `decrypt_message()` 解析 |
| 手机链上注册外部 session key | ✅ `register_external_session_key()` |
| 手机充值 SOL + SPL token | ✅ `fund_session_key()` |
| 手机同时处理注册+充值+授权 | ✅ `register_and_fund_session_key()` + challenge_screen 集成 |
| payment-auth-response 回传 | ✅ 已有 session key 数据字段 |

**代码位置**:
- MCP 创建 session key: `ignite-pay-mcp/src/main.rs` — `create_session_key_for_request()`
- DIDComm 消息构建: `ignite-pay-core/src/didcomm.rs` — `build_authorization_request_inner()` + `NewSessionKeyRequest`
- 手机解析: `ignite_pay_app/rust/src/api/simple.rs` — `decrypt_message()`
- 手机注册外部 key: `ignite_pay_app/rust/src/api/session.rs` — `register_external_session_key()`
- 手机充值: `ignite_pay_app/rust/src/api/session.rs` — `fund_session_key()`
- 手机一键完成: `ignite_pay_app/rust/src/api/session.rs` — `register_and_fund_session_key()`
- Flutter 集成: `ignite_pay_app/lib/challenge_screen.dart` — `_onAuthorize()` MCP key 路径

---

### F3: Session Key 余额不足 → 充值请求

**状态**: ✅ 已实现

当 MCP 有 session key 但余额不足以完成支付时，通过 DIDComm 请求手机用户充值。

**目标流程**:

```
MCP                              Phone                          Solana
  |                                |                               |
  | 尝试支付，检测余额不足         |                               |
  |                                |                               |
  |--- session-fund-request (DC) ->|                               |
  |   "session key 余额不足"       |                               |
  |   "当前余额: X SOL"            |                               |
  |   "需要: Y SOL + Z USDC"       |                               |
  |   "公钥: <ephemeral_pubkey>"   |                               |
  |                                |                               |
  |                                | 用户选择:                      |
  |                                | A. 充值（输入金额）            |
  |                                | B. 拒绝                        |
  |                                |                               |
  |                                | [如果选择A]                    |
  |                                |--- transfer SOL ------------->|
  |                                |    (owner → ephemeral)         |
  |                                |--- transfer USDC ------------>|
  |                                |    (owner_ATA → ephemeral_ATA) |
  |                                |                               |
  |<-- session-fund-response -----|                               |
  |   { action: "funded",          |                               |
  |     sol_tx: "...",             |                               |
  |     usdc_tx: "..." }           |                               |
  |   或                            |                               |
  |   { action: "rejected" }       |                               |
  |                                |                               |
  | [如果充值成功]                 |                               |
  |--- execute_payment --------------------------------------->|
```

**需要的 DIDComm 消息类型**（已实现）:
- `ignite-pay/1.0/session-fund-request` — MCP → Phone，请求充值 ✅
- `ignite-pay/1.0/session-fund-response` — Phone → MCP，确认充值 ✅

**需要的代码**:
- MCP: 检测余额不足时发送 `session-fund-request` ✅
- Phone: 接收请求，展示充值界面，执行链上转账 ✅
- Phone: 发送 `session-fund-response` ✅
- MCP: 确认余额后继续支付 ✅

---

### F4: x402 支付授权（含支付方式选择）

**状态**: ✅ 已实现

MCP 解析 x402 challenge，验证商户 DID，风控决策后：
- 自动批准（白名单/全局阈值）→ MCP 直接执行支付（优先 MagicBlock，回退 Session Key）
- 需要授权 → MCP 确定可用支付方式，发送 `payment-auth-request`（含 `available_payment_methods`），手机用户选择方式后返回 `payment_method` 字段

`payment-auth-request` 新增字段：
```json
{
  "payment_id": "pay-123",
  "merchant_did": "did:ignite:zMerchant",
  "amount": 500000000,
  "description": "...",
  "available_payment_methods": ["session_key", "magicblock"],
  "new_session_key": { ... }
}
```

`payment-auth-response` 新增字段：
```json
{
  "payment_id": "pay-123",
  "authorized": true,
  "payment_method": "magicblock",
  "..."
}
```

**代码位置**:
- `PaymentMethod` 枚举: `ignite-pay-core/src/didcomm.rs` — `PaymentMethod::SessionKey`, `PaymentMethod::MagicBlock`, `PaymentMethod::Relayer`
- `get_available_payment_methods()`: `ignite-pay-mcp/src/main.rs`
- `has_mb_channel()`: `ignite-pay-mcp/src/main.rs`
- `execute_payment_auto()`: 接受 `preferred_method` 参数，按用户选择执行

Agent 遇到付费墙 → MCP 解析 x402 → 风控决策 → 需要授权时通过 DIDComm 请求手机授权。

详细流程见 `docs/agent-payment-flow.md`。

**DIDComm 消息类型**:
- `ignite-pay/1.0/payment-auth-request` — MCP → Phone
- `ignite-pay/1.0/payment-auth-response` — Phone → MCP

---

### F5: 支付执行（Session Key 链上转账）

**状态**: ✅ 已实现

MCP 使用 session key 签名链上交易，通过 session key 合约的 CPI 执行 SOL/SPL 转账。

```
MCP                                   Solana
  |                                      |
  | build execute_payment IX             |
  | sign with session keypair            |
  |                                      |
  |--- sendTransaction ----------------->|
  |                                      |
  |   on-chain:                          |
  |     verify session valid             |
  |     verify not expired               |
  |     verify spending limit            |
  |     CPI: transfer(ephemeral→merchant)|
  |     update current_spent             |
  |                                      |
  |<-- tx signature ---------------------|
```

**前提条件**:
1. Session key 已链上注册
2. Ephemeral 地址有足够的 SOL/稳定币
3. Spending limit 未耗尽
4. Session 未过期

**代码位置**:
- MCP: `ignite-pay-mcp/src/main.rs:216` — `execute_payment()`
- Solana: `ignite-pay-solana/src/session_program.rs:78` — `build_execute_payment_ix()`
- 链上程序: `ignite-pay-session/programs/ignite-pay-session/src/lib.rs`

---

### F6: 支付执行（MagicBlock Voucher）

**状态**: ✅ 已实现

如果商家有 MagicBlock channel（spending cap 记账），MCP 签链下 voucher，资金仍在统一金库中。

```
MCP                              VoucherStore
  |                                   |
  | derive channel PDA                |
  | query channel on-chain            |
  | check spending_cap - settled      |
  |                                   |
  | sign_voucher(channel_id, seq, $)  |
  | SHA256(channel_id ‖ seq ‖ amount) |
  | Ed25519 sign                      |
  |                                   |
  | store voucher -------------------->|
  |                                   |
  | return voucher proof to Agent     |
```

**代码位置**:
- MCP: `ignite-pay-mcp/src/main.rs:294` — `try_mb_voucher_payment()`
- 签名: `ignite-pay-mb/sdk/src/signing.rs:33` — `sign_voucher()`
- 存储: `ignite-pay-mcp/src/voucher_store.rs`

---

### F7: 额度不足 → 充值请求

**状态**: ✅ 已实现

当 MCP 尝试支付时，检测到 session key 余额不足（SOL 或稳定币），需要通知手机用户充值。

**需要的流程**:

```
MCP                              Phone                          Solana
  |                                |                               |
  | 尝试 execute_payment           |                               |
  | 检测余额 < amount              |                               |
  |                                |                               |
  |--- fund-request (DIDComm) ---->|                               |
  |   "余额不足，需要充值"         |                               |
  |   "当前: X SOL, 需要: Y SOL"   |                               |
  |                                |                               |
  |                                | 用户选择:                      |
  |                                | A. 充值（输入金额）            |
  |                                | B. 拒绝                        |
  |                                |                               |
  |                                | [如果选择A]                    |
  |                                |--- transfer(owner→ephemeral)->|
  |                                |                               |
  |<-- fund-response (DIDComm) ----|                               |
  |   { action: "funded", tx: .. } |                               |
  |   或                            |                               |
  |   { action: "rejected" }       |                               |
  |                                |                               |
  | [如果充值成功]                 |                               |
  |--- execute_payment --------------------------------------->|
```

**已实现的组件**:
1. ✅ MCP 余额检测逻辑（执行支付前检查 ephemeral 地址余额）
2. ✅ DIDComm 消息类型 `session-fund-request` / `session-fund-response`
3. ✅ 手机端充值界面 + 链上转账
4. ✅ MCP 等待充值响应后重试支付

---

### F8: 商户未授权 / 授权超额 → 追加授权

**状态**: ✅ 已实现

当 MCP 需要支付给某个商户时，如果用户之前没有明确授权该商户（不在白名单中），或者该商户的累计支付金额已超过用户设定的授权额度，需要请求手机用户追加授权。

**需要的流程**:

```
场景 A: 商户不在白名单（未授权）
MCP                              Phone
  |                                |
  | risk_check → NeedsAuth        |
  |                                |
  |--- merchant-auth-request ----->|
  |   "新商户请求授权"             |
  |   "商户 DID: did:ignite:z..."  |
  |   "请求金额: X"                |
  |   "授权选项: 单次/按额度/永久" |
  |                                |
  |                                | 用户选择:
  |                                | A. 授权（设定额度和期限）
  |                                | B. 拒绝
  |                                |
  |<-- merchant-auth-response -----|
  |   { authorized: true,          |
  |     max_amount: X,             |
  |     label: "trusted",          |
  |     duration: 86400 }          |
  |   或                            |
  |   { authorized: false }        |

场景 B: 商户已授权但额度不足（超额）
MCP                              Phone
  |                                |
  | 累计支付 > whitelist.max_amount|
  |                                |
  |--- merchant-auth-request ----->|
  |   "商户额度即将用尽"           |
  |   "已用: X, 上限: Y"           |
  |   "本次需要: Z"                |
  |                                |
  |                                | 用户选择:
  |                                | A. 提升额度
  |                                | B. 仅本次授权
  |                                | C. 拒绝
  |                                |
  |<-- merchant-auth-response -----|
```

**当前实现对比**:
- 当前的 `payment-auth-request` 只处理单次支付授权，不区分 "商户授权" 和 "支付授权"
- 白名单机制存在（`ListStore`），但没有 "商户额度耗尽时请求追加" 的 DIDComm 消息
- `payment-auth-response` 中的 `list_action` 字段可以触发白名单更新，但这是事后操作（支付后才更新）

**已实现的组件**:
1. ✅ 商户授权额度跟踪（累计支付 vs 授权上限）
2. ✅ 额度耗尽时的自动检测
3. ✅ 差异化的 DIDComm 消息（区分 "新商户授权" 和 "额度追加"）
4. ✅ 手机端商户授权管理界面

---

### F9: 商户白名单/黑名单管理

**状态**: ✅ 已实现

MCP 支持通过工具调用管理白名单和黑名单，手机授权支付时可通过 `list_action` 字段触发自动加入白名单。

**代码位置**:
- MCP 工具: `add_merchant`, `update_merchant`, `remove_merchant`, `verify_merchant`
- 风控: `ignite-pay-core/src/list_store.rs` — `risk_check()`
- 触发: `process_x402_challenge` 中 `resp.list_action` 处理

---

### F10: MagicBlock 全局金库充值

**状态**: ✅ 已实现

用户通过 MCP 的 `mb_deposit` 工具向全局金库充值 SOL。

```
User → MCP.mb_deposit(amount) → Solana (deposit instruction)
                               → global_buyer_vault lamports += amount
                               → GlobalState.total_deposited += amount
```

**代码位置**:
- MCP 工具: `ignite-pay-mcp/src/main.rs:1344` — `mb_deposit`
- 交易构建: `ignite-pay-mb/sdk/src/transaction.rs:143` — `build_deposit_tx()`
- 链上处理: `ignite-pay-mb/programs/ignite-pay-mb/src/lib.rs:96` — `deposit` instruction

---

### F11: MagicBlock 批量结算

**状态**: ✅ 已实现（MCP 工具层面）

MCP 可以重建 Merkle 树并签名批量结算，商家可以提交 `settle_batch` 或 `optimistic_settle` 到链上。

**代码位置**:
- MCP: `mb_sign_settlement`, `mb_sign_voucher`
- SDK: `ignite-pay-mb/sdk/src/merkle.rs` — Sum Merkle tree
- 链上: `ignite-pay-mb/programs/ignite-pay-mb/src/lib.rs:119` — `settle_batch`

---

### F12: 争议与仲裁

**状态**: ✅ 已实现（链上程序层面）

买家可以 dispute 结算，提供 Merkle proof resolve dispute，商家可以在 dispute_period 后 force_release。

**代码位置**:
- MCP 工具: `mb_dispute`, `mb_resolve_dispute`
- 链上: `dispute`, `resolve_dispute`, `force_release` instructions

---

### F13: 余额查询与通知

**状态**: ✅ 已实现

MCP 没有定期查询 session key 或全局金库余额的机制，也没有主动通知手机余额不足的功能。

**需要的流程**:

```
MCP                              Phone
  |                                |
  | 定期检查:                      |
  |   session key SOL 余额         |
  |   session key USDC 余额        |
  |   MagicBlock 金库余额          |
  |   spending limit 剩余          |
  |                                |
  | [当余额低于阈值时]             |
  |--- balance-notification ------->|
  |   "SOL 余额: 0.01 (低)"        |
  |   "USDC 余额: 5.00"            |
  |   "建议充值"                   |
```

---

### F14: Session Key 续期 / 替换

**状态**: ✅ 已实现

Session key 过期后需要手动创建新的。没有自动续期或无缝替换机制。

**目标流程**（MCP 创建新 key → 手机充值）:

```
MCP                              Phone                          Solana
  |                                |                               |
  | 检测 session 即将过期          |                               |
  |                                |                               |
  | 本地创建新 ephemeral keypair   |                               |
  | 可选: 链上注册新 session PDA   |                               |
  |                                |                               |
  |--- session-renew-request ----->|                               |
  |   "新 session key 已创建"      |                               |
  |   "公钥: <new_ephemeral>"      |                               |
  |   "请充值: X SOL + Y USDC"     |                               |
  |                                |                               |
  |                                | 用户确认充值                   |
  |                                |--- transfer(owner→new_key) --->|
  |                                |                               |
  |<-- session-renew-response -----|                               |
  |   { action: "funded",          |                               |
  |     sol_tx: "...",             |                               |
  |     usdc_tx: "..." }           |                               |
  |                                |                               |
  | 替换旧 session key             |                               |
  | 旧 session 可选 refund --------|------------------------------>|
```

---

### F15: 多商户并发支付

**状态**: ✅ 已实现

MCP 的 `process_x402_challenge` 支持并发请求处理。通过支付互斥锁和原子执行机制：

- ✅ 共享同一个 session key 时，spending limit 检查通过互斥锁保证原子性
- ✅ MagicBlock voucher 的 seq 分配有并发保护
- ✅ 支付队列和优先级机制已实现

---

### F16: 支付方式选择（Session Key / MagicBlock / Relayer）

**状态**: ✅ 已实现

用户在手机端授权时可以选择支付方式。MCP 根据当前状态确定可用方式并发送给手机，手机用户选择后 MCP 按选择执行。

**支持的方式**:

| 方式 | 说明 | 状态 |
|------|------|------|
| `session_key` | Session Key 链上直接转账 | ✅ 可用 |
| `magicblock` | MagicBlock 链下 voucher 签名 | ✅ 可用（需有 channel） |
| `relayer` | 代付模式 + Relayer 服务 | ✅ 可用（session key 签名，relayer 代付 gas） |

**流程**:
```
MCP                                             Phone
  |                                                |
  | determine available_payment_methods            |
  |   - session_key: always available              |
  |   - magicblock: if channel exists on-chain     |
  |                                                |
  |--- payment-auth-request (available_methods) -->|
  |                                                |
  |                    User sees: [Session Key] [MagicBlock]
  |                    User selects method
  |                                                |
  |<-- payment-auth-response (payment_method) -----|
  |                                                |
  | execute_payment_auto(preferred_method)         |
  |   - "session_key" → on-chain transfer          |
  |   - "magicblock" → sign voucher                |
  |   - "relayer" → execute_payment_sponsored (session key signs, relayer pays gas) |
```

**代码位置**:
- `PaymentMethod` 枚举: `ignite-pay-core/src/didcomm.rs:12-30`
- `get_available_payment_methods()`: `ignite-pay-mcp/src/main.rs`
- `has_mb_channel()`: `ignite-pay-mcp/src/main.rs`
- `execute_payment_auto()`: `ignite-pay-mcp/src/main.rs` — 接受 `preferred_method` 参数
- `build_authorization_request_with_methods()`: `ignite-pay-core/src/didcomm.rs`
- `build_authorization_response_v1_3()`: `ignite-pay-core/src/didcomm.rs`

**DIDComm 消息字段变更**:
- `payment-auth-request` 新增: `available_payment_methods: string[]`
- `payment-auth-response` 新增: `payment_method: "session_key" | "magicblock" | "relayer"`

---

### F17: 用户扫码商家支付（QR → Phone → MCP → 执行支付）

**状态**: ✅ 已实现

用户扫描商家二维码后，选择支付方式（Session Key / MagicBlock），手机创建 DIDComm `qr-payment-request` 消息，通过 MCP 的 mediator 路由发送给 MCP，MCP 执行支付后返回 `qr-payment-response`。支付成功后，MCP 还会通过 DIDComm 通知商家 MCP，触发商家 App 语音播报。

**完整流程**:

```
Merchant          Phone              Mediator           MCP              Solana
  |                 |                    |                |                  |
  | [展示 QR]       |                    |                |                  |
  |  merchant_did   |                    |                |                  |
  |  amount         |                    |                |                  |
  |  order_id       |                    |                |                  |
  |  mediator_url   |                    |                |                  |
  |                 |                    |                |                  |
  |                 | [扫描 QR]          |                |                  |
  |                 | 解析 PaymentQrData |                |                  |
  |                 |                    |                |                  |
  |                 | [展示支付详情]      |                |                  |
  |                 | 金额、商户、订单    |                |                  |
  |                 | [选择支付方式]      |                |                  |
  |                 | ○ Session Key      |                |                  |
  |                 | ○ MagicBlock       |                |                  |
  |                 |                    |                |                  |
  |                 | [用户确认支付]      |                |                  |
  |                 |                    |                |                  |
  |                 | build_qr_payment_  |                |                  |
  |                 | request            |                |                  |
  |                 | (JWE encrypted)    |                |                  |
  |                 |                    |                |                  |
  |                 |-- qr-payment-req ->|                |                  |
  |                 |   (via WS)         |                |                  |
  |                 |                    |                |                  |
  |                 |                    |-- forward ---->|                  |
  |                 |                    |                |                  |
  |                 |                    |                | [MCP 解密消息]   |
  |                 |                    |                | QrPaymentCommand |
  |                 |                    |                |                  |
  |                 |                    |                | execute_payment_  |
  |                 |                    |                | auto(method)     |
  |                 |                    |                |                  |
  |                 |                    |                |--- session key ->|
  |                 |                    |                |   或 MB voucher  |
  |                 |                    |                |<-- tx sig/voucher|
  |                 |                    |                |                  |
  |                 |                    |                | build_qr_payment_ |
  |                 |                    |                | response         |
  |                 |                    |                |                  |
  |                 |                    |<-- qr-payment- |                  |
  |                 |                    |    response ---|                  |
  |                 |                    |    (JWE)       |                  |
  |                 |                    |                |                  |
  |                 |<-- qr-payment- ---|                |                  |
  |                 |    response        |                |                  |
  |                 |                    |                |                  |
  |                 | [展示支付结果]      |                |                  |
  |                 | 成功/失败          |                |                  |
  |                 |                    |                |                  |
  |                 |                    |                | [支付成功后]     |
  |                 |                    |                | build_qr_payment_ |
  |                 |                    |                | notify           |
  |                 |                    |                |                  |
  |                 |                    |                |-- qr-payment- -->|
  |                 |                    |                |   notify (JWE)   |
  |                 |                    |                |   → Merchant MCP |
  |<-- channel-payment-confirm ---------|<---------------|                  |
  |   (via merchant mediator)           |                |                  |
  |                 |                    |                |                  |
  | [商家 App 语音播报]                 |                |                  |
  | "收到收款 X.XX USDC"               |                |                  |
```

**DIDComm 消息类型**:
- `ignite-pay/1.0/qr-payment-request` — Phone → MCP（用户扫码后发起支付请求，含 `merchant_mediator_url`）
- `ignite-pay/1.0/qr-payment-response` — MCP → Phone（支付结果）
- `ignite-pay/1.0/qr-payment-notify` — Buyer MCP → Merchant MCP（支付成功后通知商家）

**qr-payment-request 消息格式**:
```json
{
  "type": "https://didcomm.org/ignite-pay/1.0/qr-payment-request",
  "from": "did:ignite:zPhone...",
  "to": ["did:ignite:zMCP..."],
  "body": {
    "merchant_did": "did:ignite:zMerchant...",
    "amount": 500000000,
    "description": "Coffee",
    "order_id": "uuid-v4",
    "payment_method": "session_key",
    "token": "SOL",
    "merchant_mediator_url": "https://merchant-relay.example.com/"
  }
}
```

**qr-payment-response 消息格式**:
```json
{
  "type": "https://didcomm.org/ignite-pay/1.0/qr-payment-response",
  "from": "did:ignite:zMCP...",
  "to": ["did:ignite:zPhone..."],
  "body": {
    "order_id": "uuid-v4",
    "success": true,
    "payment_proof": "Tx: abc123...",
    "payment_method": "session_key"
  }
}
```

**qr-payment-notify 消息格式**（Buyer MCP → Merchant MCP）:
```json
{
  "type": "https://didcomm.org/ignite-pay/1.0/qr-payment-notify",
  "from": "did:ignite:zBuyerMcp...",
  "to": ["did:ignite:zMerchantMcp..."],
  "body": {
    "order_id": "uuid-v4",
    "amount": 500000000,
    "payment_method": "session_key",
    "payment_proof": "Tx: abc123..."
  }
}
```

**支付方式对应的 MCP 执行路径**:

| 用户选择 | MCP 执行动作 |
|---------|------------|
| `session_key` | 使用 session key 签名链上 SOL/SPL 转账 |
| `magicblock` | 链下签名 voucher: `SHA256(channel_id ‖ seq ‖ amount)` + Ed25519 |
| `relayer` | session key 签名交易，relayer 代付 gas 上链 |

**代码位置**:
- DIDComm 消息构建: `ignite-pay-core/src/didcomm.rs` — `build_qr_payment_request()`, `build_qr_payment_response()`, `build_qr_payment_notify()`
- MCP mediator 处理: `ignite-pay-mcp/src/mediator.rs` — `QrPaymentCommand` + `qr-payment-request` handler + `send_to_mediator()`
- MCP 支付执行: `ignite-pay-mcp/src/main.rs` — QR payment handler background task（含商家通知）
- 手机端发送: `ignite_pay_app/rust/src/api/simple.rs` — `send_qr_payment_request()`
- 手机端 QR 解析: `ignite_pay_app/rust/src/api/channel.rs` — `parse_payment_qr()` (含 `merchant_mediator_url`)
- `send_to_phone()`: `ignite-pay-mcp/src/mediator.rs` — 通用 DIDComm 消息发送方法
- 商家 MCP 处理: `ignite-pay-merchant-mcp/src/mediator.rs` — `qr-payment-notify` handler → `channel-payment-confirm` → 商家 App

**与 F16 (支付方式选择) 的区别**:
- F16: MCP 在 x402 支付授权时让手机用户选择方式
- F17: 手机在扫码时直接选择方式，发送给 MCP 执行
- 两者共用 `PaymentMethod` 枚举和 `execute_payment_auto()` 调度逻辑

---

### F18: 商家语音播报（QR 支付成功后通知商家 MCP → 商家 App 播报）

**状态**: ✅ 已实现

用户扫码支付成功后，买家 MCP 通过 DIDComm `qr-payment-notify` 消息通知商家 MCP，商家 MCP 再通过 `channel-payment-confirm` 通知商家 App，触发语音播报（"收到收款 X.XX USDC"）。

**完整流程**:

```
Buyer MCP                  Merchant Mediator         Merchant MCP         Merchant App
    |                            |                        |                     |
    | QR 支付执行成功             |                        |                     |
    |                            |                        |                     |
    | build_qr_payment_notify    |                        |                     |
    | (order_id, amount, method, |                        |                     |
    |  payment_proof)            |                        |                     |
    |                            |                        |                     |
    | pack_encrypted(merchant_did)|                       |                     |
    |                            |                        |                     |
    |--- forward(JWE) ---------->|                        |                     |
    |   POST merchant_mediator   |                        |                     |
    |   _url                     |                        |                     |
    |                            |                        |                     |
    |                            |-- deliver to --------->|                     |
    |                            |   merchant DID         |                     |
    |                            |                        |                     |
    |                            |                        | 解密 qr-payment-    |
    |                            |                        | notify              |
    |                            |                        |                     |
    |                            |                        | build_channel_      |
    |                            |                        | payment_confirm     |
    |                            |                        |                     |
    |                            |                        |--- channel-payment ->|
    |                            |                        |   confirm (JWE)     |
    |                            |                        |                     |
    |                            |                        |                     | 语音播报:
    |                            |                        |                     | "收到收款
    |                            |                        |                     |  X.XX USDC"
```

**前提条件**:
1. QR 码包含 `merchant_mediator_url` 字段（商家 mediator 的 HTTP URL）
2. 商家 MCP 已与商家 App 配对
3. 商家 App 已开启 `VoiceService`（flutter_tts）

**DIDComm 消息类型**:
- `ignite-pay/1.0/qr-payment-notify` — Buyer MCP → Merchant MCP（支付成功通知）
- `ignite-pay/1.0/channel-payment-confirm` — Merchant MCP → Merchant App（触发语音播报）

**qr-payment-notify 消息格式**:
```json
{
  "type": "https://didcomm.org/ignite-pay/1.0/qr-payment-notify",
  "from": "did:ignite:zBuyerMcp...",
  "to": ["did:ignite:zMerchantMcp..."],
  "body": {
    "order_id": "uuid-v4",
    "amount": 500000000,
    "payment_method": "session_key",
    "payment_proof": "Tx: abc123..."
  }
}
```

**代码位置**:
- DIDComm 消息构建: `ignite-pay-core/src/didcomm.rs` — `build_qr_payment_notify()`
- 买家 MCP 发送: `ignite-pay-mcp/src/main.rs` — QR payment handler（支付成功后发送 notify）
- 买家 MCP mediator: `ignite-pay-mcp/src/mediator.rs` — `send_to_mediator()` 方法
- `QrPaymentCommand`: `ignite-pay-mcp/src/mediator.rs` — 含 `merchant_mediator_url` 字段
- 商家 MCP 处理: `ignite-pay-merchant-mcp/src/mediator.rs` — `qr-payment-notify` handler → `channel-payment-confirm`
- QR 数据解析: `ignite_pay_app/rust/src/api/channel.rs` — `PaymentQrData.merchant_mediator_url`
- 商家 App 语音: `ignite_pay_merchant_app/lib/services/voice_service.dart` — `VoiceService` (flutter_tts)

**QR 码 JSON 格式（新增字段）**:
```json
{
  "type": "ignite-pay-request",
  "merchant_did": "did:ignite:z...",
  "amount": 500000000,
  "description": "Coffee",
  "order_id": "uuid-v4",
  "hub_endpoint": "https://...",
  "timestamp": 1700000000,
  "merchant_mb_pubkey": "",
  "merchant_mediator_url": "https://merchant-relay.example.com/"
}
```

**与 MagicBlock voucher 流程的区别**:
- MB voucher 流程: Phone → Merchant MCP（`mb-voucher`）→ 验证签名 → 确认订单 → `channel-payment-confirm` → 语音播报
- QR 支付通知流程: Buyer MCP → Merchant MCP（`qr-payment-notify`）→ 直接确认 → `channel-payment-confirm` → 语音播报
- 区别: MB voucher 需要商家验证买家签名，QR 通知直接基于买家 MCP 的信任关系（已加密认证）

---

## 3. DIDComm 消息类型完整列表

### 已定义的消息类型

| 消息类型 | 方向 | 用途 |
|---------|------|------|
| `ignite-pay/1.0/connection-request` | Phone → MCP | 配对请求 |
| `ignite-pay/1.0/connection-response` | MCP → Phone | 配对响应 |
| `ignite-pay/1.0/connection-confirm` | Phone → MCP | 配对确认 |
| `ignite-pay/1.0/connection-confirm-response` | MCP → Phone | 配对最终确认 |
| `ignite-pay/1.0/payment-auth-request` | MCP → Phone | 支付授权请求（含 `available_payment_methods`） |
| `ignite-pay/1.0/payment-auth-response` | Phone → MCP | 支付授权响应（含 session key + `payment_method`） |
| `ignite-pay/1.0/channel-payment-request` | MCP → Phone | 状态通道支付请求 |
| `ignite-pay/1.0/channel-payment-confirm` | MCP → Phone | 状态通道支付确认 |
| `ignite-pay/1.0/create-channel-request` | Phone → MCP | 请求创建状态通道 |
| `ignite-pay/1.0/create-channel-response` | MCP → Phone | 状态通道创建响应 |
| `ignite-pay/1.0/list-sync-notification` | MCP → Phone | 白名单/黑名单变更通知 |
| `ignite-pay/1.0/mb-voucher` | Phone → Merchant | MagicBlock voucher 发送给商家 |
| `ignite-pay/1.0/qr-payment-request` | Phone → MCP | 用户扫码后发起支付请求（含 payment_method + merchant_mediator_url） |
| `ignite-pay/1.0/qr-payment-response` | MCP → Phone | 扫码支付结果（含 payment_proof） |
| `ignite-pay/1.0/qr-payment-notify` | Buyer MCP → Merchant MCP | QR 支付成功后通知商家 MCP（触发商家 App 语音播报） |

### 需要新增的消息类型

| 消息类型 | 方向 | 用途 | 关联流程 |
|---------|------|------|---------|
| `payment-auth-request` 扩展 | MCP → Phone | 新增 `available_payment_methods`、`new_session_key` 字段 | F2, F16 ✅ 已实现 |
| `session-fund-request` | MCP → Phone | 余额不足时请求充值（复用 `payment-auth-request` 或独立消息） | F3 ✅ 已实现 |
| `session-fund-response` | Phone → MCP | 充值结果（已充值 + tx sig / 已拒绝） | F3 ✅ 已实现 |
| `ignite-pay/1.0/merchant-auth-request` | MCP → Phone | 新商户授权 / 额度追加请求 | F8 ✅ 已实现 |
| `ignite-pay/1.0/merchant-auth-response` | Phone → MCP | 商户授权结果 | F8 ✅ 已实现 |
| `ignite-pay/1.0/balance-notification` | MCP → Phone | 余额不足预警 | F13 ✅ 已实现 |
| `ignite-pay/1.0/session-renew-request` | MCP → Phone | Session key 即将过期，请求续期（MCP 创建新 key → 发给手机充值） | F14 ✅ 已实现 |
| `ignite-pay/1.0/session-renew-response` | Phone → MCP | 新 session key 充值完成确认 | F14 ✅ 已实现 |

---

## 4. 优先级建议

| 优先级 | 流程 | 原因 |
|-------|------|------|
| **P0** | F2: Session Key 创建（嵌入支付流程） | 支付时无 session key → MCP 创建 → 和支付请求一起发给手机 → 手机一次性处理注册+充值+授权 |
| **P0** | F3: 余额不足 → 充值请求 | 支付失败的常见原因，需要闭环 |
| **P1** | F8: 商户未授权 → 追加授权 | 安全性和用户体验的核心 |
| **P2** | F14: Session Key 续期 | MCP 创建新 key → 手机充值，避免支付中断 |
| **P2** | F13: 余额查询与通知 | 主动运维，减少支付失败 |
| **P3** | F15: 多商户并发支付 | 性能优化，非功能阻断 |

> **原则**: Session key 只能由 MCP 本地创建（MCP 独占私钥），手机端只负责充值（SOL gas + 稳定币）。手机端不应创建 session key。
