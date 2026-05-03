Ignite-Pay 支付基础设施完整落地的实现方案

---

### 1. 架构整体设计

整个系统由**Solana 主网（L1）**、**MagicBlock 瞬时执行层（ER）**以及**链下防欺诈层**三部分组成。

* **L1（Solana 主网）**：负责通道创建、资金锁定、签名校验与最终结算。所有限额与余额状态均存储在链上 PDA 中，不可被外部参数篡改。

> **当前范围**：以下实现仅支持 SOL。SPL Token（USDC/USDT 等）的扩展方式：`deposit` 改用 Token Program CPI 将 token 转入 Vault 的 Associated Token Account；`settle_batch` 和 `release_settlement` 的资金转移改为 Token Program `transfer` 指令，Vault PDA 作为 signer 授权。核心逻辑（双重签名、Merkle 树、挑战窗口）不变。
* **ER（MagicBlock 瞬时执行层）**：负责高速状态转换（延迟 $< 50\text{ms}$，免 Gas），记录每笔微支付的 Voucher。
* **链下防欺诈层**：负责挑战窗口内的纠纷裁决。买家可在窗口期内提交单笔 Voucher 凭证，与链上 `merkle_root` 做证明校验。

---

### 2. 运作流程：四步闭环

#### 第一步：通道建立 (Initialize Channel)

买家调用链上 `initialize_channel`，创建通道 PDA：

* PDA seeds：`["channel", buyer pubkey, merchant pubkey]`
* 写入 `spending_cap`（消费限额）和 `merchant` 公钥
* **同时计算**对应的 Vault PDA：`["vault", channel pubkey]`，用于托管买家资金
* Channel 中记录 `vault_bump`，后续所有操作通过 seeds + bump 验证 vault 归属

此步骤**记录限额与双方身份，并计算 Vault PDA 地址**。Vault 不由程序 `init`，保持其 owner 为 **System Program**。这样买家通过 `system_instruction::transfer` 存入资金时，Vault 作为普通系统账户接收 SOL（兼容性最佳）；程序后续可通过 `invoke_signed` 调用 System Program 的 `transfer` 从 Vault 转出资金（PDA seeds 匹配即可签名）。Vault 在第一次 `deposit` 之前不存在于链上，首次 `deposit` 时由 System Program 自动创建。

#### 第二步：资金存入 (Deposit)

买家调用链上 `deposit`，将 SOL 转入通道关联的 Vault PDA：

* 通过 `system_instruction::transfer` 从 buyer（Signer）转入 Vault PDA
* Vault PDA 由 **System Program** 拥有（未由程序 `init`），可接收 SOL
* 合约更新 `channel.balance += amount`
* Vault 的 lamports 余额即为该通道的真实可用资金

> **SPL Token 扩展**：当前设计仅支持 SOL。SPL token 的 deposit 需通过 Token Program CPI（`transfer` 指令）将 token 从 buyer ATA 转入 Vault ATA，Vault PDA 作为 signer 授权。

#### 第三步：链下微支付 (State Transition on ER)

买家在 ER 层对每笔消费签署微支付凭证（Voucher）：

$$\text{Voucher} = (\text{channel\_id},\ \text{voucher\_seq},\ \text{amount},\ \text{cumulative\_amount},\ \text{buyer\_sig})$$

字段说明：
* `amount`：本笔消费金额（单笔）。这是 Merkle 树 leaf 的核心字段。
* `cumulative_amount`：截至本笔的累计消费（$= \sum$ 历史 `amount`）。仅用于买家本地校验是否超限，**不参与 Merkle 树构造**。
* `voucher_seq`：Voucher 序号，从 0 开始单调递增。用于区分同一通道内的不同微支付，防重放。
* `buyer_sig`：买家对本笔 Voucher 的 Ed25519 签名。签名消息 = `SHA256(channel_id || voucher_seq || amount)`。

买家在签署前校验 `cumulative_amount <= spending_cap`，确保自身不超限。

> **注意**：`voucher_seq` 与 Channel 上的 `batch_nonce` 是两个不同的计数器。`voucher_seq` 是每笔微支付的序号（可以有数千个），`batch_nonce` 是结算批次号（每次 `settle_batch` 递增 1）。

**Merkle 树构造**：商家在结算周期结束时，将该周期内所有 Voucher 的 `amount` 字段构建 Merkle 树。每个 leaf 为：

$$\text{Leaf} = \text{SHA256}(\text{0x00} \ ||\ \text{channel\_id} \ ||\  \text{voucher\_seq} \ ||\  \text{amount} \ ||\  \text{buyer\_pubkey} \ ||\  \text{buyer\_sig})$$

> 域分隔符 `0x00`（leaf 节点）和 `0x01`（内部节点）用于防御**第二原像攻击**。内部节点格式为 `SHA256(0x01 || lo_hash || lo_sum || hi_hash || hi_sum)`，确保攻击者无法将 leaf 伪造成有效的内部节点。

买家在结算前签署一份**批次授权消息**：

$$\text{SettlementMsg} = \text{SHA256}(\text{merkle\_root}\ ||\ \text{total\_amount}\ ||\ \text{channel\_id}\ ||\ \text{batch\_nonce})$$

其中 `batch_nonce` 即为链上 `channel.nonce`（当前结算批次号）。买家对此消息的 Ed25519 签名即为 `buyer_batch_sig`，商家也对同一消息签名得到 `merchant_batch_sig`。两个签名将在 `settle_batch` 中由合约验证。

#### 第四步：链上结算 (Settlement on L1)

商家调用 `settle_batch`，合约执行三道防线校验：

| 防线 | 校验内容 | 目的 |
|------|---------|------|
| 防线一 | `settled_amount + total_amount <= channel.spending_cap` | 校验累计结算不超过链上限额 |
| 防线二 | `total_amount <= channel.balance` | 校验不超过 Vault 实时余额（`settle_batch` 后 `balance` 同步扣减） |
| 防线三 | 验证买家与商家对 `(merkle_root, total_amount, channel_id, batch_nonce)` 的双重签名 | 确保 Merkle root 和总金额经双方认可 |

校验通过后，资金进入**待释放状态**（存入 Settlement Escrow PDA），而非直接转入商家账户。挑战窗口期结束后，商家才可调用 `release_settlement` 提取资金。

#### 买家提取未使用资金 (Withdraw)

买家可随时调用 `withdraw` 提取通道中的未使用资金。合约校验：

* 调用者是 `channel.buyer`（`has_one` 约束）
* 提取金额不超过 `channel.balance`（`settle_batch` 成功后会同步扣减 `balance`，因此 `balance` 始终等于 Vault 实时余额）

提取后 `channel.balance` 相应减少，Vault PDA 中的 lamports 直接转回买家账户。

---

### 3. 完整代码落地实现

#### ① 链上智能合约代码 (Rust / Anchor)

