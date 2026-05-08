# Agent Skill Payment Flow

> **Related:** For the full MCP internal payment flow (12-step sequence diagram, risk control decisions, on-chain verification details, etc.), see [agent-payment-flow.md](./agent-payment-flow.md). This document focuses on how the skill layer calls MCP via REST API.

## 1. System Architecture

### 1.1 Component Diagram

```
┌──────────┐     HTTP 402      ┌──────────────┐
│  OpenClaw │ ◄──────────────── │   Merchant    │
│  (Agent)  │ ──────────────►  │   Service     │
└─────┬─────┘   retry + Proof   └──────────────┘
      │
      │ invoke skill
      ▼
┌──────────────────┐
│ ignite-pay-skill │  ← Thin client: HTTP API calls + local risk control
│   (Python SDK)   │
└──────┬───────────┘
       │ POST /api/x402
       ▼
┌──────────────────┐    DIDComm     ┌────────────────┐    push     ┌──────────┐
│  ignite-pay-mcp  │ ─────────────► │ DIDComm Router │ ──────────► │ Phone App │
│  (Payment        │ ◄───────────── │  (Mediator)    │ ◄────────── │ (User     │
│   Orchestrator)  │  auth response └────────────────┘  confirm    │  Authorize)│
└──────┬───────────┘                                           └──────────┘
       │
       │ on-chain payment
       ▼
┌──────────────┐
│ Solana Chain │
│ (Session Key │
│  / MB Voucher)│
└──────────────┘
```

### 1.2 Component Responsibilities

| Component | Role | Responsibilities |
|-----------|------|-----------------|
| **ignite-pay-mcp** | Payment Orchestrator | Parse x402, verify merchant DID, risk control decisions, DIDComm push to phone, wait for authorization, execute on-chain payment |
| **ignite-pay-skill** | Thin Client | HTTP API calls to MCP, local whitelist/blacklist queries (optional) |
| **OpenClaw** | AI Agent | Business requests, capture 402, invoke skill, retry with Proof |
| **Phone App** | Authorization Terminal | Receive payment authorization requests, user confirm/reject, register Session Key |

---

## 2. Complete 7-Step Flow

### Step 1: OpenClaw sends business request, captures HTTP 402

OpenClaw sends a business request to the merchant service (e.g. an API call). The merchant returns HTTP 402 with an x402 payment payload in the response body.

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

### Step 2: OpenClaw invokes the local skill's `process_x402()`

OpenClaw detects the 402 response and passes the response body and headers to the skill:

```python
from ignite_pay_rs import IgnitePaySkill

skill = IgnitePaySkill(mcp_url="http://127.0.0.1:9001")

result = skill.process_x402(
    challenge_body=response_body,
    x402_merchant_did=response_headers.get("x402-merchant-did"),
    x402_payment_address=response_headers.get("x402-payment-address"),
)
```

### Step 3: Skill calls MCP's `POST /api/x402`

The skill sends a structured HTTP POST request to the MCP REST API:

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

### Step 4: MCP internal processing

The MCP executes the full payment orchestration:

1. **Parse x402**: Supports Coinbase x402 standard format and legacy accepts array format
2. **Verify merchant**: On-chain DID verification (if Solana is configured)
3. **Risk control decision**:
   - Blacklist → reject immediately
   - Whitelist → auto-approve (no phone confirmation needed)
   - Global threshold (`auto_approve_max`) → auto-approve
   - Otherwise → phone authorization required
4. **DIDComm push to phone**: Send authorization request to the phone App via Mediator WebSocket
5. **Wait for authorization**: Block until phone responds (timeout: `auth_timeout` seconds)
6. **Execute payment**:
   - MagicBlock voucher (off-chain) → instant signing
   - Session Key → on-chain Solana transaction
   - Relayer → gas-sponsored on-chain transaction

### Step 5: MCP returns structured JSON

**Success response:**
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

**Rejection response:**
```json
{
  "status": "rejected",
  "payment_id": "uuid-xxx",
  "reason": "Rejected by user"
}
```

### Step 6: Skill passes result back to OpenClaw

The skill performs no additional processing — it returns the MCP JSON response dict directly to OpenClaw.

### Step 7: OpenClaw retries with `X-Payment-Proof` header

