# Ignite Pay

> **License:** This project is licensed under Business Source License 1.1. The source code is public and free for non-production use. Commercial production use is restricted until January 1, 2031, at which point the license will convert to Apache License 2.0.

**Agent Economy 去中心化支付基础设施** — 让 AI Agent 自主完成支付，人类手机实时授权；让消费者扫码即可完成链上微支付。

> **⚠️ v0.1.0 Beta** — 目前处于 v0.1.0 Beta 阶段，已知部分高并发边界条件待优化。

---

## 核心特色

### 1. Agent 自主支付，人类实时授权

AI Agent 在访问付费资源时遇到 HTTP 402 支付墙，自动通过 MCP (Model Context Protocol) 触发支付流程。金额、商户身份、风控策略自动校验后，通过 DIDComm 端到端加密推送至用户手机——用户一滑即确认，Agent 拿到支付证明继续工作。

```
Agent 遇到 402 → MCP 解析 x402 challenge → 验证商户 DID → 风控决策
    ↓ 自动通过（白名单/小额）                ↓ 需要授权
    → 直接支付                               → 手机推送 → 用户确认
    → Agent 继续访问                         → Agent 继续访问
```

### 2. 扫码微支付

商户生成支付二维码（`ignite://pay?d=<base64url>`），消费者用手机一扫即可完成链上支付。支持 SOL、USDC、USDT 等多种代币，用户可选择 Session Key 秒级链上支付或 MagicBlock 通道即时到账。支付确认后商户手机自动语音播报。

```
商户生成 QR → 消费者扫码 → 选择支付方式 → DIDComm 加密传输
                                          ↓
                                  Session Key / MB 通道 / 钱包直连
                                          ↓
                                  商户手机确认 + 语音播报
```

### 3. MagicBlock 高频支付通道

基于 MagicBlock 并行运行时构建链下支付通道，实现 <50ms 延迟的即时微支付。买家从全局资金池（GlobalVault）签名 Voucher 即可支付，无需每次链上交互。商户批量收集 Voucher 后构建 Sum-Merkle Tree 一次性链上结算，极大降低 gas 成本。

三层安全防线：
- **链上**：Spending Cap 限制单通道额度，资金锁定在 PDA 账户
- **ER 层**：MagicBlock 实时状态验证，gas-free 高速处理
- **链下**：挑战窗口期争议机制，Sum-Merkle Proof 欺诈证明

```
买家签名 Voucher (Ed25519)    →    商户收集 Vouchers
       ↓                              ↓
SHA256(channel ‖ seq ‖ amount)    构建 Sum-Merkle Tree
       ↓                              ↓
MagicBlock ER 即时记录 (<50ms)   双签提交链上结算
```

### 4. x402 标准协议

