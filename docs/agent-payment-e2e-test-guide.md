# Agent 支付流程端到端测试指南

本文档是 `agent-payment-flow.md` 所描述 12 步流程的可执行端到端测试指南。
覆盖自动通过、手机授权 + Session Key、手机授权 + MagicBlock Voucher、黑名单拒绝、商户链上验证失败五条核心路径。

**关联文档：**
- [Agent 支付流程](agent-payment-flow.md) — 12 步时序图与代码位置
- [手动测试演练手册](manual-test-walkthrough.md) — 分阶段 UI 测试
- [App 功能测试](ignite-pay-app-test-plan.md) — 42 条手机端测试用例

---

## 1. 前置条件清单

### 1.1 工具与运行环境

| 工具 | 版本要求 | 用途 |
|------|----------|------|
| Docker & Docker Compose | 20+ / v2 | 启动后端服务 |
| Rust toolchain | stable + sbf | 编译链上程序（WSL 下） |
| Solana CLI | 1.18+ | 密钥生成、devnet 空投 |
| Flutter SDK | 3.x | 构建手机 App |
| Python | 3.10+ | 电商 Demo 服务器与测试脚本 |
| Android 设备/模拟器 | API 34+ | 运行手机 App |
| curl / httpie | — | 手动 API 调试 |

### 1.2 网络与外部服务

| 条件 | 验证方式 |
|------|----------|
| Solana Devnet RPC 可达 | `curl -s https://api.devnet.solana.com -X POST -d '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' -H "Content-Type: application/json"` |
| Photon RPC（ZK Compression）可达（当前未使用，未来考虑） | `curl -s "<photon_url>" -X POST -d '{"jsonrpc":"2.0","id":1,"method":"getHealth"}'` |
| FCM 可用（海外推送场景） | Google Play Services 正常 |

### 1.3 密钥与钱包

| 密钥 | 位置 | 用途 |
|------|------|------|
| 用户 Solana 密钥对 | `test-user.json` | 买家钱包，需要 devnet SOL |
| 商户 Solana 密钥对 | `test-merchant.json` | 商户收款地址 |
| DID Registry payer | `deploy/keys/payer.key` | DID 上链交易的付费方 |
| Platform signing key | `deploy/keys/platform-signing.key` | 签发商户 VC |
| Channel User/Provider/Hub keys | `deploy/keys/user.key` 等 | 状态通道密钥（当前未使用，未来考虑） |

### 1.4 配置文件检查

确认以下配置已正确填写：

| 文件 | 关键字段 |
|------|----------|
| `.env` | `SOLANA_RPC_URL`、`DID_PROGRAM_ID`、`SESSION_PROGRAM_ID`、`JWT_SECRET` |
| `ignite-pay-mcp/config.toml` | `mediator.ws_url`、`mediator.phone_did`、`policy.auto_approve_max`、`solana.rpc_url` |
| `ignite-pay-merchant-mcp/config.toml` | `merchant.wallet`、`mediator.ws_url` |
| `ignite-pay-ecom-demo/config.json` | `merchant_did`、`payment_address`、`rpc_url` |

---

## 2. 环境搭建

### 2.1 初始化配置

```bash
# 1. 创建环境变量文件
make init
# 编辑 .env，填入真实的密钥和配置值

# 2. 创建密钥目录并放入密钥文件
make keys
# 将 payer.key、user.key、provider.key、hub.key 放入 deploy/keys/

# 3. 生成测试用 Solana 密钥对
solana-keygen new --outfile test-user.json --no-bip39-passphrase
solana-keygen new --outfile test-merchant.json --no-bip39-passphrase

# 4. Devnet 空投
solana airdrop 2 test-user.json --url devnet
solana airdrop 2 test-merchant.json --url devnet
```

### 2.2 启动后端服务

```bash
# 5. 构建并启动所有 Docker 服务
make build
make up

# 6. 验证所有服务健康
make health
```

