# Session Key Payment Execution Flow

This document describes the complete flow of MCP (ignite-pay-mcp / ignite-pay-merchant-mcp) executing on-chain payments using Session Keys.

Related documents:
- [Session Keys Design](./session-keys.md) -- Design comparison of two modes
- [DIDComm Pairing Flow](./didcomm-pairing-flow.md) -- Establishing connection between Phone and MCP

---

## 1. Architecture Overview

```
Phone App                    MCP Server                     Solana
   │                              │                            │
   │  Generate ephemeral keypair  │                            │
   │  register_session_key ──────────────────────────────────>│  Create PDA
   │                              │                            │
   │  DIDComm: session key data ─>│                            │
   │                              │                            │
   │  (Wait for payment request)  │                            │
   │                              │                            │
   │<── DIDComm: auth-request ────│  (received x402 challenge) │
   │── DIDComm: auth-response ──>│  (carries auth + session key)│
   │                              │                            │
   │                              │  execute_payment ──────────>│  CPI transfer
   │                              │<── tx signature ───────────│
   │                              │                            │
   │<── DIDComm: payment-confirm ─│                            │
```

### Core Components

| Component | File | Responsibility |
|-----------|------|----------------|
| `ignite-pay-session-program` | `ignite-pay-session-program/src/` | On-chain Anchor program, verifies + executes payments |
| `SessionManager` | `ignite-pay-solana/src/session.rs` | Off-chain session lifecycle management (sled storage) |
| `session_program.rs` | `ignite-pay-solana/src/session_program.rs` | Instruction builder (derive PDA, build IX) |
| `IgnitePayClient` | `ignite-pay-solana/src/payment.rs` | Solana RPC interaction, build + send transactions |
| MCP Mediator | `ignite-pay-mcp/src/mediator.rs` | DIDComm message send/receive |
| MCP Tool | `ignite-pay-mcp/src/main.rs` | Payment flow orchestration |

### Program Deployment

`ignite-pay-session-program` is deployed once to the Solana chain, shared by all users:

- **Program ID**: `6EFvVTh7rEBpHH2JGryjKQmBLRtbYtSEerGNfkHqKiei`
- **PDA Seeds**: `["session", owner, ephemeral]` -- each user + ephemeral key combination produces a unique PDA
- **Self-Funded Mode**: Ephemeral key holds SOL, serving as the transaction's funding source and fee payer

---

## 2. Session Key Lifecycle

### 2.1 Registration (Phone Side)

Phone generates an ephemeral Ed25519 key pair and calls the on-chain `register_session_key` instruction to create a PDA:

```rust
// ignite-pay-session-program/src/lib.rs
pub fn register_session_key(
    ctx: Context<RegisterSessionKey>,
    expires_at: i64,
    spending_limit: u64,
    scopes: Vec<String>,
) -> Result<()> {
    let session = &mut ctx.accounts.session;
    session.owner = ctx.accounts.owner.key();
    session.ephemeral_signer = ctx.accounts.ephemeral_signer.key();
    session.expires_at = expires_at;
    session.spending_limit = spending_limit;
    session.scopes = scopes;
    session.current_spent = 0;
    session.revoked = false;
    Ok(())
}
```

PDA account state:

```rust
// ignite-pay-session-program/src/state.rs
#[account]
pub struct SessionKeyAccount {
    pub owner: Pubkey,            // Main wallet address
    pub ephemeral_signer: Pubkey, // Ephemeral key public key
    pub target_program: Pubkey,   // Target program
    pub expires_at: i64,          // Expiration timestamp
    pub spending_limit: u64,      // Maximum spending limit (lamports)
    pub current_spent: u64,       // Amount already spent
    pub scopes: Vec<String>,      // Authorized scope (e.g., ["sol:transfer"])
    pub revoked: bool,            // Whether revoked
    pub bump: u8,                 // PDA bump
}
```

### 2.2 Transfer to MCP

Phone sends session key data to MCP via DIDComm, included in the `payment-auth-response` message:

```rust
// ignite-pay-mcp/src/payment.rs
pub struct AuthResponse {
    pub authorized: bool,
    pub session_key_pubkey: Option<String>,      // Ephemeral public key (base58)
    pub session_key_secret_key: Option<String>,  // Ephemeral private key (base58)
    pub session_key_tx_signature: Option<String>, // register_session_key transaction signature
    pub session_expires_at: Option<i64>,         // Expiration time
    pub spending_limit: Option<u64>,             // Spending limit
    pub scopes: Option<Vec<String>>,             // Authorized scope
    // ...
}
```

### 2.3 Revocation

The owner can call `revoke_session` at any time to revoke a session key, or call `close_session` to close the PDA and reclaim rent.

---

## 3. Payment Execution Flow

### 3.1 Trigger: x402 Challenge

