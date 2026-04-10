基于 Rust (Anchor 框架) 实现 Session Keys 的方案，核心在于构建一个**权限校验层**。

这套方案将权限逻辑下沉到合约（On-chain），并结合前端和后端的 Fee Payer 逻辑，确保 `Ignite-Pay` 既能支持完全去中心化的自付，也能支持用户无感的代付。

---

## 1. 合约层实现 (Anchor Rust)

首先，我们需要在链上定义 `SessionToken` 账户模型，用来记录授权状态。

### 数据结构定义
```rust
use anchor_lang::prelude::*;

#[account]
pub struct SessionToken {
    pub owner: Pubkey,          // 授权者（主钱包）
    pub ephemeral_signer: Pubkey, // 临时密钥（Session Key）
    pub target_program: Pubkey, // 授权的目标程序
    pub expires_at: i64,        // 过期时间戳
    pub spending_limit: u64,    // 允许支付的最大总额
    pub current_spent: u64,     // 已支付总额
    pub scopes: Vec<String>,    // 授权的指令列表
    pub bump: u8,
}
```

### 核心校验指令
在具体的业务指令（如 `process_payment`）中，通过 `Constraint` 或手动检查来验证 Session 有效性。

```rust
#[derive(Accounts)]
pub struct ProcessPayment<'info> {
    #[account(mut)]
    pub session_token: Account<'info, SessionToken>,
    
    /// CHECK: 业务资金账户
    #[account(mut)]
    pub vault: AccountInfo<'info>,

    #[account(signer)] // 这里的签名者是临时密钥 Ephemeral Key
    pub ephemeral_signer: Signer<'info>,

    pub system_program: Program<'info, System>,
}

pub fn handle_payment(ctx: Context<ProcessPayment>, amount: u64) -> Result<()> {
    let session = &mut ctx.accounts.session_token;
    let clock = Clock::get()?;

    // 1. 验证过期时间
    require!(clock.unix_timestamp < session.expires_at, ErrorCode::SessionExpired);

    // 2. 验证签名者匹配
    require!(
        ctx.accounts.ephemeral_signer.key() == session.ephemeral_signer,
        ErrorCode::InvalidSessionSigner
    );

    // 3. 验证额度（Spending Limit）
    session.current_spent = session.current_spent.checked_add(amount)
        .ok_or(ErrorCode::Overflow)?;
    require!(session.current_spent <= session.spending_limit, ErrorCode::LimitExceeded);

    // 执行业务支付逻辑...
    Ok(())
}
```

---

## 2. 客户端集成方案 (Rust SDK)

作为 SDK 开发者，你需要提供一个统一的接口来处理 `Transaction` 的构建，并根据模式选择不同的 `Fee Payer`。

### 交易构建器 (Client-side)
```rust
pub struct IgnitePayClient {
    pub mode: PayMode, // Enum: SelfFunded 或 Sponsored
    pub relayer_url: Option<String>,
}

impl IgnitePayClient {
    pub async fn execute_agent_tx(
        &self,
        ephemeral_keypair: &Keypair,
        instruction: Instruction,
        fee_payer_pubkey: Pubkey, // 如果是代付，这里传入 Relayer Pubkey
    ) -> Result<Signature> {
        let mut tx = Transaction::new_with_payer(
            &[instruction],
            Some(&fee_payer_pubkey),
        );

        // 获取最新 Blockhash
        let recent_blockhash = self.rpc_client.get_latest_blockhash()?;
        
        // 模式逻辑
        match self.mode {
            PayMode::SelfFunded => {
                // 自付：临时密钥持有 SOL，直接完全签名
                tx.sign(&[ephemeral_keypair], recent_blockhash);
                self.rpc_client.send_and_confirm_transaction(&tx)
            },
            PayMode::Sponsored => {
                // 代付：临时密钥只进行局部签名 (Partial Sign)
                tx.partial_sign(&[ephemeral_keypair], recent_blockhash);
                
                // 将部分签名的 tx 序列化发送给 Relayer
                let serialized_tx = bincode::serialize(&tx).map_err(|_| Error::SerializationError)?;
                self.call_relayer_api(serialized_tx).await
            }
        }
    }
}
```

---

## 3. 代付服务端实现 (Relayer Rust)

代付后端负责最后一道签名并垫付 Gas。

```rust
#[post("/sponsor")]
async fn sponsor_handler(
    payload: web::Json<SponsorRequest>,
    state: web::Data<AppState>,
) -> impl Responder {
    let mut tx: Transaction = bincode::deserialize(&payload.tx_bytes).unwrap();
    
    // 1. 业务逻辑校验：比如检查该 Session 是否还有足够的代付额度
    if !validate_request(&tx, &state.db).await {
        return HttpResponse::Forbidden();
    }

    // 2. 代付人签名 (Relayer Wallet)
    // 此时 tx 已经有了 Ephemeral Signer 的签名，这里追加 Relayer 签名
    tx.partial_sign(&[&state.relayer_keypair], tx.message.recent_blockhash);

    // 3. 广播交易
    match state.rpc_client.send_and_confirm_transaction(&tx) {
        Ok(sig) => HttpResponse::Ok().json(sig.to_string()),
        Err(_) => HttpResponse::InternalServerError().finish(),
    }
}
```

---

## 4. 关键差异点对比 (落地实操)

### 自付模式实现细节：
* **初始化成本：** 用户主钱包在 `CreateSession` 时，额外通过 `system_instruction::transfer` 转入 0.01~0.05 SOL 到 `ephemeral_signer.pubkey()`。
* **优势：** 无需服务器参与，抗审查。

### 代付模式实现细节：
* **初始化成本：** 用户主钱包仅支付 `SessionToken` 账户的租金（Rent）。
* **Relayer 角色：** 建议使用 Redis 缓存每个 Session 的代付次数或频率，防止 Relayer 钱包被恶意刷取 Gas。
* **优势：** 真正的 Agentic 体验，Agent 只要有权限就能跑，不需要管 SOL。

---

## 5. 安全建议 (架构师视点)

1.  **最小权限原则 (Instruction Scoping):**
    在 `SessionToken` 的 `scopes` 中存储可调用指令的 `sighash`。在合约内使用 `require!(session.scopes.contains(&current_instruction_hash))` 确保安全。
2.  **自动清理:**
    建议实现一个 `close_session` 指令，允许用户在 Session 到期后销毁 PDA 账户并退回租金，或者让 Relayer 能够批量清理过期 Session。