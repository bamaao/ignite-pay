# Product Technical Document: Agent Payment Identity (DID) System Based on SPL Account Compression

## 1. Product Definition
This solution aims to build a high-performance, low-cost decentralized identity verification and payment gateway for AI Agents. Leveraging **SPL Account Compression** technology, merchant credit and payment authorization are stored on-chain, ensuring that Agents possess real-time risk control and automated decision-making capabilities when processing X402 payment-required requests.

## 2. Core Architecture Design

### 2.1 Storage Model: Compressed State Tree
Unlike traditional on-chain account storage, this system utilizes Solana's `ConcurrentMerkleTree` to store merchant identities.
* **Leaf (Leaf Node)**: Stores the core metadata hash of the merchant's DID.
    * `Leaf Data = Hash(Merchant_DID_Hash, Active_Pubkey, Platform_VC_Hash, Slot_Updated)`
    * Where `Merchant_DID_Hash` is the 32-byte SHA-256 hash of the merchant's DID public key (extracting the Ed25519 public key from the `did:ignite` string and then hashing it), and `Active_Pubkey` is the merchant's Solana receiving address (unrelated to the Ed25519 signing public key in the DID document)
* **Tree Authority**: The platform operator (e.g., Ignite-Pay official) acts as the tree manager, responsible for signing new merchant onboarding and updating merchant status.

### 2.2 Chain of Trust
1.  **Merchant Layer**: Holds the private key corresponding to the DID, used for signing payment requests.
2.  **Platform Layer**: Audits merchants and attaches a **Platform Attestation** to their leaf node.
3.  **Protocol Layer (Ignite-Pay)**: Verifies Merkle Proofs on-chain, ensuring payments only flow to trusted merchants.

---

## 3. Business Process Specifications

### 3.1 Merchant Onboarding and State Compression On-Chain
1.  **Application**: The merchant generates a `did:ignite` key pair and a separate Solana receiving key pair, then submits DID information and service metadata to the platform.
2.  **Attestation**: After the platform's review, it issues a Verifiable Credential (VC) containing declarations such as the merchant's DID, validity period, service type, etc. (see `ignite-pay-did.md` §4.5 for VC structure details).
3.  **On-Chain**: The platform inserts `MerchantLeaf { merchant_did_hash, active_pubkey, platform_vc_hash, status, slot_updated }` as a new leaf node into the on-chain Merkle Tree.
4.  **Indexing**: The indexer captures the transaction and generates a queryable **Merkle Proof**.
5.  **Delivery**: The platform returns the VC to the merchant. The merchant includes the VC in subsequent X402 responses (either directly embedded or referenced via IPFS CID).

### 3.2 Key Rotation
When a merchant needs to update their Solana receiving address:
* The merchant must submit a new receiving address declaration signed by the **DID private key** (i.e., the Ed25519 signing key in `did:ignite`).
* After the platform verifies the signature, it calls the `update_leaf` instruction to update the `active_pubkey` field at the original leaf position.
* The DID remains unchanged (`merchant_did_hash` unchanged), ensuring business continuity.

### 3.3 Payment Discovery and Dual-Layer Verification (X402 Flow)
When an Agent receives X402 payment-required information from a service provider:

#### Layer 1: Off-Chain Fast Filtering (Payment Skill)

This layer combines VC verification (see `ignite-pay-did.md` §4.5 for details) with on-chain Merkle Proof verification, ensuring the merchant has both platform attestation and on-chain records:

1. **VC Verification**: Extract the merchant's VC from the 402 response, and verify the VC signature and validity period using the built-in platform public key.
2. **Merkle Proof Verification**: Obtain the Merkle Proof of the merchant's leaf node from the indexer, fetch the current Tree Root from the chain, and locally compute `Proof + Leaf == Root`.
3. **Consistency Check**: Verify that the `credentialSubject.id` in the VC corresponds to the same merchant as the `merchant_did_hash` in the leaf node.
* **Result**: If any verification fails or the merchant is on the blacklist, the payment is blocked immediately; if all verifications pass and the amount is within the whitelist limit, the payment is automatically approved.