```rust
use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
    ed25519_program, system_instruction, hash::hashv,
    sysvar::instructions::{load_current_index_checked, load_instruction_at_checked},
};

declare_id!("IgnitePay11111111111111111111111111111111111");

// PDA seed 常量
pub const CHANNEL_SEED: &[u8] = b"channel";
pub const VAULT_SEED: &[u8] = b"vault";
pub const SETTLEMENT_SEED: &[u8] = b"settlement";

#[program]
pub mod ignite_pay {
    use super::*;

    // ── 第一步：创建通道 ─────────────────────────────────────

    pub fn initialize_channel(
        ctx: Context<InitializeChannel>,
        spending_cap: u64,
        challenge_period: i64,
        dispute_period: i64,
    ) -> Result<()> {
        let channel = &mut ctx.accounts.channel;
        channel.buyer = ctx.accounts.buyer.key();
        channel.merchant = ctx.accounts.merchant.key();
        channel.balance = 0;
        channel.spending_cap = spending_cap;
        channel.settled_amount = 0;
        channel.nonce = 0;
        channel.bump = ctx.bumps.channel;
        // vault 未 init，通过 seeds 手动推导 bump 并存入 Channel
        let (_, vault_bump) = Pubkey::find_program_address(
            &[VAULT_SEED, ctx.accounts.channel.key().as_ref()],
            ctx.program_id,
        );
        channel.vault_bump = vault_bump;
        // 可配置时间窗口，适配不同场景（高频小额用短窗口，中频大额用长窗口）
        channel.challenge_period = challenge_period;  // 建议 21600 (6h)
        channel.dispute_period = dispute_period;      // 建议 172800 (48h)
        Ok(())
    }

    // ── 第二步：买家存入资金 ─────────────────────────────────

    pub fn deposit(ctx: Context<Deposit>, amount: u64) -> Result<()> {
        // buyer 是 Signer，直接通过 system_instruction::transfer 转入 Vault PDA
        // Vault PDA 由 System Program 拥有（非程序 init），可直接接收 SOL，无需 invoke_signed
        // 若 Vault 尚未存在于链上（首次 deposit），System Program 会自动创建该账户
        let transfer_ix = system_instruction::transfer(
            &ctx.accounts.buyer.key(),
            &ctx.accounts.vault.key(),
            amount,
        );
        anchor_lang::solana_program::program::invoke(
            &transfer_ix,
            &[
                ctx.accounts.buyer.to_account_info(),
                ctx.accounts.vault.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
        )?;

        // 更新链上余额
        let channel = &mut ctx.accounts.channel;
        channel.balance = channel
            .balance
            .checked_add(amount)
            .ok_or(ErrorCode::ArithmeticOverflow)?;
        Ok(())
    }

    // ── 第四步：批量结算（三道防线） ──────────────────────────

    pub fn settle_batch(
        ctx: Context<BatchSettle>,
        merkle_root: [u8; 32],
        total_amount: u64,
        buyer_batch_sig: [u8; 64],
        merchant_batch_sig: [u8; 64],
    ) -> Result<()> {
        let channel = &ctx.accounts.channel;

        // 防线一：链上限额校验（累计已结算 + 本次 ≤ spending_cap）
        let cumulative = channel
            .settled_amount
            .checked_add(total_amount)
            .ok_or(ErrorCode::ArithmeticOverflow)?;
        require!(
            cumulative <= channel.spending_cap,
            ErrorCode::SpendingCapExceeded
        );

        // 防线二：链上余额校验（balance 即 Vault 实时余额，settle 后同步扣减）
        require!(
            total_amount <= channel.balance,
            ErrorCode::InsufficientBalance
        );

        // 防线三：双重签名校验（指令自省 / Instruction Introspection）
        // Ed25519 是预编译指令，不支持 CPI 调用。
        // 正确做法：客户端将 Ed25519 验证指令作为交易的前置指令，
        // 合约通过 Instructions Sysvar 自省检查这些指令的参数是否匹配。
        //
        // 消息 = SHA256(merkle_root || total_amount || channel_id || batch_nonce)
        // 其中 batch_nonce == channel.nonce（当前批次号，尚未递增）
        let mut msg_preimage = Vec::with_capacity(32 + 8 + 32 + 8);
        msg_preimage.extend_from_slice(&merkle_root);
        msg_preimage.extend_from_slice(&total_amount.to_le_bytes());
        msg_preimage.extend_from_slice(ctx.accounts.channel.key().as_ref());
        msg_preimage.extend_from_slice(&channel.nonce.to_le_bytes());
        let message_hash = hashv(&[&msg_preimage]);

        // 交易结构：[ed25519_verify(s), settle_batch]
        // settle_batch 位于索引 current_ix
        // 支持两种布局：
        //   (a) 两条独立 Ed25519 指令：[ed25519_buyer, ed25519_merchant, settle_batch]
        //   (b) 一条打包指令（num_sigs=2）：[ed25519_packed, settle_batch]
        // 遍历所有前置指令，按 public_key 匹配 buyer / merchant 的 Ed25519 签名
        let current_ix = load_current_index_checked(&ctx.accounts.instruction_sysvar)?;
        require!(
            current_ix >= 1,
            ErrorCode::InvalidTransactionLayout
        );

        let mut buyer_verified = false;
        let mut merchant_verified = false;
        for ix_idx in 0..(current_ix as usize) {
            let result = verify_ed25519_for_pubkey(
                &ctx.accounts.instruction_sysvar,
                ix_idx,
                &message_hash.to_bytes(),
                &buyer_batch_sig,
                &merchant_batch_sig,
                &channel.buyer.to_bytes(),
                &channel.merchant.to_bytes(),
            );
            match result {
                Ok(VerifiedParty::Buyer) => buyer_verified = true,
                Ok(VerifiedParty::Merchant) => merchant_verified = true,
                _ => {} // 跳过不匹配的指令（可能是其他交易的 Ed25519 指令）
            }
        }
        require!(buyer_verified, ErrorCode::BuyerSignatureNotFound);
        require!(merchant_verified, ErrorCode::MerchantSignatureNotFound);

        // 更新已结算金额和余额（balance 同步扣减，保持与 Vault 实际 lamports 一致）
        let channel = &mut ctx.accounts.channel;
        channel.settled_amount = channel
            .settled_amount
            .checked_add(total_amount)
            .ok_or(ErrorCode::ArithmeticOverflow)?;
        channel.balance = channel
            .balance
            .checked_sub(total_amount)
            .ok_or(ErrorCode::ArithmeticOverflow)?;

        // 将资金从 Vault PDA 转入 Settlement Escrow（而非直接给商家）
        let vault_seeds = &[
            VAULT_SEED,
            ctx.accounts.channel.key().as_ref(),
            &[channel.vault_bump],
        ];
        let transfer_ix = system_instruction::transfer(
            &ctx.accounts.vault.key(),
            &ctx.accounts.settlement_escrow.key(),
            total_amount,
        );
        anchor_lang::solana_program::program::invoke_signed(
            &transfer_ix,
            &[
                ctx.accounts.vault.to_account_info(),
                ctx.accounts.settlement_escrow.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
            &[vault_seeds],
        )?;

        // 记录结算批次信息（nonce 使用递增前的值，与 PDA seeds 和签名消息一致）
        let settlement = &mut ctx.accounts.settlement_escrow;
        settlement.channel = ctx.accounts.channel.key();
        settlement.merchant = ctx.accounts.merchant.key();
        settlement.amount = total_amount;
        settlement.merkle_root = merkle_root;
        settlement.nonce = channel.nonce; // 使用当前 nonce（尚未递增）
        settlement.created_at = Clock::get()?.unix_timestamp;
        settlement.claimed = false;
        settlement.disputed = false;
        settlement.bump = ctx.bumps.settlement_escrow;

        // 最后递增 batch_nonce，为下一个批次准备
        channel.nonce = channel
            .nonce
            .checked_add(1)
            .ok_or(ErrorCode::ArithmeticOverflow)?;

        Ok(())
    }

    // ── 挑战窗口后商家提取资金（并关闭 Escrow 回收 Rent） ─────

    pub fn release_settlement(ctx: Context<ReleaseSettlement>) -> Result<()> {
        let settlement = &mut ctx.accounts.settlement_escrow;
        require!(!settlement.claimed, ErrorCode::AlreadyClaimed);
        require!(!settlement.disputed, ErrorCode::Disputed);

        // 挑战窗口：从 channel 读取可配置值
        let now = Clock::get()?.unix_timestamp;
        require!(
            now >= settlement.created_at + ctx.accounts.channel.challenge_period,
            ErrorCode::ChallengePeriodNotExpired
        );

        settlement.claimed = true;

        // 从 Settlement Escrow PDA 转给商家
        let escrow_seeds = &[
            SETTLEMENT_SEED,
            settlement.channel.as_ref(),
            &settlement.nonce.to_le_bytes(),
            &[settlement.bump],
        ];
        let transfer_ix = system_instruction::transfer(
            &ctx.accounts.settlement_escrow.key(),
            &ctx.accounts.merchant.key(),
            settlement.amount,
        );
        anchor_lang::solana_program::program::invoke_signed(
            &transfer_ix,
            &[
                ctx.accounts.settlement_escrow.to_account_info(),
                ctx.accounts.merchant.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
            &[escrow_seeds],
        )?;

        // 关闭 Escrow 账户，回收 Rent lamports 给商家
        let escrow_info = ctx.accounts.settlement_escrow.to_account_info();
        let merchant_info = ctx.accounts.merchant.to_account_info();
        **merchant_info.lamports.borrow_mut() = merchant_info
            .lamports()
            .checked_add(escrow_info.lamports())
            .ok_or(ErrorCode::ArithmeticOverflow)?;
        **escrow_info.lamports.borrow_mut() = 0;
        *escrow_info.try_borrow_mut_data()? = &mut [];

        Ok(())
    }

    // ── 买家发起争议（冻结资金，而非直接退款） ───────────────

    pub fn dispute(ctx: Context<Dispute>) -> Result<()> {
        let settlement = &mut ctx.accounts.settlement_escrow;
        require!(!settlement.claimed, ErrorCode::AlreadyClaimed);
        require!(!settlement.disputed, ErrorCode::AlreadyDisputed);

        // 必须在挑战窗口内（从 channel 读取可配置值）
        let now = Clock::get()?.unix_timestamp;
        require!(
            now < settlement.created_at + ctx.accounts.channel.challenge_period,
            ErrorCode::ChallengePeriodExpired
        );

        // 验证调用者是买家
        require!(
            ctx.accounts.buyer.key() == ctx.accounts.channel.buyer,
            ErrorCode::NotBuyer
        );

        // 冻结资金：仅标记 disputed = true，阻止商家 release_settlement
        // 资金留在 Escrow 中，等待进一步裁决：
        //   - 买家可提交 Merkle proof 证明商家造假（链下举证）
        //   - 或引入仲裁机制 / 延长挑战期由商家提交完整明细反驳
        settlement.disputed = true;

        Ok(())
    }

    // ── 争议超时后商家强制提取（解决买家消失导致资金锁死） ───

    pub fn force_release(ctx: Context<ForceRelease>) -> Result<()> {
        let settlement = &mut ctx.accounts.settlement_escrow;
        require!(settlement.disputed, ErrorCode::NotDisputed);
        require!(!settlement.claimed, ErrorCode::AlreadyClaimed);

        // 争议有效期：dispute 后 dispute_period 秒
        // 若买家在此期间未提交有效 Fraud Proof，商家可强制提取
        let now = Clock::get()?.unix_timestamp;
        require!(
            now >= settlement.created_at + ctx.accounts.channel.dispute_period,
            ErrorCode::DisputePeriodNotExpired
        );

        settlement.claimed = true;

        // 转给商家（含 Escrow Rent 回收）
        let escrow_seeds = &[
            SETTLEMENT_SEED,
            settlement.channel.as_ref(),
            &settlement.nonce.to_le_bytes(),
            &[settlement.bump],
        ];
        let transfer_ix = system_instruction::transfer(
            &ctx.accounts.settlement_escrow.key(),
            &ctx.accounts.merchant.key(),
            settlement.amount,
        );
        anchor_lang::solana_program::program::invoke_signed(
            &transfer_ix,
            &[
                ctx.accounts.settlement_escrow.to_account_info(),
                &ctx.accounts.merchant.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
            &[escrow_seeds],
        )?;

        // 关闭 Escrow 账户，回收 Rent
        let escrow_info = ctx.accounts.settlement_escrow.to_account_info();
        let merchant_info = ctx.accounts.merchant.to_account_info();
        **merchant_info.lamports.borrow_mut() = merchant_info
            .lamports()
            .checked_add(escrow_info.lamports())
            .ok_or(ErrorCode::ArithmeticOverflow)?;
        **escrow_info.lamports.borrow_mut() = 0;
        *escrow_info.try_borrow_mut_data()? = &mut [];

        Ok(())
    }

    // ── 买家提交欺诈证明，验证后退款 ─────────────────────────

    /// 争议逻辑终点：买家通过单笔 Voucher + Sum Merkle Proof 证明商家虚增总额。
    ///
    /// Sum Merkle Tree 的每个内部节点存储 (hash, cumulative_sum)。
    /// 买家只需提交单笔 Voucher 及其 O(log n) 的 Merkle path，
    /// 合约沿 path 重建 root，比对链上 root hash，并检查 root sum 是否与
    /// settlement.amount 一致。若 root hash 匹配但 sum < amount → 欺诈成立。
    ///
    /// 交易大小：Merkle path = log2(N) × (32+8) bytes。128 笔 Voucher 仅需
    /// 7 × 40 = 280 bytes，远低于 Solana 1232 字节交易上限。
    pub fn resolve_dispute(
        ctx: Context<ResolveDispute>,
        // 单笔 Voucher leaf 数据
        voucher_seq: u64,
        voucher_amount: u64,
        buyer_voucher_sig: [u8; 64],
        // Sum Merkle path（siblings）
        sibling_hashes: Vec<[u8; 32]>,
        sibling_sums: Vec<u64>,
    ) -> Result<()> {
        let settlement = &mut ctx.accounts.settlement_escrow;
        require!(settlement.disputed, ErrorCode::NotDisputed);
        require!(!settlement.claimed, ErrorCode::AlreadyClaimed);
        require!(
            ctx.accounts.buyer.key() == ctx.accounts.channel.buyer,
            ErrorCode::NotBuyer
        );
        require!(
            sibling_hashes.len() == sibling_sums.len(),
            ErrorCode::InvalidFraudProof
        );

        // 计算 leaf hash（域分隔符 0x00 防 second preimage attack）
        let mut leaf_preimage = Vec::with_capacity(1 + 32 + 8 + 8 + 32 + 64);
        leaf_preimage.push(0x00); // leaf domain separator
        leaf_preimage.extend_from_slice(ctx.accounts.channel.key().as_ref());
        leaf_preimage.extend_from_slice(&voucher_seq.to_le_bytes());
        leaf_preimage.extend_from_slice(&voucher_amount.to_le_bytes());
        leaf_preimage.extend_from_slice(ctx.accounts.channel.buyer.as_ref());
        leaf_preimage.extend_from_slice(&buyer_voucher_sig);
        let leaf_hash = hashv(&[&leaf_preimage]).to_bytes();

        // 沿 Merkle path 重建 root（验证 proof + 计算 root sum）
        let (root_matches, computed_total) = verify_sum_merkle_proof(
            &leaf_hash,
            voucher_amount,
            &sibling_hashes,
            &sibling_sums,
            &settlement.merkle_root,
        )?;

        // Root hash 必须匹配（proof 有效）
        require!(root_matches, ErrorCode::InvalidFraudProof);
        // Root sum < 声称总额 → 商家虚增 → 欺诈成立
        require!(
            computed_total < settlement.amount,
            ErrorCode::FraudNotProven
        );

        // 回滚 settled_amount：欺诈部分的额度不占用 spending_cap
        let channel = &mut ctx.accounts.channel;
        channel.settled_amount = channel
            .settled_amount
            .checked_sub(settlement.amount)
            .ok_or(ErrorCode::ArithmeticOverflow)?;

        settlement.claimed = true;

        // 退还买家
        let escrow_seeds = &[
            SETTLEMENT_SEED,
            settlement.channel.as_ref(),
            &settlement.nonce.to_le_bytes(),
            &[settlement.bump],
        ];
        let transfer_ix = system_instruction::transfer(
            &ctx.accounts.settlement_escrow.key(),
            &ctx.accounts.buyer.key(),
            settlement.amount,
        );
        anchor_lang::solana_program::program::invoke_signed(
            &transfer_ix,
            &[
                ctx.accounts.settlement_escrow.to_account_info(),
                ctx.accounts.buyer.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
            &[escrow_seeds],
        )?;

        // 关闭 Escrow，回收 Rent 给买家
        let escrow_info = ctx.accounts.settlement_escrow.to_account_info();
        let buyer_info = ctx.accounts.buyer.to_account_info();
        **buyer_info.lamports.borrow_mut() = buyer_info
            .lamports()
            .checked_add(escrow_info.lamports())
            .ok_or(ErrorCode::ArithmeticOverflow)?;
        **escrow_info.lamports.borrow_mut() = 0;
        *escrow_info.try_borrow_mut_data()? = &mut [];

        Ok(())
    }

    // ── 买家提取未使用资金 ──────────────────────────────────

    pub fn withdraw(ctx: Context<Withdraw>, amount: u64) -> Result<()> {
        let channel = &mut ctx.accounts.channel;

        // 校验可提取余额（balance 即 Vault 实时余额）
        require!(amount <= channel.balance, ErrorCode::InsufficientBalance);

        channel.balance = channel
            .balance
            .checked_sub(amount)
            .ok_or(ErrorCode::ArithmeticOverflow)?;

        // 从 Vault PDA 转回买家
        let vault_seeds = &[
            VAULT_SEED,
            ctx.accounts.channel.key().as_ref(),
            &[channel.vault_bump],
        ];
        let transfer_ix = system_instruction::transfer(
            &ctx.accounts.vault.key(),
            &ctx.accounts.buyer.key(),
            amount,
        );
        anchor_lang::solana_program::program::invoke_signed(
            &transfer_ix,
            &[
                ctx.accounts.vault.to_account_info(),
                ctx.accounts.buyer.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
            &[vault_seeds],
        )?;

        Ok(())
    }
}

// ── Ed25519 指令自省辅助函数 ──────────────────────────────────
// Solana 的 Ed25519 预编译指令不支持 CPI，必须通过 Instructions Sysvar
// 自省检查交易中是否包含正确的签名验证指令。
//
// Ed25519 指令数据格式（支持 num_sigs >= 1，即单条指令可验证多个签名）：
//   [0..2]   num_signatures (u16 LE) — 该指令包含的签名数量
//   [2..4]   padding
//   每个签名占 14 字节 header + 实际数据：
//     [off+0..off+2]   signature_offset (u16 LE)
//     [off+2..off+4]   signature_instruction_index (u16 LE)
//     [off+4..off+6]   public_key_offset (u16 LE)
//     [off+6..off+8]   public_key_instruction_index (u16 LE)
//     [off+8..off+10]  message_data_offset (u16 LE)
//     [off+10..off+12] message_data_size (u16 LE)
//     [off+12..off+14] message_instruction_index (u16 LE)
//   [header_end..]  实际数据：每条 sig = signature(64B) + public_key(32B) + message(N bytes)

// ── Sum Merkle Proof 验证辅助函数 ──────────────────────────
// Sum Merkle Tree 的每个节点存储 (hash, cumulative_sum)。
// 内部节点: hash = SHA256(lo_hash || lo_sum_le || hi_hash || hi_sum_le)
//           sum = left_sum + right_sum
// 其中 lo/hi 按 hash 字典序排序。
//
// 买家只需提供 log2(N) 个 sibling 节点，合约沿 path 向上重建 root，
// 返回 (root_hash_matches, computed_total_sum)。
// 交易大小：log2(128) × (32+8) = 280 bytes，远低于 1232 字节限制。

fn verify_sum_merkle_proof(
    leaf_hash: &[u8; 32],
    leaf_amount: u64,
    sibling_hashes: &[[u8; 32]],
    sibling_sums: &[u64],
    expected_root: &[u8; 32],
) -> Result<(bool, u64)> {
    let mut current_hash = *leaf_hash;
    let mut current_sum = leaf_amount;

    for i in 0..sibling_hashes.len() {
        let (lo_hash, lo_sum, hi_hash, hi_sum) = if current_hash <= sibling_hashes[i] {
            (current_hash, current_sum, sibling_hashes[i], sibling_sums[i])
        } else {
            (sibling_hashes[i], sibling_sums[i], current_hash, current_sum)
        };

        // 节点 hash = SHA256(0x01 || lo_hash || lo_sum_le || hi_hash || hi_sum_le)
        // 域分隔符 0x01 防止 second preimage attack（与 leaf 的 0x00 区分）
        let mut buf = [0u8; 81]; // 1 + 32 + 8 + 32 + 8
        buf[0] = 0x01; // internal node domain separator
        buf[1..33].copy_from_slice(&lo_hash);
        buf[33..41].copy_from_slice(&lo_sum.to_le_bytes());
        buf[41..73].copy_from_slice(&hi_hash);
        buf[73..81].copy_from_slice(&hi_sum.to_le_bytes());

        current_hash = hashv(&[&buf]).to_bytes();
        current_sum = current_sum
            .checked_add(sibling_sums[i])
            .ok_or(ErrorCode::ArithmeticOverflow)?;
    }

    Ok((current_hash == *expected_root, current_sum))
}

// 遍历式 Ed25519 签名校验：按 public_key 自动匹配 buyer / merchant
// 替代硬编码索引方案，防止组合交易中的索引混淆攻击。
// 支持 num_sigs >= 1：单条 Ed25519 指令可包含多个签名（如买家+商家打包在同一条指令中）。

enum VerifiedParty { Buyer, Merchant }

fn verify_ed25519_for_pubkey(
    ix_sysvar: &AccountInfo<'_>,
    ix_index: usize,
    expected_message: &[u8],
    buyer_sig: &[u8],
    merchant_sig: &[u8],
    buyer_pubkey: &[u8],
    merchant_pubkey: &[u8],
) -> Result<VerifiedParty> {
    let ix = load_instruction_at_checked(ix_index, ix_sysvar)
        .map_err(|_| ErrorCode::InvalidSignatureInstruction)?;

    require!(
        ix.program_id == ed25519_program::id(),
        ErrorCode::InvalidSignatureInstruction
    );

    let data = &ix.data;
    require!(data.len() >= 4, ErrorCode::InvalidSignatureInstruction);

    let num_sigs = u16::from_le_bytes([data[0], data[1]]) as usize;
    require!(num_sigs >= 1, ErrorCode::InvalidSignatureInstruction);

    // 遍历该指令中的每个签名条目
    for sig_idx in 0..num_sigs {
        let header_offset = 4 + sig_idx * 14;
        require!(data.len() >= header_offset + 14, ErrorCode::InvalidSignatureInstruction);

        let sig_offset = u16::from_le_bytes([data[header_offset], data[header_offset + 1]]) as usize;
        let sig_ix_idx = u16::from_le_bytes([data[header_offset + 2], data[header_offset + 3]]);
        let pubkey_offset = u16::from_le_bytes([data[header_offset + 4], data[header_offset + 5]]) as usize;
        let pubkey_ix_idx = u16::from_le_bytes([data[header_offset + 6], data[header_offset + 7]]);
        let msg_offset = u16::from_le_bytes([data[header_offset + 8], data[header_offset + 9]]) as usize;
        let msg_size = u16::from_le_bytes([data[header_offset + 10], data[header_offset + 11]]) as usize;
        let msg_ix_idx = u16::from_le_bytes([data[header_offset + 12], data[header_offset + 13]]);

        require!(sig_ix_idx == 0, ErrorCode::InvalidSignatureInstruction);
        require!(pubkey_ix_idx == 0, ErrorCode::InvalidSignatureInstruction);
        require!(msg_ix_idx == 0, ErrorCode::InvalidSignatureInstruction);

        // 提取公钥
        require!(data.len() >= pubkey_offset + 32, ErrorCode::InvalidSignatureInstruction);
        let pubkey = &data[pubkey_offset..pubkey_offset + 32];

        // 判断是 buyer 还是 merchant 的签名
        let (expected_sig, party) = if pubkey == buyer_pubkey {
            (buyer_sig, VerifiedParty::Buyer)
        } else if pubkey == merchant_pubkey {
            (merchant_sig, VerifiedParty::Merchant)
        } else {
            continue; // 跳过不匹配的公钥（可能是同一指令中其他签名）
        };

        // 比对签名
        require!(data.len() >= sig_offset + 64, ErrorCode::InvalidSignatureInstruction);
        require!(
            &data[sig_offset..sig_offset + 64] == expected_sig,
            ErrorCode::SignatureMismatch
        );

        // 比对消息
        require!(data.len() >= msg_offset + msg_size, ErrorCode::InvalidSignatureInstruction);
        require!(
            &data[msg_offset..msg_offset + msg_size] == expected_message,
            ErrorCode::SignatureMismatch
        );

        return Ok(party);
    }

    Err(ErrorCode::InvalidSignatureInstruction.into())
}

// ── Account Structs ───────────────────────────────────────────

#[derive(Accounts)]
#[instruction(spending_cap: u64, challenge_period: i64, dispute_period: i64)]
pub struct InitializeChannel<'info> {
    #[account(
        init,
        payer = buyer,
        space = Channel::SPACE,
        seeds = [CHANNEL_SEED, buyer.key().as_ref(), merchant.key().as_ref()],
        bump,
    )]
    pub channel: Account<'info, Channel>,
    /// CHECK: Vault PDA — 不由程序 init，保持 System Program 拥有。
    /// 这样 buyer 的 system_instruction::transfer 可直接存入（无需 invoke_signed），
    /// 程序转出时通过 invoke_signed + PDA seeds 即可代表 vault 签名。
    #[account(
        mut,
        seeds = [VAULT_SEED, channel.key().as_ref()],
        bump,
    )]
    pub vault: UncheckedAccount<'info>,
    #[account(mut)]
    pub buyer: Signer<'info>,
    /// CHECK: Trusted merchant account
    pub merchant: SystemAccount<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Deposit<'info> {
    #[account(
        mut,
        has_one = buyer,
        seeds = [CHANNEL_SEED, channel.buyer.as_ref(), channel.merchant.as_ref()],
        bump = channel.bump,
    )]
    pub channel: Account<'info, Channel>,
    /// CHECK: Vault PDA — seeds bind it to this channel, owner 为 System Program
    #[account(
        mut,
        seeds = [VAULT_SEED, channel.key().as_ref()],
        bump = channel.vault_bump,
    )]
    pub vault: UncheckedAccount<'info>,
    #[account(mut)]
    pub buyer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct BatchSettle<'info> {
    #[account(
        mut,
        has_one = merchant,
        seeds = [CHANNEL_SEED, channel.buyer.as_ref(), merchant.key().as_ref()],
        bump = channel.bump,
    )]
    pub channel: Account<'info, Channel>,
    /// CHECK: Vault PDA bound to channel, owner 为 System Program
    #[account(
        mut,
        seeds = [VAULT_SEED, channel.key().as_ref()],
        bump = channel.vault_bump,
    )]
    pub vault: UncheckedAccount<'info>,
    /// CHECK: Settlement escrow — one per batch
    #[account(
        init,
        payer = merchant,
        space = SettlementEscrow::SPACE,
        seeds = [SETTLEMENT_SEED, channel.key().as_ref(), channel.nonce.to_le_bytes().as_ref()],
        bump,
    )]
    pub settlement_escrow: Account<'info, SettlementEscrow>,
    #[account(mut)]
    pub merchant: Signer<'info>,
    /// CHECK: Instructions sysvar — 用于指令自省校验 Ed25519 签名
    #[account(address = anchor_lang::solana_program::sysvar::instructions::id())]
    pub instruction_sysvar: AccountInfo<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ReleaseSettlement<'info> {
    #[account(mut, has_one = merchant)]
    pub channel: Account<'info, Channel>,
    #[account(
        mut,
        seeds = [SETTLEMENT_SEED, channel.key().as_ref(), settlement_escrow.nonce.to_le_bytes().as_ref()],
        bump = settlement_escrow.bump,
        constraint = !settlement_escrow.claimed @ ErrorCode::AlreadyClaimed,
        constraint = !settlement_escrow.disputed @ ErrorCode::Disputed,
    )]
    pub settlement_escrow: Account<'info, SettlementEscrow>,
    #[account(mut)]
    pub merchant: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Dispute<'info> {
    #[account(mut)]
    pub channel: Account<'info, Channel>,
    #[account(
        mut,
        seeds = [SETTLEMENT_SEED, channel.key().as_ref(), settlement_escrow.nonce.to_le_bytes().as_ref()],
        bump = settlement_escrow.bump,
        constraint = !settlement_escrow.claimed @ ErrorCode::AlreadyClaimed,
        constraint = !settlement_escrow.disputed @ ErrorCode::AlreadyDisputed,
    )]
    pub settlement_escrow: Account<'info, SettlementEscrow>,
    #[account(mut)]
    pub buyer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ForceRelease<'info> {
    #[account(mut, has_one = merchant)]
    pub channel: Account<'info, Channel>,
    #[account(
        mut,
        seeds = [SETTLEMENT_SEED, channel.key().as_ref(), settlement_escrow.nonce.to_le_bytes().as_ref()],
        bump = settlement_escrow.bump,
        constraint = settlement_escrow.disputed @ ErrorCode::NotDisputed,
        constraint = !settlement_escrow.claimed @ ErrorCode::AlreadyClaimed,
    )]
    pub settlement_escrow: Account<'info, SettlementEscrow>,
    #[account(mut)]
    pub merchant: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ResolveDispute<'info> {
    #[account(mut)]
    pub channel: Account<'info, Channel>,
    #[account(
        mut,
        seeds = [SETTLEMENT_SEED, channel.key().as_ref(), settlement_escrow.nonce.to_le_bytes().as_ref()],
        bump = settlement_escrow.bump,
        constraint = settlement_escrow.disputed @ ErrorCode::NotDisputed,
        constraint = !settlement_escrow.claimed @ ErrorCode::AlreadyClaimed,
    )]
    pub settlement_escrow: Account<'info, SettlementEscrow>,
    #[account(mut)]
    pub buyer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Withdraw<'info> {
    #[account(
        mut,
        has_one = buyer,
        seeds = [CHANNEL_SEED, channel.buyer.as_ref(), channel.merchant.as_ref()],
        bump = channel.bump,
    )]
    pub channel: Account<'info, Channel>,
    /// CHECK: Vault PDA bound to channel, owner 为 System Program
    #[account(
        mut,
        seeds = [VAULT_SEED, channel.key().as_ref()],
        bump = channel.vault_bump,
    )]
    pub vault: UncheckedAccount<'info>,
    #[account(mut)]
    pub buyer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

// ── State Structs ─────────────────────────────────────────────

#[account]
pub struct Channel {
    pub buyer: Pubkey,            // 32
    pub merchant: Pubkey,         // 32
    pub balance: u64,             // 8 — Vault 实时余额（settle 后同步扣减）
    pub spending_cap: u64,        // 8 — 消费限额（累计结算总额上限）
    pub settled_amount: u64,      // 8 — 已结算额度占用（spending_cap 计数器，争议退款时回滚）
    pub nonce: u64,               // 8 — 结算批次号
    pub challenge_period: i64,    // 8 — 挑战窗口（秒）
    pub dispute_period: i64,      // 8 — 争议解决期（秒）
    pub bump: u8,                 // 1 — channel PDA bump
    pub vault_bump: u8,           // 1 — vault PDA bump
}

impl Channel {
    pub const SPACE: usize = 8 + 32 + 32 + 8 + 8 + 8 + 8 + 8 + 8 + 1 + 1; // 122
}

#[account]
pub struct SettlementEscrow {
    pub channel: Pubkey,          // 32
    pub merchant: Pubkey,         // 32
    pub amount: u64,              // 8
    pub merkle_root: [u8; 32],    // 32
    pub nonce: u64,               // 8 — 结算时的 channel.nonce（递增前），同时用于 PDA seeds
    pub created_at: i64,          // 8 — 结算时间戳
    pub claimed: bool,            // 1 — 商家是否已提取
    pub disputed: bool,           // 1 — 买家是否发起争议
    pub bump: u8,                 // 1
}

impl SettlementEscrow {
    pub const SPACE: usize = 8 + 32 + 32 + 8 + 32 + 8 + 8 + 1 + 1 + 1; // 131
}

#[error_code]
pub enum ErrorCode {
    #[msg("Exceeds the allowed spending cap.")]
    SpendingCapExceeded,
    #[msg("Insufficient balance in the channel.")]
    InsufficientBalance,
    #[msg("Settlement already claimed.")]
    AlreadyClaimed,
    #[msg("Settlement is under dispute.")]
    Disputed,
    #[msg("Settlement already disputed.")]
    AlreadyDisputed,
    #[msg("Challenge period has not expired.")]
    ChallengePeriodNotExpired,
    #[msg("Challenge period has expired.")]
    ChallengePeriodExpired,
    #[msg("Only the buyer can dispute.")]
    NotBuyer,
    #[msg("Arithmetic overflow.")]
    ArithmeticOverflow,
    #[msg("Invalid Ed25519 signature instruction.")]
    InvalidSignatureInstruction,
    #[msg("Signature does not match expected value.")]
    SignatureMismatch,
    #[msg("Invalid transaction layout: at least one Ed25519 instruction must precede settle_batch.")]
    InvalidTransactionLayout,
    #[msg("Settlement is not disputed.")]
    NotDisputed,
    #[msg("Dispute period has not expired.")]
    DisputePeriodNotExpired,
    #[msg("Invalid fraud proof.")]
    InvalidFraudProof,
    #[msg("Fraud not proven: actual total matches claimed total.")]
    FraudNotProven,
    #[msg("Buyer Ed25519 signature instruction not found in transaction.")]
    BuyerSignatureNotFound,
    #[msg("Merchant Ed25519 signature instruction not found in transaction.")]
    MerchantSignatureNotFound,
}
```

