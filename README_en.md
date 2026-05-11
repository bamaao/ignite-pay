# Ignite Pay

> **License:** This project is licensed under Business Source License 1.1. The source code is public and free for non-production use. Commercial production use is restricted until January 1, 2031, at which point the license will convert to Apache License 2.0.

**Decentralized Payment Infrastructure for the Agent Economy** — AI Agents make payments autonomously, humans authorize in real-time on their phones; consumers scan QR codes for instant on-chain micro-payments.

> **Note: v0.1.0 Beta** — Currently in v0.1.0 Beta stage. Some high-concurrency edge cases are known to need optimization.

---

## Key Features

### 1. Agent Autonomous Payments with Real-time Human Authorization

When an AI Agent encounters an HTTP 402 paywall while accessing paid resources, it automatically triggers the payment flow via MCP (Model Context Protocol). After verifying amount, merchant identity, and risk policies, the request is pushed to the user's phone via DIDComm end-to-end encryption — the user swipes to confirm, and the Agent receives a payment proof to continue working.

```
Agent hits 402 → MCP parses x402 challenge → Verifies merchant DID → Risk decision
    ↓ Auto-approve (whitelist/small amount)       ↓ Needs authorization
    → Direct payment                                → Phone push → User confirms
    → Agent continues                               → Agent continues
```

### 2. QR Code Micro-Payments

Merchants generate payment QR codes (`ignite://pay?d=<base64url>`), and consumers scan to complete on-chain payments. Supports SOL, USDC, USDT and more. Users can choose Session Key for on-chain payments or MagicBlock channel for instant settlement. The merchant app automatically announces confirmation via voice.

```
Merchant generates QR → Consumer scans → Chooses payment method → DIDComm encrypted
                                                                          ↓
                                                                  Session Key / MB / Wallet
                                                                          ↓
                                                                  Merchant confirms + voice
```

### 3. MagicBlock High-Frequency Payment Channel

Built on MagicBlock parallel runtime for off-chain payment channels with <50ms latency instant micro-payments. Buyers sign Vouchers from the global pool (GlobalVault) — no per-payment on-chain interaction. Merchants batch-collect Vouchers, build a Sum-Merkle Tree, and settle on-chain in one transaction, drastically reducing gas costs.

Three-layer security:
- **On-chain**: Spending Cap limits per-channel amount, funds locked in PDA accounts
- **ER Layer**: MagicBlock real-time state validation, gas-free high-speed processing
- **Off-chain**: Challenge window dispute mechanism, Sum-Merkle Proof fraud proofs

```
Buyer signs Voucher (Ed25519)    →    Merchant collects Vouchers
       ↓                                ↓
SHA256(channel ‖ seq ‖ amount)       Build Sum-Merkle Tree
       ↓                                ↓
MagicBlock ER instant record (<50ms)  Dual-sign on-chain settlement
```

### 4. x402 Standard Protocol