#### Layer 2: On-Chain Enforcement (ExecutePayment Contract)
* **Action**: The Agent calls Ignite-Pay's settlement contract to execute the transfer.
* **Session Key**: The contract requires a signature from an on-chain Session Key created by the user during mobile-side authorization. The Session Key is registered on-chain by the mobile side through the DIDComm authorization flow (bound to owner, spending_limit, scopes, expires_at). After the MCP/Skill receives the authorization response, it uses this Session Key to sign on-chain transactions on behalf of the user.
* **Constraint**: The contract mandates the submission of `Proof` and `Leaf Data`, along with a valid Session Key signature.
* **Logic**: Internally, the contract uses `spl_account_compression::verify_leaf` to confirm that the merchant is indeed attested by the platform, while simultaneously verifying the Session Key's validity (not expired, spending limit not exceeded, scope matched).
* **Safety**: If any verification fails (merchant verification failure, Session Key invalid/expired/over limit), the Solana transaction rolls back immediately, and funds cannot be transferred out.

---

## 4. Data Structures and Interface Definitions

### 4.1 Compressed Leaf Node Definition (Rust Struct)
```rust
struct MerchantLeaf {
    pub merchant_did_hash: [u8; 32],   // SHA-256 hash of the merchant's DID public key (extracting the Ed25519 public key from the did:ignite string and then hashing it)
    pub active_pubkey: Pubkey,          // Merchant Solana receiving address (not the Ed25519 signing public key in the DID document; used for receiving payments)
    pub platform_vc_hash: [u8; 32],     // SHA-256 hash of the platform-issued VC, computed as: SHA-256(canonical_json(VC))
    pub status: u8,                     // Merchant status: 0=active, 1=suspended, 2=revoked
    pub slot_updated: u64,              // Last updated slot height
}
```

### 4.2 X402 Extension Field Specifications
The response headers returned by the service provider must include:
* `x402-merchant-did`: The merchant's `did:ignite` identifier (used for whitelist/blacklist matching, see `ignite-pay-did.md` §4.2 for details).
* `x402-payment-address`: The merchant's Solana receiving address (must be consistent with the `active_pubkey` in the on-chain compressed account).
* `x402-merkle-context`: (Optional) The on-chain Merkle Tree address, indicating to the Agent which tree should be used for identity verification.

---

## 5. Product Advantages
1.  **Extreme Scalability**: Based on SPL Account Compression, a single tree can support onboarding of millions of merchants, while the platform only needs to maintain a fixed-size account.
2.  **Enforced Compliance**: Through on-chain contract Proof verification, "platform attestation" becomes a physical condition for payment, rather than a simple logical check.
3.  **Privacy Protection**: Although the tree is on-chain, detailed whitelist/blacklist management documents can be stored on IPFS, visible only to authorized Payment Skills.
4.  **Low Latency**: Off-chain fast filtering on the Agent side ensures an "instant payment" experience; only the final settlement incurs on-chain costs.

---

## 6. Future Roadmap
> The following version roadmap is consistent with `ignite-pay-did.md` §8. Both documents together describe the evolution of the same system.

* **V0.1** (Current): `did:ignite` local identity + DIDComm V2 communication + Mock payment + MCP Server.
* **V1.0**: Mobile-side DIDComm authorization flow + Session Key on-chain registration (created during mobile-side authorization) + SPL Account Compression merchant onboarding + On-chain identity verification program.
* **V1.1**: VC merchant attestation + IPFS whitelist/blacklist + Mobile-side list management + sled local cache for risk control decisions + On-chain payment contract (using Session Key signatures).
* **V2.0**: Solana on-chain payment integration (Session Key driven) + Multi-chain DID mapping, allowing Agents to use unified identity credentials for payments in a multi-chain environment.