#### ② 客户端数据绑定接口 (TypeScript)

买家在结算前需要签署批次消息，以下是签名消息的组装与 `settle_batch` 参数的构造：

```typescript
import { Connection, PublicKey, Transaction, SystemProgram, TransactionInstruction, Keypair } from '@solana/web3.js';
import * as borsh from 'borsh';
import { createHash } from 'crypto';
import nacl from 'tweetnacl';

// ── Voucher 结构（链下单笔凭证） ──────────────────────────────

export class Voucher {
    channelId: Buffer;     // 32 bytes
    voucherSeq: bigint;    // u64 — 微支付序号（非 batch_nonce）
    amount: bigint;        // u64 — 单笔金额
    cumulativeAmount: bigint; // u64 — 累计金额（仅用于本地校验）
    buyerPubkey: Buffer;   // 32 bytes
    buyerSig: Buffer;      // 64 bytes — 买家对单笔的签名
}

// ── Merkle 树构建 ─────────────────────────────────────────────

// Leaf = SHA256(0x00 || channel_id || voucher_seq || amount || buyer_pubkey || buyer_sig)
// 0x00 域分隔符防 second preimage attack
export function hashLeaf(voucher: Voucher): Buffer {
    const seqBuf = Buffer.alloc(8);
    seqBuf.writeBigUInt64LE(voucher.voucherSeq);
    const amountBuf = Buffer.alloc(8);
    amountBuf.writeBigUInt64LE(voucher.amount);
    const data = Buffer.concat([
        Buffer.from([0x00]), // leaf domain separator
        voucher.channelId,
        seqBuf,
        amountBuf,
        voucher.buyerPubkey,
        voucher.buyerSig,
    ]);
    return createHash('sha256').update(data).digest();
}

// ── Sum Merkle 树构建 ──────────────────────────────────────────
// Sum Merkle Tree 的每个节点存储 (hash, cumulative_sum)。
// 内部节点: hash = SHA256(lo_hash || lo_sum_le || hi_hash || hi_sum_le)
//           sum = left_sum + right_sum
// 排序规则与 Rust 端 verify_sum_merkle_proof 一致：按 hash 字典序排序。

interface SumMerkleNode {
    hash: Buffer;
    sum: bigint;
}

export function buildSumMerkleTree(vouchers: Voucher[]): { root: Buffer; totalAmount: bigint; nodes: SumMerkleNode[][] } {
    let nodes: SumMerkleNode[][] = [];
    let currentLevel: SumMerkleNode[] = vouchers.map(v => ({
        hash: hashLeaf(v),
        sum: v.amount,
    }));
    let totalAmount = 0n;
    for (const v of vouchers) totalAmount += v.amount;
    nodes.push(currentLevel);

    while (currentLevel.length > 1) {
        const nextLevel: SumMerkleNode[] = [];
        for (let i = 0; i < currentLevel.length; i += 2) {
            const left = currentLevel[i];
            const right = i + 1 < currentLevel.length
                ? currentLevel[i + 1]
                : { hash: Buffer.alloc(32), sum: 0n };
            const [lo, hi] = left.hash.compare(right.hash) < 0
                ? [left, right]
                : [right, left];
            const loSumBuf = Buffer.alloc(8);
            loSumBuf.writeBigUInt64LE(lo.sum);
            const hiSumBuf = Buffer.alloc(8);
            hiSumBuf.writeBigUInt64LE(hi.sum);
            const combined = Buffer.concat([
                Buffer.from([0x01]), // internal node domain separator
                lo.hash, loSumBuf, hi.hash, hiSumBuf,
            ]);
            nextLevel.push({
                hash: createHash('sha256').update(combined).digest(),
                sum: left.sum + right.sum,
            });
        }
        currentLevel = nextLevel;
        nodes.push(currentLevel);
    }
    return { root: currentLevel[0].hash, totalAmount, nodes };
}

// 从 Sum Merkle Tree 中提取单笔 Voucher 的 Merkle proof
export function generateMerkleProof(
    voucher: Voucher,
    nodes: SumMerkleNode[][],
): { siblingHashes: Buffer[]; siblingSums: bigint[] } {
    const leafHash = hashLeaf(voucher);
    const siblingHashes: Buffer[] = [];
    const siblingSums: bigint[] = [];

    let currentHash = leafHash;
    for (let level = 0; level < nodes.length - 1; level++) {
        const levelNodes = nodes[level];
        const idx = levelNodes.findIndex(n => n.hash.equals(currentHash));
        if (idx === -1) throw new Error('Voucher not found in Merkle tree');
        const siblingIdx = idx % 2 === 0 ? idx + 1 : idx - 1;
        const sibling = siblingIdx < levelNodes.length
            ? levelNodes[siblingIdx]
            : { hash: Buffer.alloc(32), sum: 0n };
        siblingHashes.push(sibling.hash);
        siblingSums.push(sibling.sum);

        // Move to parent
        const parentIdx = Math.floor(idx / 2);
        currentHash = nodes[level + 1][parentIdx].hash;
    }
    return { siblingHashes, siblingSums };
}

// ── 批次签名消息 ─────────────────────────────────────────────

// SettlementMsg = SHA256(merkle_root || total_amount || channel_id || batch_nonce)
// batch_nonce 即链上 channel.nonce（当前批次号）
export function buildSettlementMessage(
    merkleRoot: Buffer,
    totalAmount: bigint,
    channelId: Buffer,
    batchNonce: bigint,
): Buffer {
    const amountBuf = Buffer.alloc(8);
    amountBuf.writeBigUInt64LE(totalAmount);
    const nonceBuf = Buffer.alloc(8);
    nonceBuf.writeBigUInt64LE(batchNonce);
    const data = Buffer.concat([
        merkleRoot,
        amountBuf,
        channelId,
        nonceBuf,
    ]);
    return createHash('sha256').update(data).digest();
}

// ── settle_batch 指令参数 ────────────────────────────────────

// ⚠️ 安全提醒：买家客户端在签署 SettlementMsg 之前，必须执行以下本地校验：
//   1. 重新计算 Merkle Tree Root，确认与商家提供的 merkle_root 一致
//   2. 确认 total_amount 等于所有 Voucher 的 amount 之和
//   3. 确认 batch_nonce 等于链上 channel.nonce（当前批次号）
//   4. 确认 total_amount 不超过 channel.spending_cap - channel.settled_amount
// 这些校验无法在链上完成，必须在 SDK 中强制执行，防止商家伪造参数骗取签名。

export class SettleBatchParams {
    merkleRoot: number[];
    totalAmount: bigint;
    buyerBatchSig: number[];      // 买家对 SettlementMsg 的签名
    merchantBatchSig: number[];   // 商家对 SettlementMsg 的签名

    constructor(fields: { merkleRoot: number[]; totalAmount: bigint; buyerBatchSig: number[]; merchantBatchSig: number[] }) {
        this.merkleRoot = fields.merkleRoot;
        this.totalAmount = fields.totalAmount;
        this.buyerBatchSig = fields.buyerBatchSig;
        this.merchantBatchSig = fields.merchantBatchSig;
    }
}

export const settleBatchSchema = new Map([
    [
        SettleBatchParams,
        {
            kind: 'struct',
            fields: [
                ['merkleRoot', [32]],
                ['totalAmount', 'u64'],
                ['buyerBatchSig', [64]],
                ['merchantBatchSig', [64]],
            ],
        },
    ],
]);
```

