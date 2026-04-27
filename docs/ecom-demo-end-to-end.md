# E-Commerce Demo: End-to-End x402 Payment Flow

## Overview

The `ignite-pay-ecom-demo` server is a minimal FastAPI e-commerce application that returns x402 HTTP 402 payment challenges. It enables full end-to-end testing of the ignite-pay payment flow:

```
AI Agent → x402 Challenge → MCP Authorization → Phone Approval →
Session Key Creation → Payment Execution → Order Confirmation
```

## Architecture

```
┌──────────┐     HTTP      ┌──────────────┐    402     ┌──────────────┐
│ AI Agent │ ─────────────→ │ E-Commerce   │ ────────→ │ ignite-pay   │
│          │ ←───────────── │ Demo Server  │           │ MCP          │
└──────────┘   Products,    └──────────────┘           └──────┬───────┘
               Orders                                     DIDComm
                                                          │
                                               ┌──────────▼──────────┐
                                               │ didcomm-router      │
                                               │ (mediator)          │
                                               └──────────┬──────────┘
                                                          │
                                               ┌──────────▼──────────┐
                                               │ Phone App           │
                                               │ (Ignite Sentinel)   │
                                               │ - Approve payment   │
                                               │ - Create session key│
                                               │ - Register on-chain │
                                               └─────────────────────┘
```

## Components

| Component | Port | Purpose |
|-----------|------|---------|
| E-Commerce Demo | 9090 | Returns x402 402 challenges for unpaid orders |
| ignite-pay-mcp | 3000 | Processes x402 challenges, orchestrates payment |
| didcomm-router | 8082 | Routes DIDComm messages between MCP and phone |
| Phone App | — | Receives payment requests, approves, creates session keys |
| Solana Devnet | — | Settlement layer for SOL transfers |

## Setup

### 1. Configure the E-Commerce Server

Edit `ignite-pay-ecom-demo/config.json`:

```json
{
  "merchant": {
    "did": "did:ignite:z...",
    "payment_address": "<your_solana_wallet_base58>"
  },
  "solana": {
    "network": "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1",
    "asset_native": "So11111111111111111111111111111111111111112"
  },
  "x402": {
    "scheme": "exact",
    "maxTimeoutSeconds": 60
  }
}
```

Get the merchant DID and payment address from the merchant MCP:

```bash
# Call get_identity on the merchant MCP to get the DID
```

### 2. Install Dependencies

```bash
cd ignite-pay-ecom-demo
pip install -r requirements.txt
```

### 3. Start Backend Services

```powershell
.\deploy-local.ps1 start
```

This starts the didcomm-router, hub-registry, ignite-pay-mcp, and merchant MCP.

### 4. Start the E-Commerce Server

```bash
cd ignite-pay-ecom-demo
python server.py
```

### 5. Pair Phone with MCP

Open the Ignite Sentinel phone app and scan the pairing QR code from ignite-pay-mcp.

### 6. Run the Standalone Mock Test

```bash
python test_flow.py
```

This verifies the e-commerce server is working without needing the full payment flow.

## End-to-End Flow

### Step-by-Step

```
 1. AI Agent → GET /products
    → Returns product list with prices in lamports

 2. AI Agent → POST /orders {"product_id": "coffee"}
    → Returns HTTP 402 with x402 payment challenge

 3. AI Agent → ignite-pay-mcp: process_x402_challenge(challenge_body, headers)
    → MCP parses the 402 response and creates a PaymentRequest

 4. MCP → DIDComm → didcomm-router → Phone App: payment-auth-request
    → Payment request sent to user's phone

 5. Phone App: User reviews and approves payment

 6. Phone App: Creates ephemeral session key, registers on-chain

 7. Fund the session key (devnet airdrop):
    python fund_session.py <session_pubkey_base58>

 8. Phone App → DIDComm → didcomm-router → MCP: payment-auth-response
    → Response includes session key for payment execution

 9. MCP: Executes SOL transfer via session key on Solana devnet

10. AI Agent → POST /orders {"product_id": "coffee"}
    Header: X-Payment-Proof: <tx_signature>
    → Server verifies on-chain payment, returns order confirmation

11. GET /orders/{order_id} to poll/confirm final status
```

### 402 Challenge Format (Coinbase x402 Standard)

