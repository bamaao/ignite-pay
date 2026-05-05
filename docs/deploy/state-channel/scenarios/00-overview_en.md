# State Channel Business Scenarios Overview

## Scenario Categories

| # | Business Scenario | Document | Roles Involved |
|:--|:---------|:-----|:---------|
| 1 | Channel Open & Funding | [01-channel-open.md](01-channel-open.md) | User, Provider |
| 2 | Off-chain Payment & Split | [02-offchain-payment.md](02-offchain-payment.md) | User, Provider |
| 3 | Batch Payment & Atomic Operations | [03-batch-pipeline.md](03-batch-pipeline.md) | User |
| 4 | HTLC Conditional Payment | [04-htlc-payment.md](04-htlc-payment.md) | User, Provider |
| 5 | Cooperative Channel Close | [05-cooperative-close.md](05-cooperative-close.md) | User, Provider |
| 6 | Dispute Resolution | [06-dispute-resolution.md](06-dispute-resolution.md) | User, Provider |
| 7 | HTLC Settlement & Refund | [07-htlc-settlement.md](07-htlc-settlement.md) | User, Provider |
| 8 | Hub Routing Network | [08-hub-routing.md](08-hub-routing.md) | Hub |
| 9 | Multi-hop Payment | [09-multihop-payment.md](09-multihop-payment.md) | User, Hub, Provider |
| 10 | Auto Close & Watchtower | [10-auto-close.md](10-auto-close.md) | User, Provider |
| 11 | Compliance Management & Audit | [11-compliance-audit.md](11-compliance-audit.md) | User, Provider |
| 12 | WebSocket Real-time Communication | [12-websocket.md](12-websocket.md) | User, Provider, Hub |

## Role Descriptions

| Role | Binary | Description |
|:-----|:-------|:-----|
| **User** | `channel-user` | Payment initiator, manages own channel and UTXOs |
| **Provider** (Merchant) | `channel-provider` | Payment receiver, co-signs and accepts payments |
| **Hub** (Routing Relay) | `channel-hub` | Inherits all Provider functionality, additionally provides route discovery and multi-hop relay |

## Global Conventions

- All `{id}` are 64-character hexadecimal strings (32-byte channel_id)
- Amount units are SPL Token smallest units (e.g., for USDC with 6 decimal places, 1000000 = 1 USDC)
- Slot time: 1 slot ≈ 400ms (Solana Mainnet), Devnet may be slower
- Signature algorithm: Ed25519 (ed25519-dalek v1)
- Off-chain message format: `SHA-256(channel_id || sequence || ...)` detailed in each scenario
- HTTP interfaces uniformly return JSON, error format: `{"error": "<code>", "message": "<description>"}`