---

### 4. 纠纷处理流程 (Dispute & Freeze & Force Release)

资金不直接转给商家，而是进入 Settlement Escrow PDA，经历以下生命周期：

```
settle_batch          6h 挑战窗口           release_settlement
    │                     │                        │
    ▼                     ▼                        ▼
┌──────────┐       ┌──────────┐             ┌──────────┐
│  Vault   │──────►│ Escrow   │──无争议──►  │ Merchant │ → close escrow
│  (买家)  │       │ (托管)   │             │  (商家)  │   (回收 rent)
└──────────┘       └──────────┘             └──────────┘
                         │
                         │ dispute (买家挑战)
                         ▼
                   ┌──────────┐
                   │  Escrow  │  ← 资金冻结，等待裁决
                   │ (冻结)   │
                   └──────────┘
                         │
            ┌────────────┼────────────┐
            │            │            │
            ▼            ▼            ▼
      买家举证       48h 后买家     双方协商
      (Fraud Proof)  仍无动作       解冻
            │            │            │
            ▼            ▼            ▼
   resolve_dispute  force_release   release 给商家
   (验证+退款给买家)(商家强制提取)   (close escrow)
```

**挑战窗口**：`settle_batch` 后 6 小时（21600 秒）内：
* **商家**：无法提取资金（`release_settlement` 被时间锁拦截）
* **买家**：可调用 `dispute` 冻结 Escrow 资金。合约校验调用者是 `channel.buyer` 后，标记 `disputed = true`，**资金留在 Escrow 中不退还**，阻止 `release_settlement`。
* **Merkle 举证**：买家如需证明商家伪造了明细，可在链下提供单笔 Voucher 的 Merkle proof，与链上 `settlement_escrow.merkle_root` 比对。若 proof 不匹配，说明树中不包含该 Voucher，构成商家造假证据。

