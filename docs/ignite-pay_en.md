A payment gateway design for **AI Agent + Decentralized Identity (DID) + MagicBlock Payment Channel**. By deeply coupling the payment flow with identity authentication (DID), and combining MagicBlock payment channels for high-frequency, low-latency micropayments, the system builds an efficient, privacy-preserving platform with fine-grained permission management.

---

## 1. Core Flow Architecture Diagram

```
Agent → External Service Provider (402) → Buyer MCP Server → Mediator → Mobile App
                                    ↑                        ↓
                              Payment Decision Engine            User Approve/Reject
                        (VC verification + on-chain DID verification + list + quota)        ↓
                                    ↑              DIDComm Auth Response
                             IPFS list sync ←—————————————┘
                                    ↓
                       Session Key on-chain payment (SOL/SPL Token)

                ┌───────────────────────────────────────────────┐
                │          MagicBlock Payment Channel (Independent Flow)         │
                │                                               │
                │  Buyer MCP                    Merchant MCP    │
                │  mb_deposit                   mb_receive_voucher
                │  mb_sign_voucher (off-chain) ──► mb_settle_batch / optimistic_settle
                │  mb_sign_settlement           mb_release_settlement
                │  mb_dispute / resolve_dispute mb_force_release
                │  mb_withdraw                                   │
                └───────────────────────────────────────────────┘
                                    ↓
                           Solana on-chain settlement
                    (GlobalVault → Escrow → Merchant)
```

---

## 2. Technical Analysis of Key Components

### A. Service Provider Discovery and X402 Protocol
The X402 protocol serves as a "value-exchange handshake" in this context.
* **Trigger mechanism**: When an Agent requests a resource from an external service provider without providing valid credentials, the external service provider returns an extended version of `402 Payment Required` (X402). The MCP Server parses this response and initiates the payment flow.
* **Metadata separation**: The returned information stream contains:
  * `accepts[].recipient`: **Wallet address**, used for payment routing (not DID)
  * `provider_did`: **Merchant's `did:ignite`** (separate field), used for reputation tracing and blacklist/whitelist matching
  * `accepts[].amount/token/network`: Payment amount, token type, and network
* **VC attachment**: The 402 response can include a Verifiable Credential issued by the platform, used for merchant identity endorsement verification.

### B. DID Management Based on ZK Compression (Light Protocol)

Uses **ZK Compression (Light Protocol)** on the Solana blockchain to store merchant DID accounts, enabling on-chain verifiable merchant identity management. Compressed account data is stored in hash form within the Light Protocol state Merkle tree, with no rent-exemption required.

* **Architecture**:
  * **On-chain program**: `ignite-pay-did-program` (Anchor), manages compressed DID accounts via Light System Program CPI
  * **Compressed account**: `MerchantCompressedDid`, data stored in hash form within the Light state tree
  * **Account fields**: `original_pk` (original public key), `controller_pk` (current controller), `recovery_pk` (recovery key), `vc_hash` (platform VC hash), `last_updated`, `nonce` (anti-replay counter)
  * **Trust chain**: Platform Ed25519 signature `sign(credential_subject_pk || vc_hash)` → on-chain `PlatformConfig` PDA stores the platform public key → on-chain verification
* **VC Revocation Registry**:
  * `RevokedVc` PDA: `seeds = [b"revoked-vc", vc_hash]`, verifiers check PDA existence to determine revocation status
  * Only platform authority can invoke `revoke_vc`
* **Operations**:
  * Platform initialization: `init_platform` → stores the platform Ed25519 public key in the `[b"platform-config"]` PDA (one-time)
  * Merchant onboarding: `initialize_did` → creates compressed DID (requires platform signature + ZK validity proof)
  * VC update: `update_did_with_vc` → updates `vc_hash` (requires platform signature + controller authorization + nonce)
  * Key rotation: `set_recovery_key` + `recover_controller` → takes over controller via recovery key
  * VC revocation: `revoke_vc` → creates `RevokedVc` PDA (platform authority only)