兼容 [Coinbase x402](https://github.com/coinbase/x402) HTTP 402 支付协议。任何支持 x402 的服务都可以接入——Agent 无需专用 SDK，只需处理 HTTP 402 响应中的支付要求，调用 MCP 完成支付后重试请求。

### 5. 多路径支付引擎

根据场景自动选择最优支付路径：

| 支付方式 | 延迟 | Gas | 适用场景 |
|----------|------|-----|----------|
| **MagicBlock 通道** | <50ms | 免费 | 高频微支付，扫码/Agent 重复消费 |
| **Session Key** | ~400ms | 正常 | 链上直接支付，临时密钥授权 |
| **钱包直连** | ~400ms | 正常 | Phantom/Solflare deep link，MCP 不接触私钥 |
| **Relayer 代付** | ~400ms | 代付 | 用户无 gas 的赞助支付模式 |
| **CCTP 跨链充值** | 10-30min | 源链 gas | EVM → Solana USDC 跨链充值 (Circle CCTP V2 Forwarding) |

### 6. CCTP 跨链 USDC 充值

基于 [Circle CCTP V2 Forwarding](https://developers.circle.com/stablecoins/docs/cctp-forwarding) 协议，买家手机应用支持一键将 USDC 从 EVM 链（Ethereum / Base / Arbitrum / OP）跨链转移到 Solana 钱包。用户通过 MetaMask 完成链上操作（approve + depositForBurnWithHook），Circle 自动在 Solana 上 mint 等额 USDC 到目标 ATA。

```
用户选源链 + 输入金额 + Solana 地址
       ↓
Rust 层: 查询 Iris 手续费 + 推导 Solana ATA + ABI 编码 calldata
       ↓
MetaMask: approve USDC → TokenMessengerV2 → depositForBurnWithHook
       ↓
Circle Iris: 验证 → Attestation → Solana 上 mint USDC
       ↓
App 轮询确认到账 → 展示 Solana tx hash + Solscan 链接
```

详见 [docs/cctp-cross-chain-deposit.md](docs/cctp-cross-chain-deposit.md)。

### 7. DIDComm v2 端到端加密

Agent 与手机之间的所有通信通过 DIDComm v2 协议加密（JWE authcrypt），中继服务器无法读取明文。基于 Ed25519 签名 + X25519 密钥协商，DID 标识符格式 `did:ignite:z<multicodec>`。

### 8. PDA 链上身份

商户 DID 通过 PDA 账户注册到 Solana 链上，标准 Solana RPC 即可读写，无需额外基础设施。支持平台 VC（Verifiable Credential）签发 + 链上注册 + 链上签名验证。

### 9. 六级风控

| 优先级 | 策略 | 行为 |
|--------|------|------|
| 1 | 黑名单 | 直接拒绝 |
| 2 | IPFS CID 黑名单 | 拉取列表后拒绝 |
| 3 | 单笔限额 | 超额需授权 |
| 4 | 白名单自动通过 | 无需手机确认 |
| 5 | IPFS CID 白名单 | 拉取列表后自动通过 |
| 6 | 默认 | 推送手机授权 |

---

## 系统架构

```
┌─────────────────────────────────────────────────────────┐
│                    Application Layer                     │
│  AI Agent (MCP Client)  │  Buyer App  │  Merchant App   │
└──────────────────────────┬──────────────────────────────┘
                           │ MCP (JSON-RPC 2.0)
┌──────────────────────────▼──────────────────────────────┐
│                    Service Layer                          │
│  Buyer MCP  │  Merchant MCP  │ Hub                    │
└──────────────────────────┬──────────────────────────────┘
                           │ DIDComm v2 (JWE authcrypt)
┌──────────────────────────▼──────────────────────────────┐
│                 Communication Layer                       │
│          DIDComm Mediator (Router)                        │
└──────────────────────────┬──────────────────────────────┘
                           │
┌──────────────────────────▼──────────────────────────────┐
│                   On-chain Layer                          │
│  DID (PDA) │ Session Key │ MB Channel                    │
│  Solana + MagicBlock                                     │
└─────────────────────────────────────────────────────────┘
```

---

## 端到端支付流程

### Agent x402 支付

```
Agent                 Buyer MCP              Merchant MCP          Phone
  │  HTTP GET /paid-content                   │                    │
  │◄─ HTTP 402 + x402 challenge ──────────────│                    │
  │  call process_x402_challenge              │                    │
  ├───────────────────────────────────────────►│                    │
  │                        解析 challenge      │                    │
  │                        验证商户 VC + DID   │                    │
  │                        风控决策            │                    │
  │                             ├─ 自动通过    │                    │
  │                             └─ 需要授权 ──►│── DIDComm JWE ────►│
  │                                            │   用户确认支付     │
  │                                            │◄─ 授权结果 ────────│
  │                        执行支付            │                    │
  │                        (MB Voucher / Session Key / Wallet)     │
  │◄─ payment proof ──────────────────────────│                    │
  │  HTTP GET /paid-content (with proof)       │                    │
  │◄─ HTTP 200 + content ─────────────────────│                    │
```

### 商户 QR 支付

```
Merchant App       Merchant MCP         Buyer MCP          Buyer App
     │  生成支付 QR   │                    │                    │
     │◄───────────────│                    │                    │
     │  展示 QR       │                    │  扫码              │
     │                │                    │◄───────────────────│
     │                │  DIDComm: payment  │                    │
     │                │◄───────────────────│                    │
     │                │  验证 + 确认订单    │                    │
     │  语音播报 + 确认│                    │                    │
     │◄───────────────│                    │                    │
```

---

## 项目结构

```
ignite-pay/
├── ignite-pay-core/              # 共享基础库 (DID, DIDComm, VC, 审计)
├── ignite-pay-solana/            # Solana RPC 集成 (支付, PDA DID, Session Key)
├── ignite-pay-state-channel/     # 链下 UTXO Merkle Tree 状态通道引擎（暂未启用）
│
├── ignite-pay-did-program/       # 商户 DID 链上程序 (PDA, 6 instructions)
├── ignite-pay-session-program/   # Session Key 链上程序 (Anchor, 4 instructions)
├── ignite-pay-mb/                # MagicBlock 支付通道
│   ├── programs/                 #   链上程序 (Anchor, 10 instructions)
│   └── sdk/                      #   Rust SDK (PDA, Merkle Tree, 签名, 交易构建)
│
├── didcomm-router/               # DIDComm 消息中继服务
├── did-registry/                 # DID 链上注册 REST 服务
├── ignite-pay-channel-service/   # 状态通道 HTTP+WS 服务（暂未启用）
├── ignite-pay-program/           # 状态通道链上程序（暂未启用）
├── ignite-pay-hub-registry/      # Hub 注册发现服务 (PostgreSQL)
├── ignite-pay-relayer/           # 赞助支付 Gas 代付服务
│
├── ignite-pay-mcp/               # 买家 MCP 服务器 (23 tools)
├── ignite-pay-merchant-mcp/      # 商户 MCP 服务器 (14 tools)
├── ignite-pay-skill/             # Python SDK (PyO3 bindings)
│
├── ignite_pay_app/               # 买家手机应用 (Flutter + Rust Bridge)
├── ignite_pay_merchant_app/      # 商户手机应用 (Flutter + Rust Bridge)
├── ignite-pay-ecom-demo/         # x402 电商演示服务器 (Python FastAPI)
│
├── tests/                        # 链上程序测试 (litesvm + mollusk)
├── docs/                         # 设计文档 + 业务流程
└── deploy/                       # Docker 部署配置
```

---

## MCP 工具一览

### 买家 MCP (`ignite-pay-mcp`) — 23 Tools

| 工具 | 说明 |
|------|------|
| `process_x402_challenge` | 完整 x402 支付流程 (解析→验证→风控→授权→支付) |
| `check_authorization` | 查询支付状态 |
| `get_payment_history` | 支付历史 |
| `get_identity` | 查看 DID、Mediator、Solana、MB 状态 |
| `generate_pairing_invitation` | 生成 DIDComm 配对二维码 |
| `create_session` | 创建 Session Key (SOL/SPL Token) |
| `get_session_status` | 查询 Session Key 状态 |
| `close_session` | 关闭 Session Key |
| `execute_spl_payment` | SPL Token 链上支付 |
| `add_merchant` | 添加商户 DID |
| `update_merchant` | 更新商户 DID 数据 |
| `verify_merchant` | 验证商户链上身份 |
| `mb_init_global` ~ `mb_withdraw` | MagicBlock 支付通道全套操作 (11 tools) |

### 商户 MCP (`ignite-pay-merchant-mcp`) — 16 Tools

| 工具 | 说明 |
|------|------|
| `list_products` | 返回产品目录 (Agent 查询) |
| `create_order` | 创建订单，返回 x402 challenge |
| `verify_payment` | 验证支付证明 (链上 tx / MB Voucher) |
| `generate_payment_qr` | 生成支付二维码 |
| `check_payment` | 查询订单状态 |
| `get_payment_history` | 订单历史 |
| `get_identity` | 商户 DID、MB Pubkey |
| `register_merchant` | 注册链上身份 (VC + PDA DID) |
| `verify_merchant_did` | 验证链上 DID |
| `mb_get_channel` ~ `mb_force_release` | MagicBlock 支付通道商户端操作 (7 tools) |

---

## MagicBlock 支付通道

> MagicBlock 支付通道已进入可用阶段，是当前微支付的主推方案。

三层架构实现高频微支付：

```
┌─────────────────────────────────────┐
│  L1 (Solana)                        │
│  通道创建 / 资金锁定 / 签名验证 / 结算 │
├─────────────────────────────────────┤
│  ER (MagicBlock)                    │
│  高速状态转换 (<50ms) / gas-free     │
│  实时记录每个 Voucher                │
├─────────────────────────────────────┤
│  Off-chain                          │
│  挑战窗口争议 / Merkle Proof 欺诈证明 │
└─────────────────────────────────────┘
```

- **Voucher**: `SHA256(channel_id ‖ seq ‖ amount)` + Ed25519 签名
- **结算**: 构建 Sum-Merkle Tree，买卖双方双签提交链上
- **争议**: 挑战期内可发起争议，提交 Merkle Proof 反欺诈
- **稳定币**: 原生支持 SOL / USDC / USDT

---

## 技术栈

| 层 | 技术 |
|----|------|
| 链上 | Solana (Anchor), MagicBlock |
| 身份 | `did:ignite` 方法, Ed25519/X25519, JWE authcrypt |
| 通信 | DIDComm v2, MCP (JSON-RPC 2.0), x402 HTTP 402 |
| 后端 | Rust, Axum 0.8, tokio, sled, reqwest |
| 手机 | Flutter + Rust Bridge (flutter_rust_bridge) |
| SDK | Rust, Python (PyO3) |
| 部署 | Docker Compose, nginx, PostgreSQL |

---

## 快速开始

### 前置条件

- Rust 1.80+ (MSRV)
- Solana CLI 2.x
- Anchor Framework 0.30+ / 1.0+
- Flutter 3.x (手机应用)

### 编译

```bash
# 编译全部 Rust crate
cargo build

# 编译买家 MCP
cargo build -p ignite-pay-mcp

# 编译商户 MCP
cargo build -p ignite-pay-merchant-mcp

# 编译 MagicBlock SDK
cargo build -p ignite-pay-mb-sdk

# 编译链上程序 (需要 Solana toolchain)
cd ignite-pay-mb && anchor build
```

### 运行测试

```bash
# 全部测试
cargo test

# 单个 crate
cargo test -p ignite-pay-merchant-mcp
cargo test -p ignite-pay-mb-sdk

# 链上程序测试 (需要 local-validator 或 svm)
cd tests/svm-litesvm && cargo test
```

### Docker 部署

```bash
# 启动全部服务 (PostgreSQL + DIDComm Router + DID Registry + Hub)
docker-compose up -d
```

### 运行 MCP 服务器

```bash
# 买家 MCP (stdio 模式，供 Claude Desktop / Cursor 等调用)
cd ignite-pay-mcp
cp config.toml.example config.toml  # 编辑配置
cargo run

# 商户 MCP (stdio + SSE 模式)
cd ignite-pay-merchant-mcp
cp config.toml.example config.toml  # 编辑配置
cargo run
```

### x402 演示

```bash
# 启动电商演示服务器
cd ignite-pay-ecom-demo
pip install fastapi uvicorn solana-py
python server.py
```

---

## 文档

| 文档 | 说明 |
|------|------|
| [AGENTS.md](AGENTS.md) | 完整 crate 级架构文档 (中文) |
| [docs/agent-payment-flow.md](docs/agent-payment-flow.md) | Agent x402 支付流程 |
| [docs/business-flows.md](docs/business-flows.md) | 全部业务流程 (18 条) |
| [docs/ignite-pay-magicblock.md](docs/ignite-pay-magicblock.md) | MagicBlock 支付通道设计 |
| [docs/session-key-payment-flow.md](docs/session-key-payment-flow.md) | Session Key 支付流程 |
| [docs/direct-wallet-payment-flow.md](docs/direct-wallet-payment-flow.md) | 钱包直连支付流程 |
| [docs/sponsored-relayer-payment-flow.md](docs/sponsored-relayer-payment-flow.md) | 赞助支付流程 |
| [docs/cctp-cross-chain-deposit.md](docs/cctp-cross-chain-deposit.md) | CCTP Forwarding EVM→Solana 跨链 USDC 充值 |

---

## License

Private / Proprietary
