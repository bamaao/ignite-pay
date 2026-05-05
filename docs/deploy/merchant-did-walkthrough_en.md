# Merchant Digital Identity On-Chain — Business Use Case

This document provides a complete end-to-end walkthrough demonstrating how a merchant can complete digital identity registration, VC issuance, on-chain anchoring, key rotation, and recovery through the Ignite Pay platform.

---

## Use Case Scenario

**Merchant**: "Spark Convenience Store"
**Operator**: Merchant administrator
**Objective**: Complete digital identity registration on the Ignite Pay platform, obtain a Verifiable Credential issued by the platform, and anchor the identity hash on the Solana blockchain.

This document provides complete examples for two on-chain modes:
- **Mode A (Sponsored — Platform Pays)**: The platform signs and sends the transaction, and records a service fee.
- **Mode B (SelfOnchain — Merchant Self-Service)**: The merchant obtains a ZK proof via the public proof endpoint, constructs the transaction locally, signs and broadcasts it, then notifies the platform upon completion.

---

## Prerequisites

- The did-registry service is deployed and running at `http://localhost:8081`
- The ignite-pay-did-program has been deployed to Solana Devnet
- The merchant has generated an Ed25519 key pair and a `did:ignite` identifier locally

---

## Complete Flow

### Step 1: Merchant Generates Key Pair and DID Locally

The merchant client generates an Ed25519 key pair locally and derives the `did:ignite` identifier through multicodec encoding.

```bash
# Generate a 32-byte Ed25519 private key
openssl rand -out merchant_private.key 32
```

The DID identifier derivation rule:

```
Public key (32 bytes)
    → Add multicodec prefix: [0xed, 0x01] + pubkey (34 bytes total)
    → Base58 encode
    → Concatenate prefix: "did:ignite:z" + encoded
```

Assuming the generated DID is:
```
did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK
```

The corresponding Solana public key (active_pubkey) is the key pair's Solana-format public key:
```
7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU
```

---

### Step 2: Obtain Server Nonce

```bash
curl -s http://localhost:8081/v1/auth/nonce | jq
```

**Response**:
```json
{
  "nonce": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
  "expires_in": 300
}
```

> Nonce is valid for 5 minutes and is single-use.

---

### Step 3: Platform Issues Verifiable Credential

Using the nonce from the previous step, the merchant signs with their DID private key and requests the platform to issue a VC.

Signed message format: `issue_vc:{merchant_did}:{merchant_name}:{nonce}`

```bash
# Merchant local signing (pseudocode)
# message = "issue_vc:did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK:Spark Convenience Store:a1b2c3d4-e5f6-7890-abcd-ef1234567890"
# did_signature = ed25519_sign(merchant_private_key, message)

curl -s -X POST http://localhost:8081/v1/vc/issue \
  -H "Content-Type: application/json" \
  -d '{
    "merchant_did": "did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK",
    "merchant_name": "Spark Convenience Store",
    "category": "retail",
    "validity_hours": 8760,
    "nonce": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
    "did_signature": "<base64-Ed25519-sig>"
  }' | jq
```

**Response**:
```json
{
  "verifiable_credential": {
    "@context": [
      "https://www.w3.org/2018/credentials/v1",
      "https://ignite-pay.com/credentials/v1"
    ],
    "id": "urn:uuid:f47ac10b-58cc-4372-a567-0e02b2c3d479",
    "type": ["VerifiableCredential", "MerchantAttestation"],
    "issuer": "did:ignite:z6MkplatformPublicKeyEncoded...",
    "issuanceDate": "2025-06-15T08:30:00Z",
    "expirationDate": "2026-06-15T08:30:00Z",
    "credentialSubject": {
      "id": "did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK",
      "name": "Spark Convenience Store",
      "category": "retail"
    },
    "credentialStatus": {
      "type": "IgniteVcRevocationRegistry",
      "program_id": "<DID Program ID>"
    },
    "proof": {
      "type": "Ed25519Signature2020",
      "created": "2025-06-15T08:30:00Z",
      "proofPurpose": "assertionMethod",
      "verificationMethod": "did:ignite:z6MkplatformPublicKeyEncoded...#key-signing-1",
      "proofValue": "UXJhvK3n2pR8wN7eQm..."
    }
  },
  "vc_hash": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
}
```