预期输出 — 所有服务报告 OK：
```
--- PostgreSQL ---       OK
--- Hub Registry ---     OK (direct :3004)
--- DIDComm Router (user) ---     OK (direct :8080)
--- DIDComm Router (merchant) --- OK (direct :4000)
--- DID Registry ---     OK (direct :8081)
--- Channel User ---     OK (direct :3001)
--- Channel Provider --- OK (direct :3002)
--- Channel Hub ---      OK (direct :3003)
```

**如果某服务不健康：** 查看 `make logs S=<service-name>` 获取详细错误。

### 2.3 启动买家 MCP

```bash
# MCP 作为本地进程运行（不在 Docker 内）
cd ignite-pay-mcp
# 确认 config.toml 中的 mediator.ws_url 指向正确的 DIDComm Router
cargo run
```

MCP 启动后监听 SSE 端口 9001。确认日志中出现 `SSE server listening on 0.0.0.0:9001`。

### 2.4 启动商户 MCP

```bash
cd ignite-pay-merchant-mcp
cargo run
```

商户 MCP 监听端口 9002。

### 2.5 启动电商 Demo 服务器

```bash
cd ignite-pay-ecom-demo
pip install -r requirements.txt
python server.py
```

验证：
```bash
curl http://localhost:9090/products
# 应返回 JSON 产品列表：coffee、sandwich、juice
```

### 2.6 安装并配置手机 App

1. 构建并安装消费者 App：
   ```bash
   cd ignite_pay_app
   flutter build apk --split-per-abi
   adb install build/app/outputs/flutter-apk/app-arm64-v8a-release.apk
   ```
2. 启动 App → 完成首次向导（创建 DID → 连接 Mediator）
3. 在 App 中扫描 MCP 配对二维码完成 DIDComm 握手
4. （可选）构建并安装商户 App：
   ```bash
   cd ignite_pay_merchant_app
   flutter build apk --split-per-abi
   ```

### 2.7 MagicBlock 初始化（当前未使用，未来考虑）

通过 MCP 工具完成一次性设置：

```
1. mb_init_global        → 创建全局状态 + Vault PDA
2. mb_deposit <amount>   → 向 Vault 存入 SOL（如 2 SOL = 2000000000 lamports）
3. mb_create_channel <merchant_pubkey> → 开通与目标商户的支付通道
4. mb_update_spending_cap <merchant_pubkey> <cap> → 设置商户消费上限
```

验证通道已创建：
```
mb_get_channel <merchant_pubkey>
# 应返回通道 PDA、序列号、spending cap 等信息
```

---

## 3. 测试用例

### 测试路径一：自动通过（白名单 / 全局阈值）

> 覆盖流程步骤：1 → 2 → 3 → 4 → 5 → 6 → 7（自动通过）→ 11 → 12

**目的：** 验证当商户在白名单或金额低于全局阈值时，支付无需手机授权即可自动完成。

#### 前置条件

- MCP `config.toml` 中 `policy.auto_approve_max` 设置为一个合理的阈值（如 `1000000`，即 0.001 SOL）
- 或商户已添加到白名单（通过之前的支付授权中 List Action = Whitelist）

#### 步骤

| # | 操作 | 验证点 |
|---|------|--------|
| 1 | Agent 调用 `GET http://localhost:9090/products` | 返回产品列表 JSON |
| 2 | Agent 调用 `POST http://localhost:9090/orders {"product_id": "coffee"}` | HTTP 402，响应体包含 `PaymentRequirements`，响应头包含 `PAYMENT-REQUIRED`（base64）、`x402-merchant-did`、`x402-payment-address` |
| 3 | Agent 调用 MCP 工具 `process_x402_challenge(challenge_body, headers)` | MCP 日志显示：解析 x402 挑战成功、提取 network/amount/recipient/merchant_did |
| 4 | 检查 MCP 日志 | `risk_check` 判定结果为 "auto approve"，**无** DIDComm `payment-auth-request` 发出 |
| 5 | 等待 MCP 完成支付 | MCP 日志显示 `execute_payment_auto` 执行，返回支付凭证（交易签名或 Voucher） |
| 6 | Agent 携带凭证重发 `POST /orders`，添加 `X-Payment-Proof` 头 | HTTP 200，订单状态 = `"paid"` |