* **Verification model**:
  * Off-chain: Obtain ZK validity proof via Light RPC (Photon) + `DidService` client verification
  * On-chain: Platform signature verification + subject binding check + nonce anti-replay
* **Registry service**: `did-registry` provides REST API (`/v1/merchants/register`, `/v1/merchants/verify/{did}`, `/v1/vc/issue`, `/v1/vc/revoke`, etc.), supporting two modes: `Sponsored` (platform pays on behalf) and `SelfOnchain` (merchant self-pay)

### C. Session Keys

A temporary key system used for secure execution of on-chain payments:

* **Self-funded mode (SelfFunded)**: User pre-deposits SOL to the temporary key, which then makes direct payments
  * Flow: Create Session → Pre-deposit SOL → Build SOL/SPL transfer → Sign and send → Record spending
* **Sponsored mode**: Project Relayer pays gas on behalf
  * Flow: Build transaction (fee_payer = relayer) → Temporary key partial signature → Send to Relayer `POST /sponsor` → Relayer appends fee_payer signature and broadcasts
* **Risk controls**:
  * Expiration time check (`expires_at`)
  * Per-transaction spending limit (`spending_limit`)
  * Permission scope restriction (`scopes`: `["sol:transfer", "spl:transfer"]`)
* **Persistence**: Session data is serialized via borsh and stored in a sled database

### D. MagicBlock Payment Channel

A high-frequency micropayment system based on on-chain Solana payment channels. Voucher signing is entirely off-chain, with no need to pre-create on-chain channels. Channels are created on-demand only when a merchant initiates L1 settlement.

**Three-Layer Architecture:**

| Layer | Description |
|------|------|
| L1 (Solana) | Channel creation, fund locking, signature verification, final settlement |
| ER (MagicBlock) | High-speed state transitions (<50ms latency, gasless), records each Voucher |
| Off-chain fraud layer | Challenge window dispute resolution, based on Sum-Merkle Proof |

**Core Data Structures:**

| Account | Size | Fields |
|------|------|------|
| GlobalState | 89 bytes | `buyer`, `token_mint`, `total_deposited`, `total_allocated`, `bump` |
| Channel | 145 bytes | `buyer`, `merchant`, `token_mint`, `spending_cap`, `settled_amount`, `nonce`, `challenge_period`, `dispute_period`, `bump` |
| SettlementEscrow | 164 bytes | `channel`, `merchant`, `token_mint`, `amount`, `merkle_root`, `nonce`, `created_at`, `claimed`, `disputed`, `optimistic`, `bump` |

**Stablecoin-First Support:**
- All accounts include a `token_mint` field to distinguish SOL (`Pubkey::default()`) from SPL Tokens (USDC/USDT, etc.)
- PDA seeds include `token_mint`: `[b"global_state", buyer, token_mint]`, `[b"channel", buyer, merchant, token_mint]`
- The same buyer can establish multiple channels with the same merchant (distinguished by token type)
- Mobile deposit defaults to USDC, supporting USDC / USDT / SOL tokens

**Complete Payment Flow:**

```
1. SETUP (Buyer)
   mb_init_global    → Create GlobalState + GlobalVault PDA (distinguished by token_mint)
   mb_deposit        → Deposit into GlobalVault (initiated from mobile, supports USDC/USDT/SOL)

2. Off-chain Micropayment (Buyer → Merchant, no on-chain channel needed)
   Buyer: mb_sign_voucher(seq, amount)           → Ed25519 signature, local storage
   Pre-sign check: outstanding_vouchers + amount <= total_deposited - total_allocated
   Merchant: mb_receive_voucher(buyer_sig)           → Verify signature, local storage

3a. Cooperative Settlement (Merchant)
   Merchant: mb_settle_batch(buyer_batch_sig)        → Build Merkle Sum Tree, dual-signature settlement
   On-chain Channel auto-created at settlement time (if not exists)
   Merchant: mb_release_settlement                   → Release funds after challenge period
   Dispute path: Buyer mb_dispute → Buyer mb_resolve_dispute (fraud proof)
             Or: Merchant mb_force_release (after dispute period)

3b. Optimistic Settlement (Merchant, when buyer is uncooperative)
   Merchant: mb_optimistic_settle                    → Merchant signature only
   Follow-up: Same challenge/dispute path
```