**Key information extracted**:
- `vc_hash`: `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`
- The VC has been signed by the platform's Ed25519 private key
- The VC has been persisted to the sled database (key: `vc:{vc_hash_hex}`)
- The platform verified the DID signature, confirming the requester holds the private key for that DID

---

### Step 4: Obtain a New Nonce (for Registration)

```bash
curl -s http://localhost:8081/v1/auth/nonce | jq
```

**Response**:
```json
{
  "nonce": "b2c3d4e5-f6a7-8901-bcde-f12345678901",
  "expires_in": 300
}
```

---

### Step 5: Merchant Signs Registration Message

The merchant uses their local Ed25519 private key to sign the following structured message:

```
register:did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK:7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855:b2c3d4e5-f6a7-8901-bcde-f12345678901
```

Format: `register:{merchant_did}:{active_pubkey}:{platform_vc_hash}:{nonce}`

Signature result (example):
```
did_signature = "j7Kd8xR2mN3pQ5vW9yA0bC4fG6hIjLkMnOpQrStUvWxYzA1B2C3D4E5F6G7H8I9J0K1L2M3N4O5P6Q7R8S9T0U1V2W3X4Y5Z6=="
```

---

### Step 6: Submit On-Chain Registration

#### Mode A: Sponsored (Platform Pays, Default)

```bash
curl -s -X POST http://localhost:8081/v1/merchants/register \
  -H "Content-Type: application/json" \
  -d '{
    "merchant_did": "did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK",
    "active_pubkey": "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU",
    "platform_vc_hash": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    "did_signature": "j7Kd8xR2mN3pQ5vW9yA0bC4fG6hIjLkMnOpQrStUvWxYzA1B2C3D4E5F6G7H8I9J0K1L2M3N4O5P6Q7R8S9T0U1V2W3X4Y5Z6==",
    "nonce": "b2c3d4e5-f6a7-8901-bcde-f12345678901",
    "mode": "sponsored"
  }' | jq
```

> The `mode` field is optional; it defaults to `sponsored` (backward compatible) and can be omitted.

**Server-side processing flow**:
1. Verify `merchant_did` starts with `did:ignite:`
2. Consume the nonce (prevent replay)
3. Verify the DID signature (extract public key from DID, verify Ed25519 signature)
4. Parse `active_pubkey` and `vc_hash`
5. Obtain a ZK Compression validity proof from the Photon RPC
6. Derive the compressed PDA address: `seeds = [b"merchant-did", active_pubkey]`
7. Call `DidService::initialize_did` to send the on-chain transaction (signed by platform payer)
8. Cache the merchant DID to sled
9. Record the service fee to sled (`fee:register:{ts}:{did_hash_hex}`)

**Response**:
```json
{
  "signature": "5Jj8nL2kP4mN6qR8sT0uV2wX4yZ6aB8cD0eF2gH4iJ6kL8mN0oP2qR4sT6uV8wX0yZ2aB4cD6eF8gH0iJ2kL4mN6oP8qR0sT2uV4wX6yZ8aB0cD2eF4gH6i"
}
```

#### Mode B: SelfOnchain (Merchant Self-Service On-Chain)

Method 1: Obtain an unsigned transaction via the register endpoint

```bash
curl -s -X POST http://localhost:8081/v1/merchants/register \
  -H "Content-Type: application/json" \
  -d '{
    "merchant_did": "did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK",
    "active_pubkey": "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU",
    "platform_vc_hash": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    "did_signature": "j7Kd8xR2mN3pQ5vW9yA0bC4fG6hIjLkMnOpQrStUvWxYzA1B2C3D4E5F6G7H8I9J0K1L2M3N4O5P6Q7R8S9T0U1V2W3X4Y5Z6==",
    "nonce": "b2c3d4e5-f6a7-8901-bcde-f12345678901",
    "mode": "self_onchain"
  }' | jq
```

Method 2: Obtain a ZK proof via the public proof endpoint, and construct the transaction locally

```bash
# Obtain proof (no authentication required)
curl -s -X POST http://localhost:8081/v1/proof \
  -H "Content-Type: application/json" \
  -d '{
    "pubkey": "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU",
    "operation": "register"
  }' | jq
```