**争议解决路径**（`dispute` 之后）：
1. **欺诈证明（`resolve_dispute`）**：买家调用 `resolve_dispute`，提交**单笔 Voucher + O(log N) 的 Sum Merkle Proof**。合约沿 Merkle path 重建 root：若 root hash 匹配但 root sum < `settlement.amount`，证明商家虚增 `total_amount`，合约将 Escrow 资金退还买家并关闭账户。交易大小：128 笔 Voucher 仅需 280 bytes proof，远低于 1232 字节限制。
2. **强制释放（`force_release`）**：若 `dispute` 后 48 小时内买家未提交有效 Fraud Proof，商家可调用 `force_release` 强制提取资金。这防止买家恶意 dispute 后消失导致资金永久锁死。
3. **仲裁机制**：引入第三方仲裁者裁决（可在 Channel 中预设 `arbiter` 公钥扩展）。

> 这种设计平衡了买卖双方利益：买家不能无条件拿回资金（需举证），商家也不会因恶意 dispute 永久损失资金（48h 超时保护）。

> **`merkle_root` 的作用**：`merkle_root` 在链上不参与 leaf 级别的验证，其作用是：(1) 作为双重签名消息的一部分，将签名绑定到具体的 Voucher 集合；(2) 存储在 `SettlementEscrow` 中，供链下 Merkle 举证使用。如果未来需要链上验证单笔 Voucher，可添加 `verify_voucher` 指令，接受 Merkle proof 并与存储的 root 比对。

