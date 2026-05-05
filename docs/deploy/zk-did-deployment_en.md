# ZK Compression DID Management System Deployment Guide

This document covers the complete deployment process for ignite-pay-did-program (on-chain Solana program) and did-registry (off-chain REST service).

---

## Table of Contents

1. [System Architecture Overview](#1-system-architecture-overview)
2. [Prerequisites](#2-prerequisites)
3. [On-Chain Program Deployment (ignite-pay-did-program)](#3-on-chain-program-deploymentignite-pay-did-program)
4. [Off-Chain Service Deployment (did-registry)](#4-off-chain-service-deploymentdid-registry)
5. [Configuration Details](#5-configuration-details)
6. [Dual On-Chain Mode (Sponsored / SelfOnchain)](#6-dual-on-chain-modesponsored--selfonchain)
7. [Storage Architecture](#7-storage-architecture)
8. [API Reference](#8-api-reference)
9. [Security Considerations](#9-security-considerations)

---

## 1. System Architecture Overview

```
                        ┌─────────────────────────────┐
                        │     Merchant (Client)        │
                        │  Ed25519 Keypair (local)     │
                        │  did:ignite:z<multibase>     │
                        └──────────┬──────────────────┘
                                   │ HTTP REST
                                   ▼
                        ┌─────────────────────────────┐
                        │      did-registry            │
                        │   (Axum HTTP Server)         │
                        │                              │
                        │  ┌─────────┐  ┌───────────┐ │
                        │  │ VC      │  │ Nonce     │ │
                        │  │ Issuance│  │ Management│ │
                        │  └─────────┘  └───────────┘ │
                        │  ┌──────────────────────────┐│
                        │  │ Platform Signing Key      ││
                        │  │ (Ed25519, 32 bytes)       ││
                        │  └──────────────────────────┘│
                        │  ┌──────────────────────────┐│
                        │  │ sled (persistent storage) ││
                        │  │ - Merchant DID cache      ││
                        │  │ - VC storage              ││
                        │  │ - Leaf index mapping      ││
                        │  │ - Fee records             ││
                        │  └──────────────────────────┘│
                        │                              │
                        │  Dual on-chain mode:         │
                        │  ┌──────────┐ ┌────────────┐ │
                        │  │Sponsored │ │SelfOnchain  │ │
                        │  │Platform  │ │Return       │ │
                        │  │signs     │ │unsigned TX  │ │
                        │  │+sends    │ │Merchant     │ │
                        │  │+records  │ │signs+sends  │ │
                        │  │fees      │ │             │ │
                        │  └──────────┘ └────────────┘ │
                        └──────┬──────────┬────────────┘
                               │          │
                    ┌──────────┘          └──────────┐
                    ▼                                ▼
          ┌──────────────┐               ┌─────────────────────┐
          │ Solana RPC    │               │ Photon RPC          │
          │ (RPC URL)     │               │ (ZK Compression     │
          │               │               │  Indexer)            │
          └──────┬────────┘               └──────────┬──────────┘
                 │                                   │
                 ▼                                   ▼
          ┌──────────────────────────────────────────────────┐
          │         Solana Blockchain (Devnet/Mainnet)       │
          │                                                   │
          │  ┌─────────────────────────────────────┐         │
          │  │  ignite-pay-did-program              │         │
          │  │  (Anchor + Light SDK)                │         │
          │  │                                      │         │
          │  │  Instructions:                       │         │
          │  │  - init_platform (one-time)          │         │
          │  │  - initialize_did                    │         │
          │  │  - update_did_with_vc                │         │
          │  │  - set_recovery_key                  │         │
          │  │  - recover_controller                │         │
          │  │  - revoke_vc                         │         │
          │  └─────────────────────────────────────┘         │
          │                                                   │
          │  ┌─────────────────────────────────────┐         │
          │  │  PlatformConfig PDA                  │         │
          │  │  seeds: [b"platform-config"]          │         │
          │  │  Stores platform Ed25519 public key   │         │
          │  │  initialize_did / update_did_with_vc  │         │
          │  │  verify platform signature before     │         │
          │  │  writing vc_hash                      │         │
          │  └─────────────────────────────────────┘         │
          │                                                   │
          │  ┌─────────────────────────────────────┐         │
          │  │  Light Protocol State Trees          │         │
          │  │  (Merkle Tree — compressed account   │         │
          │  │   storage)                            │         │
          │  │                                      │         │
          │  │  MerchantCompressedDid:               │         │
          │  │  - original_pk   (immutable)         │         │
          │  │  - controller_pk (rotatable)         │         │
          │  │  - recovery_pk   (recovery key)      │         │
          │  │  - vc_hash       (VC hash)           │         │
          │  │  - last_updated  (timestamp)         │         │
          │  │  - nonce         (anti-replay counter)│        │
          │  └─────────────────────────────────────┘         │
          │                                                   │
          │  ┌─────────────────────────────────────┐         │
          │  │  RevokedVc PDA (revocation registry) │        │
          │  │  seeds: [b"revoked-vc", vc_hash]      │         │
          │  │  One PDA created per VC revocation    │         │
          │  │  Verifiers check PDA existence        │         │
          │  │  to determine revocation status       │         │
          │  └─────────────────────────────────────┘         │
          └──────────────────────────────────────────────────┘
```

### Data Flow

```
0. At deployment: init_platform(platform_ed25519_pubkey) → writes PlatformConfig PDA
1. Merchant generates Ed25519 keypair → derives did:ignite:z...
2. GET  /v1/auth/nonce              → obtains server nonce
3. Merchant signs "issue_vc:{did}:{merchant_name}:{nonce}"
4. POST /v1/vc/issue + did_signature → platform verifies DID ownership → issues VC → obtains vc_hash
5. Merchant signs "register:{did}:{pubkey}:{vc_hash}:{nonce}"
6. POST /v1/merchants/register      → platform signs(credential_subject_pk || vc_hash) → on-chain compressed DID creation
   ├── On-chain verification: subject_binding + platform_sig_verify
   ├── mode=sponsored (default): platform signs+sends, records service fee
   └── mode=self_onchain:  returns unsigned TX, merchant signs+broadcasts
7. POST /v1/merchants/confirm (SelfOnchain only) → merchant notifies platform that tx is on-chain
8. GET  /v1/did/resolve/{did}       → resolves DID Document
9. POST /v1/merchants/update-vc     → updates on-chain VC hash (platform signature verification + Subject Binding, supports dual mode)
10. POST /v1/merchants/rotate-key    → rotates control key (also supports dual mode)
11. GET  /v1/fees                    → queries fee records
12. POST /v1/proof                   → obtains ZK proof + platform_config_address (public endpoint)
13. POST /v1/vc/revoke               → revokes VC (platform authority only, creates RevokedVc PDA)
```

---

## 2. Prerequisites

### 2.1 Toolchain

| Tool | Version | Purpose |
|---|---|---|
| Rust | 1.75+ | Compile all Rust crates |
| Solana CLI | 1.18+ | Deploy on-chain programs |
| Anchor CLI | 0.31.1 | Build ignite-pay-did-program |
| cargo-build-sgx (or solana bpf) | — | Compile BPF/SBF programs |
| Node.js / Yarn | Optional | Run Anchor test scripts |

### 2.2 Account Preparation

- **Payer Keypair**: Used to pay for on-chain transaction fees, requires sufficient SOL (devnet can use `solana airdrop`)
- **Platform Signing Key**: 32-byte Ed25519 private key used for issuing VCs (recommended to generate via secure methods)
- **Photon RPC API Key**: ZK Compression proof service provided by Helius or other Light Protocol indexers

### 2.3 Generate Platform Signing Key

```bash
# Generate a 32-byte random private key file
openssl rand -out platform_signing.key 32

# If you need to view the corresponding public key and DID (for debugging)
# You can use the project's identity module to derive them
```

### 2.4 Networks

| Network | Solana RPC | Photon RPC |
|---|---|---|
| Localnet | `http://127.0.0.1:8899` | Local Photon (requires separate deployment) |
| Devnet | `https://api.devnet.solana.com` | `https://photon.helius.com?api-key=<KEY>` |
| Mainnet | `https://api.mainnet-beta.solana.com` | `https://photon.helius.com?api-key=<KEY>` |

---

## 3. On-Chain Program Deployment (ignite-pay-did-program)

### 3.1 Project Structure

```
ignite-pay-did-program/
├── Anchor.toml          # Anchor configuration (program ID, cluster, wallet)
├── Cargo.toml           # Rust dependencies
├── src/
│   ├── lib.rs           # Program entry point, 6 instructions + ed25519 verification
│   ├── state.rs         # MerchantCompressedDid + PlatformConfig + RevokedVc structs
│   └── error.rs         # DidError error code definitions (including PlatformNotInitialized, AlreadyRevoked, etc.)
└── tests/               # TypeScript integration tests (optional)
```

### 3.2 Account Structures

#### MerchantCompressedDid (Compressed Account)

```rust
// Stored in Light Protocol Merkle Tree (not a traditional on-chain account)
pub struct MerchantCompressedDid {
    pub original_pk: Pubkey,      // Initial public key (immutable anchor)
    pub controller_pk: Pubkey,    // Current controller (rotatable)
    pub recovery_pk: Pubkey,      // Recovery key
    pub vc_hash: [u8; 32],        // Platform-issued VC SHA-256 hash
    pub last_updated: i64,        // Last update Unix timestamp
    pub nonce: u64,               // Anti-replay counter
}
```

**PDA Derivation**: `seeds = [b"merchant-did", original_pk]`, deterministic address in the Address Tree.

#### PlatformConfig (On-Chain PDA Account)

```rust
// On-chain PDA, stores platform Ed25519 public key for verifying platform signatures
// Seeds: [b"platform-config"]
pub struct PlatformConfig {
    pub platform_ed25519_pubkey: [u8; 32],  // Platform Ed25519 public key
    pub authority: Pubkey,                   // Address authorized to update the platform key
    pub bump: u8,                            // PDA bump seed
}
```

**Space**: 8 (discriminator) + 32 + 32 + 1 = 73 bytes. Initialized once via the `init_platform` instruction.

#### RevokedVc (On-Chain PDA Account — Revocation Registry)

```rust
// On-chain PDA, one RevokedVc account created per revoked VC
// Seeds: [b"revoked-vc", vc_hash]
pub struct RevokedVc {
    pub vc_hash: [u8; 32],              // Revoked VC hash
    pub credential_subject_pk: Pubkey,   // VC subject public key
    pub revoked_at: i64,                 // Revocation timestamp
    pub reason: u8,                      // Revocation reason (0=unspecified, 1=violation, 2=expired, etc.)
    pub authority: Pubkey,               // Platform authority that executed the revocation
    pub bump: u8,                        // PDA bump seed
}
```

**Space**: 8 (discriminator) + 32 + 32 + 8 + 1 + 32 + 1 = 114 bytes. Created via the `revoke_vc` instruction.

**Revocation Check**: Third-party verifiers check whether the `RevokedVc` PDA exists to determine if a VC has been revoked. PDA address = `find_program_address(&[b"revoked-vc", vc_hash], program_id)`.

### 3.3 Instruction List

| Instruction | Account Structure | Function |
|---|---|---|
| `init_platform` | `[authority, platform_config, system_program]` | One-time initialization of platform Ed25519 public key |
| `initialize_did` | `[signer, platform_config, ...remaining]` | Create compressed DID, requires platform signature |
| `update_did_with_vc` | `[signer, platform_config, ...remaining]` | Bind/update VC hash, requires platform signature |
| `set_recovery_key` | `[signer, ...remaining]` | Set/change recovery key |
| `recover_controller` | `[signer, ...remaining]` | Reset controller via recovery key |
| `revoke_vc` | `[authority, platform_config, revoked_vc, system_program]` | Revoke VC, create RevokedVc PDA |

**Platform Signature Verification** (`initialize_did` / `update_did_with_vc`):

The on-chain program verifies the platform's Ed25519 signature over `(credential_subject_pk || vc_hash)` before the CPI write, and enforces `credential_subject_pk == signer.key()`. This simultaneously achieves:
- **Account Binding**: The signature is bound to a specific signer, preventing cross-account replay
- **Subject Binding**: The on-chain enforcement that the signer must be the VC's subject, preventing identity impersonation

Signature message format: `credential_subject_pk (32 bytes) || vc_hash (32 bytes)` = 64 bytes

**`initialize_did` instruction data format**:
```
[discriminator(8)] [proof(var)] [address_tree_info(borsh)] [output_state_tree_index(1)]
[vc_hash(32)] [platform_signature(64)] [credential_subject_pk(32)]
```

**`update_did_with_vc` instruction data format**:
```
[discriminator(8)] [proof(var)] [current_did(borsh)] [account_meta(borsh)]
[vc_hash(32)] [nonce(8)] [platform_signature(64)] [credential_subject_pk(32)]
```

### 3.4 Error Codes

| Error Code | Meaning |
|---|---|
| `AlreadyInitialized` | DID already exists |
| `NotInitialized` | DID does not exist |
| `InvalidControllerKey` | Signer is not the current controller |
| `NonceMismatch` | Provided nonce does not match current value |
| `InvalidRecoveryKey` | Signer is not the recovery key |
| `ArithmeticOverflow` | Nonce overflow |
| `InsufficientCpiAccounts` | Insufficient CPI accounts |
| `PlatformNotInitialized` | PlatformConfig PDA not initialized (call `init_platform` first) |
| `InvalidPlatformSignature` | Platform Ed25519 signature verification failed |
| `VcSubjectMismatch` | credential_subject_pk does not match signer |
| `AlreadyRevoked` | This VC has already been revoked (RevokedVc PDA already exists) |
| `UnauthorizedRevocation` | Caller is not the platform authority, not authorized to revoke |

### 3.5 Build and Deploy

#### Step 1: Configure Anchor.toml

```toml
[features]
seeds = false
skip-lint = false

[programs.devnet]  # or mainnet
ignite_pay_did_program = "<YOUR_PROGRAM_ID>"

[registry]
url = "https://api.apr.dev"

[provider]
cluster = "devnet"  # or mainnet
wallet = "~/.config/solana/id.json"
```

Replace `YOUR_PROGRAM_ID` with the actual program ID. Generate a new keypair:

```bash
solana-keygen new -o target/deploy/ignite_pay_did_program-keypair.json
```

The program ID is automatically derived from the keypair. Update it in `Anchor.toml` and the `declare_id!` macro.

#### Step 2: Build

```bash
cd ignite-pay-did-program

# Build (debug)
anchor build

# Or build directly with cargo
cargo build-sbf
```

The build artifact is located at `target/deploy/ignite_pay_did_program.so`.

#### Step 3: Deploy

```bash
# Devnet deployment
anchor deploy --provider.cluster devnet

# Or manual deployment
solana program deploy \
  target/deploy/ignite_pay_did_program.so \
  --program-id target/deploy/ignite_pay_did_program-keypair.json \
  --url devnet
```

#### Step 4: Verify Deployment

```bash
solana program show <PROGRAM_ID> --url devnet
```

Confirm the program is deployed and immutable (or upgradeable, depending on deployment strategy).

#### Step 5: Record Program ID

After deployment, record the Program ID for did-registry configuration:

```
did_program_id = "<DEPLOYED_PROGRAM_ID>"
```

#### Step 6: Initialize PlatformConfig

After deployment, you must call the `init_platform` instruction **once** to write the platform's Ed25519 public key to the on-chain PDA:

```bash
# Call init_platform using Anchor CLI or solana-sdk
# Parameter: platform_ed25519_pubkey (32 bytes)
# Accounts: [authority(signer), platform_config(PDA), system_program]
# PDA seeds: [b"platform-config"]
```

> Until `init_platform` is called, `initialize_did` and `update_did_with_vc` will fail with the `PlatformNotInitialized` error.

---

## 4. Off-Chain Service Deployment (did-registry)

### 4.1 Project Structure

```
did-registry/
├── Cargo.toml           # Rust dependencies
├── config.toml          # Service configuration
└── src/
    ├── main.rs          # Entry point (tokio + tracing + axum)
    ├── server.rs        # Route definitions (14 routes)
    ├── config.rs        # Config struct (including FeesConfig)
    ├── state.rs         # RegistryState shared state
    ├── error.rs         # RegistryError
    ├── handlers/
    │   ├── mod.rs
    │   ├── nonce.rs     # GET  /v1/auth/nonce
    │   ├── register.rs  # POST /v1/merchants/register (supports mode field)
    │   ├── confirm.rs   # POST /v1/merchants/confirm (SelfOnchain confirmation)
    │   ├── resolve.rs   # GET  /v1/did/resolve/{did}
    │   ├── verify.rs    # GET  /v1/merchants/verify/{did}
    │   ├── status.rs    # GET  /v1/merchants/status/{did}
    │   ├── rotate_key.rs# POST /v1/merchants/rotate-key (supports mode field)
    │   ├── update_vc.rs # POST /v1/merchants/update-vc (supports mode field)
    │   ├── issue_vc.rs  # POST /v1/vc/issue (requires DID signature)
    │   ├── revoke_vc.rs # POST /v1/vc/revoke (platform authority only)
    │   ├── proof.rs     # POST /v1/proof (public, no auth required)
    │   └── fees.rs      # GET  /v1/fees
    ├── did/
    │   ├── resolver.rs  # DID hash computation, signature verification
    │   └── ignite_store.rs  # In-memory DID document cache
    └── storage/
        └── sled_store.rs    # sled persistent storage (including fee records)
```

### 4.2 Build

```bash
cd did-registry
cargo build --release
```

Build artifact: `target/release/did-registry`

### 4.3 Configure config.toml

```toml
[server]
host = "0.0.0.0"
port = 8081

[solana]
# Solana RPC endpoint
rpc_url = "https://api.devnet.solana.com"
# Deployed ignite-pay-did-program ID
did_program_id = "<DEPLOYED_PROGRAM_ID>"
# Transaction payer keypair file path (Solana JSON keypair format)
payer_keypair_path = "/path/to/payer-keypair.json"

[light]
# Photon RPC URL (ZK Compression indexer)
# Format: https://photon.helius.com?api-key=<YOUR_API_KEY>
photon_url = "https://photon.helius.com?api-key=<YOUR_API_KEY>"

[auth]
# JWT signing secret
jwt_secret = "<random-strong-secret>"
# Platform Ed25519 public key (Base64 encoded), used to verify platform signatures in update-vc requests
platform_public_key = "<BASE64_PUBLIC_KEY>"
# Platform Ed25519 private key file path (32 bytes raw binary)
platform_signing_key_path = "/path/to/platform_signing.key"

[fees]
# Service fees in Sponsored mode (unit: lamports, 1 SOL = 1,000,000,000 lamports)
register_fee_lamports = 5000      # Registration fee
update_vc_fee_lamports = 2000     # VC update fee
rotate_key_fee_lamports = 2000    # Key rotation fee
```

#### Configuration Field Descriptions

| Field | Required | Description |
|---|---|---|
| `server.host` | Yes | Listen address |
| `server.port` | Yes | Listen port |
| `solana.rpc_url` | Yes | Solana RPC endpoint URL |
| `solana.did_program_id` | Yes | Deployed ignite-pay-did-program program ID |
| `solana.payer_keypair_path` | Required in production | Transaction payer Keypair (empty string = ephemeral keypair, development only) |
| `light.photon_url` | Required in production | ZK Compression Photon RPC URL (empty string will cause proof retrieval to fail) |
| `auth.jwt_secret` | Yes | JWT signing secret |
| `auth.platform_public_key` | Required in production | Platform Ed25519 public key (Base64), used to verify platform signatures for update-vc |
| `auth.platform_signing_key_path` | Required in production | 32-byte Ed25519 private key file path (empty = ephemeral key, development only) |
| `fees.register_fee_lamports` | Yes | Sponsored mode registration service fee (lamports) |
| `fees.update_vc_fee_lamports` | Yes | Sponsored mode VC update service fee (lamports) |
| `fees.rotate_key_fee_lamports` | Yes | Sponsored mode key rotation service fee (lamports) |

### 4.4 Prepare Key Files

#### Payer Keypair

```bash
# Generate Solana keypair (standard JSON format)
solana-keygen new -o /path/to/payer-keypair.json

# Devnet SOL airdrop
solana airdrop 2 <PAYER_ADDRESS> --url devnet
```

#### Platform Signing Key

```bash
# Generate 32-byte random private key
openssl rand -out /path/to/platform_signing.key 32

# Set permissions (owner read-only)
chmod 400 /path/to/platform_signing.key
```

To obtain the corresponding `platform_public_key` (for `config.toml` and `update-vc` verification), you need to derive the public key from the private key file using a tool, then Base64 encode it.

### 4.5 Start the Service

```bash
# Run directly
./target/release/did-registry /path/to/config.toml

# Or use default config.toml (current directory)
./target/release/did-registry

# Override log level via environment variable
RUST_LOG=did_registry=debug ./target/release/did-registry
```

After startup, the following output is displayed:

```
INFO did_registry: Starting DID Registry on 0.0.0.0:8081
INFO did_registry::state: Registry payer pubkey: <PAYER_PUBKEY>
INFO did_registry::state: Platform DID: did:ignite:z<...>
INFO did_registry: Listening on 0.0.0.0:8081
```

### 4.6 Docker Deployment (Recommended)

```dockerfile
FROM rust:1.82-slim AS builder
WORKDIR /app
COPY . .
RUN cargo build --release -p did-registry

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/did-registry /usr/local/bin/
COPY did-registry/config.toml /etc/did-registry/config.toml
EXPOSE 8081
ENTRYPOINT ["did-registry", "/etc/did-registry/config.toml"]
```

```bash
docker build -t did-registry .
docker run -d \
  -p 8081:8081 \
  -v /path/to/config.toml:/etc/did-registry/config.toml \
  -v /path/to/payer-keypair.json:/secrets/payer.json:ro \
  -v /path/to/platform_signing.key:/secrets/platform.key:ro \
  -v did-registry-data:/var/lib/did-registry \
  --name did-registry \
  did-registry
```

> Note: The sled database writes to the `./did_registry_data` directory by default. In Docker, you need to mount a volume for persistence.

### 4.7 Health Check

```bash
curl http://localhost:8081/health
# Expected response: ok
```

---

## 5. Configuration Details

### 5.1 State Tree

ZK Compression relies on Light Protocol's Merkle tree for storing compressed accounts. Each compressed DID is effectively a leaf node hash in the tree. The Photon RPC is responsible for indexing these leaves and providing Merkle proofs.

- **Address Tree**: Used for addressing, `derive_address([b"merchant-did", original_pk], address_tree, program_id)` deterministically derives the address
- **State Tree**: Stores the actual compressed account data hashes

At startup, did-registry automatically connects to the Photon RPC via `LightClient::new(config)` to obtain available tree information.

### 5.2 Key Architecture

```
Merchant Key Three-Layer Architecture:
┌─────────────────────────────────────┐
│ Original Key (original_pk)          │  Immutable, identity anchor
│ - Determined at registration,       │
│   never changes                     │
│ - Used for PDA derivation:          │
│   [b"merchant-did", pk]             │
└──────────────┬──────────────────────┘
               │ Control can be transferred via recovery process
               ▼
┌─────────────────────────────────────┐
│ Controller Key (controller_pk)      │  Rotatable, daily operations
│ - Signs update-vc, set-recovery     │
│   operations                        │
│ - Rotated via rotate-key endpoint   │
└──────────────┬──────────────────────┘
               │ Disaster recovery
               ▼
┌─────────────────────────────────────┐
│ Recovery Key (recovery_pk)          │  Disaster recovery
│ - Can reset controller_pk           │
│ - Recovery key holder signs recover │
│ - Keep securely offline             │
└─────────────────────────────────────┘
```

### 5.3 Platform DID

The Platform DID is derived from the `platform_signing_key` public key:

```
Public key → multicodec(0xed, 0x01) → Base58 → "did:ignite:z" + encoded
```

This DID serves as the `issuer` field for all issued VCs, as well as the `verification_method` prefix in VC proofs.

---

## 6. Dual On-Chain Mode (Sponsored / SelfOnchain)

All DID on-chain operations (register / update-vc / rotate-key) support two on-chain modes, selected via the `mode` field in the request body.

### 6.1 OnchainMode Enum

```rust
pub enum OnchainMode {
    Sponsored,    // Default, platform pays
    SelfOnchain,  // Merchant self-service
}
```

### 6.2 Sponsored Mode (Default, Backward Compatible)

```
┌──────────┐     ┌──────────────┐     ┌────────────┐
│ Merchant │────▶│ did-registry │────▶│ Solana RPC │
└──────────┘     │              │     └────────────┘
                 │ 1. Build instruction   │
                 │ 2. Platform signature  │
                 │ 3. Send transaction    │
                 │ 4. Record fee          │
                 └──────────────┘
```

**Flow**:
1. did-registry signs and sends the transaction using the `payer` keypair
2. After the transaction succeeds, a fee entry is recorded in sled
3. Returns `{ "signature": "..." }`

**Fee Recording**: Each Sponsored operation writes to sled for offline settlement.

### 6.3 SelfOnchain Mode (Merchant Self-Service)

```
┌──────────┐     ┌──────────────┐
│ Merchant │────▶│ did-registry │
└────┬─────┘     │              │
     │           │ 1. Build instruction     │
     │◀──────────│ 2. Get blockhash         │
     │           │ 3. Return unsigned TX    │
     │           └──────────────┘
     │
     │ 4. Sign locally
     │ 5. Broadcast transaction
     ▼
┌────────────┐
│ Solana RPC │
└────────────┘
```

**Flow**:
1. did-registry builds an unsigned `Transaction` (including recent_blockhash)
2. Serializes with bincode, base64 encodes, and returns to the merchant
3. Merchant client: deserializes → signs with their own keypair → broadcasts via RPC

**SelfOnchain Response Format**:

```json
{
  "transaction": "<base64 bincode-encoded unsigned Transaction>",
  "message": "sign and broadcast within 90 seconds; blockhash expires"
}
```

**Merchant Client Processing Steps**:

```rust
// 1. Decode base64
let tx_bytes = base64::decode(&tx_b64)?;
// 2. Deserialize Transaction
let mut tx: Transaction = bincode::deserialize(&tx_bytes)?;
// 3. Sign with merchant keypair
tx.sign(&[&merchant_keypair], tx.message.recent_blockhash);
// 4. Broadcast
let sig = rpc_client.send_and_confirm_transaction(&tx)?;
```

> **Note**: The unsigned transaction contains a `recent_blockhash` that expires after approximately 90 seconds. The merchant must complete signing and broadcasting before expiration.

### 6.4 Mode Field per Endpoint

| Endpoint | Mode Field | SelfOnchain Signer |
|---|---|---|
| `POST /v1/merchants/register` | `mode` (default `sponsored`) | `active_pubkey` (merchant public key in the request) |
| `POST /v1/merchants/update-vc` | `mode` (default `sponsored`) | `controller_pk` (current on-chain controller) |
| `POST /v1/merchants/rotate-key` | `mode` (default `sponsored`) | `controller_pk` (current on-chain controller) |

---

## 7. Storage Architecture

### 7.1 On-Chain (Compressed Storage)

Data is stored in Light Protocol's Merkle Tree and does not occupy traditional on-chain account space. Each `MerchantCompressedDid` is approximately 150 bytes, existing as a hash in a tree leaf node.

**Advantages**:
- No rent-exemption required
- A single tree can store thousands of DIDs
- Transaction costs are significantly lower than traditional accounts

### 7.2 Off-Chain (sled Database)

did-registry uses the embedded sled database, default path `./did_registry_data`.

| Key Pattern | Value | Purpose |
|---|---|---|
| `merchant:{hex(did_hash)}` | Borsh-serialized `MerchantDidAccount` | Merchant DID cache |
| `leaf_index:{hex(did_hash)}` | 4-byte LE u32 | Merkle tree leaf index |
| `vc:{vc_hash_hex}` | Raw JSON | Issued VC storage |
| `fee:{operation}:{timestamp_ms}:{did_hash_hex}` | JSON | Sponsored mode fee records |
| `revoked_vc:{vc_hash_hex}` | JSON | VC revocation record cache |

> `did_hash` = `SHA-256(did_string)`

**Fee Record Format** (automatically written in Sponsored mode):

```json
{
  "merchant_did": "did:ignite:z...",
  "operation": "register",
  "fee_lamports": 5000,
  "timestamp": 1718438400000,
  "mode": "sponsored"
}
```

Fee records can be queried via the `GET /v1/fees` endpoint, with support for filtering by operation type and time range.

---

## 8. API Reference

| Method | Route | Function |
|---|---|---|
| GET | `/health` | Health check |
| GET | `/v1/auth/nonce` | Get anti-replay nonce |
| POST | `/v1/vc/issue` | Issue W3C VC (requires DID signature verification) |
| POST | `/v1/vc/revoke` | Revoke VC (platform authority only, creates on-chain RevokedVc PDA) |
| POST | `/v1/proof` | Get ZK proof (public endpoint, no auth required) |
| POST | `/v1/merchants/register` | Register on-chain compressed DID (supports `mode` field) |
| POST | `/v1/merchants/confirm` | SelfOnchain registration confirmation (merchant notifies platform after broadcasting) |
| GET | `/v1/did/resolve/{did}` | Resolve DID Document |
| GET | `/v1/merchants/verify/{did}` | Verify merchant DID |
| GET | `/v1/merchants/status/{did}` | Query merchant status |
| POST | `/v1/merchants/update-vc` | Update on-chain VC hash (supports `mode` field) |
| POST | `/v1/merchants/rotate-key` | Rotate control key (supports `mode` field) |
| GET | `/v1/fees` | Query fee records |

### 8.1 GET /v1/auth/nonce

Obtains a one-time nonce, valid for 5 minutes, used for anti-replay protection in subsequent requests.

**Response**:
```json
{
  "nonce": "550e8400-e29b-41d4-a716-446655440000",
  "expires_in": 300
}
```

### 8.2 POST /v1/vc/issue

Platform issues a W3C Verifiable Credential. Requires DID signature verification of identity ownership. If the merchant is already registered, it also verifies that the signer is the controller or original key.

**Request**:
```json
{
  "merchant_did": "did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK",
  "merchant_name": "Example Store",
  "category": "retail",
  "validity_hours": 8760,
  "nonce": "<server-issued-nonce>",
  "did_signature": "<base64-Ed25519-sig>"
}
```

**Response**:
```json
{
  "verifiable_credential": {
    "@context": [
      "https://www.w3.org/2018/credentials/v1",
      "https://ignite-pay.com/credentials/v1"
    ],
    "id": "urn:uuid:...",
    "type": ["VerifiableCredential", "MerchantAttestation"],
    "issuer": "did:ignite:z<platform>",
    "issuanceDate": "2025-01-01T00:00:00Z",
    "expirationDate": "2026-01-01T00:00:00Z",
    "credentialSubject": {
      "id": "did:ignite:z<merchant>",
      "name": "Example Store",
      "category": "retail"
    },
    "credentialStatus": {
      "type": "IgniteVcRevocationRegistry",
      "program_id": "<DID Program ID>"
    },
    "proof": {
      "type": "Ed25519Signature2020",
      "created": "2025-01-01T00:00:00Z",
      "proofPurpose": "assertionMethod",
      "verificationMethod": "did:ignite:z<platform>#key-signing-1",
      "proofValue": "<base64-signature>"
    }
  },
  "vc_hash": "<sha256-hex-of-vc-json>"
}
```

### 8.3 POST /v1/vc/revoke

Revokes an issued VC. Only platform authority can call this. Creates a `RevokedVc` PDA on-chain and caches the revocation record in sled.

**Request**:
```json
{
  "vc_hash": "<hex 32 bytes, hash of the VC being revoked>",
  "credential_subject_pk": "<VC subject public key (base58)>",
  "reason": 1,
  "platform_signature": "<base64 signature, message: revoke:{vc_hash}:{nonce}>",
  "nonce": "<server-nonce>"
}
```

- `reason`: Revocation reason code (0=unspecified, 1=violation, 2=expired, etc.)
- `platform_signature`: Platform Ed25519 signature, message format `revoke:{vc_hash}:{nonce}`

**Response**:
```json
{
  "signature": "<solana-tx-signature>",
  "revoked_vc_pda": "<RevokedVc PDA address (base58)>"
}
```

**Verifier Revocation Check**: Third parties use `find_program_address(&[b"revoked-vc", vc_hash], program_id)` to derive the PDA address and query whether the account exists. If it exists, the VC has been revoked.

### 8.4 POST /v1/merchants/register

Registers a merchant DID as an on-chain compressed account. Supports dual on-chain mode.

**Request**:
```json
{
  "merchant_did": "did:ignite:z...",
  "active_pubkey": "<Solana-base58-pubkey>",
  "platform_vc_hash": "<hex-32-bytes>",
  "did_signature": "<base64-Ed25519-sig>",
  "nonce": "<server-nonce>",
  "mode": "sponsored"
}
```

- `mode`: Optional, default `"sponsored"`. Possible values: `"sponsored"` | `"self_onchain"`

Signature message format: `register:{merchant_did}:{active_pubkey}:{platform_vc_hash}:{nonce}`

**Sponsored Mode Response**:
```json
{
  "signature": "<solana-tx-signature>"
}
```

**SelfOnchain Mode Response**:
```json
{
  "transaction": "<base64-bincode-encoded unsigned Transaction>",
  "message": "sign and broadcast within 90 seconds; blockhash expires"
}
```

### 8.5 GET /v1/did/resolve/{did}

Resolves a DID into a W3C DID Document.

**Response**:
```json
{
  "@context": ["https://www.w3.org/ns/did/v1"],
  "id": "did:ignite:z...",
  "verificationMethod": [{
    "id": "did:ignite:z...#key-signing-1",
    "type": "Ed25519VerificationKey2020",
    "controller": "did:ignite:z...",
    "publicKeyMultibase": "z..."
  }],
  "controller_pubkey": "<base58>",
  "original_pubkey": "<base58>",
  "last_updated": 1700000000
}
```

### 8.6 POST /v1/merchants/update-vc

Updates the VC hash of the on-chain compressed DID. Supports dual on-chain mode.

**Request**:
```json
{
  "merchant_did": "did:ignite:z...",
  "new_vc_hash": "<hex-32-bytes>",
  "platform_signature": "<base64-sig>",
  "nonce": "<server-nonce>",
  "account_meta_b64": "<optional-base64-borsh-CompressedAccountMeta>",
  "mode": "sponsored"
}
```

- `mode`: Optional, default `"sponsored"`. In SelfOnchain mode, the signer is the current `controller_pk`

Signature message format: `update-vc:{merchant_did}:{new_vc_hash}:{nonce}`

### 8.7 POST /v1/merchants/rotate-key

Rotates the merchant control key. Supports dual on-chain mode.

**Request**:
```json
{
  "merchant_did": "did:ignite:z...",
  "new_active_pubkey": "<base58>",
  "did_signature": "<base64-sig>",
  "nonce": "<server-nonce>",
  "account_meta_b64": "<optional>",
  "mode": "sponsored"
}
```

- `mode`: Optional, default `"sponsored"`. In SelfOnchain mode, the signer is the current `controller_pk`

Signature message format: `rotate-key:{merchant_did}:{new_active_pubkey}:{nonce}`

### 8.8 GET /v1/fees

Queries fee records generated in Sponsored mode.

**Query Parameters**:

| Parameter | Type | Default | Description |
|---|---|---|---|
| `operation` | string | None (all) | Filter by operation type: `register` / `update_vc` / `rotate_key` |
| `since` | int64 | None (all) | Only return records after this timestamp (Unix milliseconds) |
| `limit` | int | 100 | Maximum number of records to return |

**Request Example**:
```bash
curl "http://localhost:8081/v1/fees?operation=register&since=1718438400000&limit=50"
```

**Response**:
```json
{
  "fees": [
    {
      "merchant_did": "did:ignite:z...",
      "operation": "register",
      "fee_lamports": 5000,
      "timestamp": 1718438400000,
      "mode": "sponsored"
    }
  ]
}
```

### 8.9 POST /v1/proof

Public endpoint to obtain a ZK Compression validity proof. No authentication required. Merchants use the returned proof data to build and sign transactions locally.

**Request**:
```json
{
  "pubkey": "<Merchant Solana public key (base58)>",
  "operation": "register",
  "account_hash": "<hex 32 bytes, required for update_vc/rotate_key>"
}
```

- `operation`: `"register"` | `"update_vc"` | `"rotate_key"`
- `account_hash`: Only required for `update_vc` and `rotate_key` (hash of existing compressed account)

**Response**:
```json
{
  "proof": "<base64 borsh-serialized ZK proof>",
  "compressed_address": "<base58>",
  "address_seed": "<base58>",
  "address_merkle_tree": "<base58>",
  "address_tree_info": "<base64 borsh-serialized>",
  "output_state_tree_index": 0,
  "remaining_accounts": [
    { "pubkey": "...", "is_signer": false, "is_writable": true }
  ],
  "program_id": "DID Program ID (base58)",
  "platform_config_address": "PlatformConfig PDA address (base58)"
}
```

> `platform_config_address` must be passed as the second account in the accounts list (readonly) to the `initialize_did` and `update_did_with_vc` instructions.

### 8.10 POST /v1/merchants/confirm

For SelfOnchain mode only. After the merchant successfully broadcasts a transaction, they notify the platform to cache the merchant data, enabling subsequent operations (verify/status/update-vc/rotate-key).

**Request**:
```json
{
  "merchant_did": "did:ignite:z...",
  "tx_signature": "<Solana transaction signature (base58)>",
  "active_pubkey": "<Merchant public key (base58)>",
  "platform_vc_hash": "<hex 32 bytes>",
  "did_signature": "<base64 signature, message: confirm:{did}:{tx_signature}:{nonce}>",
  "nonce": "<server-nonce>"
}
```

**Response**:
```json
{ "status": "confirmed" }
```

Idempotent: if the merchant is already cached, returns `{ "status": "already_confirmed" }`.

---

## 9. Security Considerations

### 9.0 Platform Signature Verification (Anti-Replay + Anti-Impersonation)

The on-chain program stores the platform Ed25519 public key in the `PlatformConfig` PDA. `initialize_did` and `update_did_with_vc` perform two-layer verification before writing vc_hash:

1. **Subject Binding**: `credential_subject_pk == signer.key()` — ensures the submitter is the VC's subject
2. **Platform Signature Verification**: `verify(platform_pubkey, credential_subject_pk || vc_hash, platform_signature)` — ensures the platform has authorized this binding

Even if an attacker intercepts `(vc_hash, platform_signature, credential_subject_pk)`, they cannot submit it with their own signer:
- If using the original `credential_subject_pk`, the subject binding check fails (signer mismatch)
- If tampering with `credential_subject_pk`, the platform signature verification fails (signature message mismatch)

### 9.0b VC Revocation Mechanism

The platform can revoke an issued VC via `POST /v1/vc/revoke`. The revocation flow:

1. **On-chain**: Calls the `revoke_vc` instruction, creating a `RevokedVc` PDA (seeds: `[b"revoked-vc", vc_hash]`)
2. **Off-chain**: Caches the revocation record in sled (`revoked_vc:{vc_hash_hex}`)
3. **In VC**: Each issued VC contains a `credentialStatus` field pointing to the on-chain revocation registry

**Verifier Check Flow**:
1. Verify the VC's Ed25519 signature and validity period
2. Extract `credentialStatus.program_id` from the VC (DID program address)
3. Compute `vc_hash = SHA-256(vc_json)`
4. Derive PDA: `find_program_address(&[b"revoked-vc", vc_hash], program_id)`
5. Query whether the PDA exists — if it exists, the VC has been revoked

**Access Control**: Only `PlatformConfig.authority` can call `revoke_vc`, preventing unauthorized revocation.

### 9.1 Key Management

- `platform_signing.key` must be kept secure; leakage means anyone can issue forged VCs
- `payer_keypair.json` requires regular SOL balance replenishment
- Hardware Security Modules (HSM) or Key Management Services (KMS) are recommended for managing production keys

### 9.2 Nonce Mechanism

- Server nonce is valid for 5 minutes and destroyed after single use
- On-chain nonce is an incrementing counter, incremented by 1 for each mutation operation
- Dual-layer nonce design prevents cross-domain replay attacks

### 9.3 ZK Compression Considerations

- Photon RPC is part of the trust assumption — if the indexer provides fraudulent proofs, transactions may fail
- Compressed account data is not stored directly on-chain and must be read through an indexer
- Reliable Photon RPC providers such as Helius are recommended

### 9.4 Network Security

- In production, deploy a reverse proxy (nginx/caddy) in front of did-registry
- Enable TLS (HTTPS)
- Consider adding rate limiting
- `jwt_secret` should use a strong random value

### 9.5 SelfOnchain Mode Security

- Unsigned transactions contain a `recent_blockhash` that expires after approximately 90 seconds; merchants must complete signing and broadcasting within this window
- The platform does not record fees for SelfOnchain mode (only Sponsored mode records fees)
- In SelfOnchain mode, the merchant assumes full responsibility for transaction signing and broadcasting
- Merchants should implement a timeout retry mechanism in their client: if the blockhash expires, re-request an unsigned transaction
- `POST /v1/proof` is a public endpoint; anyone can obtain a ZK proof, but building a transaction still requires the merchant's private key for signing
- After broadcasting, SelfOnchain merchants **must** call `POST /v1/merchants/confirm` to notify the platform, otherwise subsequent operations will not be available

### 9.6 VC Issuance Security

- `POST /v1/vc/issue` requires a DID signature (`issue_vc:{did}:{merchant_name}:{nonce}`), ensuring the requester holds the DID private key
- For updates: the platform verifies whether the signer is the current controller or original key, preventing unauthorized parties from requesting new VCs
- For first-time issuance (merchant not registered): only DID signature is verified; registration is not required