**Response (additional fields)**:
```json
{
  "proof": "<base64>",
  "compressed_address": "<base58>",
  "address_seed": "<base58>",
  "address_merkle_tree": "<base58>",
  "address_tree_info": "<base64>",
  "output_state_tree_index": 0,
  "remaining_accounts": [
    { "pubkey": "...", "is_signer": false, "is_writable": true }
  ],
  "program_id": "DID Program ID (base58)",
  "platform_config_address": "PlatformConfig PDA address (base58)"
}
```

> `platform_config_address` is the PlatformConfig PDA address. When constructing `initialize_did` / `update_did_with_vc` instructions, the accounts list must be `[signer(writable), platform_config(readonly), ...remaining_accounts]`. The instruction data must include `vc_hash(32) + platform_signature(64) + credential_subject_pk(32)`.

**Server-side processing flow**:
1. Same as steps 1-6 above (verification, nonce, signature, proof)
2. Generate platform signature: `sign(credential_subject_pk || vc_hash)`
3. Call `DidService::prepare_initialize_did` to build an unsigned transaction (including the platform_config account and platform signature)
4. Serialize using bincode, return base64-encoded

**Response** (Method 1):
```json
{
  "transaction": "AQAAAAAAAAABAAABA4njKdHxaNnCoWmNk9p5WjnUk4KwbbOGrFckyTSDj5k7CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAmr7Vi8Y0JB3X7JYBmSvvyLIj3j8WLXQirU1pImYJE4YBAAAAAAKCAQIDBAUG...",
  "message": "sign and broadcast within 90 seconds; blockhash expires"
}
```

**Merchant client processing** (Rust example):

```rust
use solana_sdk::transaction::Transaction;
use solana_client::rpc_client::RpcClient;

// Method 1: Decode the unsigned transaction returned by the platform
let tx_bytes = base64::engine::general_purpose::STANDARD.decode(&tx_b64)?;
let mut tx: Transaction = bincode::deserialize(&tx_bytes)?;
tx.sign(&[&merchant_keypair], tx.message.recent_blockhash);
let rpc_client = RpcClient::new("https://api.devnet.solana.com");
let sig = rpc_client.send_and_confirm_transaction(&tx)?;

// Method 2: Build transaction locally using proof (requires light-sdk + ignite-pay-did-program IDL)
// 1. Decode proof, address_tree_info, remaining_accounts
// 2. Build Anchor instruction:
//    discriminator(8) + proof + address_tree_info(borsh) + output_state_tree_index(1)
//    + vc_hash(32) + platform_signature(64) + credential_subject_pk(32)
// 3. accounts: [signer(writable), platform_config(readonly), ...remaining_accounts]
//    where platform_config address is obtained from the platform_config_address field in the /v1/proof response
// 4. Transaction::new_unsigned(message) → sign → broadcast
// Note: platform_signature and credential_subject_pk must be obtained from the platform (issued by the platform)
```

> **Note**: In SelfOnchain mode, the merchant must complete signing and broadcasting within 90 seconds (blockhash expiration limit). If the timeout is exceeded, a new unsigned transaction must be requested.

**Important: Must Notify Platform After Broadcasting**

In SelfOnchain mode, the platform does not participate in the transaction and therefore does not know the merchant has gone on-chain. After a successful broadcast, the merchant must call the confirm endpoint:

```bash
# Get a new nonce
NONCE=$(curl -s http://localhost:8081/v1/auth/nonce | jq -r '.nonce')

# Merchant signature: "confirm:{did}:{tx_signature}:{nonce}"

curl -s -X POST http://localhost:8081/v1/merchants/confirm \
  -H "Content-Type: application/json" \
  -d '{
    "merchant_did": "did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK",
    "tx_signature": "'"${TX_SIG}"'",
    "active_pubkey": "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU",
    "platform_vc_hash": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    "did_signature": "<base64 signature>",
    "nonce": "'"${NONCE}"'"
  }' | jq
```

**Response**:
```json
{ "status": "confirmed" }
```

> If the merchant is already cached, the response is `{ "status": "already_confirmed" }` (idempotent). Until confirm is called, verify/status/update-vc/rotate-key all return 404.

At this point, a `MerchantCompressedDid` compressed account has been created on-chain:
```
original_pk   = 7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU
controller_pk = 7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU  (= original_pk)
recovery_pk   = 11111111111111111111111111111111                 (not set)
vc_hash       = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
nonce         = 0
```

---

### Step 7: Verify On-Chain State