#### 每步验证点

**MCP 日志关键词：**
```
Parsed x402 challenge: network=solana-devnet, amount=100000, ...
VC verification: skipped (no VC in challenge)
DID on-chain verify: merchant account found
risk_check: auto_approve (amount <= threshold)
execute_payment_auto: ...
Payment proof: tx=<signature>
```

**链上验证：**
```bash
# 查询交易签名
solana confirm <tx_signature> --url devnet
# 应输出 "Confirmed"
```

**服务器端验证：**
- 电商 Demo 日志显示收到 `/orders/{id}/verify-tx` 请求
- 收款方余额增长 >= 支付金额

---

### 测试路径二：手机授权 + Session Key 链上支付

> 覆盖流程步骤：1 → 2 → 3 → 4 → 5 → 6 → 7（需授权）→ 8 → 9 → 10 → 11（Session Key）→ 12

**目的：** 验证当需要手机授权时，通过 Session Key 执行链上 SOL 转账的完整流程。

#### 前置条件

- `policy.auto_approve_max = 0`（或金额超过阈值）
- 商户不在白名单中
- 手机 App 已配对 MCP

#### 步骤

| # | 操作 | 验证点 |
|---|------|--------|
| 1 | Agent 调用 `POST /orders {"product_id": "coffee"}` | HTTP 402 + x402 挑战 |
| 2 | Agent 调用 MCP `process_x402_challenge(...)` | MCP 日志显示 `risk_check: need_authorization` |
| 3 | 检查 MCP 日志 | `get_available_payment_methods()` 返回 `["session_key"]`（无 MagicBlock 通道时） |
| 4 | MCP 通过 DIDComm 发送 `payment-auth-request` | MCP 日志显示 `Sending payment-auth-request via DIDComm` |
| 5 | 查看手机 App | Dashboard 显示 amber 横幅 "Payment authorization requested" |
| 6 | 点击 "Authorize Payment" | 打开 Challenge 弹窗，显示商户 DID、金额、描述、可用支付方式 |
| 7 | 验证弹窗内容 | List Action 默认 "This time only"；显示 "Slide to Authorize" 滑动条 |
| 8 | 滑动授权到 85%+ | 弹出签名方式选择器（Built-in Key / Phantom-Solflare / Mobile Wallet） |
| 9 | 选择 "Built-in Key" | 显示 "Registering session key on-chain..." |
| 10 | 等待注册完成 | 显示 "Authorized with session key"；1.2s 后弹窗关闭 |
| 11 | MCP 日志确认 | 收到 `payment-auth-response`，包含 `authorized=true`、`payment_method=session_key` |
| 12 | MCP 执行链上支付 | 日志显示 `execute_payment(session_key)` → 交易签名 |
| 13 | Agent 携带凭证重发 `POST /orders` | HTTP 200，订单状态 = `"paid"` |

#### 每步验证点

**手机 App UI 状态：**
- Step 5：Dashboard 顶部 amber 横幅可见
- Step 6：ChallengeScreen 全屏弹窗，Merchant Card 显示截断 DID、Amount 大字号显示
- Step 9：ResultBanner loading spinner
- Step 10：ResultBanner 绿色 "Authorized with session key"

**MCP 日志关键词：**
```
risk_check: need_authorization
get_available_payment_methods: ["session_key"]
Sending payment-auth-request via mediator: payment_id=pay-xxx
Received payment-auth-response: authorized=true, method=session_key
Session key registered: <pubkey>
execute_payment: tx=<signature>
Payment proof: tx=<signature>
```

**链上验证：**
```bash
# 确认 Session Key 已注册
solana program show <session_key_pubkey> --url devnet
# 确认支付交易
solana confirm <tx_signature> --url devnet
# 确认收款方余额
solana balance <merchant_payment_address> --url devnet
```

---

### 测试路径三：手机授权 + MagicBlock Voucher 支付（当前未使用，未来考虑）

