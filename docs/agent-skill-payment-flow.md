# Agent Skill 支付流程

> **相关文档：** MCP 内部的完整支付流程（12 步时序图、风控决策、链上验证细节等）请参考 [agent-payment-flow.md](./agent-payment-flow.md)。本文档聚焦 skill 层如何通过 REST API 调用 MCP。

## 1. 系统架构

### 1.1 组件关系

```
┌──────────┐     HTTP 402      ┌──────────────┐
│  OpenClaw │ ◄──────────────── │   商家服务     │
│  (Agent)  │ ──────────────►  │  (Merchant)   │
└─────┬─────┘   重试 + Proof    └──────────────┘
      │
      │ 调用 skill
      ▼
┌──────────────────┐
│ ignite-pay-skill │  ← 薄客户端：HTTP API 调用 + 本地风控
│   (Python SDK)   │
└──────┬───────────┘
       │ POST /api/x402
       ▼
┌──────────────────┐    DIDComm     ┌────────────────┐    推送     ┌──────────┐
│  ignite-pay-mcp  │ ─────────────► │ DIDComm Router │ ──────────► │ 手机 App  │
│  (支付编排器)      │ ◄───────────── │  (Mediator)    │ ◄────────── │ (用户授权) │
└──────┬───────────┘    授权响应      └────────────────┘    确认     └──────────┘
       │
       │ 链上支付
       ▼
┌──────────────┐
│ Solana 链上   │
│ (Session Key  │
│  / MB Voucher)│
└──────────────┘
```

### 1.2 组件职责

| 组件 | 角色 | 职责 |
|------|------|------|
| **ignite-pay-mcp** | 支付编排器 | 解析 x402、验证商户 DID、风控决策、DIDComm 推送手机、等待授权、执行链上支付 |
| **ignite-pay-skill** | 薄客户端 | HTTP API 调用 MCP、本地白名单/黑名单查询（可选） |
| **OpenClaw** | AI Agent | 业务请求、捕获 402、调用 skill、带 Proof 重试 |
| **手机 App** | 授权终端 | 接收支付授权请求、用户确认/拒绝、注册 Session Key |

---

## 2. 完整 7 步流程

### Step 1: OpenClaw 执行业务请求，捕获 HTTP 402

OpenClaw 向商家服务发送业务请求（如 API 调用）。商家返回 HTTP 402，响应体包含 x402 支付信息。

```
POST /api/data HTTP/1.1
Host: merchant.example.com

→ HTTP/1.1 402 Payment Required
  Content-Type: application/json
  X-Payment-Version: x402-v1

{
  "scheme": "exact",
  "network": "solana:devnet",
  "amount": "1000000",
  "asset": "USDC",
  "payTo": "MerchantSolanaAddress..."
}
```

### Step 2: OpenClaw 调用本地 skill 的 `process_x402()`

OpenClaw 检测到 402 响应，将响应体和 headers 传递给 skill：

```python
from ignite_pay_rs import IgnitePaySkill

skill = IgnitePaySkill(mcp_url="http://127.0.0.1:9001")

result = skill.process_x402(
    challenge_body=response_body,
    x402_merchant_did=response_headers.get("x402-merchant-did"),
    x402_payment_address=response_headers.get("x402-payment-address"),
)
```

### Step 3: skill 调用 MCP 的 `POST /api/x402`

skill 通过 HTTP POST 将结构化请求发送给 MCP 的 REST API：

```http
POST http://127.0.0.1:9001/api/x402
Content-Type: application/json

{
  "challenge_body": "{\"scheme\":\"exact\",\"network\":\"solana:devnet\",\"amount\":\"1000000\",\"asset\":\"USDC\",\"payTo\":\"...\"}",
  "phone_did": "",
  "x402_merchant_did": "did:ignite:z...",
  "x402_payment_address": "SolanaAddress..."
}
```

### Step 4: MCP 内部处理

MCP 执行完整的支付编排：

1. **解析 x402**：支持 Coinbase x402 标准格式和 legacy accepts 数组格式
2. **验证商户**：on-chain DID 验证（如已配置 Solana）
3. **风控决策**：
   - 黑名单 → 直接拒绝
   - 白名单 → 自动批准（无需手机确认）
   - 全局阈值（`auto_approve_max`）→ 自动批准
   - 其他 → 需要手机授权
4. **DIDComm 推送手机**：通过 Mediator WebSocket 将授权请求推送到手机 App
5. **等待授权**：阻塞等待手机响应（超时 `auth_timeout` 秒）
6. **执行支付**：
   - MagicBlock voucher（off-chain）→ 即时签名
   - Session Key → on-chain Solana 交易
   - Relayer → 赞助 gas 的链上交易

### Step 5: MCP 返回结构化 JSON

```json
{
  "status": "success",
  "payment_id": "uuid-xxx",
  "proof": {
    "type": "tx_signature",
    "signature": "5Kj...base58"
  },
  "amount": 1000000,
  "token": "USDC",
  "recipient": "MerchantSolanaAddress...",
  "merchant_did": "did:ignite:z...",
  "method": "session_key"
}
```

或拒绝：
```json
{
  "status": "rejected",
  "payment_id": "uuid-xxx",
  "reason": "Rejected by user"
}
```

### Step 6: skill 原样返回给 OpenClaw

skill 不做任何处理，直接将 MCP 返回的 JSON dict 返回给 OpenClaw。