```bash
curl -s http://localhost:8081/v1/merchants/verify/did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK | jq
```

**Response**:
```json
{
  "verified": true,
  "original_pubkey": "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU",
  "controller_pubkey": "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU",
  "last_updated": 1718438400
}
```

---

### Step 8: Resolve DID Document

```bash
curl -s http://localhost:8081/v1/did/resolve/did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK | jq
```

**Response**:
```json
{
  "@context": ["https://www.w3.org/ns/did/v1"],
  "id": "did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK",
  "verificationMethod": [{
    "id": "did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK#key-signing-1",
    "type": "Ed25519VerificationKey2020",
    "controller": "did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK",
    "publicKeyMultibase": "z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK"
  }],
  "controller_pubkey": "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU",
  "original_pubkey": "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU",
  "last_updated": 1718438400
}
```

---

## Subsequent Operations

### Update VC Hash

When a VC expires and is renewed, or its content changes, the on-chain `vc_hash` needs to be updated.

```bash
# 1. Get nonce
NONCE=$(curl -s http://localhost:8081/v1/auth/nonce | jq -r '.nonce')

# 2. Platform signature: "update-vc:{did}:{new_vc_hash}:{nonce}"
#    Here the platform signs using platform_signing_key

# 3. Submit (Sponsored mode)
curl -s -X POST http://localhost:8081/v1/merchants/update-vc \
  -H "Content-Type: application/json" \
  -d '{
    "merchant_did": "did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK",
    "new_vc_hash": "<new 32-byte hex>",
    "platform_signature": "<platform base64 signature>",
    "nonce": "'"${NONCE}"'",
    "mode": "sponsored"
  }'
```

The on-chain nonce increments from 0 to 1. In Sponsored mode, the `update_vc` fee is recorded in sled.

If the merchant wishes to sign and broadcast themselves, set `"mode": "self_onchain"`, and the platform returns an unsigned transaction:

```bash
# SelfOnchain mode: returns { "transaction": "<base64>", "message": "..." }
curl -s -X POST http://localhost:8081/v1/merchants/update-vc \
  -H "Content-Type: application/json" \
  -d '{
    "merchant_did": "did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK",
    "new_vc_hash": "<new 32-byte hex>",
    "platform_signature": "<platform base64 signature>",
    "nonce": "'"${NONCE}"'",
    "mode": "self_onchain"
  }'
```

In SelfOnchain mode, the signer is the current on-chain `controller_pk`; the merchant must hold the corresponding private key.

### Set Recovery Key

The merchant should set a recovery key as soon as possible, in case the controller key is lost.

```bash
NONCE=$(curl -s http://localhost:8081/v1/auth/nonce | jq -r '.nonce')

# Merchant signature: "rotate-key:{did}:{recovery_pubkey}:{nonce}"

# Sponsored mode (default)
curl -s -X POST http://localhost:8081/v1/merchants/rotate-key \
  -H "Content-Type: application/json" \
  -d '{
    "merchant_did": "did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK",
    "new_active_pubkey": "<RECOVERY_KEY_BASE58>",
    "did_signature": "<merchant base64 signature>",
    "nonce": "'"${NONCE}"'"
  }'
```

The on-chain nonce increments, and `recovery_pk` is set. In Sponsored mode, a `rotate_key` fee is recorded.

### Query Fee Records

View service fees generated by the platform paying on behalf of the merchant:

```bash
# Query fees for all operations
curl -s "http://localhost:8081/v1/fees" | jq

# Query registration fees only
curl -s "http://localhost:8081/v1/fees?operation=register" | jq

# Query fees after a specified time
curl -s "http://localhost:8081/v1/fees?since=1718438400000&limit=50" | jq

# Query VC update fees
curl -s "http://localhost:8081/v1/fees?operation=update_vc&limit=20" | jq
```

**Response example**:
```json
{
  "fees": [
    {
      "merchant_did": "did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK",
      "operation": "register",
      "fee_lamports": 5000,
      "timestamp": 1718438400000,
      "mode": "sponsored"
    },
    {
      "merchant_did": "did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK",
      "operation": "update_vc",
      "fee_lamports": 2000,
      "timestamp": 1718438500000,
      "mode": "sponsored"
    }
  ]
}
```

> SelfOnchain mode does not generate fee records (the merchant bears on-chain costs themselves).

### Disaster Recovery

