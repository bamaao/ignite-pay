**Merchant DID Identity System Development Design Guide Based on ZK Compression**

This document can be delivered directly to the development team as a technical specification (Spec).

---

# Merchant DID Identity System Development Design Guide (V1.0)

## 1. Core Architecture Overview
This solution uses **ZK Compression** technology to implement low-cost, high-self-sovereignty merchant identity management on Solana.
* **DID (Decentralized Identifier)**: The merchant's permanent identity identifier.
* **VC (Verifiable Credential)**: The platform's credit endorsement for the merchant.
* **ZK Compression**: Used to compress and store DID and VC data in state trees, drastically reducing storage costs.

---

## 2. Identity Identifier and Key System
To ensure security and flexibility, the system adopts a three-layer key structure:

| Key Type | Definition | Storage Location | Purpose |
| :--- | :--- | :--- | :--- |
| **Original Public Key (Root)** | The Solana address used during merchant registration | Permanent on-chain ID | Serves as the DID anchor; cannot be changed. |
| **Controller Key** | Pure Ed25519 key pair | Merchant local/offline | Holds the modification rights for the DID document (the key to the "lock"). |
| **Recovery Key** | Backup Ed25519 key pair | Offline cold storage | Used to reset permissions when the Controller Key is lost. |

---

## 3. Business Workflow

### 3.1 Identity Initialization
1. The merchant generates a `Controller Key` and `Recovery Key` locally.
2. The DID is derived from the `Original Public Key`: `did:ignite:platform:[Original_PK]`.

### 3.2 Platform VC Issuance (Verifiable Credential)
1. The merchant submits identity verification materials and their `Original Public Key` to the platform.
2. After the platform approves the review, it issues a structured credential (VC).
3. **Key Security Point**: The VC payload must include `subject: [Original_PK]`, binding the credential to the specific merchant address.
4. **DID Signature Verification**: The merchant must sign the request with their DID private key (`issue_vc:{did}:{merchant_name}:{nonce}`), and the platform verifies it before issuing the VC, ensuring the requester actually holds that DID.
5. **Update Scenario**: If the merchant is already registered, the platform will also verify that the signer is the current controller or original key, preventing unauthorized parties from requesting a new VC.

### 3.3 Merchant Self-On-chain (On-chain Registration)
1. The merchant calls an instruction to write the VC into their ZK compressed account.
2. **Verification Logic**:
   * Verify whether the transaction initiator (Signer) is the merchant's `Original Public Key`.
   * Verify that the `subject` in the VC matches the `Signer`.
   * Verify that the VC carries a valid digital signature from the platform.
3. **Two On-chain Modes**:
   * **Sponsored (Platform-paid)**: The platform signs and sends the transaction; the merchant does not need a Solana private key. The platform records the service fee.
   * **SelfOnchain (Merchant Self-service)**: The merchant obtains a ZK proof through the public `POST /v1/proof` endpoint, constructs and signs the transaction locally, and broadcasts it. Alternatively, they can use `light-sdk` + a self-hosted Photon RPC for complete independence. After broadcasting, the merchant must call `POST /v1/merchants/confirm` to notify the platform.

---

## 4. Security Protection Scheme: Preventing "Impersonation On-chain"
To address the risks of "malicious actors using someone else's VC to register on-chain" or "merchants using the wrong account to register on-chain," the following verifications are enforced:

### A. On-chain Platform Signature Verification (Implemented)

The on-chain program stores the platform's Ed25519 public key (`PlatformConfig` PDA, seeds: `[b"platform-config"]`), and verifies the platform signature in the `initialize_did` and `update_did_with_vc` instructions:

* **Signed Message**: `credential_subject_pk (32 bytes) || vc_hash (32 bytes)` = 64 bytes
* **Verification Logic**: `verify(platform_pubkey, credential_subject_pk || vc_hash, platform_signature)`
* **Purpose**: Ensures that the vc_hash is authorized by the platform; attackers cannot forge a VC to register on-chain.

### B. Subject Binding — On-chain Enforcement (Implemented)

The on-chain instruction additionally accepts a `credential_subject_pk: Pubkey` parameter and enforces verification:

* **Rule**: `credential_subject_pk == signer.key()`
* **Purpose**: The VC subject must be the transaction submitter. Even if an attacker intercepts the platform signature, they cannot register on-chain with a different account — the subject binding check will reject it first.

### B. Deterministic Address Derivation (PDA Derivation)
Leveraging the indexing feature of ZK Compression, merchant data is stored at a fixed location calculated from their public key:
* **Calculation Formula**: `Index = Hash(Program_ID + Original_PK)`.
* **Purpose**: Ensures each merchant has only one valid "slot" in the state tree; no one can preempt another's position.