**Security Model:**

| Protection | Description |
|------|------|
| Vault balance check (off-chain) | When signing voucher: `outstanding_vouchers + amount <= total_deposited - total_allocated` (queries on-chain GlobalState) |
| Spending cap (on-chain) | On-chain check at settlement: `settled_amount + total_amount <= spending_cap` |
| Balance check (on-chain) | `total_amount <= vault.lamports` (actual balance) |
| Dual signature | Ed25519 instruction introspection verifies buyer + merchant signatures |

**Fraud Proof:** Sum-Merkle Tree design, buyer only needs a single Voucher + O(log N) sibling nodes. A proof for 128 Vouchers is only 280 bytes, well within the Solana 1232-byte transaction limit.

**Global Vault Design:** One global Vault (GlobalVault PDA) per Buyer. `total_allocated` tracks the sum of spending caps across all channels, preventing over-allocation. The Vault is always owned by the System Program; buyers deposit via `system_instruction::transfer`, and the program withdraws via `invoke_signed`.

### E. Payment Decision Flow

| Priority | Scenario | Condition | Action |
| :--- | :--- | :--- | :--- |
| 1 | **VC verification failed** | Attached VC signature is invalid/expired/issuer mismatch | Reject payment, return verification failure reason |
| 2 | **On-chain DID verification failed** | Merchant DID not registered on-chain as compressed account | Reject payment, return "merchant not found on-chain" |
| 3 | **Blacklist block** | `provider_did` is on blacklist | Immediate interruption, return `Security Risk: Provider Blocked` |
| 4 | **Whitelist auto-approve** | `provider_did` is on whitelist && amount <= `max_amount` | Execute on-chain payment directly |
| 5 | **Global threshold auto-approve** | Amount <= `auto_approve_max` | Auto-execute on-chain payment, no mobile authorization needed |
| 6 | **Interactive authorization** | None of the above are met | Trigger DIDComm V2 protocol, push authorization request to user's mobile |

**Payment Execution:**
* If Solana is configured: Execute real SOL/SPL Token transfer via Session Key
* If Solana is not configured: Use mock payment to generate simulated signature (development mode)

---

## 3. Authorization Routing: DIDComm V2 and Mediator

In this long chain (Agent → MCP → Mediator → Mobile App), the role of the **Mediator** is critical:

1. **Async processing**: The Agent cannot wait indefinitely for the user to tap their phone. The MCP Server uses oneshot channel + timeout mechanism for asynchronous waiting.
2. **DIDComm V2 protocol**: Ensures end-to-end encryption of cross-device messages. Key distinctions:
   * **Platform VC**: A merchant identity endorsement credential issued by the platform DID, used to verify merchant legitimacy. The mobile app does not issue VCs.
   * **Authorization response**: The mobile app signs a `payment-auth-response` message (containing payment_id, authorized, list_action), not a VC.
3. **List management**: During authorization, users can choose `list_action` (whitelist/blacklist/none). After authorization, the local sled list is auto-updated and synced (current IPFS sync is in mock mode).

---

## 4. Platform VC Merchant Endorsement Flow

```
Platform (Platform DID) → Issue VC → Attach to 402 response → MCP Server verifies
                                          ↑
                                   Contains merchant DID, name,
                                   category, validity period, Ed25519 signature
```