When the controller key is lost, use the recovery key to recover:

1. Sign the `recover_controller` instruction with the recovery key
2. Call `DidService::recover_controller` directly (or via a future API endpoint)
3. Set a new `controller_pk`
4. On-chain nonce increments

### VC Revocation

When a merchant violates rules or a VC needs to be invalidated before expiration, the platform can revoke the VC:

```bash
# 1. Get nonce
NONCE=$(curl -s http://localhost:8081/v1/auth/nonce | jq -r '.nonce')

# 2. Platform signature: "revoke:{vc_hash}:{nonce}"
#    Sign using platform_signing_key

# 3. Submit revocation
curl -s -X POST http://localhost:8081/v1/vc/revoke \
  -H "Content-Type: application/json" \
  -d '{
    "vc_hash": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    "credential_subject_pk": "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU",
    "reason": 1,
    "platform_signature": "<platform base64 signature>",
    "nonce": "'"${NONCE}"'"
  }' | jq
```

**Response**:
```json
{
  "signature": "<solana-tx-signature>",
  "revoked_vc_pda": "<RevokedVc PDA address>"
}
```

**How a verifier checks**: After receiving a VC, a third party can:
1. Compute `vc_hash = SHA-256(vc_json)`
2. Derive the PDA: `find_program_address(&[b"revoked-vc", vc_hash], program_id)`
3. Query whether the PDA exists — if it exists, the VC has been revoked

---

## State Flow Diagram

```
  ┌────────────────────────────────────────────────────────────┐
  │  One-time at deployment: init_platform                     │
  │  → Write platform Ed25519 public key to PlatformConfig PDA │
  │  → seeds: [b"platform-config"]                             │
  │  → initialize_did / update_did_with_vc rejected if not init│
  └────────────────────────────────────────────────────────────┘

                     ┌───────────────────────┐
                     │  Merchant generates    │
                     │  key pair              │
                     │  → did:ignite:z...     │
                     └───────────┬───────────┘
                                 │
                     ┌───────────▼───────────┐
                     │  GET /v1/auth/nonce    │
                     │  → nonce (5min TTL)    │
                     └───────────┬───────────┘
                                 │
                     ┌───────────▼───────────┐
                     │  Merchant signs        │
                     │  "issue_vc:{did}:      │
                     │   {name}:{nonce}"      │
                     └───────────┬───────────┘
                                 │
                     ┌───────────▼───────────┐
                     │  POST /v1/vc/issue     │
                     │  + did_signature       │
                     │  → Platform verifies   │
                     │    DID ownership       │
                     │  → VC + vc_hash        │
                     └───────────┬───────────┘
                                 │
                     ┌───────────▼───────────┐
                     │  GET /v1/auth/nonce    │
                     │  → new nonce           │
                     └───────────┬───────────┘
                                 │
                     ┌───────────▼───────────┐
                     │  Merchant signs        │
                     │  locally               │
                     │  "register:{...}"      │
                     └───────────┬───────────┘
                                 │
              ┌──────────────────▼──────────────────┐
              │  POST /v1/merchants/register         │
              │  mode = "sponsored" | "self_onchain" │
              │  → Platform signs (subject_pk||vc_hash)│
              │  → Photon proof                      │
              ├─────────────┬────────────────────────┤
              │  Sponsored  │  SelfOnchain            │
              │  Platform   │  Returns unsigned TX    │
              │  signs +    │  Merchant self-signs    │
              │  sends +    │  + broadcasts           │
              │  records fee│                         │
              └─────────────┴────────────────────────┘
                                 │
              → On-chain verification:
                ① subject_binding: credential_subject_pk == signer
                ② platform_sig: verify(platform_pk, subject_pk||vc_hash, sig)
              → MerchantCompressedDid created
                original_pk = controller_pk = signer
                vc_hash = VC hash, nonce = 0
                                 │
                     ┌───────────▼───────────┐
                     │  SelfOnchain only:     │
                     │  POST /v1/merchants/   │
                     │       confirm          │
                     │  → Notify platform to  │
                     │    cache merchant data │
                     └───────────┬───────────┘
                                 │
           ┌─────────────────────┼─────────────────────┐
           │                     │                     │
  ┌────────▼────────┐  ┌────────▼────────┐  ┌────────▼────────┐
  │ update-vc       │  │ set-recovery    │  │ rotate-key      │
  │ nonce 0→1→...   │  │ nonce increments│  │ nonce increments│
  │ update vc_hash  │  │ set recovery    │  │ update controller│
  │ +platform sig   │  │                 │  │ (both modes     │
  │  verification   │  │                 │  │  supported)     │
  │ +subject binding│  │                 │  │                 │
  │ (both modes     │  │                 │  │                 │
  │  supported)     │  │                 │  │                 │
  └─────────────────┘  └─────────────────┘  └─────────────────┘

  ┌────────────────────────────────────────────────────────────┐
  │  POST /v1/proof (public endpoint)                          │
  │  → Get ZK proof + platform_config_address                  │
  │  → Merchant can build transaction independently            │
  │    (requires platform signature data)                      │
  │  → Can also use light-sdk + self-hosted Photon RPC for     │
  │    full independence                                       │
  └────────────────────────────────────────────────────────────┘

  ┌────────────────────────────────────────────────────────────┐
  │  GET /v1/fees?operation=register&since=ts&limit=100        │
  │  → Query Sponsored mode fee records (for offline           │
  │    settlement)                                             │
  └────────────────────────────────────────────────────────────┘
```

