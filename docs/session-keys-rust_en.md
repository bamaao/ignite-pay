The Session Keys implementation based on Rust (Anchor framework) centers on building a **permission verification layer**.

This approach moves permission logic down to the contract layer (on-chain), combined with frontend and backend Fee Payer logic, ensuring that `Ignite-Pay` supports both fully decentralized self-funded payments and seamless sponsored payments.

---

## 1. Contract Layer Implementation (Anchor Rust)

First, we need to define the `SessionToken` account model on-chain to record authorization state.

### Data Structure Definition
```rust
use anchor_lang::prelude::*;

#[account]
pub struct SessionToken {
    pub owner: Pubkey,          // Authorizer (main wallet)
    pub ephemeral_signer: Pubkey, // Ephemeral key (Session Key)
    pub target_program: Pubkey, // Authorized target program
    pub expires_at: i64,        // Expiration timestamp
    pub spending_limit: u64,    // Maximum total payment allowed
    pub current_spent: u64,     // Total amount spent
    pub scopes: Vec<String>,    // List of authorized instructions
    pub bump: u8,
}
```

### Core Verification Instructions
In specific business instructions (e.g., `process_payment`), verify session validity via `Constraint` or manual checks.

```rust
#[derive(Accounts)]
pub struct ProcessPayment<'info> {
    #[account(mut)]
    pub session_token: Account<'info, SessionToken>,

    /// CHECK: Business fund account
    #[account(mut)]
    pub vault: AccountInfo<'info>,

    #[account(signer)] // The signer here is the ephemeral key
    pub ephemeral_signer: Signer<'info>,

    pub system_program: Program<'info, System>,
}

pub fn handle_payment(ctx: Context<ProcessPayment>, amount: u64) -> Result<()> {
    let session = &mut ctx.accounts.session_token;
    let clock = Clock::get()?;

    // 1. Verify expiration time
    require!(clock.unix_timestamp < session.expires_at, ErrorCode::SessionExpired);

    // 2. Verify signer matches
    require!(
        ctx.accounts.ephemeral_signer.key() == session.ephemeral_signer,
        ErrorCode::InvalidSessionSigner
    );

    // 3. Verify spending limit
    session.current_spent = session.current_spent.checked_add(amount)
        .ok_or(ErrorCode::Overflow)?;
    require!(session.current_spent <= session.spending_limit, ErrorCode::LimitExceeded);

    // Execute business payment logic...
    Ok(())
}
```

---

## 2. Client Integration (Rust SDK)

As an SDK developer, you need to provide a unified interface for `Transaction` construction, selecting different `Fee Payer` based on the mode.

### Transaction Builder (Client-side)
```rust
pub struct IgnitePayClient {
    pub mode: PayMode, // Enum: SelfFunded or Sponsored
    pub relayer_url: Option<String>,
}

impl IgnitePayClient {
    pub async fn execute_agent_tx(
        &self,
        ephemeral_keypair: &Keypair,
        instruction: Instruction,
        fee_payer_pubkey: Pubkey, // For sponsored mode, pass in the Relayer Pubkey
    ) -> Result<Signature> {
        let mut tx = Transaction::new_with_payer(
            &[instruction],
            Some(&fee_payer_pubkey),
        );

        // Get latest blockhash
        let recent_blockhash = self.rpc_client.get_latest_blockhash()?;

        // Mode logic
        match self.mode {
            PayMode::SelfFunded => {
                // Self-funded: ephemeral key holds SOL, fully sign directly
                tx.sign(&[ephemeral_keypair], recent_blockhash);
                self.rpc_client.send_and_confirm_transaction(&tx)
            },
            PayMode::Sponsored => {
                // Sponsored: ephemeral key only partially signs
                tx.partial_sign(&[ephemeral_keypair], recent_blockhash);

                // Serialize partially signed tx and send to Relayer
                let serialized_tx = bincode::serialize(&tx).map_err(|_| Error::SerializationError)?;
                self.call_relayer_api(serialized_tx).await
            }
        }
    }
}
```

---

## 3. Sponsored Payment Server Implementation (Relayer Rust)

The sponsored backend is responsible for the final signature and advancing the Gas.

```rust
#[post("/sponsor")]
async fn sponsor_handler(
    payload: web::Json<SponsorRequest>,
    state: web::Data<AppState>,
) -> impl Responder {
    let mut tx: Transaction = bincode::deserialize(&payload.tx_bytes).unwrap();

    // 1. Business logic verification: e.g., check if the Session has sufficient sponsored quota
    if !validate_request(&tx, &state.db).await {
        return HttpResponse::Forbidden();
    }

    // 2. Sponsor signature (Relayer Wallet)
    // At this point tx already has the Ephemeral Signer's signature, append the Relayer signature here
    tx.partial_sign(&[&state.relayer_keypair], tx.message.recent_blockhash);

    // 3. Broadcast transaction
    match state.rpc_client.send_and_confirm_transaction(&tx) {
        Ok(sig) => HttpResponse::Ok().json(sig.to_string()),
        Err(_) => HttpResponse::InternalServerError().finish(),
    }
}
```

---

## 4. Key Differences Comparison (Implementation Practice)

### Self-Funded Mode Implementation Details:
* **Initialization Cost:** The user's main wallet additionally transfers 0.01~0.05 SOL to `ephemeral_signer.pubkey()` via `system_instruction::transfer` during `CreateSession`.
* **Advantage:** No server involvement required, censorship-resistant.

### Sponsored Mode Implementation Details:
* **Initialization Cost:** The user's main wallet only pays the rent (Rent) for the `SessionToken` account.
* **Relayer Role:** It is recommended to use Redis to cache each Session's sponsored count or frequency, preventing the Relayer wallet from being maliciously drained of Gas.
* **Advantage:** True Agentic experience -- the Agent runs as long as it has permission, without needing to worry about SOL.

---

## 5. Security Recommendations (Architect's Perspective)

1.  **Principle of Least Privilege (Instruction Scoping):**
    Store the callable instruction's `sighash` in `SessionToken`'s `scopes`. Use `require!(session.scopes.contains(&current_instruction_hash))` in the contract to ensure security.
2.  **Automatic Cleanup:**
    It is recommended to implement a `close_session` instruction that allows users to destroy the PDA account and reclaim rent after the Session expires, or enables the Relayer to batch-clean expired Sessions.