* **Issuer**: Platform (signed using the platform DID's Ed25519 private key)
* **Verification content**: Signature validity, VC not expired, issuer matches configured platform DID
* **Configuration**: Configure the `[platform]` section (did + verifying_key_b64) in MCP Server's config.toml

---

## 5. List Management Flow (Local sled + IPFS Sync)

```
Mobile authorization → list_action != "none"
         → MCP updates sled local cache
         → Upload merged list to IPFS → Get new CID
         → Send list-sync-notification to mobile
```

* **Storage structure**: IPFS stores `MerchantLists` (containing whitelist + blacklist arrays)
* **Local cache**: sled database maintains two B-trees (`__whitelist__`, `__blacklist__`)
* **IPFS client**: Dynamically selected via `[ipfs]` configuration section:
  * `mode = "mock"`: Uses MockIpfsClient (development mode, in-memory storage)
  * `mode = "kubo"`: Uses KuboIpfsClient (production mode, requires local Kubo node, specify RPC address via `kubo_url`)
* **Configuration**: Configure `[ipfs]` section in config.toml (`mode` + `kubo_url`)

---

## 6. Crate Structure

```
ignite-pay-core/                    # Core protocol library
├── src/
│   ├── identity.rs                 # DID generation, DID Document construction, identity persistence
│   ├── didcomm.rs                  # DIDComm message builders (17 message types), JWE encryption/decryption
│   ├── solana_did.rs               # SolanaDidBridge: DID on-chain verification bridge layer
│   ├── types.rs                    # Shared types: PaymentRequest, MerchantListEntry, etc.
│   ├── list_store.rs               # Whitelist/blacklist management (sled + IPFS sync)
│   ├── vc.rs                       # Verifiable Credential issuance and verification
│   ├── ipfs.rs                     # IPFS upload/download abstraction layer
│   ├── audit_merkle.rs             # SHA-256 Merkle tree audit log
│   └── log_*.rs                    # E2EE audit log (encrypt → Zstd compress → IPFS sync)

ignite-pay-solana/                  # Solana on-chain interaction
├── src/
│   ├── lib.rs                      # Module declarations + re-export solana_sdk
│   ├── types.rs                    # MerchantDidAccount, SessionTokenData, PayMode, PaymentResult
│   ├── error.rs                    # SolanaError unified error type
│   ├── compression.rs              # DidService: ZK Compression DID operations (initialize_did, update_did_with_vc, etc.)
│   ├── session.rs                  # SessionManager: temporary key creation/persistence/verification
│   ├── session_program.rs          # Session Program instruction building
│   ├── channel.rs                  # Payment channel interaction
│   └── payment.rs                  # IgnitePayClient: SOL/SPL Token transfer (SelfFunded + Sponsored)

ignite-pay-relayer/                 # Relayer sponsored payment service
├── config.toml                     # [relayer] keypair, rpc_url, listen_addr, rate_limit
└── src/
    └── main.rs                     # Axum HTTP: GET /info (public key), POST /sponsor (co-sign + broadcast)

ignite-pay-did-program/             # On-chain DID program (Anchor + Light SDK)
├── src/
│   ├── lib.rs                      # 6 instructions: init_platform, initialize_did, update_did_with_vc, set_recovery_key, recover_controller, revoke_vc
│   ├── state.rs                    # MerchantCompressedDid, PlatformConfig, RevokedVc
│   └── error.rs                    # DidError error codes

did-registry/                       # DID registry service (REST API)
├── src/
│   ├── server.rs                   # Axum routes: /v1/merchants/*, /v1/did/*, /v1/vc/*, /v1/proof
│   ├── state.rs                    # RegistryState: DidService + LightClient + platform signature
│   ├── config.rs                   # Server, Solana, Light (Photon), authentication, fee configuration
│   ├── handlers/                   # register, confirm, verify, status, rotate_key, update_vc, issue_vc, revoke_vc, proof, nonce, fees
│   ├── did/                        # resolver (DID hash/signature verification), ignite_store (DID document cache)
│   └── storage/                    # sled_store (MerchantStore: merchant records, VCs, fees, revocation status)

ignite-pay-mb/sdk/                  # MagicBlock payment channel SDK
├── src/
│   ├── lib.rs                      # Module declarations
│   ├── pda.rs                      # PDA derivation: derive_global_state_pda, derive_channel_pda, derive_settlement_pda
│   ├── merkle.rs                   # Sum-Merkle Tree: build_sum_merkle_tree, MerkleProof
│   ├── signing.rs                  # sign_voucher, sign_settlement, verify_signature
│   └── transaction.rs              # 11 transaction builders

ignite-pay-mcp/                     # Buyer MCP Server (23 tools)
├── config.toml                     # [solana] + [magicblock] configuration
└── src/
    ├── main.rs                     # IgnitePayMcpServer: X402 + Session Key + MB channel
    ├── lib.rs                      # audit, mediator, payment, tools, voucher_store
    ├── tools.rs                    # Tool input structs
    ├── voucher_store.rs            # StoredVoucher + VoucherStore (sled)
    ├── mediator.rs                 # MediatorConnection (DIDComm)
    ├── payment.rs                  # PaymentStore (sled)
    └── audit.rs                    # AuditLogStore (sled)

ignite-pay-merchant-mcp/            # Merchant MCP Server (11 tools)
├── config.toml                     # [solana] + [magicblock] + [merchant] configuration
└── src/
    ├── main.rs                     # MerchantMcpServer: QR + Voucher collection + settlement
    ├── lib.rs                      # audit, config, mediator, payment, qr, settlement_store, tools, voucher_store
    ├── tools.rs                    # Tool input structs
    ├── config.rs                   # Config, MagicBlockConfig
    ├── voucher_store.rs            # CollectedVoucher + MerchantVoucherStore (sled)
    ├── settlement_store.rs         # SettlementRecord + SettlementStore (sled)
    ├── mediator.rs                 # MerchantMediator (DIDComm)
    ├── payment.rs                  # PaymentOrderStore (sled)
    ├── qr.rs                       # PaymentQrData, generate_payment_qr_text
    └── audit.rs                    # AuditLogStore (sled)
```

---

## 7. Configuration

### Buyer MCP Configuration

```toml
[solana]
rpc_url = "https://api.devnet.solana.com"
pay_mode = "self_funded"   # "self_funded" or "sponsored"
relayer_url = "http://localhost:3030"  # Only needed for sponsored mode

[ipfs]
mode = "mock"                      # "mock" (development) or "kubo" (production, requires local Kubo node)
kubo_url = "http://127.0.0.1:5001" # Kubo RPC URL (only used when mode = "kubo")

[magicblock]
rpc_url = "https://api.devnet.solana.com"
program_id = "6pFXAg1oiV61wVvaJvMHqYdGMe2fscDwmN9UBUSvNuU3"
```

### Relayer Configuration

```toml
[relayer]
keypair_b58 = ""                                    # Empty = auto-generated at startup
rpc_url = "https://api.devnet.solana.com"
listen_addr = "0.0.0.0:3030"
rate_limit = 60
```

### Merchant MCP Configuration

```toml
[merchant]
did = ""
hub_endpoint = ""
hub_ws_url = "ws://localhost:3003/ws"
wallet = ""                    # Merchant Solana wallet address (base58)
accept_tokens = ["USDC"]      # Accepted tokens for QR payments

[magicblock]
rpc_url = "https://api.devnet.solana.com"
program_id = "6pFXAg1oiV61wVvaJvMHqYdGMe2fscDwmN9UBUSvNuU3"
```

### Environment Variables

| Variable | Purpose |
|------|------|
| `IGNITE_PAY_CONFIG` | Buyer MCP config file path (default `config.toml`) |
| `IGNITE_MERCHANT_CONFIG` | Merchant MCP config file path (default `config.toml`) |

---

## 8. MCP Tool Inventory

### Buyer MCP (23 tools)

**X402 Payment Tools:**

| Tool | Purpose |
|------|------|
| `process_x402_challenge` | Process HTTP 402 payment challenge: parse x402, verify merchant, risk control, authorize, execute payment |
| `check_authorization` | Query payment authorization status |
| `get_payment_history` | Get payment history |

**Identity & Pairing:**

| Tool | Purpose |
|------|------|
| `get_identity` | Get buyer DID, Mediator status, Solana status, MB Buyer Pubkey/Program ID |
| `generate_pairing_invitation` | Generate DIDComm pairing QR code |

**Session Key Management:**

| Tool | Purpose |
|------|------|
| `create_session` | Create Session Key (SOL or SPL Token), optional on-chain registration |
| `get_session_status` | Query Session Key status (balance, expiration) |
| `close_session` | Close Session Key, optionally refund SOL |
| `execute_spl_payment` | Execute SPL Token transfer using Session Key |

**On-chain DID Management:**

| Tool | Purpose |
|------|------|
| `add_merchant` | Add merchant ZK compressed DID account |
| `update_merchant` | Update merchant ZK compressed DID data |
| `verify_merchant` | Verify merchant on-chain identity |

**MagicBlock Payment Channel (11 tools):**

| Tool | Purpose |
|------|------|
| `mb_init_global` | Initialize global state (create GlobalState + GlobalVault PDA) |
| `mb_deposit` | Deposit SOL into GlobalVault |
| `mb_create_channel` | Create payment channel (specify merchant, spending cap, challenge period, dispute period) |
| `mb_update_spending_cap` | Adjust channel spending cap |
| `mb_get_channel` | Query channel status |
| `mb_get_global_state` | Query global state |
| `mb_sign_voucher` | Sign Voucher (Ed25519 signature of `SHA256(channel_id \|\| seq \|\| amount)`) |
| `mb_sign_settlement` | Sign settlement message (rebuild Merkle Tree, verify then sign) |
| `mb_dispute` | Dispute settlement (freeze Escrow) |
| `mb_resolve_dispute` | Resolve dispute (submit Sum-Merkle Proof fraud proof) |
| `mb_withdraw` | Withdraw unallocated funds |

### Merchant MCP (11 tools)

**Order Management:**

| Tool | Purpose |
|------|------|
| `generate_payment_qr` | Generate payment QR code (includes merchant MB Pubkey) |
| `check_payment` | Query order status |
| `get_payment_history` | Get order history |
| `get_identity` | Get merchant DID, MB Merchant Pubkey, Program ID |

**MagicBlock Payment Channel (7 tools):**

| Tool | Purpose |
|------|------|
| `mb_get_channel` | Query channel status with a buyer |
| `mb_receive_voucher` | Receive buyer Voucher: verify signature, store |
| `mb_settle_batch` | Batch settlement: build Merkle Sum Tree, merchant signature, dual-signature submission |
| `mb_optimistic_settle` | Optimistic settlement: merchant signature only (requires challenge_period > 0) |
| `mb_get_settlement` | Query settlement Escrow status |
| `mb_release_settlement` | Release settlement (after challenge period, funds transfer to merchant) |
| `mb_force_release` | Force release (after dispute period) |

---

## 9. DIDComm Message Types

| Message Type URI | Direction | Purpose |
|------|------|------|
| `ignite-pay/1.0/connection-request` | Phone → MCP | Mobile initiates pairing |
| `ignite-pay/1.0/connection-response` | MCP → Phone | MCP accepts pairing |
| `ignite-pay/1.0/connection-confirm` | Phone → MCP | Mobile confirms pairing |
| `ignite-pay/1.0/payment-auth-request` | MCP → Phone | Payment authorization request |
| `ignite-pay/1.0/payment-auth-response` | Phone → MCP | Mobile approve/reject payment |
| `ignite-pay/1.0/list-sync-notification` | MCP → Phone | List sync notification |
| `ignite-pay/1.0/qr-payment-request` | Phone → MCP | Mobile scans merchant QR to initiate payment |
| `ignite-pay/1.0/qr-payment-response` | MCP → Phone | QR payment result |
| `ignite-pay/1.0/qr-payment-notify` | MCP → Merchant MCP | Payment success notification to merchant |
| `ignite-pay/1.0/mb-deposit-request` | Phone → MCP | Mobile initiates MB shared vault deposit (includes `token`: USDC/USDT/SOL) |
| `ignite-pay/1.0/mb-deposit-response` | MCP → Phone | MB deposit result (includes total_deposited, tx_signature, token) |

---

## 10. Optimization Suggestions and Potential Challenges

### 1. State Synchronization
* **Challenge**: Blacklist/whitelist updates on IPFS may have latency.
* **Suggestion**: Ensure instant queries via local sled cache on the MCP Server; use IPFS only for cross-device synchronization.

### 2. Privacy Protection
* **Suggestion**: When sending payment intent to the mediator, stealth addresses or transaction amount obfuscation can be used to prevent the mediator from building a consumer profile.

### 3. Agent Retry Logic
* **Flow**: After the Agent obtains payment information, it re-requests with the information in the HTTP Header (e.g., `Authorization: Bearer <Payment_Proof>`).
* **Fault tolerance**: If payment succeeds but the service provider does not return the resource, the system needs an arbitration or appeal mechanism based on `provider_did`.

### 4. Performance Considerations
* **ZK Compression DID**: Compressed accounts require no rent-exemption; validity proof obtained via Light RPC, off-chain verification in milliseconds
* **On-chain DID operations**: Platform signature verification + nonce anti-replay, controllable transaction size
* **Session management**: sled persistence, auto-recovery of active sessions after restart
* **MB payment channel**: Pure off-chain Voucher signing (millisecond-level, only queries on-chain GlobalState balance), batch settlement merges multiple payments into a single on-chain transaction, channels created on-demand
* **MB Keypair persistence**: sled storage, auto-recovery after restart

---

## 11. Phased Roadmap

| Phase | Features | Status |
| :--- | :--- | :--- |
| **V0.1** | Basic MCP + DIDComm encryption + Mediator + Mock payment | Completed |
| **V1.0** | Mobile authorization loop (Flutter Rust Bridge + WS bidirectional communication) | Completed |
| **V1.1** | VC verification + IPFS blacklist/whitelist + list sync | Completed |
| **V2.0** | ZK Compression (Light Protocol) DID + Session Keys + on-chain payment | Completed |
| **V2.1** | MagicBlock payment channel (off-chain Voucher + Merkle settlement + dispute mechanism) | Completed |
| **V2.2** | Sponsored payment mode + Relayer service | Completed |
| **V2.3** | Mobile-initiated MB shared vault deposit + pure off-chain voucher signing + stablecoin-first support | Completed |
| **V2.4** | QR payment improvements: receiving address + token selection + SPL Token support (all payment methods support USDC/USDT) | Completed |

---

## Summary

This system provides complete payment infrastructure for the **"Agent Economy"**. It enables pay-per-use via X402, establishes a trust framework through VC verification, and ensures user self-sovereignty via DIDComm V2. On-chain payments are executed through Session Keys for secure and convenient SOL/SPL Token transfers (supporting both SelfFunded and Sponsored modes). The MagicBlock payment channel enables high-frequency micropayment scenarios (pure off-chain Voucher signing, vault balance verification, merchant on-demand L1 batch settlement + fraud proof dispute mechanism). The Relayer service provides gas sponsorship capability for Sponsored mode, allowing users to complete payments without holding SOL.
