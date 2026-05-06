# Session Key 支付执行流程

本文档记录 MCP（ignite-pay-mcp / ignite-pay-merchant-mcp）使用 Session Key 执行链上支付的完整流程。

相关文档：
- [Session Keys 设计方案](./session-keys.md) — 两种模式的设计对比
- [DIDComm 配对流程](./didcomm-pairing-flow.md) — Phone 与 MCP 建立连接

---

## 1. 架构概览

```
Phone App                    MCP Server                     Solana
   │                              │                            │
   │  生成 ephemeral keypair      │                            │
   │  register_session_key ──────────────────────────────────>│  创建 PDA
   │                              │                            │
   │  DIDComm: session key data ─>│                            │
   │                              │                            │
   │  (等待支付请求)               │                            │
   │                              │                            │
   │<── DIDComm: auth-request ────│  (收到 x402 challenge)     │
   │── DIDComm: auth-response ──>│  (携带授权+session key)    │
   │                              │                            │
   │                              │  execute_payment ──────────>│  CPI transfer
   │                              │<── tx signature ───────────│
   │                              │                            │
   │<── DIDComm: payment-confirm ─│                            │
```

### 核心组件

| 组件 | 文件 | 职责 |
|------|------|------|
| `ignite-pay-session-program` | `ignite-pay-session-program/src/` | 链上 Anchor 程序，验证+执行支付 |
| `SessionManager` | `ignite-pay-solana/src/session.rs` | 链下 session 生命周期管理（sled 存储） |
| `session_program.rs` | `ignite-pay-solana/src/session_program.rs` | 指令构建器（derive PDA, build IX） |
| `IgnitePayClient` | `ignite-pay-solana/src/payment.rs` | Solana RPC 交互，构建+发送交易 |
| MCP Mediator | `ignite-pay-mcp/src/mediator.rs` | DIDComm 消息收发 |
| MCP Tool | `ignite-pay-mcp/src/main.rs` | 支付流程编排 |

### 程序部署

`ignite-pay-session-program` 部署一次到 Solana 链上，所有用户共享：

- **Program ID**: `6EFvVTh7rEBpHH2JGryjKQmBLRtbYtSEerGNfkHqKiei`
- **PDA Seeds**: `["session", owner, ephemeral]` — 每个用户+临时密钥组合产生唯一 PDA
- **Self-Funded 模式**：ephemeral 密钥持有 SOL，作为交易的资金来源和 fee payer

---

## 2. Session Key 生命周期

### 2.1 注册（Phone 端）

Phone 生成 ephemeral Ed25519 密钥对，调用链上 `register_session_key` 指令创建 PDA：

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

PDA 账户状态：

```rust
// ignite-pay-session-program/src/state.rs
#[account]
pub struct SessionKeyAccount {
    pub owner: Pubkey,            // 主钱包地址
    pub ephemeral_signer: Pubkey, // 临时密钥公钥
    pub target_program: Pubkey,   // 目标程序
    pub expires_at: i64,          // 过期时间戳
    pub spending_limit: u64,      // 最大支付额度 (lamports)
    pub current_spent: u64,       // 已花费金额
    pub scopes: Vec<String>,      // 授权范围 (如 ["sol:transfer"])
    pub revoked: bool,            // 是否已撤销
    pub bump: u8,                 // PDA bump
}
```

### 2.2 传输给 MCP

Phone 通过 DIDComm 将 session key 数据发送给 MCP，包含在 `payment-auth-response` 消息中：

```rust
// ignite-pay-mcp/src/payment.rs
pub struct AuthResponse {
    pub authorized: bool,
    pub session_key_pubkey: Option<String>,      // ephemeral 公钥 (base58)
    pub session_key_secret_key: Option<String>,  // ephemeral 私钥 (base58)
    pub session_key_tx_signature: Option<String>, // register_session_key 交易签名
    pub session_expires_at: Option<i64>,         // 过期时间
    pub spending_limit: Option<u64>,             // 消费限额
    pub scopes: Option<Vec<String>>,             // 授权范围
    // ...
}
```