When MCP receives an AI Agent's `process_x402_challenge` tool call:

1. Parse the HTTP 402 response, extract `network`, `amount`, `token`, `recipient`
2. Create a `PaymentRequest` (status: `PendingAuth`)
3. Execute risk check

```
ignite-pay-mcp/src/main.rs -- process_x402_challenge()
```

### 3.2 Risk Decision

```
                    ┌─────────────┐
                    │ Risk check   │
                    └──────┬──────┘
                           │
               ┌───────────┼───────────┐
               ▼                       ▼
       AutoApproved              NeedsAuth
       (whitelist/low amount)    (requires authorization)
               │                       │
               ▼                       ▼
      Execute payment directly    DIDComm auth-request -> Phone
                                     │
                                     ▼
                            Phone returns auth-response
                            (authorized + session key)
                                     │
                                     ▼
                              Execute payment
```

**Auto-approval conditions** (`ignite-pay-mcp/src/main.rs`):
- Merchant is in the whitelist and amount is below `max_amount`
- Amount is below the global auto-approval threshold

When **authorization is required**, MCP sends a `payment-auth-request` to Phone via DIDComm and waits for a `payment-auth-response`.

### 3.3 Obtaining Session Key

Two sources:

| Source | Description | Code Location |
|--------|-------------|---------------|
| Phone auth response | Phone generates ephemeral keypair, returns with authorization | `main.rs: get_session_from_auth_response()` |
| MCP local session | `SessionManager` finds existing active session from sled | `main.rs: get_active_session()` |

MCP prioritizes the session key returned by Phone; if none, uses the local session.

`get_session_from_auth_response()` decodes the base58 key returned by Phone into a `Keypair`, constructs `SessionTokenData` and stores it in sled:

```rust
// ignite-pay-mcp/src/main.rs
fn get_session_from_auth_response(&self, resp: &AuthResponse) -> Option<SessionKeypair> {
    let keypair_bytes = bs58::decode(secret_key_b58).into_vec().ok()?;
    let keypair = Keypair::try_from(&keypair_array[..]).ok()?;

    let session_data = SessionTokenData {
        owner: self.default_owner,
        ephemeral_signer: keypair.pubkey(),
        target_program: system_program::id(),
        expires_at, spending_limit,
        current_spent: 0,
        scopes,
    };
    // Store in sled for reuse
    // ...
    Some(SessionKeypair { keypair, session_data })
}
```

### 3.4 Building On-Chain Transaction

`IgnitePayClient.execute_sol_transfer()` (`ignite-pay-solana/src/payment.rs`):

```rust
pub async fn execute_sol_transfer(
    &self,
    session: &SessionKeypair,
    recipient: &Pubkey,
    amount_lamports: u64,
) -> Result<PaymentResult> {
    // 1. Verify session is not expired
    if self.session_manager.is_expired(&session.session_data) {
        return Err(SolanaError::SessionExpired);
    }
    // 2. Verify spending limit
    if !self.session_manager.check_spending_limit(&session.session_data, amount_lamports) {
        return Err(SolanaError::SpendingLimitExceeded);
    }

    // 3. Derive session PDA
    let program_id = session_program_id();
    let (session_pda, _) = derive_session_pda(
        &session.session_data.owner,
        &session.keypair.pubkey(),
        &program_id,
    );

    // 4. Build execute_payment instruction
    let ix = build_execute_payment_ix(
        &program_id,
        &session_pda,
        &session.keypair.pubkey(),
        recipient,
        amount_lamports,
        "sol:transfer",
    );

    // 5. Build + sign transaction
    let recent_blockhash = self.rpc_client.get_latest_blockhash()?;
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&session.keypair.pubkey()),  // fee payer = ephemeral signer
        &[&session.keypair],              // signer = ephemeral keypair
        recent_blockhash,
    );

    // 6. Send to Solana RPC
    let sig = self.rpc_client.send_and_confirm_transaction(&tx)?;

    // 7. Record local spending
    self.session_manager.record_spent(&session.keypair.pubkey(), amount_lamports)?;

    Ok(PaymentResult { signature: sig.to_string(), .. })
}
```

### 3.5 On-Chain Instruction Construction

`build_execute_payment_ix()` (`ignite-pay-solana/src/session_program.rs`):

```rust
pub fn build_execute_payment_ix(
    program_id: &Pubkey,
    session_pda: &Pubkey,
    ephemeral_signer: &Pubkey,
    recipient: &Pubkey,
    amount: u64,
    scope: &str,
) -> Instruction {
    let sighash = anchor_sighash("execute_payment");
    // data = [8 bytes sighash][8 bytes amount u64][borsh String scope]

    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new(*session_pda, false),                    // writable
            AccountMeta::new(*ephemeral_signer, true),                // writable + signer
            AccountMeta::new(*recipient, false),                      // writable
            AccountMeta::new_readonly(system_program::id(), false),   // System Program
            AccountMeta::new_readonly(sysvar::clock::id(), false),    // Clock sysvar
        ],
        data,
    }
}
```