---

## 5. ZK Compressed Account Structure Definition (Rust Example)
In the contract, the merchant's compressed data is defined as follows:

```rust
pub struct MerchantCompressedDid {
    pub original_pk: Pubkey,     // Initial anchor public key
    pub controller_pk: Pubkey,   // Current controller public key (Ed25519)
    pub recovery_pk: Pubkey,     // Recovery public key
    pub vc_hash: [u8; 32],       // Hash of the platform VC
    pub last_updated: i64,       // Last update timestamp
    pub nonce: u64,              // Anti-replay counter
}
```

---
## 6. Detailed Business Workflow for Merchant DID Establishment

**The merchant has complete self-sovereignty**, while **the platform is only responsible for credit endorsement**.

Through this approach, the merchant does not need to surrender any private key permissions to the platform; instead, on-chain registration is completed through "Proof."

---

### 1. Business Workflow: Three-Step Architecture

#### **Step 1: Merchant Creates DID (Local Generation)**
The merchant generates an **Ed25519** key pair locally (e.g., via SDK or offline environment):
* **Private Key**: Kept secure by the merchant; never shared.
* **Public Key**: Used as the `Verification Method` in the DID document.
* **DID Identifier**: The merchant derives their DID address from this public key (e.g., `did:ignite:merchant_abc...`).

#### **Step 2: Platform Issues VC (Credit Authorization)**
The platform does not participate in modifying the merchant's DID document; it does only one thing — **issues credentials**:
1.  The merchant sends their DID identifier and necessary information (such as real-name verification materials) to the platform.
2.  After the platform verifies and approves, it uses the **platform's private key** to sign a declaration about the "merchant's DID."
3.  The platform returns this signed **VC (Verifiable Credential)** to the merchant.
4.  **Identity Verification**: The merchant must sign the request with their DID private key (`issue_vc:{did}:{name}:{nonce}`), and the platform verifies it before issuing. In update scenarios, the platform also verifies that the signer is the controller.

#### **Step 3: Merchant Self-On-chain (State Finalization)**
This is the most critical step. The merchant takes the VC provided by the platform, combines it with their own Solana account, and initiates a transaction:
1.  **Construct Transaction**: The merchant packages their DID document and the platform-provided VC hash into transaction parameters.
2.  **Dual Proof**:
    * **Authorization Proof**: The merchant uses their Solana account (as `Signer`) to pay Gas and proves they are the owner of the DID.
    * **Endorsement Proof**: The VC carried in the transaction contains the platform's signature, proving that the DID has been certified by the platform.
3.  **ZK Compressed Storage**: After the Solana contract verifies both proofs, it updates the DID state into the **ZK Compression** state tree.
4.  **On-chain Mode Selection**:
    * **Sponsored Mode**: The platform signs and sends the transaction with its own keypair; the merchant does not need to involve their Solana private key.
    * **SelfOnchain Mode**: The merchant obtains a ZK proof through the public `POST /v1/proof` endpoint, constructs and signs the transaction locally. If the merchant self-hosts a Photon RPC, they can operate completely independently without relying on the platform. After broadcasting, the merchant must call `POST /v1/merchants/confirm` to notify the platform.

---

### 2. Why This Design Is Powerful

This workflow solves several core pain points in Web3 commerce:

* **Data Ownership**: The DID is "registered on-chain" by the merchant, not "assigned" by the platform. Even if the platform shuts down, the merchant's DID still exists on the Solana chain, and the merchant holds the unique control private key.
* **Compliant Endorsement**: Through on-chain VC registration, any third party (such as other AI Agents or payment gateways) querying the merchant's DID on-chain can see: "This merchant has passed the platform's risk control certification."
* **Privacy and Cost**: Thanks to **ZK Compression**, the merchant's complete VC content can be stored off-chain, with only a hash stored on-chain. This both protects commercial privacy (sensitive data is not publicly disclosed) and reduces on-chain costs to nearly zero.

---

### 3. Technical Implementation Details (For Ignite-Pay)

When writing the smart contract, the verification logic for the `update_did_with_vc` instruction:

```rust
// Actual on-chain logic (implemented)
pub fn update_did_with_vc(
    ctx: Context<DidWithPlatformAccounts>,
    proof: ValidityProof,
    current_did: MerchantCompressedDid,
    account_meta: CompressedAccountMeta,
    vc_hash: [u8; 32],
    nonce: u64,
    platform_signature: [u8; 64],
    credential_subject_pk: Pubkey,
) -> Result<()> {
    // 1. Controller authorization
    require!(current_did.controller_pk == ctx.accounts.signer.key());

    // 2. Anti-replay
    require!(current_did.nonce == nonce);

    // 3. Subject Binding: signer must be the VC subject
    require!(credential_subject_pk == ctx.accounts.signer.key());

    // 4. Platform signature verification: sign(credential_subject_pk || vc_hash)
    let message = credential_subject_pk.as_ref() || vc_hash;
    require!(verify_ed25519_signature(&message, &platform_signature, &platform_pubkey));

    // 5. Update ZK compression tree state
    did.vc_hash = vc_hash;
    did.nonce += 1;
    Ok(())
}
```

### 4. Summary

The merchant registering on-chain through their own Solana account has the greatest significance in **establishing a permanent, tamper-proof "reputation ID."**

* **Merchant**: Pays a minimal Gas fee in exchange for identity independence.
* **Platform**: As the Issuer, establishes the ecosystem's trust threshold through VCs.
* **AI Agent**: Directly reads the compressed DID state on-chain, achieving millisecond-level trust decisions.

**Currently, merchant on-chain registration supports two modes: Sponsored (platform pays Gas) and SelfOnchain (merchant pays Gas), selectable via the `mode` field in the request body.**

---

## 7. The Core Contradiction Between "Identity Uniqueness (Sybil Resistance)" and "Account Binding"

If a merchant could use **Account B** to register a VC originally belonging to **Account A** on-chain, it would cause logical confusion in your DID system: the platform considers this the same merchant, but two unrelated addresses exist on-chain.

To address the problems caused by "merchants arbitrarily switching on-chain accounts," defenses can be implemented from the following three dimensions:

---

### 1. VC Internal "Subject Binding"
This is the most fundamental defense. The VC issued by the platform should not be "generic"; it must be **"tied to both the person and the address."**

* **Implementation**: In the VC JSON payload issued by the platform, a `subject` field must be included, with its value fixed to the **Original Public Key (Original Address)** associated when the merchant created the DID.
* **Verification Logic**: When the on-chain contract processes the on-chain request, it parses the VC plaintext:
  > If `Transaction Initiator (Signer)` **does not equal** `VC.subject`, the contract rejects immediately.
* **Result**: The merchant cannot switch to a different account for on-chain registration, because the VC is a "personal use only" pass.

---

### 2. Derived Address "Physical Isolation" (PDA Derivation)
In Solana, we can leverage the **PDA (Program Derived Address)** feature to give each merchant's DID account a "predetermined slot" on-chain.

* **Logic**: The merchant's compressed account address is no longer randomly generated, but:
  $$Address = Hash(ProgramID + VC.subject + "DID\_SEED")$$
* **Significance**:
  * If the merchant registers with **Account A**, their data can only be stored at **Location A'**.
  * If they try to register on-chain with **Account B**, the contract will find they are attempting to write to **Location B'**.
  * Even if they forcibly stuff the VC into **Location B'**, your payment SDK (Ignite-Pay) will still look for data at **Location A'** based on their **original identity ID**.
* **Conclusion**: A merchant using the wrong account for on-chain registration will result in being unable to find their data, or the data being invalid.

---