The e-commerce server returns the [Coinbase x402](https://github.com/coinbase/x402) standard `PaymentRequirements` structure. The response body is the PaymentRequirements JSON, and the `PAYMENT-REQUIRED` header contains the same JSON base64-encoded per the x402 specification.

```json
{
  "scheme": "exact",
  "network": "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1",
  "maxTimeoutSeconds": 60,
  "amount": "100000",
  "asset": "So11111111111111111111111111111111111111112",
  "payTo": "<merchant_payment_address>",
  "extra": {
    "memo": "did:ignite:z..."
  }
}
```

Response headers:

| Header | Value |
|--------|-------|
| `PAYMENT-REQUIRED` | Base64-encoded PaymentRequirements JSON (Coinbase x402 standard) |
| `x402-merchant-did` | Merchant's DID identifier (Ignite-specific) |
| `x402-payment-address` | Merchant's Solana wallet address (Ignite-specific) |
| `x402-order-id` | Unique order identifier (Ignite-specific) |

The MCP parser supports both the standard Coinbase x402 format (detected by presence of `scheme` field) and the legacy `accepts` array format for backward compatibility.

### Payment Verification

The server verifies payments by calling Solana RPC `getTransaction` and checking:

1. Transaction exists and is confirmed
2. Transaction has no error
3. A balance increase >= expected amount occurred at the recipient address

## API Reference

### `GET /health`

Health check endpoint.

**Response:**
```json
{
  "status": "ok",
  "merchant_did": "did:ignite:z...",
  "merchant_name": "Ignite Demo Store"
}
```

### `GET /products`

List available products.

**Response:**
```json
{
  "products": [
    {
      "id": "coffee",
      "name": "Premium Coffee",
      "price_lamports": 100000,
      "price_sol": 0.0001
    }
  ]
}
```

### `POST /orders`

Create an order.

**Request body:**
```json
{"product_id": "coffee"}
```

**Unpaid (402):**
```json
{
  "scheme": "exact",
  "network": "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1",
  "maxTimeoutSeconds": 60,
  "amount": "100000",
  "asset": "So11111111111111111111111111111111111111112",
  "payTo": "<merchant_payment_address>",
  "extra": {
    "memo": "did:ignite:z..."
  }
}
```

**Paid (200) — with `X-Payment-Proof` header:**
```json
{
  "id": "abc123",
  "product_id": "coffee",
  "product_name": "Premium Coffee",
  "amount_lamports": 100000,
  "status": "paid",
  "tx_signature": "...",
  "created_at": 1234567890.0,
  "paid_at": 1234567890.0
}
```

### `GET /orders/{order_id}`

Get order status. Used for polling after payment.

**Response:**
```json
{
  "id": "abc123",
  "product_id": "coffee",
  "product_name": "Premium Coffee",
  "amount_lamports": 100000,
  "status": "pending_payment",
  "created_at": 1234567890.0
}
```

### `POST /orders/{order_id}/verify-tx`

Verify a specific transaction against an order.

**Request body:**
```json
{"tx_signature": "<base58_tx_signature>"}
```

**Success (200):** Order marked as paid.

**Failure (400):** Transaction not found or amount mismatch.

## Troubleshooting

### Server won't start

- Check that port 9090 is not in use: `netstat -an | findstr 9090`
- Verify config.json is valid JSON

### 402 challenge has wrong format

- Ensure `config.json` has valid `merchant.did` and `merchant.payment_address`
- The server uses the Coinbase x402 standard `PaymentRequirements` format
- The `PAYMENT-REQUIRED` header must be base64-encoded PaymentRequirements JSON
- The MCP parser auto-detects the format (standard x402 vs legacy `accepts` array)

### Payment verification fails

- The tx signature must be a real confirmed transaction on Solana devnet
- The recipient must match `config.json` payment address exactly
- The amount transferred must be >= the order's `price_lamports`
- Devnet can be slow — allow time for confirmation

### Session key airdrop fails

- Devnet airdrops are rate-limited. Wait a few minutes and retry.
- Try a smaller amount: `python fund_session.py <pubkey> 0.1`

### Phone doesn't receive payment request

- Verify phone is paired with ignite-pay-mcp (check QR code scan)
- Check didcomm-router is running on port 8082
- Verify the phone DID matches what MCP has stored