### 2.3 撤销

Owner 可随时调用 `revoke_session` 撤销 session key，也可调用 `close_session` 关闭 PDA 并回收 rent。

---

## 3. 支付执行流程

### 3.1 触发：x402 Challenge

MCP 收到 AI Agent 的 `process_x402_challenge` 工具调用时：

1. 解析 HTTP 402 响应，提取 `network`、`amount`、`token`、`recipient`
2. 创建 `PaymentRequest`（状态：`PendingAuth`）
3. 执行风控检查

```
ignite-pay-mcp/src/main.rs — process_x402_challenge()
```

### 3.2 风控决策

```
                    ┌─────────────┐
                    │ 风控检查     │
                    └──────┬──────┘
                           │
               ┌───────────┼───────────┐
               ▼                       ▼
       AutoApproved              NeedsAuth
       (白名单/低额)              (需要授权)
               │                       │
               ▼                       ▼
      直接执行支付            DIDComm auth-request → Phone
                                     │
                                     ▼
                            Phone 返回 auth-response
                            (authorized + session key)
                                     │
                                     ▼
                              执行支付
```

**自动批准条件**（`ignite-pay-mcp/src/main.rs`）：
- 商户在白名单中，且金额低于 `max_amount`
- 金额低于全局自动批准阈值

**需要授权**时，MCP 通过 DIDComm 向 Phone 发送 `payment-auth-request`，等待 `payment-auth-response`。

### 3.3 获取 Session Key

两种来源：

| 来源 | 说明 | 代码位置 |
|------|------|----------|
| Phone auth response | Phone 生成 ephemeral keypair，随授权返回 | `main.rs: get_session_from_auth_response()` |
| MCP 本地 session | `SessionManager` 从 sled 查找已有活跃 session | `main.rs: get_active_session()` |

MCP 优先使用 Phone 返回的 session key，若无则使用本地 session。

`get_session_from_auth_response()` 将 Phone 返回的 base58 密钥解码为 `Keypair`，构建 `SessionTokenData` 并存入 sled：

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
    // 存入 sled 以便复用
    // ...
    Some(SessionKeypair { keypair, session_data })
}
```

### 3.4 构建链上交易

`IgnitePayClient.execute_sol_transfer()`（`ignite-pay-solana/src/payment.rs`）：

```rust
pub async fn execute_sol_transfer(
    &self,
    session: &SessionKeypair,
    recipient: &Pubkey,
    amount_lamports: u64,
) -> Result<PaymentResult> {
    // 1. 验证 session 未过期
    if self.session_manager.is_expired(&session.session_data) {
        return Err(SolanaError::SessionExpired);
    }
    // 2. 验证消费限额
    if !self.session_manager.check_spending_limit(&session.session_data, amount_lamports) {
        return Err(SolanaError::SpendingLimitExceeded);
    }

    // 3. 派生 session PDA
    let program_id = session_program_id();
    let (session_pda, _) = derive_session_pda(
        &session.session_data.owner,
        &session.keypair.pubkey(),
        &program_id,
    );

    // 4. 构建 execute_payment 指令
    let ix = build_execute_payment_ix(
        &program_id,
        &session_pda,
        &session.keypair.pubkey(),
        recipient,
        amount_lamports,
        "sol:transfer",
    );

    // 5. 构建+签名交易
    let recent_blockhash = self.rpc_client.get_latest_blockhash()?;
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&session.keypair.pubkey()),  // fee payer = ephemeral signer
        &[&session.keypair],              // signer = ephemeral keypair
        recent_blockhash,
    );

    // 6. 发送到 Solana RPC
    let sig = self.rpc_client.send_and_confirm_transaction(&tx)?;

    // 7. 记录本地消费
    self.session_manager.record_spent(&session.keypair.pubkey(), amount_lamports)?;

    Ok(PaymentResult { signature: sig.to_string(), .. })
}
```

F15 新增 `execute_payment_atomic` 方法，在 `payment_mutex` 互斥锁保护下原子化执行：
1. `check_session_balance()` — 检查会话余额是否充足
2. `execute_payment_auto()` — 执行实际支付
3. `record_spent()` — 记录会话消费
4. `record_merchant_spent()` — 记录商户累计消费

### 3.5 链上指令构建

`build_execute_payment_ix()`（`ignite-pay-solana/src/session_program.rs`）：

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

### 3.6 链上验证与执行

Anchor 程序 `execute_payment`（`ignite-pay-session-program/src/lib.rs`）：

```rust
pub fn execute_payment(ctx: Context<ExecutePayment>, amount: u64, scope: String) -> Result<()> {
    let session = &mut ctx.accounts.session;
    let now = ctx.accounts.clock.unix_timestamp;

    // 1. 未过期
    require!(now < session.expires_at, SessionError::SessionExpired);
    // 2. 未撤销
    require!(!session.revoked, SessionError::SessionRevoked);
    // 3. Scope 允许
    require!(session.scopes.contains(&scope), SessionError::ScopeNotPermitted);
    // 4. 消费限额
    let new_spent = session.current_spent.checked_add(amount)
        .ok_or(SessionError::ArithmeticOverflow)?;
    require!(new_spent <= session.spending_limit, SessionError::SpendingLimitExceeded);

    // 5. CPI: system_program::transfer (ephemeral → recipient)
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

    // 6. 更新链上已花费金额
    session.current_spent = new_spent;
    Ok(())
}
```

**账户验证**（Anchor `#[derive(Accounts)]`）：

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