> 覆盖流程步骤：1 → 2 → 3 → 4 → 5 → 6 → 7（需授权）→ 8 → 9 → 10 → 11（MagicBlock）→ 12

**目的：** 验证当存在 MagicBlock 通道时，用户可选择链下 Voucher 支付。

#### 前置条件

- 已完成 MagicBlock 初始化（Section 2.7）
- 与目标商户存在支付通道
- 手机 App 已配对 MCP

#### 步骤

| # | 操作 | 验证点 |
|---|------|--------|
| 1 | Agent 调用 `POST /orders {"product_id": "coffee"}` | HTTP 402 + x402 挑战 |
| 2 | Agent 调用 MCP `process_x402_challenge(...)` | MCP 检测到 MagicBlock 通道存在 |
| 3 | 检查 MCP 日志 | `get_available_payment_methods()` 返回 `["session_key", "magicblock"]` |
| 4 | MCP 发送 `payment-auth-request` | `available_payment_methods` 数组包含两种方式 |
| 5 | 手机 App 显示授权页面 | 页面显示两种支付方式可选 |
| 6 | 用户选择 MagicBlock 方式并滑动授权 | 授权请求携带 `payment_method="magicblock"` |
| 7 | MCP 收到授权响应 | 日志显示 `method=magicblock` |
| 8 | MCP 签名 Voucher | `mb_sign_voucher(channel_id, seq, amount)` → SHA256(channel_id ‖ seq ‖ amount) + Ed25519 签名 |
| 9 | 检查 Voucher 存储 | Voucher 已写入 sled VoucherStore |
| 10 | MCP 返回 Voucher 凭证 | 凭证包含 Channel、Seq、Amount、Signature、Message hash |
| 11 | Agent 携带 Voucher 凭证重发请求 | HTTP 200，订单状态 = `"paid"`（取决于商户是否支持 Voucher 验证） |

#### 每步验证点

**MCP 日志关键词：**
```
has_mb_channel: true for merchant <pubkey>
get_available_payment_methods: ["session_key", "magicblock"]
Sending payment-auth-request: methods=["session_key","magicblock"]
Received payment-auth-response: authorized=true, method=magicblock
mb_sign_voucher: channel=<pda>, seq=1, amount=100000
Voucher signed: msg_hash=<hash>, signature=<sig>
Voucher stored in VoucherStore
Payment proof: voucher(channel=<pda>, seq=1)
```

**Voucher 凭证格式：**
```
Channel: <channel_pda>
Seq: <序列号>
Amount: <lamports>
Signature: <base58 Ed25519 签名>
Message hash: <base58 SHA256(channel_id || seq || amount)>
```

**链上验证（Voucher 存储后）：**
```
mb_get_channel <merchant_pubkey>
# 确认 last_seq 已递增
```

---

### 测试路径四：黑名单拒绝

> 覆盖流程步骤：1 → 2 → 3 → 4 → 5 → 6 → 7（黑名单）→ 拒绝

**目的：** 验证当商户在黑名单中时，支付请求被立即拒绝。

#### 前置条件

- 商户已被加入黑名单（通过之前的支付授权中 List Action = Blacklist）

#### 步骤

| # | 操作 | 验证点 |
|---|------|--------|
| 1 | Agent 调用 `POST /orders {"product_id": "coffee"}` | HTTP 402 + x402 挑战 |
| 2 | Agent 调用 MCP `process_x402_challenge(...)` | MCP 日志显示 `risk_check: blocked (merchant in blacklist)` |
| 3 | 检查 MCP 返回 | 返回错误：`"支付拒绝：商户在黑名单"` 或类似消息 |
| 4 | 确认无 DIDComm 消息发出 | MCP 日志中**无** `payment-auth-request` 发送记录 |
| 5 | 确认手机无通知 | 手机 App 不显示任何支付请求横幅 |

#### 每步验证点

**MCP 日志关键词：**
```
risk_check: merchant in blacklist → blocked
Returning error: payment denied (blacklisted merchant)
```

**手机 App UI：**
- 无 amber 横幅
- Messages 列表无新 payment-auth-request