**Escrow 账户生命周期**：`release_settlement` 和 `force_release` 在转移资金后会**关闭 Escrow 账户并回收 Rent**，避免高频结算场景下大量废弃账户消耗 SOL 押金。

**正常窗口过期后**：商家调用 `release_settlement`，合约验证时间戳后释放资金并关闭账户。

---

### 5. 设计约束与说明

* **单通道约束**：Channel PDA seeds 为 `["channel", buyer, merchant]`，同一买家-商家对仅能创建一个通道。如需多通道并行，需扩展 seeds（如加入 `channel_id`）。
* **`spending_cap` 是可循环使用的额度上限**：`settled_amount` 记录当前已结算但未争议的累计金额，约束为 `settled_amount + new_batch_amount <= spending_cap`。若发生欺诈争议且买家胜诉（`resolve_dispute`），该批次的 `amount` 从 `settled_amount` 中回滚，释放对应的 `spending_cap` 额度供后续批次使用。这意味着 `spending_cap` 不是通道生命周期内的"一次性总额度"，而是"任意时刻在途 + 已完成结算的上限"。这种设计确保：被欺诈退回的资金不会永久占用买家的消费额度。
* **`balance` 即 Vault 实时余额**：`settle_batch` 成功后同步扣减 `channel.balance`（资金已转入 Escrow）。`withdraw` 直接校验 `amount <= channel.balance`。`balance` 与 `settled_amount` 是两个独立的记账维度——`balance` 跟踪实际资金（Vault lamports），`settled_amount` 跟踪额度占用（spending_cap 计数器）。
* **`cumulative_amount` 是客户端侧软约束**：此字段仅在买家本地客户端中用于自我提醒不超限，链上不校验。保护效力取决于客户端是否正确实现。
* **Sum Merkle Tree**：Merkle 树每个内部节点存储 `(hash, cumulative_sum)`，root 的 sum 即 Voucher 金额总和。节点 hash = `SHA256(0x01 || lo_hash || lo_sum_le || hi_hash || hi_sum_le)`，leaf hash = `SHA256(0x00 || ...)`。域分隔符 `0x00`/`0x01` 防**第二原像攻击**。欺诈证明仅需单笔 Voucher + O(log N) 个 sibling 节点，128 笔 Voucher 仅需 280 bytes，远低于 1232 字节交易上限。
* **当前仅支持 SOL**：SPL Token 扩展需将 `deposit` 改用 Token Program CPI，`settle_batch` / `release_settlement` / `withdraw` 的资金转移改为 Token Program `transfer` 指令。核心逻辑（双重签名、Merkle 树、挑战窗口）不变。
* **Ed25519 签名验证使用遍历式指令自省**：合约遍历交易中所有前置指令，按 `public_key` 自动匹配 buyer / merchant 的 Ed25519 签名，而非依赖硬编码索引。支持 `num_sigs >= 1`，允许客户端将多个签名打包在同一条 Ed25519 预编译指令中（节省交易空间），或使用独立的指令（兼容性更好）。
* **Vault 不由程序 init**：Vault PDA 在 `initialize_channel` 时仅通过 `seeds` 校验地址正确性，**不使用 `init`**。Vault 的 owner 始终为 **System Program**。好处：(1) 买家通过普通 `system_instruction::transfer` 即可存入，无需 `invoke_signed`；(2) 程序通过 `invoke_signed` + PDA seeds 即可代表 Vault 签名转出资金（System Program 允许 PDA 签名转出其拥有的账户）。首次 `deposit` 前 Vault 不存在于链上，首次 `deposit` 时由 System Program 自动创建。
* **Escrow 账户用后即关**：`release_settlement`、`force_release`、`resolve_dispute` 完成资金转移后，立即关闭 Escrow 账户并回收 Rent lamports，避免高频结算场景下积累大量废弃账户。
* **可配置时间窗口**：`challenge_period`（挑战窗口，建议 6h）和 `dispute_period`（争议解决期，建议 48h）在 `initialize_channel` 时由双方约定，适配不同业务场景（高频小额用短窗口，中频大额用长窗口）。
* **争议有完整闭环**：买家 `dispute` 冻结资金后有三条出口：(1) `resolve_dispute` 提交 Sum Merkle Proof 退款给买家（同时回滚 `settled_amount`，释放 spending_cap 额度）；(2) dispute_period 后商家 `force_release` 强制提取；(3) 双方协商后释放。`resolve_dispute` 不影响 `channel.balance`（资金从 Escrow 直接退给买家，不在 Vault 中）。
* **双重签名 + 挑战期并存的设计理由**：双重签名确保双方链下已认可该批次（防商家单方面造假）。挑战期作为安全网覆盖以下场景：(1) 买家 Agent 签名密钥泄露，攻击者伪造签名；(2) 买家 SDK 存在 bug 导致未正确校验就签名；(3) 买家私钥丢失后需要时间冻结资金。若业务场景追求极致速度（如 Agent 间自动结算），可将 `challenge_period` 设为 0，此时双重签名即为唯一防线。