---

## Verification Checklist

After deployment is complete, verify in order:

| # | Check Item | Command | Expected |
|---|---|---|---|
| 1 | Service health | `curl localhost:8081/health` | `ok` |
| 2 | Nonce issuance | `curl localhost:8081/v1/auth/nonce` | 200 + UUID nonce |
| 3 | VC issuance (requires DID signature) | `POST /v1/vc/issue` + did_signature | 200 + VC JSON + vc_hash |
| 4 | ZK Proof retrieval | `POST /v1/proof` | 200 + proof + remaining_accounts |
| 5 | Merchant registration (Sponsored) | `POST /v1/merchants/register` (mode=sponsored) | 200 + tx signature |
| 6 | Merchant registration (SelfOnchain) | `POST /v1/merchants/register` (mode=self_onchain) | 200 + base64 transaction |
| 7 | SelfOnchain confirmation | `POST /v1/merchants/confirm` | 200 + status: confirmed |
| 8 | DID resolution | `GET /v1/did/resolve/{did}` | 200 + DID Document |
| 9 | Merchant verification | `GET /v1/merchants/verify/{did}` | 200 + verified: true |
| 10 | Status query | `GET /v1/merchants/status/{did}` | 200 + status: active |
| 11 | VC update | `POST /v1/merchants/update-vc` | 200 + tx signature |
| 12 | Key rotation | `POST /v1/merchants/rotate-key` | 200 + tx signature |
| 13 | VC revocation | `POST /v1/vc/revoke` | 200 + signature + revoked_vc_pda |
| 14 | Fee query | `GET /v1/fees` | 200 + fees array |

---

## FAQ

### Q: Registration returns "Failed to get validity proof"

The Photon RPC is not configured or unreachable. Confirm that `light.photon_url` in `config.toml` is set correctly and the API key is valid.

### Q: Registration returns "On-chain error"

The payer may have insufficient SOL balance. Check:
```bash
solana balance <PAYER_ADDRESS> --url devnet
```
If the balance is insufficient, airdrop SOL:
```bash
solana airdrop 2 <PAYER_ADDRESS> --url devnet
```

### Q: VC issuance returns "invalid DID signature"

`POST /v1/vc/issue` requires DID signature verification. Confirm that the signed message format is `issue_vc:{merchant_did}:{merchant_name}:{nonce}` and that the correct nonce and DID private key were used.

### Q: VC issuance returns "not authorized"

In update scenarios (merchant already registered), the platform verifies whether the signer is the controller or original key. Confirm that the DID signature was made with the private key corresponding to the current controller.

### Q: Signature verification fails

Confirm that the signed message format matches exactly (including colon separators) and that the correct nonce was used. An expired or already-used nonce will cause failure.

### Q: What to do when blockhash expires in SelfOnchain mode

The `recent_blockhash` in the unsigned transaction expires after approximately 90 seconds. If the merchant cannot sign in time or the broadcast fails, they need to obtain a new nonce and request a new unsigned transaction.

### Q: Fee records are empty

`GET /v1/fees` only returns fee records for Sponsored mode. If all operations use `self_onchain` mode, there will be no fee records.