Compatible with [Coinbase x402](https://github.com/coinbase/x402) HTTP 402 payment protocol. Any x402-compatible service can integrate — Agents don't need a special SDK, they just handle the payment requirement in HTTP 402 responses, call MCP to complete payment, and retry the request.

### 5. Multi-Path Payment Engine

Automatically selects the optimal payment path per scenario:

| Method | Latency | Gas | Use Case |
|--------|---------|-----|----------|
| **MagicBlock Channel** | <50ms | Free | High-frequency micro-payments, QR/Agent recurring |
| **Session Key** | ~400ms | Normal | On-chain direct payment, temporary key authorization |
| **Direct Wallet** | ~400ms | Normal | Phantom/Solflare deep link, MCP never touches private keys |
| **Relayer** | ~400ms | Sponsored | Gasless sponsored payment mode |
| **CCTP Cross-chain Deposit** | 10-30min | Source chain gas | EVM → Solana USDC cross-chain deposit (Circle CCTP V2 Forwarding) |

### 6. CCTP Cross-Chain USDC Deposit

Based on the [Circle CCTP V2 Forwarding](https://developers.circle.com/stablecoins/docs/cctp-forwarding) protocol, the buyer app supports one-tap USDC transfers from EVM chains (Ethereum / Base / Arbitrum / OP) to Solana wallets. Users complete on-chain operations (approve + depositForBurnWithHook) via MetaMask, and Circle automatically mints equivalent USDC to the target ATA on Solana.

```
User selects source chain + enters amount + Solana address
       ↓
Rust layer: query Iris fees + derive Solana ATA + ABI-encode calldata
       ↓
MetaMask: approve USDC → TokenMessengerV2 → depositForBurnWithHook
       ↓
Circle Iris: verify → attestation → mint USDC on Solana
       ↓
App polls for confirmation → show Solana tx hash + Solscan link
```

See [docs/cctp-cross-chain-deposit.md](docs/cctp-cross-chain-deposit.md) for details.

### 7. DIDComm v2 End-to-End Encryption

All communication between Agent and phone is encrypted via DIDComm v2 protocol (JWE authcrypt) — relay servers cannot read plaintext. Based on Ed25519 signing + X25519 key agreement, DID identifier format `did:ignite:z<multicodec>`.

### 8. PDA On-Chain Identity

Merchant DIDs are registered on Solana via PDA accounts. Standard Solana RPC for read/write — no additional infrastructure needed. Supports platform VC (Verifiable Credential) issuance + on-chain registration + on-chain signature verification.

### 9. Six-Level Risk Control

| Priority | Policy | Behavior |
|----------|--------|----------|
| 1 | Blacklist | Reject immediately |
| 2 | IPFS CID Blacklist | Fetch list then reject |
| 3 | Per-transaction limit | Require authorization if exceeded |
| 4 | Whitelist auto-approve | No phone confirmation needed |
| 5 | IPFS CID Whitelist | Fetch list then auto-approve |
| 6 | Default | Push to phone for authorization |

---

## System Architecture

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

## End-to-End Payment Flows

### Agent x402 Payment

```
Agent                 Buyer MCP              Merchant MCP          Phone
  │  HTTP GET /paid-content                   │                    │
  │◄─ HTTP 402 + x402 challenge ──────────────│                    │
  │  call process_x402_challenge              │                    │
  ├───────────────────────────────────────────►│                    │
  │                        Parse challenge     │                    │
  │                        Verify merchant VC + DID                │
  │                        Risk decision       │                    │
  │                             ├─ Auto-approve│                    │
  │                             └─ Needs auth ►│── DIDComm JWE ────►│
  │                                            │   User confirms   │
  │                                            │◄─ Auth result ────│
  │                        Execute payment     │                    │
  │                        (MB Voucher / Session Key / Wallet)     │
  │◄─ payment proof ──────────────────────────│                    │
  │  HTTP GET /paid-content (with proof)       │                    │
  │◄─ HTTP 200 + content ─────────────────────│                    │
```

### Merchant QR Payment

```
Merchant App       Merchant MCP         Buyer MCP          Buyer App
     │  Generate payment QR              │                    │
     │◄───────────────│                    │                    │
     │  Display QR    │                    │  Scan              │
     │                │                    │◄───────────────────│
     │                │  DIDComm: payment  │                    │
     │                │◄───────────────────│                    │
     │                │  Verify + confirm order                 │
     │  Voice + confirm                    │                    │
     │◄───────────────│                    │                    │
```

---

## Project Structure

```
ignite-pay/
├── ignite-pay-core/              # Shared foundation (DID, DIDComm, VC, audit)
├── ignite-pay-solana/            # Solana RPC integration (payment, PDA DID, Session Key)
├── ignite-pay-state-channel/     # Off-chain UTXO Merkle Tree state channel engine (inactive)
│
├── ignite-pay-did-program/       # Merchant DID on-chain program (PDA, 6 instructions)
├── ignite-pay-session-program/   # Session Key on-chain program (Anchor, 4 instructions)
├── ignite-pay-mb/                # MagicBlock payment channel
│   ├── programs/                 #   On-chain program (Anchor, 10 instructions)
│   └── sdk/                      #   Rust SDK (PDA, Merkle Tree, signing, tx builder)
│
├── didcomm-router/               # DIDComm message relay service
├── did-registry/                 # DID on-chain registration REST service
├── ignite-pay-channel-service/   # State channel HTTP+WS service (inactive)
├── ignite-pay-program/           # State channel on-chain program (inactive)
├── ignite-pay-hub-registry/      # Hub registry & discovery (PostgreSQL)
├── ignite-pay-relayer/           # Sponsored payment gas relay service
│
├── ignite-pay-mcp/               # Buyer MCP server (23 tools)
├── ignite-pay-merchant-mcp/      # Merchant MCP server (14 tools)
├── ignite-pay-skill/             # Python SDK (PyO3 bindings)
│
├── ignite_pay_app/               # Buyer mobile app (Flutter + Rust Bridge)
├── ignite_pay_merchant_app/      # Merchant mobile app (Flutter + Rust Bridge)
├── ignite-pay-ecom-demo/         # x402 e-commerce demo server (Python FastAPI)
│
├── tests/                        # On-chain program tests (litesvm + mollusk)
├── docs/                         # Design docs + business flows
└── deploy/                       # Docker deployment config
```

---

## MCP Tools

### Buyer MCP (`ignite-pay-mcp`) — 23 Tools

| Tool | Description |
|------|-------------|
| `process_x402_challenge` | Full x402 payment flow (parse→verify→risk→auth→pay) |
| `check_authorization` | Query payment status |
| `get_payment_history` | Payment history |
| `get_identity` | View DID, Mediator, Solana, MB status |
| `generate_pairing_invitation` | Generate DIDComm pairing QR code |
| `create_session` | Create Session Key (SOL/SPL Token) |
| `get_session_status` | Query Session Key status |
| `close_session` | Close Session Key |
| `execute_spl_payment` | SPL Token on-chain payment |
| `add_merchant` | Add merchant DID |
| `update_merchant` | Update merchant DID data |
| `verify_merchant` | Verify merchant on-chain identity |
| `mb_init_global` ~ `mb_withdraw` | MagicBlock payment channel operations (11 tools) |

### Merchant MCP (`ignite-pay-merchant-mcp`) — 16 Tools

| Tool | Description |
|------|-------------|
| `list_products` | Return product catalog (Agent query) |
| `create_order` | Create order, return x402 challenge |
| `verify_payment` | Verify payment proof (on-chain tx / MB Voucher) |
| `generate_payment_qr` | Generate payment QR code |
| `check_payment` | Query order status |
| `get_payment_history` | Order history |
| `get_identity` | Merchant DID, MB Pubkey |
| `register_merchant` | Register on-chain identity (VC + PDA DID) |
| `verify_merchant_did` | Verify on-chain DID |
| `mb_get_channel` ~ `mb_force_release` | MagicBlock merchant channel operations (7 tools) |

---

## MagicBlock Payment Channel

> The MagicBlock payment channel is production-ready and the recommended approach for micro-payments.

Three-layer architecture for high-frequency micro-payments:

```
┌─────────────────────────────────────┐
│  L1 (Solana)                        │
│  Channel create / Fund lock / Sig verify / Settlement
├─────────────────────────────────────┤
│  ER (MagicBlock)                    │
│  High-speed state transition (<50ms) / gas-free
│  Real-time Voucher recording        │
├─────────────────────────────────────┤
│  Off-chain                          │
│  Challenge window disputes / Merkle Proof fraud proofs
└─────────────────────────────────────┘
```

- **Voucher**: `SHA256(channel_id ‖ seq ‖ amount)` + Ed25519 signature
- **Settlement**: Build Sum-Merkle Tree, dual-sign and submit on-chain
- **Disputes**: Challenge window for disputes, Merkle Proof anti-fraud
- **Stablecoins**: Native support for SOL / USDC / USDT

---

## Tech Stack

| Layer | Technology |
|-------|------------|
| On-chain | Solana (Anchor), MagicBlock |
| Identity | `did:ignite` method, Ed25519/X25519, JWE authcrypt |
| Communication | DIDComm v2, MCP (JSON-RPC 2.0), x402 HTTP 402 |
| Backend | Rust, Axum 0.8, tokio, sled, reqwest |
| Mobile | Flutter + Rust Bridge (flutter_rust_bridge) |
| SDK | Rust, Python (PyO3) |
| Deployment | Docker Compose, nginx, PostgreSQL |

---

## Quick Start

### Prerequisites

- Rust 1.80+ (MSRV)
- Solana CLI 2.x
- Anchor Framework 0.30+ / 1.0+
- Flutter 3.x (mobile apps)

### Build

```bash
# Build all Rust crates
cargo build

# Build buyer MCP
cargo build -p ignite-pay-mcp

# Build merchant MCP
cargo build -p ignite-pay-merchant-mcp

# Build MagicBlock SDK
cargo build -p ignite-pay-mb-sdk

# Build on-chain programs (requires Solana toolchain)
cd ignite-pay-mb && anchor build
```

### Run Tests

```bash
# All tests
cargo test

# Single crate
cargo test -p ignite-pay-merchant-mcp
cargo test -p ignite-pay-mb-sdk

# On-chain program tests (requires local-validator or svm)
cd tests/svm-litesvm && cargo test
```

### Docker Deployment

```bash
# Start all services (PostgreSQL + DIDComm Router + DID Registry + Hub)
docker-compose up -d
```

### Run MCP Servers

```bash
# Buyer MCP (stdio mode, for Claude Desktop / Cursor etc.)
cd ignite-pay-mcp
cp config.toml.example config.toml  # Edit config
cargo run

# Merchant MCP (stdio + SSE mode)
cd ignite-pay-merchant-mcp
cp config.toml.example config.toml  # Edit config
cargo run
```

### x402 Demo

```bash
# Start e-commerce demo server
cd ignite-pay-ecom-demo
pip install fastapi uvicorn solana-py
python server.py
```

---

## Documentation

| Document | Description |
|----------|-------------|
| [AGENTS_en.md](AGENTS_en.md) | Complete crate-level architecture docs (English) |
| [docs/agent-payment-flow_en.md](docs/agent-payment-flow_en.md) | Agent x402 payment flow |
| [docs/business-flows.md](docs/business-flows.md) | All business flows (18 flows) |
| [docs/ignite-pay-magicblock.md](docs/ignite-pay-magicblock.md) | MagicBlock payment channel design |
| [docs/session-key-payment-flow.md](docs/session-key-payment-flow.md) | Session Key payment flow |
| [docs/direct-wallet-payment-flow.md](docs/direct-wallet-payment-flow.md) | Direct wallet payment flow |
| [docs/sponsored-relayer-payment-flow.md](docs/sponsored-relayer-payment-flow.md) | Sponsored relayer payment flow |
| [docs/cctp-cross-chain-deposit.md](docs/cctp-cross-chain-deposit.md) | CCTP Forwarding EVM→Solana cross-chain USDC deposit |

---

## License

Private / Proprietary