---

### 6. 多商户共享金库优化

#### 6.1 架构动机与边界修正

在原始的"一对一通道"设计中，如果用户向多个不同商家交易且频次较低，为每个商家单独建立通道会产生过多的账户创建成本（SOL 租金）并降低资金流动性。因此引入**全局共享金库（Global Buyer Vault）**机制。

针对多商户共享金库带来的"透支风险"，系统通过在智能合约层实施**全额锁定与额度动态分配**机制，确保商家的结算安全，并完全不增加用户的额外账户开销。

#### 6.2 全局状态与全局共享金库

为了避免买家向多个商家透支，我们在链上引入一个全局状态账户来追踪所有已分配的限额及总存入资金。

* **全局状态 PDA Seeds**：`["global_state", buyer_pubkey]`
* **全局金库 PDA Seeds**：`["global_buyer_vault", buyer_pubkey]`

##### 状态数据结构

```rust
#[account]
pub struct GlobalState {
    pub buyer: Pubkey,            // 32 字节，所属买家
    pub total_deposited: u64,     // 8 字节，全局金库总存入资金
    pub total_allocated: u64,     // 8 字节，已被各个商家通道分配的 spending_cap 总和
    pub bump: u8,                 // 1 字节
}
```

##### 额度分配校验逻辑 (Spending Cap Allocation Check)

当买家为具体商家建立通道或修改 `spending_cap` 时，在智能合约中强制执行以下校验公式，确保所有商家的额度总和不超过存入总资金：

$$\text{new\_spending\_cap} - \text{old\_spending\_cap} + \text{total\_allocated} \le \text{total\_deposited}$$