**添加黑名单操作（前置）：**
1. 触发一次该商户的正常支付请求（路径二）
2. 在 Challenge 弹窗中选择 List Action = "Blacklist"，输入 Label
3. 滑动授权（授权本身会被处理，同时商户加入黑名单）
4. 再次触发该商户支付 → 应被自动拒绝

---

### 测试路径五：商户链上验证失败

> 覆盖流程步骤：1 → 2 → 3 → 4 → 5 → 6（验证失败）→ 返回错误

**目的：** 验证当链上未找到商户 DID 账户时，支付被拒绝。

#### 前置条件

- `solana.did_program_id` 已配置（非空）
- `photon_url` 和 `address_tree` 已配置（ZK Compression 场景，当前未使用，未来考虑）
- 挑战中的 `merchant_did` 对应的链上账户不存在或已被删除

#### 步骤

| # | 操作 | 验证点 |
|---|------|--------|
| 1 | 修改电商 Demo `config.json`，将 `merchant_did` 设为一个不存在的 DID（如 `did:ignite:z6MkFakeMerchantDID123456789`） | — |
| 2 | 重启电商 Demo：`python server.py` | — |
| 3 | Agent 调用 `POST /orders {"product_id": "coffee"}` | HTTP 402 + x402 挑战（包含伪造的 merchant_did） |
| 4 | Agent 调用 MCP `process_x402_challenge(...)` | MCP 日志显示 `SolanaDidBridge::quick_verify: account not found` |
| 5 | 检查 MCP 返回 | 返回错误：`"支付拒绝：链上未找到商户"` |
| 6 | 确认无后续流程 | 无风控检查、无 DIDComm 消息、无手机通知 |

#### 每步验证点

**MCP 日志关键词：**
```
Quick verify merchant: did=did:ignite:z6MkFakeMerchantDID123456789
Photon getCompressedAccount: null (account not found)
Merchant verification failed: on-chain account not found
Returning error: payment denied (merchant not found on chain)
```

**验证完成后恢复配置：** 将 `config.json` 中的 `merchant_did` 改回正确值并重启。

---

## 4. 故障排查

### 4.1 服务无法启动

| 症状 | 排查方法 |
|------|----------|
| `make health` 显示服务不健康 | `make logs S=<service>` 查看具体错误 |
| PostgreSQL 连接失败 | `docker compose exec postgres pg_isready -U ignite`；检查 `.env` 中密码 |
| Rust 服务编译失败 | `cargo build` 检查依赖；确认 Rust toolchain 版本 |
| 端口冲突 | `netstat -tlnp \| grep <port>` 或 `docker compose ps` 查看端口映射 |

### 4.2 DIDComm 消息未送达

| 症状 | 排查方法 |
|------|----------|
| 手机未收到 `payment-auth-request` | 1. 检查 Mediator WS 连接：手机 App Settings → Connections → 状态应为 Connected |
| | 2. 检查 MCP config `mediator.ws_url` 是否正确 |
| | 3. 检查 MCP config `mediator.phone_did` 是否已填写 |
| | 4. `make logs S=router-user` 查看 DIDComm Router 是否收到并转发了消息 |
| 手机收到消息但无横幅 | 检查 App 是否在前台；FCM 通知是否被系统静默 |
| DIDComm 握手失败 | 确认二维码以 `didcomm://` 开头；检查 Router 日志中的连接错误 |

### 4.3 链上操作失败

| 症状 | 排查方法 |
|------|----------|
| Session Key 注册超时 | 1. 确认 Solana Devnet RPC 可达：`curl https://api.devnet.solana.com` |
| | 2. 确认用户钱包有足够 SOL 支付 Gas |
| | 3. 如使用 `fund_session.py`：`python fund_session.py <session_key_pubkey>` |
| 支付交易失败 | 1. 检查 Session Key 余额：`solana balance <session_key> --url devnet` |
| | 2. 检查 Session Key 是否过期（有效期默认 3600s） |
| | 3. 检查 Spending Limit 是否足够 |
| MagicBlock 通道操作失败 | 1. `mb_get_global_state` 检查 Vault 是否已初始化 |
| | 2. `mb_get_channel <merchant>` 检查通道状态 |
| | 3. 确认 Vault 余额充足 |

