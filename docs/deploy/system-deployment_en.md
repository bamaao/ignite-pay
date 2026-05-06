# Ignite Pay System Deployment Guide

> **Note:** DID on-chain storage now uses the PDA approach by default. Photon RPC deployment is no longer required. For the ZK Compression approach, compile with `--features zk-compression` and refer to the [ZK DID Deployment Guide](zk-did-deployment_en.md).

This document covers the complete deployment process for all Ignite Pay services, including infrastructure dependencies, on-chain programs, off-chain microservices, mobile applications, and the MCP agent service.

---

## Table of Contents

1. [System Overview](#1-system-overview)
2. [Environment Requirements](#2-environment-requirements)
3. [Network Topology](#3-network-topology)
4. [Deployment Steps (Ordered by Dependencies)](#4-deployment-steps-ordered-by-dependencies)
5. [Configuration File Reference](#5-configuration-file-reference)
6. [Key Management](#6-key-management)
7. [Docker Deployment](#7-docker-deployment)
8. [Production Considerations](#8-production-considerations)
9. [Health Checks](#9-health-checks)
10. [Troubleshooting](#10-troubleshooting)
11. [Backup and Recovery](#11-backup-and-recovery)
12. [Upgrade and Rollback](#12-upgrade-and-rollback)
13. [Environment Variable Reference](#13-environment-variable-reference)

---

## 1. System Overview

Ignite Pay is an off-chain payment system built on the Solana blockchain, using a UTXO + Merkle Tree state channel architecture. It supports single-hop and multi-hop payments, HTLC conditional payments, compliance auditing, and DID decentralized identity.

### 1.1 Core Components

| Component | Description |
|:----------|:------------|
| **ignite-pay-program** | On-chain state channel program (Anchor 1.0.0), Program ID: `DJBHr35jL3JAGoU7bKMsEFmpeNMrCSK7oYQE4HJ3GBUe` |
| **ignite-pay-did-program** | On-chain DID program (Anchor), PDA-based merchant DID storage (optional ZK Compression with `--features zk-compression`) |
| **didcomm-router** | DIDComm message router, providing message relay and FCM push for mobile clients |
| **did-registry** | DID registration service, managing merchant on-chain identity, VC issuance/revocation |
| **channel-user** | User-side state channel service (Party A, payer) |
| **channel-provider** | Merchant-side state channel service (Party B, payee) |
| **channel-hub** | Hub routing node, inheriting Provider functionality and supporting multi-hop routing |
| **ignite-pay-hub-registry** | Hub registry and discovery service, PostgreSQL backend |
| **ignite-pay-mcp** | User-side MCP agent service, bridging mobile clients and state channels |
| **ignite-pay-merchant-mcp** | Merchant-side MCP agent service, bridging merchant systems and state channels |
| **Sentinel (Flutter)** | User mobile App |
| **Ignite Merchant (Flutter)** | Merchant App |

### 1.2 Service and Port Overview

| Service | Binary/Directory | Port | Transport Protocol | Storage |
|:--------|:-----------------|:-----|:-------------------|:--------|
| PostgreSQL | External dependency | 5432 | TCP | PostgreSQL |
| Hub Registry | `ignite-pay-hub-registry` | 3004 | HTTP | PostgreSQL |
| DIDComm Router | `didcomm-router` | 8080 | HTTP + WS | sled |
| DIDComm Router (Merchant-side) | `didcomm-router` (same binary, different config) | 4000 | HTTP + WS | sled |
| DID Registry | `did-registry` | 8081 | HTTP | sled |
| Channel Hub | `channel-hub` | 3003 | HTTP + WS | sled |
| Channel Provider | `channel-provider` | 3002 | HTTP + WS | sled |
| Channel User | `channel-user` | 3001 | HTTP + WS | sled |
| User MCP | `ignite-pay-mcp` | stdio | JSON-RPC + WS -> :8080 | sled |
| Merchant MCP | `ignite-pay-merchant-mcp` | stdio | JSON-RPC + WS -> :4000 (Merchant Router) | sled |

### 1.3 Dependency Graph

```mermaid
graph TD
    PG[PostgreSQL :5432] --> HR[Hub Registry :3004]
    SOL[Solana Blockchain] --> DU[Channel User :3001]
    SOL --> DP[Channel Provider :3002]
    SOL --> DH[Channel Hub :3003]
    SOL --> DR[DID Registry :8081]

    DR --> SOL
    DH --> HR

    DCR[DIDComm Router :8080<br/>Instance 1] --> UMCP[User MCP stdio]
    DCR2[DIDComm Router :4000<br/>Instance 2, same binary] --> MMCP[Merchant MCP stdio]

    UMCP --> DCR
    UMCP --> DU
    MMCP --> DCR2
    MMCP --> DH

    APP[Sentinel App] --> UMCP
    APP --> DCR
    MAPP[Merchant App] --> MMCP
    MAPP --> DCR2
```

---

## 2. Environment Requirements

### 2.1 Toolchain

| Tool | Version Requirement | Purpose |
|:-----|:-------------------|:--------|
| Rust | 1.75+ | Compile all Rust crates |
| Solana CLI | 1.18+ | Deploy on-chain programs, generate keys |
| Anchor CLI | 0.31.1+ | Build on-chain programs |
| PostgreSQL | 14+ | Hub Registry database |
| Flutter | 3.x | Compile mobile Apps (optional) |

### 2.2 Runtime Dependencies

| Dependency | Purpose |
|:-----------|:--------|
| Solana RPC | On-chain transaction submission and state queries |
| Photon RPC (Helius) | ZK Compression proof service (only needed with `--features zk-compression`) |
| Firebase (optional) | FCM push notifications (DIDComm Router) |

### 2.3 Operating System

- **Production**: Linux (Ubuntu 22.04 LTS / Debian 12 recommended)
- **Development**: Linux / macOS / Windows (WSL2)

### 2.4 Hardware Recommendations

| Component | Minimum | Recommended |
|:----------|:--------|:------------|
| CPU | 2 cores | 4 cores |
| Memory | 4 GB | 8 GB |
| Disk | 40 GB SSD | 100 GB SSD |
| Network | 10 Mbps | 50 Mbps |

---

## 3. Network Topology

```
                              ┌─────────────────────┐
                              │   Solana Blockchain  │
                              │   (Devnet/Mainnet)   │
                              └──────────┬───────────┘
                                         │ RPC
                 ┌───────────────────────┼───────────────────────┐
                 │                       │                       │
        ┌────────▼────────┐    ┌─────────▼─────────┐   ┌────────▼────────┐
        │  DID Registry   │    │  Channel Services  │   │  DID Program    │
        │    :8081         │    │                    │   │  (on-chain)     │
        │  sled DB         │    │  User   :3001      │   │  State Channel  │
        └────────┬─────────┘    │  Provider :3002    │   │  Program        │
                 │              │  Hub     :3003     │   └────────────────┘
                 │              │  (each with sled DB)│
                 │              └────────┬───────────┘
                 │                       │
        ┌────────▼───────────────────────▼───────────────────────┐
        │              Nginx / Reverse Proxy (TLS)               │
        │     :443 → :3001  :3002  :3003  :8080  :8081          │
        └────────┬───────────────────────────────────────────────┘
                 │
      ┌──────────┼──────────────────────────┐
      │          │                          │
┌─────▼─────┐  ┌─▼──────────┐  ┌───────────▼──────────┐
│ DIDComm   │  │ DIDComm    │  │  Hub Registry        │
│ Router    │  │ Router     │  │    :3004              │
│ :8080     │  │ :4000      │  │  PostgreSQL :5432     │
│ sled DB   │  │ (merchant) │  └──────────────────────┘
│ +FCM(opt) │  │ sled DB    │
└─────┬─────┘  └─────┬──────┘
      │               │
      │  WebSocket    │  WebSocket
      ▼               ▼
┌───────────┐  ┌──────────────┐
│ User MCP  │  │ Merchant MCP │
│ (stdio)   │  │ (stdio)      │
│ sled DB   │  │ sled DB      │
└─────┬─────┘  └──────┬───────┘
      │               │
      │ DIDComm       │ DIDComm
      ▼               ▼
┌───────────┐  ┌──────────────┐
│ Sentinel  │  │ Ignite       │
│ (Flutter) │  │ Merchant     │
│ User App  │  │ (Flutter)    │
└───────────┘  └──────────────┘
```

### Port Connection Matrix

| Source Service | Target Service | Target Port | Protocol |
|:---------------|:--------------|:------------|:---------|
| All channel services | Solana RPC | 443 | HTTPS |
| Channel Hub | Hub Registry | 3004 | HTTP |
| Channel Hub/Provider/User | Solana RPC | 443 | HTTPS |
| User MCP | DIDComm Router | 8080 | WS |
| Merchant MCP | DIDComm Router (merchant instance) | 4000 | WS |
| User MCP | Channel User | 3001 | HTTP |
| Merchant MCP | Channel Hub | 3003 | HTTP+WS |
| DID Registry | Solana RPC (+ Photon, ZK mode only) | 443 | HTTPS |

---

## 4. Deployment Steps (Ordered by Dependencies)

> The following steps are arranged by dependency order. Each step should be verified before proceeding.

### Step 1: Deploy PostgreSQL

```bash
# Ubuntu/Debian
sudo apt update
sudo apt install postgresql postgresql-contrib

# Create database and user
sudo -u postgres psql <<EOF
CREATE USER ignite WITH PASSWORD 'ignite';
CREATE DATABASE hub_registry OWNER ignite;
EOF

# Verify
psql -U ignite -d hub_registry -h localhost -c "SELECT 1;"
```

**Verification**: Connection succeeds.

---

### Step 2: Deploy On-Chain Solana Programs

#### 2a. Deploy State Channel Program (ignite-pay-program)

```bash
cd ignite-pay-program

# Build
anchor build

# Deploy to Devnet
anchor deploy --provider.cluster devnet

# Record Program ID
# Current: DJBHr35jL3JAGoU7bKMsEFmpeNMrCSK7oYQE4HJ3GBUe
```

#### 2b. Deploy DID Program (ignite-pay-did-program)

```bash
cd ignite-pay-did-program

# Build
anchor build

# Deploy to Devnet
anchor deploy --provider.cluster devnet

# Record Program ID
# Update did_program_id in did-registry config.toml
```

#### 2c. Initialize PlatformConfig

After deployment, you must call the `init_platform` instruction **once** to write the platform Ed25519 public key to the on-chain PDA.

```bash
# Call using Anchor CLI or SDK
# Parameter: platform_ed25519_pubkey (32 bytes)
# PDA seeds: [b"platform-config"]
```

**Verification**: `solana program show <PROGRAM_ID> --url devnet`

---

### Step 3: Deploy DIDComm Router

```bash
cd didcomm-router
cargo build --release

# Create data directory
mkdir -p ./data

# Edit config.toml (modify port, FCM, TLS, etc. as needed)
# The router does not require a DID; it is ready to use upon startup

# Start
RUST_LOG=info ./target/release/didcomm-router ./config.toml
```

> **Note**: The router does not hold a DID identity; it only performs message relay. WS clients authenticate via Ed25519 signature — no pre-configured keys are needed for the router.

**Verification**: `curl http://localhost:8080/health`

#### 3b. Deploy Merchant-side DIDComm Router (Port 4000)

The merchant-side Router is a second instance of the same `didcomm-router` binary, using a separate configuration.

```bash
# Reuse the compiled binary
# Create merchant-side configuration file
mkdir -p ./config/merchant-router
cat > ./config/merchant-router/config.toml <<'EOF'
[server]
host = "0.0.0.0"
port = 4000

[router]
max_queued_messages = 1000
max_message_age_seconds = 86400

[storage]
path = "./data/merchant-router"
EOF

# Create a separate data directory
mkdir -p ./data/merchant-router

# Start the second instance
RUST_LOG=info ./target/release/didcomm-router ./config/merchant-router/config.toml
```

> **Note**: The two Router instances use different ports (8080 / 4000) and different sled data directories. The user-side MCP connects to :8080, and the merchant-side MCP connects to :4000.

**Verification**: `curl http://localhost:4000/health`

---

### Step 4: Deploy DID Registry

```bash
cd did-registry
cargo build --release --bin did-registry

# Prepare key files
solana-keygen new -o /path/to/payer-keypair.json
solana airdrop 2 <PAYER_ADDRESS> --url devnet

openssl rand -out /path/to/platform_signing.key 32
chmod 400 /path/to/platform_signing.key

# Edit configuration file config.toml
# Update did_program_id, payer_keypair_path, platform_signing_key_path, etc.

# Start
RUST_LOG=info ./target/release/did-registry ./config.toml
```

**Verification**: `curl http://localhost:8081/health` returns `ok`

---

### Step 5: Deploy Channel User Service

```bash
cd ignite-pay-channel-service
cargo build --release --bin channel-user

# Generate keys
solana-keygen new --outfile ./keys/user.key

# Create data directory
mkdir -p ./data/channel_user

# Start
RUST_LOG=info ./target/release/channel-user ./config.toml
```

**Verification**: `curl http://localhost:3001/health`

---

### Step 6: Deploy Channel Provider Service

```bash
cd ignite-pay-channel-service
cargo build --release --bin channel-provider

# Generate keys
solana-keygen new --outfile ./keys/provider.key

# Create data directory
mkdir -p ./data/channel_provider

# Start
RUST_LOG=info ./target/release/channel-provider ./config-provider.toml
```

**Verification**: `curl http://localhost:3002/health`

---

### Step 7: Deploy Channel Hub Service

```bash
cd ignite-pay-channel-service
cargo build --release --bin channel-hub

# Generate keys
solana-keygen new --outfile ./keys/hub.key

# Create data directory
mkdir -p ./data/channel_hub

# Start
RUST_LOG=info ./target/release/channel-hub ./config-hub.toml
```

**Verification**: `curl http://localhost:3003/health`

---

### Step 8: Deploy Hub Registry

```bash
cd ignite-pay-hub-registry
cargo build --release --bin ignite-pay-hub-registry

# Ensure PostgreSQL is running and hub_registry database has been created

# Start (database schema is auto-initialized)
RUST_LOG=info ./target/release/ignite-pay-hub-registry ./hub-registry.toml
```

**Verification**: `curl http://localhost:3004/health`

---

### Step 9: Deploy MCP Services

#### 9a. User MCP

```bash
cd ignite-pay-mcp
cargo build --release --bin ignite-pay-mcp

# Edit config.toml
# Set mediator.ws_url, platform.did, solana parameters, etc.

# MCP communicates via stdio, typically started by a host process
echo '{"jsonrpc":"2.0","method":"tools/list","id":1}' | ./target/release/ignite-pay-mcp ./config.toml
```

**Verification**: JSON-RPC response is normal.

#### 9b. Merchant MCP

```bash
cd ignite-pay-merchant-mcp
cargo build --release --bin ignite-pay-merchant-mcp

# Edit config.toml
# Set merchant.hub_endpoint, mediator.ws_url, etc.

echo '{"jsonrpc":"2.0","method":"tools/list","id":1}' | ./target/release/ignite-pay-merchant-mcp ./config.toml
```

**Verification**: JSON-RPC response is normal.

---

### Step 10: Build Mobile Applications (Optional)

```bash
# User App
cd ignite_pay_app
flutter pub get
flutter run

# Merchant App
cd ignite_pay_merchant_app
flutter pub get
flutter run
```

---

## 5. Configuration File Reference

### 5.1 DIDComm Router (`didcomm-router/config.toml`)

```toml
[server]
host = "0.0.0.0"       # Listen address
port = 8080             # Listen port

[router]
# Maximum queued messages in memory
max_queued_messages = 1000
# Maximum message age (seconds)
max_message_age_seconds = 86400

# Optional: pre-configured peers
# [[router.known_peers]]
# did = "did:ignite:z6Mk..."
# key_agreement_kid = "did:ignite:z6Mk...#key-agreement-1"
# key_agreement_public_base64 = "..."

# Optional: built-in TLS (not needed when using nginx reverse proxy)
# [tls]
# cert_path = "./certs/tls.crt"
# key_path = "./certs/tls.key"

# Optional: FCM push
# [fcm]
# service_account_json = "./firebase-service-account.json"
# project_id = "ignite-pay-d1217"

[storage]
path = "./data"         # sled persistent storage path
```

> **Note**: The router does not use a DID identity. When a WS client connects, the router sends a nonce challenge, the client signs the response with its Ed25519 private key, and the router extracts the public key from the client's `did:ignite` to verify the signature.

### 5.2 DID Registry (`did-registry/config.toml`)

```toml
[server]
host = "0.0.0.0"
port = 8081

[solana]
rpc_url = "https://api.devnet.solana.com"        # Solana RPC endpoint
did_program_id = "<DEPLOYED_PROGRAM_ID>"           # On-chain DID program ID
payer_keypair_path = ""                            # Transaction payer keypair path (empty = ephemeral key)

# [light]                                           # Only needed for ZK Compression mode
# photon_url = ""                                   # Photon RPC URL

[auth]
jwt_secret = "did-registry-secret"                 # JWT signing secret
platform_public_key = ""                           # Platform Ed25519 public key (Base64)
platform_signing_key_path = ""                     # Platform signing private key file path

[fees]
register_fee_lamports = 5000                       # Sponsored registration service fee
update_vc_fee_lamports = 2000                      # Sponsored VC update fee
rotate_key_fee_lamports = 2000                     # Sponsored key rotation fee
```

| Field | Required | Description |
|:------|:---------|:------------|
| `solana.did_program_id` | Yes | Deployed ignite-pay-did-program ID |
| `solana.payer_keypair_path` | Required in production | Empty = ephemeral key, development only |
| `light.photon_url` | ZK mode only | Helius Photon RPC URL (not needed in default PDA mode) |
| `auth.platform_signing_key_path` | Required in production | 32-byte Ed25519 private key |

### 5.3 Channel Hub (`ignite-pay-channel-service/config-hub.toml`)

```toml
[server]
host = "0.0.0.0"
port = 3003

[solana]
rpc_url = "https://api.devnet.solana.com"
channel_program_id = "DJBHr35jL3JAGoU7bKMsEFmpeNMrCSK7oYQE4HJ3GBUe"
keypair_path = "./keys/hub.key"

[channel]
default_tree_depth = 4               # Default Merkle tree depth (2^4=16 leaves)
default_challenge_duration = 5000    # Dispute period (slots, ~33 minutes)
default_min_challenge_delay = 1000   # Minimum dispute delay (slots)
default_settle_window = 10000        # Settlement window (slots)
auto_close_offset = 500000           # Auto-close offset (slots)
db_path = "./data/channel_hub"       # sled database path

[compliance]
spending_threshold = 1000000000      # Cumulative spending threshold
per_channel_limit = 100000000        # Maximum payment per channel
window_slots = 100000                # Sliding window (slots)
travel_rule_threshold = 500000000    # Travel Rule trigger amount
```

### 5.4 Channel Provider (`ignite-pay-channel-service/config-provider.toml`)

```toml
[server]
host = "0.0.0.0"
port = 3002

[solana]
rpc_url = "https://api.devnet.solana.com"
channel_program_id = "DJBHr35jL3JAGoU7bKMsEFmpeNMrCSK7oYQE4HJ3GBUe"
keypair_path = "./keys/provider.key"

[channel]
default_tree_depth = 4
default_challenge_duration = 5000
default_min_challenge_delay = 1000
default_settle_window = 10000
auto_close_offset = 500000
db_path = "./data/channel_provider"
```

> The Provider role does not require a `[compliance]` configuration section; compliance is managed on the User side.

### 5.5 Channel User (`ignite-pay-channel-service/config.toml`)

```toml
[server]
host = "0.0.0.0"
port = 3001

[solana]
rpc_url = "https://api.devnet.solana.com"
channel_program_id = "DJBHr35jL3JAGoU7bKMsEFmpeNMrCSK7oYQE4HJ3GBUe"
keypair_path = "./keys/user.key"

[channel]
default_tree_depth = 4
default_challenge_duration = 5000
default_min_challenge_delay = 1000
default_settle_window = 10000
auto_close_offset = 500000
db_path = "./data/channel_user"

[compliance]
spending_threshold = 1000000000
per_channel_limit = 100000000
window_slots = 100000
travel_rule_threshold = 500000000
```

### 5.6 User MCP (`ignite-pay-mcp/config.toml`)

```toml
[mediator]
ws_url = "ws://127.0.0.1:8080/ws"     # DIDComm Router WebSocket
phone_did = ""                          # Phone App DID

[storage]
path = "./data"                         # sled database path

[policy]
auto_approve_max = 0                    # Maximum auto-approve amount
auth_timeout = 300                      # Authorization timeout (seconds)

[platform]
did = "did:ignite:zPlatformDIDPlaceholder"  # Platform DID
verifying_key_b64 = ""                       # Platform Ed25519 verifying key

[ipfs]
mode = "mock"                           # IPFS mode (mock/kubo)

[solana]
rpc_url = "https://api.devnet.solana.com"
did_program_id = ""                     # On-chain DID program ID
pay_mode = "self_funded"                # Payment mode (self_funded / sponsored)
relayer_url = ""                        # Relayer URL (sponsored mode only)
default_owner = ""                      # Default owner public key
# photon_url = ""                       # Only needed for ZK Compression mode
# address_tree = ""                     # Only needed for ZK Compression mode
```

#### F13/F15 New Internal Mechanisms

The following mechanisms are built-in behaviors requiring no additional configuration:

| Mechanism | Description |
|-----------|-------------|
| Payment Mutex (F15) | `payment_mutex` ensures concurrent payment requests are serialized, preventing overspending |
| Cumulative Merchant Spending (F8) | sled `__merchant_spending__` tree tracks cumulative spending per merchant; routes to manual auth when exceeding whitelist `max_amount` |
| Balance Background Monitor (F13) | Checks session key balance every 60 seconds; sends `balance-notification` to phone when below 10% of spending limit (rate limited to once per 5 minutes per session) |
| Session Key Renewal (F14) | Background check for session key expiry; auto-sends renewal request to phone when less than 5 minutes remaining |

### 5.7 Merchant MCP (`ignite-pay-merchant-mcp/config.toml`)

```toml
[merchant]
did = ""                                    # Merchant DID (auto-generated on first run)
hub_endpoint = "http://localhost:3003"       # Hub HTTP endpoint
hub_ws_url = "ws://localhost:3003/ws"        # Hub WebSocket endpoint

[mediator]
ws_url = "ws://localhost:4000/ws"            # DIDComm Router WebSocket (merchant-side)

[storage]
path = "./data/merchant-mcp"                 # sled database path

[solana]
rpc_url = "https://api.devnet.solana.com"
program_id = ""                              # State channel program ID

[hub]
token_mint = ""                              # Default Token Mint
provider_pubkey = ""                         # Provider (Hub) public key
```

### 5.8 Hub Registry (`hub-registry.toml`)

```toml
[server]
host = "0.0.0.0"
port = 3004

[database]
url = "postgres://ignite:ignite@localhost:5432/hub_registry"
```

The database schema is auto-initialized when the service starts.

### 5.9 DIDComm Router — Merchant Side (`deploy/config/didcomm-router-merchant.toml`)

The merchant-side DIDComm Router is a second instance of the same `didcomm-router` binary with a different configuration file:

```toml
[server]
host = "0.0.0.0"
port = 4000                  # Merchant-side port (distinct from user-side :8080)

[router]
max_queued_messages = 1000
max_message_age_seconds = 86400

[storage]
path = "./data/merchant-router"   # Independent sled data directory
```

| Field | Description |
|:------|:------------|
| `server.port` | Must be `4000`, corresponding to Merchant MCP's `mediator.ws_url` |
| `storage.path` | Independent sled data directory, cannot be shared with the user-side Router |

> The two Router instances share the same source code (`didcomm-router`), differing only in configuration. FCM push and TLS configuration can optionally be enabled in the same way as the user-side configuration.

---

## 6. Key Management

### 6.1 Key Types Overview

| Key | Format | Purpose | Generation Method |
|:----|:-------|:--------|:------------------|
| Solana Keypair | JSON array (64 bytes) | Channel service signing, transaction payer | `solana-keygen new` |
| Platform Signing Key | 32-byte raw binary | DID Registry platform signing, VC issuance | `openssl rand -out file 32` |
| DID Identity | Ed25519 | `did:ignite` decentralized identity | `ignite-pay-core::identity` |
| FCM Service Account | JSON | Firebase push notifications | Firebase Console |

### 6.2 Solana Keypair Generation

```bash
# Create key directory
mkdir -p ./keys

# User-side key
solana-keygen new --outfile ./keys/user.key

# Merchant-side key
solana-keygen new --outfile ./keys/provider.key

# Hub key
solana-keygen new --outfile ./keys/hub.key

# DID Registry payer key
solana-keygen new --outfile ./keys/payer.json

# Devnet airdrop
solana airdrop 2 $(solana-keygen pubkey ./keys/user.key) --url devnet
solana airdrop 2 $(solana-keygen pubkey ./keys/provider.key) --url devnet
solana airdrop 2 $(solana-keygen pubkey ./keys/hub.key) --url devnet
solana airdrop 2 $(solana-keygen pubkey ./keys/payer.json) --url devnet
```

> If `keypair_path` is left empty (`""`), the service auto-generates an ephemeral key on startup (changes on each restart, test use only).

### 6.3 DID Identity Initialization

#### Mobile and MCP DIDs

DIDs for mobile Apps (consumer/merchant) and MCP services are auto-generated on first run via the `identity` module of `ignite-pay-core`:

```rust
use ignite_pay_core::identity::{generate_ignite_did, build_did_document, save_identity};

// First-time generation
let (identity, did) = generate_ignite_did();
save_identity(&db, &identity, &did)?;

// DID format: did:ignite:z + Base58(0xed 0x01 + Ed25519_PublicKey)
println!("DID: {}", did);
```

**DID Encoding Rule**: `did:ignite:z` + Base58(`0xed 0x01` + Ed25519 public key), where `0xed 0x01` is the multicodec identifier prefix for Ed25519 public keys.

### 6.4 Platform Signing Key Generation

```bash
# Generate a 32-byte random private key
openssl rand -out /path/to/platform_signing.key 32

# Set permissions (owner-readable only)
chmod 400 /path/to/platform_signing.key
```

### 6.5 Key Security Recommendations

| Level | Measure |
|:------|:--------|
| Required | Set `chmod 400` on all private key files |
| Required | Use HSM or KMS to manage signing keys in production |
| Required | Offline backup of Platform Signing Key |
| Recommended | Periodically transfer Solana receiving keys to cold wallets |
| Recommended | Periodically rotate DID Controller Key |

---

## 7. Docker Deployment

### 7.1 Overview

All services can be containerized. Docker Compose is recommended for orchestrating multiple services. The following describes the containerization approach for each service.

### 7.2 Containerization Notes per Service

**DIDComm Router**

- Expose ports `8080` (user-side) and `4000` (merchant-side) — two instances of the same binary
- Mount `config.toml` and `./data` directory
- No external database dependencies

**DID Registry**

- Expose port `8081`
- Mount key files (payer keypair, platform signing key)
- Mount sled data directory

**Channel Services (User/Provider/Hub)**

- Expose ports `3001` / `3002` / `3003` respectively
- Each mounts its own configuration file and sled data directory
- Each mounts its corresponding keypair file

**Hub Registry**

- Expose port `3004`
- Depends on PostgreSQL container
- Mount configuration file

**MCP Services**

- Run in stdio mode, no ports to expose
- Mount configuration file and sled data directory

### 7.3 Dockerfile Template

```dockerfile
FROM rust:1.82-slim AS builder
WORKDIR /app
COPY . .
RUN cargo build --release -p <crate-name>

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/<binary> /usr/local/bin/
COPY <config-dir>/config.toml /etc/ignite-pay/config.toml
EXPOSE <port>
ENTRYPOINT ["<binary>", "/etc/ignite-pay/config.toml"]
```

### 7.4 Complete docker-compose Configuration

```yaml
# docker-compose.yml
# Usage: docker compose up -d

services:
  # ─── Infrastructure Layer ───
  postgres:
    image: postgres:16-bookworm
    restart: unless-stopped
    environment:
      POSTGRES_USER: ignite
      POSTGRES_PASSWORD: ${PG_PASSWORD:-ignite}
      POSTGRES_DB: hub_registry
    volumes:
      - pg_data:/var/lib/postgresql/data
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U ignite -d hub_registry"]
      interval: 10s
      timeout: 5s
      retries: 5
    networks:
      - backend

  # ─── Identity Layer ───
  didcomm-router:
    build:
      context: .
      dockerfile: didcomm-router/Dockerfile
    restart: unless-stopped
    environment:
      - RUST_LOG=didcomm_router=info
      - JWT_SECRET=${JWT_SECRET:-change-me-in-production}
    volumes:
      - router_data:/app/data
      - ./didcomm-router/config.toml:/app/config.toml:ro
    expose:
      - "8080"
    networks:
      - backend

  didcomm-router-merchant:
    build:
      context: .
      dockerfile: didcomm-router/Dockerfile
    restart: unless-stopped
    environment:
      - RUST_LOG=didcomm_router=info
      - JWT_SECRET=${JWT_SECRET:-change-me-in-production}
    volumes:
      - router_merchant_data:/app/data
      - ./deploy/config/didcomm-router-merchant.toml:/app/config.toml:ro
    expose:
      - "4000"
    networks:
      - backend

  did-registry:
    build:
      context: .
      dockerfile: did-registry/Dockerfile
    restart: unless-stopped
    environment:
      - RUST_LOG=did_registry=info
    volumes:
      - did_registry_data:/app/data
      - ./did-registry/config.toml:/app/config.toml:ro
      - ./keys:/app/keys:ro
    expose:
      - "8081"
    networks:
      - backend

  # ─── Channel Layer ───
  channel-user:
    build:
      context: .
      dockerfile: ignite-pay-channel-service/Dockerfile
    restart: unless-stopped
    environment:
      - RUST_LOG=info
    volumes:
      - channel_user_data:/app/data
      - ./ignite-pay-channel-service/config.toml:/app/config.toml:ro
      - ./keys:/app/keys:ro
    expose:
      - "3001"
    networks:
      - backend

  channel-provider:
    build:
      context: .
      dockerfile: ignite-pay-channel-service/Dockerfile
    restart: unless-stopped
    environment:
      - RUST_LOG=info
    volumes:
      - channel_provider_data:/app/data
      - ./ignite-pay-channel-service/config-provider.toml:/app/config.toml:ro
      - ./keys:/app/keys:ro
    expose:
      - "3002"
    networks:
      - backend

  channel-hub:
    build:
      context: .
      dockerfile: ignite-pay-channel-service/Dockerfile
    restart: unless-stopped
    environment:
      - RUST_LOG=info
    volumes:
      - channel_hub_data:/app/data
      - ./ignite-pay-channel-service/config-hub.toml:/app/config.toml:ro
      - ./keys:/app/keys:ro
    expose:
      - "3003"
    networks:
      - backend

  # ─── Registry Layer ───
  hub-registry:
    build:
      context: .
      dockerfile: ignite-pay-hub-registry/Dockerfile
    restart: unless-stopped
    environment:
      - RUST_LOG=info
    volumes:
      - ./ignite-pay-hub-registry/hub-registry.toml:/app/hub-registry.toml:ro
    expose:
      - "3004"
    depends_on:
      postgres:
        condition: service_healthy
    networks:
      - backend

  # ─── Agent Layer (stdio mode, managed by host process) ───
  # User MCP and Merchant MCP communicate via stdio and are not suitable for direct inclusion in docker-compose.
  # For containerized execution, consider using supervisord or a custom entrypoint wrapper.

  # ─── Nginx Reverse Proxy ───
  nginx:
    image: nginx:1.27-bookworm
    restart: unless-stopped
    ports:
      - "80:80"
      - "443:443"
    volumes:
      - ./deploy/nginx/nginx.conf:/etc/nginx/conf.d/default.conf:ro
      - ./deploy/certs:/etc/nginx/certs:ro
    depends_on:
      - didcomm-router
      - did-registry
      - channel-user
      - channel-provider
      - channel-hub
      - hub-registry
    networks:
      - backend

volumes:
  pg_data:
  router_data:
  router_merchant_data:
  did_registry_data:
  channel_user_data:
  channel_provider_data:
  channel_hub_data:

networks:
  backend:
    driver: bridge
```

> **Configuration Notes**:
> - MCP services (User MCP, Merchant MCP) use stdio communication and are not suitable for direct containerization. For containerized deployment, use a custom entrypoint wrapper or supervisord
> - `didcomm-router-merchant` requires a separate configuration file `deploy/config/didcomm-router-merchant.toml` (see Section 5.9)
> - Key files are read-only mounted via the `./keys` directory
> - Use named volumes to persist sled and PostgreSQL data
> - All backend services communicate only on the internal `backend` network; only Nginx exposes external ports

---

## 8. Production Considerations

### 8.1 TLS and Nginx Reverse Proxy

All externally exposed services should use TLS via an Nginx reverse proxy.

#### Nginx Main Configuration Framework

```nginx
# General SSL configuration
ssl_protocols TLSv1.2 TLSv1.3;
ssl_ciphers HIGH:!aNULL:!MD5;
ssl_prefer_server_ciphers on;

# Common WebSocket location template
# For all services requiring WS
```

#### Channel User Reverse Proxy

```nginx
server {
    listen 443 ssl;
    server_name channel-user.ignite-pay.example.com;

    ssl_certificate     /etc/ssl/certs/ignite-pay.pem;
    ssl_certificate_key /etc/ssl/private/ignite-pay.key;

    location / {
        proxy_pass http://127.0.0.1:3001;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
    }

    location /ws {
        proxy_pass http://127.0.0.1:3001;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
    }
}
```

#### Channel Provider Reverse Proxy

```nginx
server {
    listen 443 ssl;
    server_name merchant.ignite-pay.example.com;

    ssl_certificate     /etc/ssl/certs/ignite-pay.pem;
    ssl_certificate_key /etc/ssl/private/ignite-pay.key;

    location / {
        proxy_pass http://127.0.0.1:3002;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
    }

    location /ws {
        proxy_pass http://127.0.0.1:3002;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
    }
}
```

#### Channel Hub Reverse Proxy

```nginx
server {
    listen 443 ssl;
    server_name hub.ignite-pay.example.com;

    ssl_certificate     /etc/ssl/certs/ignite-pay.pem;
    ssl_certificate_key /etc/ssl/private/ignite-pay.key;

    location / {
        proxy_pass http://127.0.0.1:3003;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
    }

    location /ws {
        proxy_pass http://127.0.0.1:3003;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
    }
}
```

#### DIDComm Router Reverse Proxy

```nginx
server {
    listen 443 ssl;
    server_name didcomm.ignite-pay.example.com;

    ssl_certificate     /etc/ssl/certs/ignite-pay.pem;
    ssl_certificate_key /etc/ssl/private/ignite-pay.key;

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
    }

    location /ws {
        proxy_pass http://127.0.0.1:8080;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
        # WebSocket long connection timeout
        proxy_read_timeout 3600s;
        proxy_send_timeout 3600s;
    }
}
```

#### DID Registry Reverse Proxy

```nginx
server {
    listen 443 ssl;
    server_name did-registry.ignite-pay.example.com;

    ssl_certificate     /etc/ssl/certs/ignite-pay.pem;
    ssl_certificate_key /etc/ssl/private/ignite-pay.key;

    location / {
        proxy_pass http://127.0.0.1:8081;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
    }
}
```

#### Hub Registry Reverse Proxy

```nginx
server {
    listen 443 ssl;
    server_name hub-registry.ignite-pay.example.com;

    ssl_certificate     /etc/ssl/certs/ignite-pay.pem;
    ssl_certificate_key /etc/ssl/private/ignite-pay.key;

    location / {
        proxy_pass http://127.0.0.1:3004;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
    }
}
```

> **Note**: DID Registry and Hub Registry have no WebSocket endpoints, so HTTP reverse proxy configuration is sufficient. If these services are only used internally (not externally exposed), Nginx configuration can be omitted and internal network addresses used directly.
```

### 8.2 systemd Service Configuration

#### Channel User

```ini
[Unit]
Description=Ignite Pay Channel User Service
After=network.target

[Service]
Type=simple
User=ignite
WorkingDirectory=/opt/ignite-pay
ExecStart=/opt/ignite-pay/channel-user /opt/ignite-pay/config.toml
Environment=RUST_LOG=info
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

#### Channel Provider

```ini
[Unit]
Description=Ignite Pay Channel Provider Service
After=network.target

[Service]
Type=simple
User=ignite
WorkingDirectory=/opt/ignite-pay
ExecStart=/opt/ignite-pay/channel-provider /opt/ignite-pay/config-provider.toml
Environment=RUST_LOG=info
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

#### Channel Hub

```ini
[Unit]
Description=Ignite Pay Channel Hub Service
After=network.target

[Service]
Type=simple
User=ignite
WorkingDirectory=/opt/ignite-pay
ExecStart=/opt/ignite-pay/channel-hub /opt/ignite-pay/config-hub.toml
Environment=RUST_LOG=info
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

#### DIDComm Router

```ini
[Unit]
Description=Ignite Pay DIDComm Router
After=network.target

[Service]
Type=simple
User=ignite
WorkingDirectory=/opt/ignite-pay
ExecStart=/opt/ignite-pay/didcomm-router /opt/ignite-pay/didcomm-router-config.toml
Environment=RUST_LOG=info
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

#### DID Registry

```ini
[Unit]
Description=Ignite Pay DID Registry
After=network.target

[Service]
Type=simple
User=ignite
WorkingDirectory=/opt/ignite-pay
ExecStart=/opt/ignite-pay/did-registry /opt/ignite-pay/did-registry-config.toml
Environment=RUST_LOG=info
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

#### Hub Registry

```ini
[Unit]
Description=Ignite Pay Hub Registry
After=network.target postgresql.service
Requires=postgresql.service

[Service]
Type=simple
User=ignite
WorkingDirectory=/opt/ignite-pay
ExecStart=/opt/ignite-pay/ignite-pay-hub-registry /opt/ignite-pay/hub-registry.toml
Environment=RUST_LOG=info
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

#### DIDComm Router — Merchant Side

```ini
[Unit]
Description=Ignite Pay DIDComm Router (Merchant)
After=network.target

[Service]
Type=simple
User=ignite
WorkingDirectory=/opt/ignite-pay
ExecStart=/opt/ignite-pay/didcomm-router /opt/ignite-pay/didcomm-router-merchant-config.toml
Environment=RUST_LOG=info
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

#### Enable and Start Services

```bash
sudo systemctl daemon-reload
sudo systemctl enable ignite-channel-user ignite-channel-provider ignite-channel-hub \
                     ignite-didcomm-router ignite-didcomm-router-merchant \
                     ignite-did-registry ignite-hub-registry
sudo systemctl start ignite-didcomm-router ignite-didcomm-router-merchant \
                      ignite-did-registry ignite-channel-user \
                      ignite-channel-provider ignite-channel-hub ignite-hub-registry
```

### 8.3 Log Management

```bash
# View systemd service logs
sudo journalctl -u ignite-channel-hub -f

# View by time range
sudo journalctl -u ignite-channel-hub --since "2025-01-01 00:00:00" --until "2025-01-02 00:00:00"

# By log level
RUST_LOG=debug  # trace / debug / info / warn / error
```

It is recommended to configure journald or logrotate for log rotation.

### 8.4 Monitoring Recommendations

| Metric | Monitoring Target | Alert Threshold | Action |
|:-------|:------------------|:----------------|:-------|
| Available liquidity | Channel Hub | < 2x average routing volume | Add liquidity |
| Channel success rate | Channel Hub | < 95% | Check channel status |
| Average latency | All channel services | > 200ms | Optimize network/node |
| sled database size | All channel services | > 2 GB | Archive historical data |
| Co-signing delay | Provider | > 500ms | Optimize node performance |
| HTLC expiry rate | Provider | > 1% | Check preimage revelation flow |
| Expired multi-hop payments | Hub | > 5% | Adjust timelock |
| PostgreSQL connection count | Hub Registry | > 80% max | Scale up |
| Active channel trend | Hub/Provider | Persistent decline | Check service quality |

### 8.5 Solana RPC Endpoints

| Environment | Recommended Approach |
|:------------|:---------------------|
| Development | `https://api.devnet.solana.com` (free, rate-limited) |
| Production | Private RPC node or paid services like Helius/QuickNode/Alchemy |

Production environments must use private RPC endpoints to avoid rate limiting on public RPC affecting transaction submission.

### 8.6 Firewall and Security Group Rules

#### Network Segmentation Design

```
┌─────────────────────────────────────────────────────────────┐
│                     Public Network (Internet)                │
│             Only Nginx :443 exposed externally               │
└───────────────────────────┬─────────────────────────────────┘
                            │
┌───────────────────────────▼─────────────────────────────────┐
│                   DMZ / Frontend Network                     │
│  Nginx reverse proxy :443 → backend services                │
└───────────────────────────┬─────────────────────────────────┘
                            │
┌───────────────────────────▼─────────────────────────────────┐
│                   Backend Service Network                    │
│  Channel User :3001    Channel Provider :3002               │
│  Channel Hub :3003     DIDComm Router :8080 / :4000         │
│  DID Registry :8081    Hub Registry :3004                   │
└───────────────────────────┬─────────────────────────────────┘
                            │
┌───────────────────────────▼─────────────────────────────────┐
│                   Data Layer Network                         │
│  PostgreSQL :5432 (accessible only by Hub Registry)          │
│  sled data directories (local to each service)              │
│  Solana RPC :443 (outbound HTTPS)                           │
└─────────────────────────────────────────────────────────────┘
```

#### iptables Rules Example

```bash
#!/bin/bash
# firewall-rules.sh

# Default policy
iptables -P INPUT DROP
iptables -P FORWARD DROP
iptables -P OUTPUT ACCEPT

# Allow established connections
iptables -A INPUT -m state --state ESTABLISHED,RELATED -j ACCEPT

# Allow local loopback
iptables -A INPUT -i lo -j ACCEPT

# SSH management (restrict source IP)
iptables -A INPUT -p tcp --dport 22 -s <ADMIN_CIDR> -j ACCEPT

# Nginx external exposure
iptables -A INPUT -p tcp --dport 443 -j ACCEPT
iptables -A INPUT -p tcp --dport 80 -j ACCEPT    # HTTP→HTTPS redirect

# Backend service ports — allow localhost and internal network only
# DIDComm Router
iptables -A INPUT -p tcp --dport 8080 -s 127.0.0.1 -j ACCEPT
iptables -A INPUT -p tcp --dport 4000 -s 127.0.0.1 -j ACCEPT
# DID Registry
iptables -A INPUT -p tcp --dport 8081 -s 127.0.0.1 -j ACCEPT
# Channel Services
iptables -A INPUT -p tcp --dport 3001 -s 127.0.0.1 -j ACCEPT
iptables -A INPUT -p tcp --dport 3002 -s 127.0.0.1 -j ACCEPT
iptables -A INPUT -p tcp --dport 3003 -s 127.0.0.1 -j ACCEPT
# Hub Registry
iptables -A INPUT -p tcp --dport 3004 -s 127.0.0.1 -j ACCEPT
# PostgreSQL
iptables -A INPUT -p tcp --dport 5432 -s 127.0.0.1 -j ACCEPT

# Log rejected connections
iptables -A INPUT -j LOG --log-prefix "DROPPED: " --log-level 4
```

#### Port Exposure Policy Overview

| Port | Service | Externally Exposed | Access Scope |
|:-----|:--------|:-------------------|:-------------|
| 443 | Nginx (TLS) | **Yes** | Public network |
| 80 | Nginx (HTTP→HTTPS) | **Yes** | Public network |
| 22 | SSH | Restricted | Admin network segment |
| 8080 | DIDComm Router | No | localhost / internal network |
| 4000 | DIDComm Router (merchant-side) | No | localhost / internal network |
| 8081 | DID Registry | No | localhost / internal network |
| 3001 | Channel User | No | localhost / internal network |
| 3002 | Channel Provider | No | localhost / internal network |
| 3003 | Channel Hub | No | localhost / internal network |
| 3004 | Hub Registry | No | localhost / internal network |
| 5432 | PostgreSQL | No | localhost only |

---

## 9. Health Checks

### 9.1 Service Health Check Endpoints

| Service | Endpoint | Expected Response |
|:--------|:---------|:------------------|
| DIDComm Router | `GET http://localhost:8080/health` | HTTP 200 |
| DIDComm Router (merchant-side) | `GET http://localhost:4000/health` | HTTP 200 |
| DID Registry | `GET http://localhost:8081/health` | `ok` |
| Channel User | `GET http://localhost:3001/health` | HTTP 200 |
| Channel Provider | `GET http://localhost:3002/health` | HTTP 200 |
| Channel Hub | `GET http://localhost:3003/health` | HTTP 200 |
| Hub Registry | `GET http://localhost:3004/health` | HTTP 200 |

### 9.2 Verification Script

```bash
#!/bin/bash
# ignite-pay-healthcheck.sh

check() {
    local name=$1
    local url=$2
    local expected=$3

    response=$(curl -s -o /dev/null -w "%{http_code}" "$url" 2>/dev/null)
    if [ "$response" = "200" ] || [ "$response" = "$expected" ]; then
        echo "[OK] $name ($url)"
    else
        echo "[FAIL] $name ($url) - HTTP $response"
    fi
}

check "DIDComm Router"           "http://localhost:8080/health"
check "DIDComm Router (Merchant)" "http://localhost:4000/health"
check "DID Registry"             "http://localhost:8081/health"
check "Channel User"     "http://localhost:3001/health"
check "Channel Provider" "http://localhost:3002/health"
check "Channel Hub"      "http://localhost:3003/health"
check "Hub Registry"     "http://localhost:3004/health"

# PostgreSQL is checked using pg_isready (curl cannot detect raw TCP ports)
if pg_isready -h localhost -p 5432 -U ignite > /dev/null 2>&1; then
    echo "[OK] PostgreSQL (localhost:5432)"
else
    echo "[FAIL] PostgreSQL (localhost:5432)"
fi
```

### 9.3 systemd Health Check Configuration

Health checks can be added to systemd unit files:

```ini
[Service]
# ... other configuration ...

ExecStartPost=/bin/sleep 2
ExecStartPost=/usr/bin/curl -sf http://localhost:3003/health

# Auto-restart on health check failure
WatchdogSec=30
```

### 9.4 Verify Channel Service API Functionality

```bash
# Query channel list
curl http://localhost:3001/v1/channels

# Hub registration info
curl http://localhost:3003/v1/hub/info

# Route discovery test
curl -X POST http://localhost:3003/v1/routes/find \
  -H "Content-Type: application/json" \
  -d '{
    "from_did_hash": "hex...",
    "to_did_hash": "hex...",
    "amount": 1000000,
    "token_mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
    "max_hops": 3
  }'

# DID resolution
curl http://localhost:8081/v1/did/resolve/did:ignite:z6Mk...

# Hub list
curl http://localhost:3004/v1/hubs?status=active
```

---

## 10. Troubleshooting

### 10.1 Common Startup Errors

#### Error: Port Already in Use

```
Error: Address already in use (os error 98)
```

**Troubleshooting**:
```bash
# Find the process using the port
sudo lsof -i :3003
# or
sudo ss -tlnp | grep 3003

# Terminate the process
kill <PID>
```

#### Error: sled Database Lock Conflict

```
Error: database is already open in another process
```

**Troubleshooting**:
- sled does not support multiple processes opening the same database simultaneously
- Ensure no residual processes remain
- After abnormal exit, delete `*.lock` files:
```bash
rm ./data/channel_hub/*.lock
```

#### Error: Solana RPC Connection Failure

```
Error: Failed to connect to Solana RPC
```

**Troubleshooting**:
```bash
# Test RPC connectivity
curl -s -X POST https://api.devnet.solana.com \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"getHealth","id":1}'

# Check RPC response
# Normal: {"jsonrpc":"2.0","result":"ok","id":1}
```

#### Error: PostgreSQL Connection Failure (Hub Registry)

```
Error: connection refused (os error 111)
```

**Troubleshooting**:
```bash
# Check PostgreSQL status
sudo systemctl status postgresql

# Test connection
psql -U ignite -d hub_registry -h localhost -c "SELECT 1;"

# Check pg_hba.conf for password authentication
sudo cat /etc/postgresql/14/main/pg_hba.conf | grep ignite
```

#### Error: Key File Not Found

```
Error: No such file or directory (keypair_path)
```

**Troubleshooting**:
- Check that the path in the configuration file is correct
- Use absolute paths rather than relative paths
- Confirm file permissions: `ls -la ./keys/`

### 10.2 Signature Verification Failure

**Symptom**: `verify_leaf_update_signature` or on-chain `InvalidSignature`

**Troubleshooting**:
1. Check that the signer's public key is correct (User or Provider)
2. Check that `prev_leaf_hash` matches the current leaf
3. Check that sequence numbers are consecutive
4. Confirm the same `channel_id` is used
5. On-chain verification uses different message formats (three signature message families); confirm the correct family is used

### 10.3 Amount Conservation Error

**Symptom**: `AmountConservation { expected, actual }`

**Troubleshooting**:
1. When splitting the tree, ensure the sum of all leaf amounts equals `total_deposited`
2. In Pipeline operations, the amount of partial_transfer must not exceed the source leaf
3. Check for concurrent modifications

### 10.4 Merkle Proof Verification Failure

**Symptom**: On-chain `ProofVerificationFailed`

**Troubleshooting**:
1. Confirm the off-chain `MerkleTree` uses sorted pair hashing: `hashv(&[min, max])`
2. Check that the leaf is at the correct index position
3. Confirm `current_root` is up to date

### 10.5 HTLC Timeout Issues

**Symptom**: `HtlcNotExpired` or `HtlcExpired`

**Troubleshooting**:
1. Solana slot time: 1 slot is approximately 400ms (normal), devnet may be slower
2. Check that `timelock_slot` satisfies the constraint: `> current_slot + challenge_duration + HTLC_SAFETY_MARGIN`
3. For multi-hop, check that timelock decrement is correct
4. `HTLC_SAFETY_MARGIN` = 1000 slots (~6.7 minutes)

### 10.6 DIDComm Router WebSocket Disconnections

**Symptom**: MCP service WebSocket connections to Router frequently disconnect

**Troubleshooting**:
1. Check Nginx WebSocket timeout configuration (`proxy_read_timeout`)
2. Check network stability
3. Check Router logs for `max_queued_messages` triggers
4. Confirm `max_message_age_seconds` is configured reasonably

### 10.7 Hub Route Discovery Returns No Results

**Symptom**: `POST /v1/routes/find` returns empty routes

**Troubleshooting**:
1. Confirm Hub is registered: `GET /v1/hub/info`
2. Confirm the route graph has edges: `POST /v1/routes/refresh`
3. Confirm Hub has sufficient liquidity
4. Check that `from_did_hash` and `to_did_hash` are correct

### 10.8 Log Level Adjustment

```bash
# Temporary adjustment (lost on restart)
RUST_LOG=debug ./channel-hub ./config-hub.toml

# Filter by module
RUST_LOG=ignite_pay_channel_service=debug,info ./channel-hub ./config-hub.toml

# Adjust systemd service
sudo systemctl edit ignite-channel-hub
# Add:
# [Service]
# Environment=RUST_LOG=debug
sudo systemctl restart ignite-channel-hub
```

---

## 11. Backup and Recovery

### 11.1 Backup Scope

| Data Type | Storage Location | Backup Method | Frequency Recommendation |
|:----------|:----------------|:--------------|:-------------------------|
| Channel sled data | `./data/channel_user/`, `./data/channel_provider/`, `./data/channel_hub/` | Filesystem snapshot | Daily |
| DID Registry sled | `./did_registry_data/` (hardcoded path) | Filesystem snapshot | Daily |
| DIDComm Router sled | `./data/` | Filesystem snapshot | Daily |
| MCP sled data | `./data/`, `./data/merchant-mcp/` | Filesystem snapshot | Daily |
| PostgreSQL (Hub Registry) | PostgreSQL data directory | `pg_dump` | Daily |
| Key files | `./keys/` | Offline backup | On change |
| Configuration files | Each service's `config.toml` | Version control | On change |

### 11.2 sled Database Backup

sled does not support a hot backup API (no `sled::export`). Backups must be performed by stopping writes or using filesystem snapshots.

**Method 1: Stop Service Backup (recommended for small-scale deployments)**

```bash
#!/bin/bash
# backup-sled.sh
BACKUP_DIR="/opt/ignite-pay/backups/$(date +%Y%m%d_%H%M%S)"
mkdir -p "$BACKUP_DIR"

# Stop all services
sudo systemctl stop ignite-channel-hub ignite-channel-provider ignite-channel-user \
                     ignite-didcomm-router ignite-did-registry

# Copy sled data directories
cp -r /opt/ignite-pay/data/channel_hub   "$BACKUP_DIR/channel_hub"
cp -r /opt/ignite-pay/data/channel_provider "$BACKUP_DIR/channel_provider"
cp -r /opt/ignite-pay/data/channel_user  "$BACKUP_DIR/channel_user"
cp -r /opt/ignite-pay/did_registry_data  "$BACKUP_DIR/did_registry_data"

# Restart services
sudo systemctl start ignite-didcomm-router ignite-did-registry ignite-channel-user \
                      ignite-channel-provider ignite-channel-hub

echo "Backup completed: $BACKUP_DIR"
```

**Method 2: Filesystem Snapshot (recommended for production)**

If data directories are on LVM or ZFS volumes, snapshots can be used to back up without stopping services:

```bash
# LVM snapshot
lvcreate -L 1G -s -n ignite_data_snap /dev/vg0/ignite_data
mkdir -p /mnt/snap && mount /dev/vg0/ignite_data_snap /mnt/snap
cp -r /mnt/snap/ /opt/ignite-pay/backups/$(date +%Y%m%d_%H%M%S)/
umount /mnt/snap && lvremove -f /dev/vg0/ignite_data_snap

# ZFS snapshot
zfs snapshot pool0/ignite-data@backup-$(date +%Y%m%d_%H%M%S)
zfs send pool0/ignite-data@backup-... | gzip > /opt/ignite-pay/backups/snap-$(date +%Y%m%d).zfs.gz
```

> **Note**: sled may have data in memory that has not been flushed to disk during writes. The 33 `.flush()` calls ensure critical writes are immediately persisted, but snapshots may still lose the last unwritten flush. Production environments should perform backups during off-peak hours.

### 11.3 PostgreSQL Backup

```bash
#!/bin/bash
# backup-postgres.sh
BACKUP_DIR="/opt/ignite-pay/backups/postgres"
mkdir -p "$BACKUP_DIR"

# Full database logical backup
pg_dump -U ignite -d hub_registry -F c \
  -f "$BACKUP_DIR/hub_registry_$(date +%Y%m%d_%H%M%S).dump"

# Retain backups from the last 7 days
find "$BACKUP_DIR" -name "*.dump" -mtime +7 -delete
```

Restore:

```bash
# Restore to a new database
pg_restore -U ignite -d hub_registry_restored \
  "$BACKUP_DIR/hub_registry_20250101_120000.dump"
```

### 11.4 Key Backup

```bash
#!/bin/bash
# backup-keys.sh — must be stored offline with encryption
tar czf - /opt/ignite-pay/keys/ | \
  gpg --symmetric --cipher-algo AES256 \
  -o "/opt/ignite-pay/backups/keys_$(date +%Y%m%d).tar.gz.gpg"
```

> Key backups must be stored on offline media separate from the server (e.g., encrypted USB drive, offline HSM backup).

### 11.5 Disaster Recovery Procedure

| Scenario | Recovery Steps |
|:---------|:--------------|
| **sled data corruption** | 1. Stop the affected service → 2. Delete the corrupted directory → 3. Restore from backup → 4. Restart the service |
| **PostgreSQL data loss** | 1. Confirm PostgreSQL is running → 2. `pg_restore` to recover → 3. Restart Hub Registry |
| **Full server failure** | 1. Install environment on new server → 2. Restore configuration files and keys → 3. Start PostgreSQL and restore → 4. Restore sled data → 5. Start all services in dependency order |
| **Key compromise** | 1. Generate new keys → 2. Update configuration files → 3. Channel services must reopen channels (old channels cannot be recovered) → 4. DID Registry must perform key rotation |

### 11.6 Known Limitations

| Limitation | Description |
|:-----------|:------------|
| sled has no hot backup API | Cannot obtain a consistent snapshot without stopping writes |
| No automated backup mechanism | The project has no backup scripts; operations must manually configure them (see script templates above) |
| Audit log recovery | Supports `restore_from_ipfs()` to restore audit logs from IPFS, but depends on IPFS data availability |
| Channel state cannot be rebuilt | sled data loss means channel state is lost; can only wait for the on-chain dispute period to expire for settlement |

---

## 12. Upgrade and Rollback

### 12.1 Upgrade Principles

- **Rolling upgrades**: Services can be upgraded independently without full system downtime
- **Backward compatibility**: sled data format changes must support auto-migration or provide migration tools
- **On-chain programs are immutable**: Solana programs cannot be modified after deployment; only new versions can be deployed (new Program ID)

### 12.2 Off-Chain Service Upgrade Process

```bash
# 1. Build new version
cd ignite-pay-channel-service
git pull origin master
cargo build --release --bin channel-hub

# 2. Backup current binary and data
cp /opt/ignite-pay/channel-hub /opt/ignite-pay/channel-hub.bak.$(date +%Y%m%d)

# 3. Replace binary
cp target/release/channel-hub /opt/ignite-pay/channel-hub

# 4. Restart service
sudo systemctl restart ignite-channel-hub

# 5. Verify
sleep 3
curl -sf http://localhost:3003/health || {
  # Rollback
  cp /opt/ignite-pay/channel-hub.bak.$(date +%Y%m%d) /opt/ignite-pay/channel-hub
  sudo systemctl restart ignite-channel-hub
  echo "Rollback executed!"
}
```

### 12.3 PostgreSQL Schema Migration

The current Hub Registry uses `include_str!()` to execute `migrations/001_init.sql` at startup, with **no version tracking**. `CREATE TABLE` statements will fail when the table already exists.

Upgrade strategy:

1. **Add new migration files**: Create `002_xxx.sql` sequentially, using `CREATE TABLE IF NOT EXISTS` or `ALTER TABLE`
2. **Execute sequentially in code**: `include_str!` all migration files in order in `main.rs`
3. **Idempotent design**: Each migration file should be re-runnable without errors

```rust
// Recommended main.rs migration execution pattern
let migrations = [
    include_str!("../migrations/001_init.sql"),
    include_str!("../migrations/002_xxx.sql"),  // New migration
];
for sql in &migrations {
    sqlx::query(sql).execute(&pool).await?;
}
```

### 12.4 On-Chain Program Upgrade

Solana on-chain program upgrade process:

```bash
# 1. Build new version
cd ignite-pay-program
anchor build

# 2. Deploy to the same Program ID (requires upgrade authority)
anchor deploy --provider.cluster devnet \
  --program-name ignite_pay_program \
  --program-keypair ./keys/program-keypair.json

# 3. Verify
solana program show <PROGRAM_ID> --url devnet
```

> **Note**: On-chain program upgrades require the key with upgrade authority. Production environments should use multi-sig governance (e.g., Squads Protocol) to manage upgrade authority.

### 12.5 Rollback Strategy

| Component | Rollback Method | Notes |
|:----------|:---------------|:------|
| Off-chain services | Replace binary + restart | sled data format must be backward compatible |
| PostgreSQL | `pg_restore` from backup | Will lose data since last backup |
| On-chain programs | Deploy old `.so` version | Requires upgrade authority key |
| Configuration files | Replace config + restart | Key changes must be synchronized across related services |

---

## 13. Environment Variable Reference

### 13.1 Runtime Environment Variables

| Variable | Applicable Services | Default | Description |
|:---------|:-------------------|:--------|:------------|
| `RUST_LOG` | All services | See table below | `tracing_subscriber` log level filter |
| `JWT_SECRET` | DIDComm Router | Random UUID v4 (generated on each startup) | JWT signing secret; can also be set via `config.toml` `[router] jwt_secret` |
| `IGNITE_PAY_CONFIG` | User MCP | `"config.toml"` | Configuration file path (overrides CLI argument) |
| `IGNITE_MERCHANT_CONFIG` | Merchant MCP | `"config.toml"` | Configuration file path (overrides CLI argument) |
| `AUDIT_LOG_DIR` | User MCP, Merchant MCP | (unset = output to stderr only) | When set, enables daily rolling audit log files |

### 13.2 RUST_LOG Defaults per Service

| Service | Default | Example Override |
|:--------|:--------|:-----------------|
| DIDComm Router | `didcomm_router=info` | `didcomm_router=debug` |
| DID Registry | `did_registry=info` | `did_registry=debug,sqlx=warn` |
| Channel User | `info` | `ignite_pay_channel_service=debug` |
| Channel Provider | `info` | `ignite_pay_channel_service=debug` |
| Channel Hub | `info` | `ignite_pay_channel_service=debug` |
| Hub Registry | `info` | `ignite_pay_hub_registry=debug,sqlx=debug` |
| User MCP | `ignite_pay_mcp=info` | `ignite_pay_mcp=debug` |
| Merchant MCP | `info` | `info,ignite_pay_core=debug` |

### 13.3 Log Level Syntax

```
# Global level
RUST_LOG=debug

# Filter by crate
RUST_LOG=ignite_pay_channel_service=debug,sqlx=warn

# Filter by module path
RUST_LOG=ignite_pay_channel_service::handlers::payment=trace

# Mixed
RUST_LOG=info,ignite_pay_channel_service=debug,sqlx=warn
```

### 13.4 systemd Environment Variable Configuration

Set environment variables in systemd unit files using `Environment` or `EnvironmentFile`:

```ini
[Service]
# Single variable
Environment=RUST_LOG=debug

# Multiple variables
Environment=RUST_LOG=debug
Environment=AUDIT_LOG_DIR=/var/log/ignite-pay/audit

# Or use a file
EnvironmentFile=/opt/ignite-pay/env
```

`/opt/ignite-pay/env` file format:

```bash
RUST_LOG=info
JWT_SECRET=your-production-jwt-secret
AUDIT_LOG_DIR=/var/log/ignite-pay/audit
```

---

## Change Log

| Version | Date | Changes |
|:--------|:-----|:--------|
| v0.1 | 2025-06-01 | Initial version: covers deployment steps, configuration, Docker, production considerations, health checks, troubleshooting |
| v0.2 | 2025-06-15 | Added merchant-side DIDComm Router deployment steps; added Backup and Recovery, Upgrade and Rollback, Environment Variable Reference sections; added firewall rules, complete docker-compose.yml, DID Registry / Hub Registry Nginx configuration; fixed health check script PostgreSQL detection |
| v0.3 | 2026-04-26 | Removed router DID requirement, WS authentication changed to Ed25519 signature verification; removed `didcomm-router-init` tool; updated Sections 3, 5.1, 5.9, 6.3 |
