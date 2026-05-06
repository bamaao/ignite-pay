# DID PDA Migration Guide

## Overview

This document describes the migration from ZK Compression (Light Protocol) to standard Solana PDA for on-chain merchant DID storage.

## Architecture Comparison

| Aspect | ZK Compression | PDA |
|--------|---------------|-----|
| Account type | CompressedAccount (event) | Standard `#[account]` PDA |
| Storage cost | ~0.0004 SOL/account | ~0.0015 SOL/account (153 bytes) |
| Query method | Photon RPC (`getCompressedAccount`) | Standard RPC (`getAccount`) |
| Proof mechanism | ValidityProof + AddressTree | No proof needed |
| Dependencies | Light Protocol, Photon RPC | No extra dependencies |
| Address derivation | `light_sdk::derive_address` | `Pubkey::find_program_address` |
| Seeds | `[b"merchant-did", original_pk]` | `[b"merchant-did", original_pk]` |

## Feature Flag Usage

All crates share the `zk-compression` feature:

```bash
# Default build (PDA mode, no Light SDK dependency)
cargo build

# Build ZK Compression variant (preserves legacy code)
cargo build --features zk-compression
```

Affected crates:
- `ignite-pay-did-program` — On-chain program
- `ignite-pay-solana` — SDK
- `ignite-pay-core` — Shared library
- `did-registry` — REST API service
- `ignite-pay-mcp` — MCP server

## PDA Structure

### MerchantDidAccount

```
Seeds: [b"merchant-did", original_pk]
Space: 153 bytes (8 discriminator + 145 data)
```

| Field | Type | Size | Description |
|-------|------|------|-------------|
| original_pk | Pubkey | 32 | Initial public key (immutable) |
| controller_pk | Pubkey | 32 | Current controller public key |
| recovery_pk | Pubkey | 32 | Recovery public key |
| vc_hash | [u8; 32] | 32 | VC hash |
| last_updated | i64 | 8 | Last update timestamp |
| nonce | u64 | 8 | Anti-replay counter |
| bump | u8 | 1 | PDA bump |

## Instruction Comparison

### initialize_did

| Parameter | ZK Version | PDA Version |
|-----------|-----------|-------------|
| proof | ValidityProof | — |
| address_tree_info | PackedAddressTreeInfo | — |
| output_state_tree_index | u8 | — |
| vc_hash | [u8; 32] | [u8; 32] |
| platform_signature | [u8; 64] | [u8; 64] |
| credential_subject_pk | Pubkey | Pubkey |

The PDA version removes all proof/tree-related parameters.

## Migration Guide

### Deployment Migration

Migrating from ZK Compression to PDA requires:

1. **Redeploy the on-chain program**: Compile `ignite-pay-did-program` with default features
2. **Initialize PlatformConfig**: Execute the `init_platform` instruction
3. **Re-register merchants**: PDA addresses differ from compressed addresses; merchants must re-register
4. **Update did-registry config**: Remove the `[light]` configuration section
5. **Update MCP config**: Remove `photon_url`, `address_tree`, and other ZK fields

### Rollback to ZK Compression

```bash
# Recompile all crates with the zk-compression feature
cargo build --features zk-compression
```

### What Remains Unchanged

The following components require no changes:
- VC (Verifiable Credential) system — Platform still issues W3C VCs
- Platform signature verification — `sign(credential_subject_pk || vc_hash)` unchanged
- Mobile App
- Merchant MCP
- DIDComm messaging protocol
- IPFS storage