### 3.6 On-Chain Verification and Execution

Anchor program `execute_payment` (`ignite-pay-session-program/src/lib.rs`):

```rust
pub fn execute_payment(ctx: Context<ExecutePayment>, amount: u64, scope: String) -> Result<()> {
    let session = &mut ctx.accounts.session;
    let now = ctx.accounts.clock.unix_timestamp;

    // 1. Not expired
    require!(now < session.expires_at, SessionError::SessionExpired);
    // 2. Not revoked
    require!(!session.revoked, SessionError::SessionRevoked);
    // 3. Scope permitted
    require!(session.scopes.contains(&scope), SessionError::ScopeNotPermitted);
    // 4. Spending limit
    let new_spent = session.current_spent.checked_add(amount)
        .ok_or(SessionError::ArithmeticOverflow)?;
    require!(new_spent <= session.spending_limit, SessionError::SpendingLimitExceeded);

    // 5. CPI: system_program::transfer (ephemeral -> recipient)
    let ix = system_instruction::transfer(
        ctx.accounts.ephemeral_signer.key,
        ctx.accounts.recipient.key,
        amount,
    );
    invoke(&ix, &[
        ctx.accounts.ephemeral_signer.to_account_info(),
        ctx.accounts.recipient.to_account_info(),
        ctx.accounts.system_program.to_account_info(),
    ])?;

    // 6. Update on-chain spent amount
    session.current_spent = new_spent;
    Ok(())
}
```

**Account verification** (Anchor `#[derive(Accounts)]`):

```rust
#[derive(Accounts)]
pub struct ExecutePayment<'info> {
    #[account(
        mut,
        constraint = !session.revoked @ SessionError::SessionRevoked,
        constraint = session.ephemeral_signer == ephemeral_signer.key() @ SessionError::Unauthorized,
    )]
    pub session: Account<'info, SessionKeyAccount>,

    #[account(mut)]
    pub ephemeral_signer: Signer<'info>,

    /// CHECK: Recipient
    #[account(mut)]
    pub recipient: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
    pub clock: Sysvar<'info, Clock>,
}
```

---

## 4. SessionManager (Off-Chain Management)

`SessionManager` (`ignite-pay-solana/src/session.rs`) manages session lifecycle based on the sled database:

| Method | Description |
|--------|-------------|
| `create_session()` | Generate random ephemeral Keypair, store in sled |
| `get_active_session()` | Find unexpired session for a specified owner |
| `get_session_by_pubkey()` | Look up directly by ephemeral public key |
| `is_expired()` | Check if `now >= expires_at` |
| `check_spending_limit()` | Check if `current_spent + amount <= spending_limit` |
| `record_spent()` | Update `current_spent` after successful transaction |
| `close_session()` | Delete session |

**sled storage format**:
- Key: `session:{ephemeral_pubkey_base58}`
- Value: `borsh(SessionTokenData) + 64_bytes_keypair`

---

## 5. Security Design

### Multi-Layer Risk Control

| Layer | Check | Location |
|-------|-------|----------|
| MCP risk control | Whitelist, auto-approval threshold | `main.rs: risk_check()` |
| Phone authorization | User confirms payment | DIDComm auth-request/response |
| Off-chain verification | Session expiration, spending limit | `SessionManager` |
| On-chain verification | Expiration, revocation, scope, limit, signature | Anchor program constraints + instructions |

### Session Key Security

- **Ephemerality**: `expires_at` for automatic expiration
- **Spending limit**: `spending_limit` constrains total spending per session
- **Scope**: `scopes` limits the types of operations that can be executed
- **Revocable**: Owner can revoke at any time
- **Isolation**: Each session has an independent PDA, with no mutual interference

---

## 6. Payment Modes

MCP's `execute_payment()` supports three modes:

```rust
// ignite-pay-mcp/src/main.rs
async fn execute_payment(
    solana_client: &Option<Arc<IgnitePayClient>>,
    payment: &PaymentRequest,
    session: &Option<SessionKeypair>,
    channel_client: &Option<Arc<ChannelClient>>,
) -> Result<String, String> {
    match (solana_client, session) {
        (Some(client), Some(sess)) => {
            // Mode 1: Session Key on-chain payment
            client.execute_payment(&payment.recipient, payment.amount, ...).await
        }
        (Some(_), None) => {
            // Mode 2: State Channel payment (no session key needed)
            channel_client.channel_pay(&channel_id, payment.amount, ...).await
        }
        _ => {
            // Mode 3: Mock payment (no Solana configuration)
            Ok(execute_mock_payment(payment))
        }
    }
}
```