### Step 7: OpenClaw 带 `X-Payment-Proof` header 重试

OpenClaw 将支付凭证附加到原始请求的 header 中重试：

```http
POST /api/data HTTP/1.1
Host: merchant.example.com
X-Payment-Version: x402-v1
X-Payment-Proof: {"type":"tx_signature","signature":"5Kj...base58"}
X-Payment-Amount: 1000000
X-Payment-Asset: USDC

→ HTTP/1.1 200 OK
```

---

## 3. API 接口规范

### 3.1 `POST /api/x402`

**请求体：**

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `challenge_body` | string | 是 | HTTP 402 响应体（JSON 字符串） |
| `phone_did` | string | 否 | 手机 DID（留空则使用已配对的手机） |
| `x402_merchant_did` | string | 否 | 商户 DID（覆盖 body 中的值） |
| `x402_payment_address` | string | 否 | 支付地址（覆盖 body 中的值） |
| `x402_merkle_context` | string | 否 | Merkle 上下文 |
| `vc_ipfs_cid` | string | 否 | IPFS CID 用于 VC 验证 |

### 3.2 响应格式

**成功 — HTTP 200：**

```json
{
  "status": "success",
  "payment_id": "uuid-string",
  "proof": { ... },
  "amount": 1000000,
  "token": "USDC",
  "recipient": "SolanaAddress...",
  "merchant_did": "did:ignite:z...",
  "method": "session_key"
}
```

**拒绝 — HTTP 402：**

```json
{
  "status": "rejected",
  "payment_id": "uuid-string",
  "reason": "描述文本"
}
```

**错误 — HTTP 400：**

```json
{
  "status": "error",
  "payment_id": "uuid-string-or-null",
  "message": "错误描述"
}
```

### 3.3 支付凭证格式

**链上交易签名：**

```json
{
  "type": "tx_signature",
  "signature": "5Kj8n...(base58 编码的交易签名)"
}
```

**MagicBlock Voucher（off-chain）：**

```json
{
  "type": "voucher",
  "channel": "ChannelPDA...",
  "seq": 1,
  "amount": 1000000,
  "msg_hash": "base58...",
  "signature": "base58..."
}
```

---

## 4. 配置说明

### 4.1 MCP `config.toml`

```toml
[mcp]
sse_port = 9001          # REST API 和 MCP SSE 共用端口

[mediator]
ws_url = "wss://mediator.ignite.com"
phone_did = "did:ignite:z..."

[policy]
auto_approve_max = 1000000   # 自动批准阈值（lamports），0 = 禁用
auth_timeout = 300            # 手机授权超时（秒）
```

### 4.2 Skill 初始化参数

```python
# MCP API 模式（推荐）
skill = IgnitePaySkill(mcp_url="http://127.0.0.1:9001")

# 本地模式（遗留）
skill = IgnitePaySkill(mediator_url="wss://mediator.ignite.com", db_path="./data")
```

### 4.3 超时设置

| 场景 | 默认值 | 说明 |
|------|--------|------|
| MCP auth_timeout | 300s | 等待手机授权响应 |
| Skill httpx timeout | 310s | 覆盖 MCP 的 300s + 网络开销 |
| Session fund timeout | 60s | 等待手机充值 Session Key |

---

## 5. 部署指南

### 5.1 启动服务顺序

```bash
# 1. 启动 DIDComm Mediator（如使用远程服务则跳过）
# 2. 启动 MCP 服务（加载 config.toml）
cd ignite-pay-mcp
cargo run -- -c config.toml

# 日志输出：
# MCP SSE server listening on http://0.0.0.0:9001/mcp
# REST API available at http://0.0.0.0:9001/api/x402
```

### 5.2 验证 REST API

```bash
# 测试无效请求 → 应返回 400
curl -X POST http://localhost:9001/api/x402 \
  -H "Content-Type: application/json" \
  -d '{"challenge_body":"invalid"}'

# 预期响应：
# HTTP/1.1 400 Bad Request
# {"status":"error","payment_id":null,"message":"Invalid JSON in challenge body: ..."}
```

### 5.3 OpenClaw Skill 注册

```python
from ignite_pay_rs import IgnitePaySkill

# 在 OpenClaw agent 初始化时注册
payment_skill = IgnitePaySkill(mcp_url="http://127.0.0.1:9001")

# 在业务请求捕获 402 后调用
def handle_402_response(response):
    result = payment_skill.process_x402(
        challenge_body=response.text,
        x402_merchant_did=response.headers.get("x402-merchant-did"),
        x402_payment_address=response.headers.get("x402-payment-address"),
    )

    if result["status"] == "success":
        # 带凭证重试原始请求
        headers = {
            "X-Payment-Proof": json.dumps(result["proof"]),
            "X-Payment-Amount": str(result["amount"]),
        }
        return retry_original_request(headers)
    else:
        # 处理拒绝或错误
        return handle_payment_failure(result)
```

### 5.4 端到端验证步骤

1. 启动 MCP 服务（`sse_port=9001`）
2. 使用手机 App 配对 DIDComm 连接
3. 发送包含 x402 body 的 curl 请求到 `POST /api/x402`
4. 手机 App 收到授权推送 → 确认
5. 收到 `{"status": "success", ...}` 响应
6. 使用返回的 proof 重试商家请求