##### 智能合约处理逻辑示例

```rust
pub fn update_spending_cap(ctx: Context<UpdateSpendingCap>, new_spending_cap: u64) -> Result<()> {
    let global_state = &mut ctx.accounts.global_state;
    let channel = &mut ctx.accounts.channel;

    // 计算本次调整的增量
    let delta = if new_spending_cap > channel.spending_cap {
        new_spending_cap - channel.spending_cap
    } else {
        0 // 缩减额度则不增加占用
    };

    // 校验：分配总和不超过存入资金
    require!(
        global_state.total_allocated.checked_add(delta).unwrap() <= global_state.total_deposited,
        ErrorCode::AllocationExceedsDeposit
    );

    // 更新状态
    global_state.total_allocated = global_state.total_allocated.checked_add(delta).unwrap();
    channel.spending_cap = new_spending_cap;

    Ok(())
}
```

#### 6.3 底层合约与数据结构变更

在基础的 `Channel` 结构体中进行适配，以适应共享资金池的校验模式：

* 移除 `balance` 字段（余额由全局金库 lamports 跟踪）
* 移除 `vault_bump` 字段（不再使用通道级 Vault）

```rust
#[account]
pub struct Channel {
    pub buyer: Pubkey,            // 32
    pub merchant: Pubkey,         // 32
    pub spending_cap: u64,        // 8 — 单个商家的消费上限
    pub settled_amount: u64,      // 8 — 已结算总额
    pub nonce: u64,               // 8
    pub challenge_period: i64,    // 8
    pub dispute_period: i64,      // 8
    pub bump: u8,                 // 1
}
// SPACE: 8 + 32 + 32 + 8 + 8 + 8 + 8 + 8 + 1 = 113
```

##### 智能合约结算校验逻辑

在结算批次处理阶段加入双重校验逻辑，防止超额支出导致坏账：

```rust
// 1. 全局限额校验：确保不超过单商户 spending_cap
let cumulative = channel
    .settled_amount
    .checked_add(total_amount)
    .ok_or(ErrorCode::ArithmeticOverflow)?;
require!(
    cumulative <= channel.spending_cap,
    ErrorCode::SpendingCapExceeded
);

// 2. 余额校验：确保从全局金库扣减时合法
require!(
    total_amount <= global_buyer_vault.lamports(),
    ErrorCode::InsufficientBalance
);
```

#### 6.4 运作流程闭环

1. **充值阶段 (`deposit`)**：买家向全局金库充值，同时更新 `global_state.total_deposited`。
2. **分配阶段 (`update_spending_cap`)**：买家为指定商家分配 `spending_cap`，合约自动校验全局额度是否超支。
3. **结算阶段 (`settle_batch`)**：在 L1 执行结算时，校验结算总额是否超过单个商家的 `spending_cap`，并同时核对全局金库的实际可用余额。
4. **提取阶段 (`withdraw`)**：买家可提取未分配的资金（`total_deposited - total_allocated`）。

#### 6.5 架构优势对比

| 维度 | 独立通道模式 | 全局资金池 + 全局限额控制 (优化后) |
| :--- | :--- | :--- |
| **部署成本** | **高**（每次与新商家交易需创建新账户） | **低**（只需在第一次初始化全局 Vault） |
| **资金利用率** | **较低**（资金分散在各个通道中，容易闲置） | **极高**（资金全部集中在统一金库中，按需分配给商家） |
| **防透支机制** | 天然隔离 | 采用全局额度总和限制 $\sum \text{spending\_cap}_i \le \text{Vault}$ |

#### 6.6 新增与修改的指令

| 指令 | 变更 |
|------|------|
| `initialize_global` | **新增** — 创建 GlobalState + 全局金库（首次设置） |
| `initialize_channel` | 修改 — 需要 GlobalState 账户，校验分配额度 |
| `update_spending_cap` | **新增** — 调整单商户消费上限，全局限额检查 |
| `deposit` | 修改 — 存入全局金库，更新 `total_deposited` |
| `settle_batch` | 修改 — 从全局金库转账（非通道级 Vault） |
| `withdraw` | 修改 — 从全局金库提取未分配资金 |
| `release_settlement` | 不变 |
| `dispute` | 不变 |
| `force_release` | 不变 |
| `resolve_dispute` | 不变 |

---

### 7. 乐观结算（Optimistic Settlement）— 商家单边提交机制

#### 7.1 设计动机

在标准结算流程中，`settle_batch` 需要买家和商家双重签名（dual-sig）。然而在某些场景下，买家可能不配合签名（如离线、Agent 故障、恶意拖延），导致商家无法结算已消费的资金。

为解决这一问题，引入 **Optimistic Settlement** 机制——类似 Optimistic Rollup 的单边提交模式：

* **配合场景**：买家配合签名 → 使用 `settle_batch`（双重签名，即时进入 Escrow）
* **不配合场景**：买家不配合 → 商家使用 `optimistic_settle`（仅商家签名，资金进入锁定 Escrow）

#### 7.2 机制流程

```
商家收集买家签名的 Vouchers
        │
        ├── 买家配合签名？ ── 是 ──→ settle_batch (双重签名) ──→ Escrow ──→ release_settlement
        │
        └── 买家不配合 ──→ optimistic_settle (仅商家签名)
                                    │
                                    ├── 挑战期内无人争议 ──→ release_settlement (释放给商家)
                                    │
                                    └── 买家发现欺诈 ──→ dispute ──→ resolve_dispute (退款给买家)
                                                                   或 force_release (超时释放给商家)
```

#### 7.3 链上实现

**新指令：`optimistic_settle`**

与 `settle_batch` 共享相同的账户结构和资金流转逻辑（全局金库 → Escrow），但有以下关键差异：

1. **仅需商家 Ed25519 签名**：不需要买家的批量签名
2. **强制要求 `challenge_period > 0`**：乐观结算的资金必须经过锁定期，不允许即时释放
3. **Escrow 标记 `optimistic = true`**：区分双重签名结算和乐观结算

```rust
pub fn optimistic_settle(
    ctx: Context<OptimisticBatchSettle>,
    merkle_root: [u8; 32],
    total_amount: u64,
    merchant_batch_sig: [u8; 64],
) -> Result<()> {
    // Defense 1: spending cap 校验
    // Defense 2: 全局金库余额校验
    // Defense 3: challenge_period > 0 (乐观结算必须锁定)
    // Defense 4: 仅验证商家 Ed25519 签名
    // ...
    settlement.optimistic = true;  // 标记为乐观结算
}
```

**SettlementEscrow 扩展**

```rust
pub struct SettlementEscrow {
    // ... 原有字段 ...
    pub optimistic: bool,         // 1 — true 表示商家单边提交
}
// SPACE: 8 + 32 + 32 + 8 + 32 + 8 + 8 + 1 + 1 + 1 + 1 = 132
```

#### 7.4 安全保障

| 保障层 | 机制 | 说明 |
|--------|------|------|
| **防超额** | `spending_cap` 校验 | `settled_amount + total_amount <= spending_cap`，与双重签名结算一致 |
| **防透支** | 全局金库余额校验 | `total_amount <= vault.lamports` |
| **防即时转移** | `challenge_period > 0` 强制 | 乐观结算资金必须锁定，不允许 `challenge_period = 0` |
| **欺诈证明** | Sum Merkle Proof | 买家可通过单笔 Voucher 证明商家虚报金额，触发 `resolve_dispute` 退款 |
| **签名可追溯** | 商家 Ed25519 签名 | 商家对 `(merkle_root || total_amount || channel || nonce)` 签名，链上可验证商家确实提交了该批次 |

#### 7.5 与双重签名结算的对比

| 维度 | `settle_batch` (双重签名) | `optimistic_settle` (单边提交) |
|------|--------------------------|-------------------------------|
| **签名要求** | 买家 + 商家 | 仅商家 |
| **challenge_period** | 可为 0（双方已认可） | 必须 > 0 |
| **资金释放** | 挑战期后可立即释放 | 挑战期后释放 |
| **适用场景** | 正常结算（买家 Agent 在线配合） | 买家不配合、离线、Agent 故障 |
| **买家保障** | 双重签名（主动认可） | 挑战期 + 欺诈证明（事后审查） |

#### 7.6 新增指令汇总

| 指令 | 说明 |
|------|------|
| `optimistic_settle` | **新增** — 商家单边提交结算，仅需商家签名，资金进入锁定 Escrow |