### 3. Authorization Token (Delegated Proof/Nonce)
If you want to allow merchants to "switch to a different account for on-chain registration" (e.g., the merchant's main wallet is out of funds, so they use a secondary account to pay Gas), but are concerned about identity impersonation, you can use **secondary authorization signing**:

1. **Platform issues VC** to the merchant's **Original Public Key A**.
2. **Merchant signs with Private Key A** an "authorization instruction": *"I authorize Account B to register this VC on-chain on my behalf."*
3. **Contract Verification**:
   * Verifies the platform signature (VC authenticity).
   * Verifies the merchant A's signature (authorization authenticity).
   * At this point, even though the transaction is initiated by **Account B**, the contract knows it is operating on behalf of **Account A**.

---

### Summary: Your Architecture's Defense Approach

To ensure the merchant system of **Ignite-Pay** remains orderly, the contract layer has implemented the following verification checks:

| Defense Layer | Check Item | Status |
| :--- | :--- | :--- |
| **On-chain Signature Verification** | `verify(platform_pk, subject_pk \|\| vc_hash, sig)` | Implemented |
| **Subject Binding** | `credential_subject_pk == signer.key()` | Implemented |
| **Controller Authorization** | `current_did.controller_pk == signer.key()` | Implemented |
| **Anti-replay Nonce** | `current_did.nonce == nonce`, incremented by 1 each time | Implemented |
| **PDA Address Isolation** | `seeds = [b"merchant-did", original_pk]` | Implemented |
| **VC Revocation** | `RevokedVc` PDA (seeds: `[b"revoked-vc", vc_hash]`); verifiers check PDA existence to determine revocation | Implemented |



### Why Is Merchants "Arbitrarily Switching Accounts" a Risk for the Platform?
If a merchant registers on-chain with Account A today and Account B tomorrow, and you do not enforce binding, your backend indexer will see two different entities. During payment settlement, an AI Agent may cause transaction failures because it cannot determine "which is the real receiving address."

**The current implementation adopts a "strict binding" mode (`Signer == VC.subject`), but supports key updates through the Controller Key rotation mechanism.**

---
## 8. Security

### On-chain Protection Mechanisms (Implemented)

The system prevents replay attacks and identity impersonation through the following on-chain mechanisms:

#### 1. PlatformConfig PDA — On-chain Storage of Platform Public Key

```
PlatformConfig PDA
seeds: [b"platform-config"]
Storage: platform_ed25519_pubkey (32 bytes)
Initialization: init_platform instruction (one-time deployment call)
```

`initialize_did` and `update_did_with_vc` read the public key from this PDA to verify the platform signature. When not initialized, all VC binding operations are rejected.

#### 2. Platform Signature Verification — Preventing Forged VCs

The platform uses its Ed25519 private key to sign `(credential_subject_pk || vc_hash)`. On-chain verification:
- Signed message: `credential_subject_pk (32B) || vc_hash (32B)` = 64 bytes
- Writing `vc_hash` is only allowed after verification passes
- Attackers without the platform's private key cannot forge the signature

#### 3. Subject Binding — On-chain Enforcement of "Real-name System"

The on-chain instruction additionally accepts `credential_subject_pk: Pubkey`, enforcing verification that `credential_subject_pk == signer.key()`.

Attack vector analysis:
- Intercepting `(vc_hash, platform_signature, credential_subject_pk)` and submitting with one's own signer -> Subject Binding check fails (signer != credential_subject_pk)
- Tampering with `credential_subject_pk` to one's own public key -> Platform signature verification fails (the signed message has changed)

#### 4. Controller + Nonce — Preventing Unauthorized Updates

- `update_did_with_vc` requires `current_did.controller_pk == signer.key()`
- On-chain nonce increments; each mutation must submit the correct nonce

### Previously Needed -> VC Revocation (Implemented)

The platform can revoke issued VCs through the `revoke_vc` instruction. An on-chain `RevokedVc` PDA is created (seeds: `[b"revoked-vc", vc_hash]`); verifiers check for PDA existence to determine revocation. Only `PlatformConfig.authority` has permission to call this. VCs contain a `credentialStatus` field pointing to the on-chain revocation registry's `program_id`, allowing third-party verifiers to locate and check it.

#### 5. VC Revocation (revoke_vc) — Implemented

The on-chain `RevokedVc` PDA provides tamper-proof revocation records:

* **PDA Seeds**: `[b"revoked-vc", vc_hash]` — unique PDA for each VC
* **Access Control**: Only `PlatformConfig.authority` can call (on-chain enforced)
* **Duplicate Prevention**: `AlreadyRevoked` error prevents duplicate revocations
* **Off-chain Cache**: did-registry caches revocation records in sled (`revoked_vc:{vc_hash_hex}`)
* **credentialStatus**: Each VC contains a `credentialStatus` field; third-party verifiers use the `program_id` to locate the on-chain registry

**Verifier Check Process**:
1. Verify the VC's Ed25519 signature and validity period
2. Calculate `vc_hash = SHA-256(vc_json)`
3. Derive PDA: `find_program_address(&[b"revoked-vc", vc_hash], program_id)`
4. Query whether the PDA exists -> if it exists, the VC has been revoked

---

## 9. Development Implementation Roadmap
1. **Contract Development (Anchor)**:
   * Define the `MerchantCompressedDid` structure.
   * Define the `PlatformConfig` PDA structure (storing the platform public key).
   * Implement the `init_platform` instruction (one-time deployment).
   * Implement the `initialize_did` instruction (platform signature verification + Subject Binding).
   * Implement the `update_did_with_vc` instruction (platform signature verification + Subject Binding + Nonce).
2. **SDK Development (TypeScript/Rust)**:
   * Provide tools for local Ed25519 key pair generation.
   * Provide functions for constructing on-chain transactions with VC data, platform signature, and credential_subject_pk.
3. **Platform Backend**:
   * Implement W3C-compliant VC issuance logic.
   * Implement the `sign_vc_binding(credential_subject_pk, vc_hash)` method.
   * Implement `platform_config_address()` PDA address derivation.