## 4. SessionManager（链下管理）

`SessionManager`（`ignite-pay-solana/src/session.rs`）基于 sled 数据库管理 session 生命周期：

| 方法 | 说明 |
|------|------|
| `create_session()` | 生成随机 ephemeral Keypair，存入 sled |
| `get_active_session()` | 查找指定 owner 的未过期 session |
| `get_session_by_pubkey()` | 按 ephemeral 公钥直接查找 |
| `is_expired()` | 检查 `now >= expires_at` |
| `check_spending_limit()` | 检查 `current_spent + amount <= spending_limit` |
| `record_spent()` | 交易成功后更新 `current_spent` |
| `record_merchant_spent(merchant_did, amount)` | F8: 记录商户累计消费金额（存储在 sled `__merchant_spending__` 树中） |
| `get_merchant_spent(merchant_did) -> u64` | F8: 查询商户累计消费金额 |
| `close_session()` | 删除 session |

**sled 存储格式**：
- Key: `session:{ephemeral_pubkey_base58}`
- Value: `borsh(SessionTokenData) + 64_bytes_keypair`

---

## 5. 安全设计

### 多层风控

| 层级 | 检查项 | 位置 |
|------|--------|------|
| MCP 风控 | 白名单、自动批准阈值 | `main.rs: risk_check()` |
| Phone 授权 | 用户确认支付 | DIDComm auth-request/response |
| 链下验证 | session 过期、消费限额 | `SessionManager` |
| 链上验证 | 过期、撤销、scope、限额、签名 | Anchor program constraints + instructions |

### Session Key 安全

- **临时性**：`expires_at` 自动过期
- **限额**：`spending_limit` 限制单 session 总消费
- **Scope**：`scopes` 限制可执行的操作类型
- **可撤销**：Owner 随时可 revoke
- **隔离性**：每个 session 独立 PDA，互不影响

---

## 6. 支付模式

MCP 的 `execute_payment()` 支持三种模式：

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
            // 模式 1: Session Key 链上支付
            client.execute_payment(&payment.recipient, payment.amount, ...).await
        }
        (Some(_), None) => {
            // 模式 2: State Channel 支付（无需 session key）
            channel_client.channel_pay(&channel_id, payment.amount, ...).await
        }
        _ => {
            // 模式 3: Mock 支付（无 Solana 配置）
            Ok(execute_mock_payment(payment))
        }
    }
}
```