### 4.4 x402 挑战解析错误

| 症状 | 排查方法 |
|------|----------|
| MCP 报告 "Failed to parse x402 challenge" | 1. 确认电商 Demo 返回标准 Coinbase x402 格式 |
| | 2. 检查 `PAYMENT-REQUIRED` header 是否为有效 base64 |
| | 3. 检查 `PaymentRequirements` JSON 字段完整性 |
| SPL Token mint 解析失败 | 确认 `x402-payment-address` 格式正确（Solana base58 地址） |

### 4.5 电商 Demo 相关

| 症状 | 排查方法 |
|------|----------|
| 服务器启动失败 | `pip install -r requirements.txt` 确认依赖 |
| 端口 9090 被占用 | `lsof -i :9090` 或修改 `config.json` 中的 bind 地址 |
| 订单验证失败 | 1. 确认 `config.json` 中 `rpc_url` 正确 |
| | 2. 确认支付地址与商户实际地址一致 |
| | 3. 查看 `server.py` 日志中的 `verify-tx` 处理细节 |

### 4.6 运行 Mock 测试快速诊断

在启动完整环境之前，可先用 Mock 测试验证电商 Demo 和 x402 协议层：

```bash
cd ignite-pay-ecom-demo
python server.py &       # 后台启动
python test_flow.py      # 运行 Mock 测试
```

预期输出：
```
=== Health Check ===
[OK] Health check passed: merchant_did=...

=== Product List ===
[OK] Found 3 products

=== Create Order ===
[OK] Order created with 402 status
[OK] PaymentRequirements validated: network=solana-devnet, amount=100000
[OK] PAYMENT-REQUIRED header is valid base64

=== Poll Order ===
[OK] Order status: pending_payment

... (其他测试步骤)
```

如果 Mock 测试通过但完整流程失败，问题在 MCP → DIDComm → 手机 这条链路上。

---

## 5. 测试结果跟踪

复制此表并在测试过程中填写：

| 测试路径 | 覆盖步骤 | 结果 | 备注 | 日期 |
|----------|----------|------|------|------|
| 路径一：自动通过（全局阈值） | 1-7,11,12 | | | |
| 路径一：自动通过（白名单） | 1-7,11,12 | | | |
| 路径二：手机授权 + Session Key | 1-12 | | | |
| 路径二：Session Key 复用 | 1-12 | | | |
| 路径三：手机授权 + MagicBlock Voucher | 1-12 | | | |
| 路径四：黑名单拒绝 | 1-7 | | | |
| 路径五：商户链上验证失败 | 1-6 | | | |

**结果取值**：PASS | FAIL | SKIP | N/A

---

## 6. 与 12 步流程的映射

| 流程步骤 | 路径一 | 路径二 | 路径三 | 路径四 | 路径五 |
|----------|--------|--------|--------|--------|--------|
| 1. Agent 遇到付费墙 | YES | YES | YES | YES | YES |
| 2. Agent 调用 MCP | YES | YES | YES | YES | YES |
| 3. 解析 x402 挑战 | YES | YES | YES | YES | YES |
| 4. 创建支付记录 | YES | YES | YES | YES | YES |
| 5. VC 验证 | YES | YES | YES | YES | YES |
| 6. 链上商户 DID 验证 | YES | YES | YES | YES | **FAIL** |
| 7. 风控决策 | 自动通过 | 需授权 | 需授权 | 黑名单 | — |
| 8. 向手机发送授权请求 | SKIP | YES | YES | SKIP | SKIP |
| 9. 用户审核并授权 | SKIP | YES | YES | SKIP | SKIP |
| 10. 手机发送授权响应 | SKIP | YES | YES | SKIP | SKIP |
| 11. MCP 执行支付 | 自动 | Session Key | MagicBlock | SKIP | SKIP |
| 12. Agent 携带凭证重试 | YES | YES | YES | SKIP | SKIP |