OpenClaw attaches the payment proof to the original request headers and retries:

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

## 3. API Specification

### 3.1 `POST /api/x402`

**Request body:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `challenge_body` | string | Yes | HTTP 402 response body (JSON string) |
| `phone_did` | string | No | Phone DID (leave empty to use paired phone) |
| `x402_merchant_did` | string | No | Merchant DID (overrides value from body) |
| `x402_payment_address` | string | No | Payment address (overrides value from body) |
| `x402_merkle_context` | string | No | Merkle context |
| `vc_ipfs_cid` | string | No | IPFS CID for VC verification |

### 3.2 Response Formats

**Success — HTTP 200:**

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

**Rejected — HTTP 402:**

```json
{
  "status": "rejected",
  "payment_id": "uuid-string",
  "reason": "Description text"
}
```

**Error — HTTP 400:**

```json
{
  "status": "error",
  "payment_id": "uuid-string-or-null",
  "message": "Error description"
}
```

### 3.3 Payment Proof Formats

**On-chain transaction signature:**

```json
{
  "type": "tx_signature",
  "signature": "5Kj8n...(base58-encoded transaction signature)"
}
```

**MagicBlock Voucher (off-chain):**

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

## 4. Configuration

### 4.1 MCP `config.toml`

```toml
[mcp]
sse_port = 9001          # Shared port for REST API and MCP SSE

[mediator]
ws_url = "wss://mediator.ignite.com"
phone_did = "did:ignite:z..."

[policy]
auto_approve_max = 1000000   # Auto-approve threshold (lamports), 0 = disabled
auth_timeout = 300            # Phone authorization timeout (seconds)
```

### 4.2 Skill Initialization

```python
# MCP API mode (recommended)
skill = IgnitePaySkill(mcp_url="http://127.0.0.1:9001")

# Local mode (legacy)
skill = IgnitePaySkill(mediator_url="wss://mediator.ignite.com", db_path="./data")
```

### 4.3 Timeout Settings

| Scenario | Default | Description |
|----------|---------|-------------|
| MCP auth_timeout | 300s | Wait for phone authorization response |
| Skill httpx timeout | 310s | Covers MCP's 300s + network overhead |
| Session fund timeout | 60s | Wait for phone to fund Session Key |

---

## 5. Deployment Guide

### 5.1 Service Startup Order

```bash
# 1. Start DIDComm Mediator (skip if using a remote service)
# 2. Start MCP service (loads config.toml)
cd ignite-pay-mcp
cargo run -- -c config.toml

# Log output:
# MCP SSE server listening on http://0.0.0.0:9001/mcp
# REST API available at http://0.0.0.0:9001/api/x402
```

### 5.2 Verify REST API

```bash
# Test with invalid request → should return 400
curl -X POST http://localhost:9001/api/x402 \
  -H "Content-Type: application/json" \
  -d '{"challenge_body":"invalid"}'

# Expected response:
# HTTP/1.1 400 Bad Request
# {"status":"error","payment_id":null,"message":"Invalid JSON in challenge body: ..."}
```

### 5.3 OpenClaw Skill Registration

```python
from ignite_pay_rs import IgnitePaySkill

# Register during OpenClaw agent initialization
payment_skill = IgnitePaySkill(mcp_url="http://127.0.0.1:9001")

# Invoke when a business request captures 402
def handle_402_response(response):
    result = payment_skill.process_x402(
        challenge_body=response.text,
        x402_merchant_did=response.headers.get("x402-merchant-did"),
        x402_payment_address=response.headers.get("x402-payment-address"),
    )

    if result["status"] == "success":
        # Retry original request with proof
        headers = {
            "X-Payment-Proof": json.dumps(result["proof"]),
            "X-Payment-Amount": str(result["amount"]),
        }
        return retry_original_request(headers)
    else:
        # Handle rejection or error
        return handle_payment_failure(result)
```

### 5.4 End-to-End Verification Steps

1. Start MCP service (`sse_port=9001`)
2. Pair phone App via DIDComm connection
3. Send a curl request with x402 body to `POST /api/x402`
4. Phone App receives authorization push → confirm
5. Receive `{"status": "success", ...}` response
6. Retry merchant request using the returned proof
