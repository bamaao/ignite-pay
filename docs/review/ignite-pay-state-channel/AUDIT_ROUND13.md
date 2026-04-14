# Ignite Pay State Channel — Round 13 审计报告

> 审查依据: `docs/utxo_merkletree_state_channel.md` (设计指导书)
> 审查范围: `ignite-pay-state-channel/` 项目 + `ignite-pay-program/` 链上程序
> 审查日期: 2026-04-11 (第十三轮，Round 13)
> 前置状态: Round 12 全部 20 个问题已修复，161 个测试全部通过

---

## 审计总览

第十三轮审计共发现 **50** 个问题：

| 类别 | P0 | P1 | P2 | P3 | 合计 |
|------|----|----|----|----|------|
| BUG  | 3  | 6  | 12 | 2  | 23   |
| DEV  | -  | 3  | 9  | 4  | 16   |
| PROG | -  | 1  | 3  | 1  | 5    |
| TEST | -  | -  | -  | 6  | 6    |
| **合计** | **3** | **10** | **24** | **13** | **50** |

---

## 一、链下代码审计 (`ignite-pay-state-channel/`)

### BUG-22: `claim_leaf` 不拒绝 HTLC 类型叶子，允许绕过 hash-lock 验证 (P1 — 资金安全)

**文件**: `src/channel.rs:959-1061`

`claim_leaf` 方法没有验证叶子类型是否为 `Standard`。按设计文档 §5.2，Claim 用于 Standard 叶子，HTLC 叶子必须使用 VerifyHTLC 或 HTLCRefund。

测试 `test_claim_leaf_and_htlc_verify_exclusive`（channel_tests.rs:1246）的注释也承认这一点："even though it's HTLC, claim_leaf doesn't check leaf_type"。

**影响**: HTLC 叶子的 owner 可以通过 `claim_leaf` 绕过 hash-lock/preimage 验证，直接领取 HTLC 资金。

**修复**: 在 `claim_leaf` 中添加 `if leaf.leaf_type != LeafType::Standard { return Err(...) }`。

---

### BUG-23: `trigger_challenge` 签名使用 `current_slot` 而非 `submitted_sequence` (P1 — 安全)

**文件**: `src/channel.rs:820-823`

```rust
let message = crate::signing::state_message(
    &state.metadata.channel_id,
    current_slot,        // BUG: 应为 submitted_sequence
    submitted_root,
);
```

设计文档 §4.3 规定状态签名格式为 `SHA-256(channel_id || sequence || root)`。签名应绑定到提交的 sequence number，而非当前 slot。

**影响**:
1. 签名不绑定到提交的状态版本，无法验证挑战者意图的 sequence
2. 同一 root/sequence 在不同 slot 下需要不同签名，语义错误

**修复**: 将 `current_slot` 改为 `submitted_sequence`。

---

### BUG-24: `partial_transfer` / `split_from_rest` 操作顺序与设计文档相反 (P2)

**文件**: `src/pipeline.rs:130-155`, `src/helpers.rs:63-93`

代码先创建目标叶子（增加总额），再扣除源叶子（恢复总额）。设计文档 §3.2.2 明确规定："必须先从 Rest 扣除，然后创建目标。此顺序保证在任何中间状态下金额守恒不变式不被打破。如果顺序相反，会在中间状态产生凭空创造的资金。"

在 `seq1`（目标创建）时，总额暂时超过 `total_deposited` 达 `amount` 之多。如果在 `seq1` 和 `seq2` 之间触发链上挑战，Merkle root 将显示 `total > deposited`，这是可以被争议的守恒违反。

代码注释承认此偏差并辩称 "宽松不变式 `sum >= total_deposited`"，但这与设计文档严格不变式 `sum == total_deposited` 不一致。

---

### BUG-25: `merge_spent_leaves` 使用 `+` 而非 `checked_add` 导致溢出 (P2)

**文件**: `src/helpers.rs:132`, `src/helpers.rs:157`

`merge_spent_leaves` 中两处使用 `+` 而非 `saturating_add` 或 `checked_add`：

```rust
// Line 132: 累加 source leaves 的 amount
total_amount += leaf.amount;  // 可能溢出 u64

// Line 157: target.amount + total_amount
let new_target = UTXOLeaf::standard(target_owner, target_leaf.amount + total_amount);  // 可能溢出
```

如果多个叶子金额之和超过 `u64::MAX`，会导致算术溢出 panic 或错误的金额。

---

### BUG-26: `helpers.rs:81` `split_from_rest` 使用 `-` 而非 `checked_sub` (P2)

**文件**: `src/helpers.rs:81`

```rust
let updated_rest = UTXOLeaf::standard(rest_leaf.owner, rest_leaf.amount - amount);
```

虽然前面已验证 `rest_leaf.amount >= amount`，所以此处不会溢出。但为代码一致性（与 `channel.rs` 中使用的 `saturating_sub` 保持一致），建议改用 `checked_sub` 或 `saturating_sub`。

---

### BUG-27: `routing.rs:187` fee 计算整数除法截断 (P2)

**文件**: `src/routing.rs:187`

```rust
let fee = req.amount * metrics.fee_rate_bps as u64 / 10000;
```

当 `req.amount * fee_rate_bps` 溢出 `u64` 时（例如 `amount = u64::MAX`, `fee_rate_bps = 65535`），乘法结果会 wrap around。应使用 `checked_mul` / `saturating_mul`。

---

### BUG-28: `routing.rs:194` 流动性检查未包含手续费 (P2)

**文件**: `src/routing.rs:194`

```rust
if metrics.available_liquidity < req.amount {
    sufficient_liquidity = false;
}
```

实际路由时，每跳需要 `amount + fee` 的流动性（因为上游需要转发金额+手续费）。设计文档 §10.3.2 规定检查应为 `min_liquidity < amount + total_fee`。检查应为 `metrics.available_liquidity < req.amount + total_fee_accumulated`。

---

### BUG-29: `compliance.rs:170` `record_payment` 传入 `slot=0` 时窗口计算错误 (P2)

**文件**: `src/channel.rs:409`, `src/compliance.rs:170`

`apply_leaf_update` 调用 `record_payment` 时传入 `slot=0`：
```rust
cm.record_payment(
    state.metadata.channel_id,
    update.new_leaf.amount,
    0, // slot not available
    ...
)?;
```

当 `slot=0` 且 `window_slots=1000` 时，`slot < window_slots`，所以跳过修剪。这导致 `window_payments` 无限增长，最终 `window_spend` 会持续超过 threshold 触发错误的 compliance hold。

---

### BUG-30: `compliance.rs:188` 阈值检查使用 `window_spend` 而非 `cumulative_spent` (P2)

**文件**: `src/compliance.rs:188`

```rust
let action = if window_spend >= state.limits.threshold {
```

设计文档 §10.7 中描述的阈值应为"累计消费"概念，而非窗口内消费。需要明确：`threshold` 是"滑动窗口内阈值"还是"累计阈值"。当前实现假设为"窗口内阈值"，但字段名 `cumulative_spent` 具有误导性。

---

### BUG-31: `htlc.rs:create_htlc` timelock 计算溢出 (P2)

**文件**: `src/htlc.rs:133`

```rust
let timelock_slot = current_slot + duration_slots;
```

如果 `current_slot` 接近 `u64::MAX`，加法可能溢出并 wrap 到过去的时间值。Pipeline 中的 `create_htlc` 使用了 `saturating_add`，但 `HtlcManager::create_htlc` 使用普通 `+`。

---

### BUG-32: `claim_htlc_verify` 在 Challenged 状态下要求 `settle_deadline` 未设置 (P2)

**文件**: `src/channel.rs:1190-1193`

当通道处于 `Challenged` 状态时，`settle_deadline` 可能未设置（只在转换到 `Settling` 时设置）。但代码无条件检查 `settle_deadline`。设计文档 §4.2 规定 VerifyHTLC 在 Challenged 状态下可用，但未指定 `settle_deadline` 检查。测试通过手动设置 `settle_deadline` 绕过此问题。

---

### DEV-12: `helpers.rs:157` merge 目标等于源时覆盖问题 (P2)

**文件**: `src/helpers.rs:157`

如果 `target_idx` 出现在 `source_indices` 中（例如合并 `[0,1]` 到 target=0），则在 Step 1 中 target 先被更新为 `target.amount + total_amount`，但此时 total_amount 中已包含 target 自身的金额（因为 target 也是 source）。Step 2 清除 source 时会清除 target，导致资金丢失。

缺少 `target_idx ∉ source_indices` 的校验。

---

### DEV-13: `routing.rs:285` `select_best_route` 假设已排序 (P3)

**文件**: `src/routing.rs:285`

```rust
pub fn select_best_route(routes: &[Route]) -> Option<&Route> {
    routes.first()
}
```

此方法假设 `routes` 已按 score 降序排列。虽然 `discover_routes` 返回时确实已排序，但如果用户传入未排序的 routes（例如外部构造的 route 列表），则会返回错误结果。建议改用 `max_by_key` 或文档说明排序前提。

---

### DEV-14: `routing.rs:238` `score_route` 中 fee 溢出 (P2)

**文件**: `src/routing.rs:238-240`

```rust
let total_fee: u64 = path_metrics.iter()
    .map(|m| amount * m.fee_rate_bps as u64 / 10000)
    .sum();
```

`.sum()` 使用默认的 `u64` 加法，无溢出保护。应使用 `saturating_add` 或 `checked_add`。同样，`amount * m.fee_rate_bps as u64` 也可能溢出。

---

### DEV-15: `signing.rs` 签名消息格式与设计文档不一致 (P2)

**文件**: `src/signing.rs:28-38`

设计文档 §4.3 规定 `state_message` 返回 72 字节原始拼接 `[u8; 72]`：
```rust
fn state_message(channel_id: &[u8; 32], sequence: u64, root: &[u8; 32]) -> [u8; 72]
```

但实现返回 `SHA-256(channel_id || sequence || root)` 的 32 字节哈希。如果链上程序使用原始 72 字节格式，链下签名将无法在链上验证。需要确保链下和链上的签名消息格式一致。

---

### DEV-16: `channel.rs:1441` `finalize_settlement` 退款精度损失 (P3)

**文件**: `src/channel.rs:1441-1449`

```rust
let ratio_a = state.metadata.deposit_a as u128 * 1_000_000 / total_deposit as u128;
let r_a = (unclaimed as u128 * ratio_a / 1_000_000) as u64;
let r_b = unclaimed.saturating_sub(r_a);
```

使用 1M 精度可能导致 1 lamport 级别的精度损失。当 `deposit_a` / `total_deposit` 不能被 1M 整除时，`r_a` 会向下取整，`r_b` 会吸收差额。这不影响资金安全（总额守恒），但分配比例会有微小偏差。建议使用 `u128` 全精度除法。

---

### DEV-16: `pipeline.rs:145` partial_transfer 使用 `-` 而非 `checked_sub` (P3)

**文件**: `src/pipeline.rs:145`

```rust
let updated_src = UTXOLeaf::standard(src_leaf.owner, src_leaf.amount - amount);
```

虽然前面已验证 `src_leaf.amount >= amount`，不会溢出。但与 `channel.rs` 的风格不一致。

---

### DEV-17: `compliance.rs` `create_compliance_leaf` 使用 `hash_lock` 字段存 compliance_hash (P3)

**文件**: `src/compliance.rs:287-296`

```rust
pub fn create_compliance_leaf(compliance_hash: [u8; 32]) -> UTXOLeaf {
    UTXOLeaf {
        leaf_type: LeafType::Compliance,
        owner: Pubkey::default(),
        amount: 0,
        hash_lock: Some(compliance_hash),  // 复用 hash_lock 字段
        ...
    }
}
```

`Compliance` 类型叶子复用 `hash_lock` 字段存储 `compliance_hash`，虽然功能上可以工作，但在语义上不太清晰。如果未来添加 HTLC 相关逻辑遍历 `hash_lock`，可能误判 Compliance 叶子。

---

### TEST-13: 缺少 `merge_spent_leaves` 溢出测试

**文件**: `src/helpers.rs` tests

缺少当 `total_amount` 累加可能溢出时的边界测试（多个大金额叶子合并）。

---

### TEST-14: 缺少 `merge_spent_leaves` target==source 冲突测试

**文件**: `src/helpers.rs` tests

缺少当 `target_idx` 在 `source_indices` 中时的错误处理测试（验证 DEV-12 场景）。

---

### TEST-15: 缺少 routing fee 溢出测试

**文件**: `src/routing.rs` tests

缺少大金额 + 高 fee_rate_bps 下的整数溢出测试。

---

### TEST-16: 缺少 `claim_leaf` 拒绝 HTLC 叶子的负面测试

**文件**: `tests/channel_tests.rs`

测试 `test_claim_leaf_and_htlc_verify_exclusive` 实际证明了 `claim_leaf` 在 HTLC 叶子上**成功**（BUG-22），但缺少验证 `claim_leaf` 应拒绝 HTLC 叶子的负面测试。

---

### TEST-17: 缺少 `trigger_challenge` submitted_sequence == current_sequence 边界测试

**文件**: `src/channel.rs` tests

缺少 `submitted_sequence == current_sequence`（相等值）的边界测试。代码检查 `submitted_sequence <= current_sequence` 正确拒绝了相等值，但没有显式测试此边界。

---

### TEST-18: 缺少 compliance slot=0 窗口行为测试

**文件**: `src/compliance.rs` tests

所有合规测试使用正 slot 值。由于通道管理器向合规模块传递 `slot=0`，应有测试验证 slot=0 时窗口修剪的行为。

---

## 二、链上代码审计 (`ignite-pay-program/`)

### BUG-33: `submit_counter_state` 未验证任何签名 (P0 — 资金安全)

**文件**: `src/instructions/submit_counter_state.rs:22-23`

```rust
pub fn submit_counter_state(
    ...
    _sig_a: [u8; 64],  // 前缀 _ 表示未使用
    _sig_b: [u8; 64],  // 前缀 _ 表示未使用
) -> Result<()> {
```

`sig_a` 和 `sig_b` 被下划线前缀标记为未使用，**签名从未被验证**。任何人都可以提交任意的 counter state，只需提供一个更高的 sequence number 就能覆盖通道状态。这是 P0 级别的资金安全漏洞。

**修复**: 使用 `verify_ed25519_signature` 验证 `sig_a` 对 `user_pubkey`、`sig_b` 对 `provider_pubkey`。消息格式: `channel_id || sequence || root`。

---

### BUG-34: `open_channel` 未验证 `sig_a` (P0 — 资金安全)

**文件**: `src/instructions/open_channel.rs`

`open_channel` 指令接受 `initial_root` 和 `channel_id` 作为参数，但未验证用户签名 (`sig_a`)。恶意用户可以为任意 pubkey 开设通道。设计文档 §4.1 要求 OpenChannel 需要至少用户签名。

**修复**: 添加 `sig_a: [u8; 64]` 参数，验证消息 `channel_id || deposit_a || tree_depth || initial_root` 对 `user_pubkey` 的签名。

---

### BUG-35: `claim.rs` / `verify_htlc.rs` / `htlc_refund.rs` 未执行 SPL Token 转账 (P1 — 资金安全)

**文件**: `src/instructions/claim.rs`, `src/instructions/verify_htlc.rs`, `src/instructions/htlc_refund.rs`

这三个指令正确验证了 Merkle proof、签名等，但**没有执行实际的 SPL Token 转账**。它们只更新了 `total_claimed` 记录和 `claimed_leaves` 列表。用户"领取"了资金，但 token 仍然在 escrow 中。

与 `finalize_settlement.rs`（已实现 CPI 转账）不同，claim 类指令缺少：
- `vault_a` / `vault_b` Token accounts
- `escrow_vault` Token account
- `token_program` Program
- `token::transfer` CPI 调用

**修复**: 为 claim/verify_htlc/htlc_refund 添加 SPL Token CPI 转账逻辑。

---

### BUG-36: `open_channel` 未执行 SPL Token 存款 (P1 — 资金安全)

**文件**: `src/instructions/open_channel.rs`

`open_channel` 记录了 `deposit_a` 但**没有从用户账户转入 token 到 escrow**。通道状态显示有存款，但实际 token 未锁定。

**修复**: 添加 SPL Token CPI 从 `user_token_account` 转入 `escrow_vault`。

---

### BUG-37: `cooperative_settle.rs:37` sequence 检查使用 `==` 而非 `>=` (P1)

**文件**: `src/instructions/cooperative_settle.rs:37`

```rust
require!(
    sequence == channel.sequence,
    ChannelError::InvalidSequence
);
```

设计文档 §4.2 规定 CooperativeSettle 接受 `sequence >= on_chain.sequence`。当前实现仅接受 `==`，如果链上 sequence 因某种原因低于最新 off-chain sequence，合法的 close 请求会被拒绝。

对于 cooperative settle，`==` 实际上是合理的（双方协商的总是最新状态），但需确认是否与设计文档一致。如果允许 `>=`，则需要额外检查 `root` 是否匹配。

---

### BUG-38: `finalize_settlement.rs` escrow_vault 缺少 PDA signing seeds (P1 — CPI 权限)

**文件**: `src/instructions/finalize_settlement.rs:96-114`

```rust
let cpi_accounts_a = Transfer {
    from: ctx.accounts.escrow_vault.to_account_info(),
    to: ctx.accounts.vault_a.to_account_info(),
    authority: ctx.accounts.escrow_vault.to_account_info(),  // 需要 PDA signer
};
token::transfer(
    CpiContext::new(
        ctx.accounts.token_program.to_account_info(),
        cpi_accounts_a,
    ),
    refund_a,
)?;
```

`escrow_vault` 作为 `Transfer` 的 `from` 和 `authority`，但 `CpiContext::new` 没有提供 PDA signing seeds。Solana Token 程序要求 authority 必须签名才能转账。应使用 `CpiContext::new_with_signer` 并提供 PDA seeds。

---

### BUG-39: `verify_htlc.rs` 缺少 HTLC 类型验证 (P2)

**文件**: `src/instructions/verify_htlc.rs`

指令接受 `leaf_hash`, `hash_lock`, `preimage`, `timelock_slot` 等参数，但**未验证叶子实际是 HTLC 类型**。攻击者可以对一个 Standard 类型的叶子执行 VerifyHTLC 操作（只要构造出匹配的 hash_lock/preimage）。

链下 `claim_htlc_verify` 在 `channel.rs:1233` 有此检查：
```rust
if leaf.leaf_type != LeafType::HTLC { return Err(...) }
```

链上缺少对应验证。

---

### BUG-40: `verify_htlc.rs` / `htlc_refund.rs` 未验证叶子参数与实际叶子数据一致 (P2)

**文件**: `src/instructions/verify_htlc.rs`, `src/instructions/htlc_refund.rs`

调用者传入 `leaf_amount`, `hash_lock`, `timelock_slot`, `beneficiary` 等参数，但链上只验证了 Merkle proof（`leaf_hash` 在树中）和签名。**没有验证这些参数与实际存储的叶子数据一致**。

例如，攻击者可以：
1. 提供正确的 Merkle proof（证明叶子在树中）
2. 但提供篡改的 `leaf_amount`（更高）来 claim 更多资金

链上缺少对 borsh 反序列化后的 `leaf_data` 的验证（claim.rs 有部分实现，但 verify_htlc/htlc_refund 没有）。

---

### BUG-41: `settle_after_timeout.rs` 未检查 `settle_deadline` 在 Challenged 状态下是否为 None (P2)

**文件**: `src/instructions/settle_after_timeout.rs`

当通道处于 `Challenged` 状态时，`settle_deadline` 可能为 `None`（因为 `trigger_challenge` 不设置 `settle_deadline`）。但 `verify_htlc.rs:49` 和 `htlc_refund.rs` 都要求 `settle_deadline` 不为 None。这意味着在 Challenged 状态下的 HTLC 操作可能因为 `settle_deadline = None` 而失败。

设计文档 §4.2 规定 VerifyHTLC/HTLCRefund 在 Challenged 状态下也可用，但前提是 `settle_deadline` 已设置。

---

### DEV-18: `open_channel.rs` tree_depth 验证在 account 初始化之后 (P2)

**文件**: `src/instructions/open_channel.rs`

`tree_depth <= 8` 的验证在 `ChannelAccount::space(tree_depth)` 计算之后执行。如果 `tree_depth` 超大，`space()` 中的 `2usize.pow(tree_depth)` 会导致 panic（在 debug 模式下）或产生错误的账户大小（release 模式下）。

应在 `space()` 调用之前验证 `tree_depth <= 8`。

---

### DEV-19: `fund_channel.rs` 未检查 SPL Token 存款 (P2)

**文件**: `src/instructions/fund_channel.rs`

链下 `fund_channel` 正确地创建了 provider 叶子并更新了 `deposit_b`，但链上 `fund_channel` 只更新了 `ChannelAccount` 的 `deposit_b` 和 `leaf_count`，**没有执行 SPL Token 从 provider 到 escrow 的转账**。

---

### DEV-20: `verify_htlc.rs:52` settle_deadline 检查使用 `<=` 与设计文档不一致 (P2)

**文件**: `src/instructions/verify_htlc.rs:52`

```rust
require!(current_slot <= deadline, ...);
```

设计文档 §5 规定 claim 操作在 `settle_deadline` 之前可用，使用 `<=` 意味着在 `deadline` 时刻仍可操作。链下 `channel.rs:978` 使用 `>`（`current_slot > deadline` 才拒绝），两者语义一致。但需确认是否与 `settle_after_timeout.rs` 中的 `>=` (deadline) 逻辑配合正确。

---

### PROG-12: 链上 `ed25519_dalek` 应改为 Solana ed25519 syscall (P1)

**文件**: `src/utils/ed25519.rs`

当前使用 `ed25519_dalek::VerifyingKey::verify_strict()` 进行签名验证。在 Solana 链上，推荐使用 `ed25519_program` syscall（通过指令内省 `InstructionError::InsufficientInstructions` 或 `solana_program::ed25519_instruction`）来验证 Ed25519 签名。

原因：
1. Solana runtime 对 ed25519 syscall 有专门的优化（并行验证）
2. ed25519_dalek 在 BPF/CBV 中的性能可能较差
3. Anchor 框架推荐使用 `ed25519_program` 进行签名验证

**注意**: 这是架构优化建议，`ed25519_dalek` 在功能上是正确的。如果为简化开发保留当前实现，应在文档中标注。

---

### PROG-13: `claim.rs` 未验证 `leaf_data` 中的 `amount` 字段 (P2)

**文件**: `src/instructions/claim.rs`

BUG-20 修复添加了 `leaf_data` 参数和 `InvalidLeafData` 错误，但实际的链上验证逻辑需要反序列化 `leaf_data` 并验证 `amount` 字段与 `claim_amount` 参数一致。如果当前实现未完整反序列化验证，则攻击者可以篡改 leaf_data 中的金额。

---

### PROG-14: `open_channel` 缺少 `payer == user_pubkey` 约束 (P2)

**文件**: `src/instructions/open_channel.rs`

OpenChannel 账户结构中，`payer`/`user` 账户缺少 `constraint = user.key() == channel.user_pubkey` 约束。任何人都可以为任意用户创建通道。

---

### PROG-15: `ChannelAccount` 缺少 `auto_close_slot` 的链上处理 (P3)

**文件**: `src/instructions/settle_after_timeout.rs`

DEV-10 在 `state.rs` 中添加了 `auto_close_slot: Option<u64>` 字段，但 `settle_after_timeout` 未检查此字段。设计文档 §3.4.3 规定：当 `current_slot >= auto_close_slot` 时，任何人可以触发 auto-settle（无需 challenge period）。

---

### TEST-16: 缺少链上 `submit_counter_state` 签名验证失败测试

**文件**: `ignite-pay-program/`

当前链上程序没有集成测试框架。`submit_counter_state` 的签名验证（修复 BUG-28 后）需要有对应的测试验证拒绝未签名/错误签名的 counter state。

---

## 三、问题汇总表

### P0 级别（资金安全 — 必须修复）

| ID | 描述 | 文件 |
|----|------|------|
| BUG-33 | `submit_counter_state` 未验证签名 | submit_counter_state.rs:22 |
| BUG-34 | `open_channel` 未验证用户签名 | open_channel.rs |
| BUG-35 | claim/verify_htlc/htlc_refund 无 SPL Token 转账 | claim.rs, verify_htlc.rs, htlc_refund.rs |

### P1 级别（重要安全问题）

| ID | 描述 | 文件 |
|----|------|------|
| BUG-22 | `claim_leaf` 不拒绝 HTLC 叶子 | channel.rs:959 |
| BUG-23 | `trigger_challenge` 签名用 current_slot | channel.rs:820 |
| BUG-25 | `merge_spent_leaves` 溢出 | helpers.rs:132,157 |
| BUG-36 | `open_channel` 无 SPL Token 存款 | open_channel.rs |
| BUG-37 | `cooperative_settle` sequence `==` vs `>=` | cooperative_settle.rs:37 |
| BUG-38 | `finalize_settlement` escrow 缺少 PDA seeds | finalize_settlement.rs:96 |
| PROG-12 | 链上应使用 ed25519 syscall | utils/ed25519.rs |

### P2 级别（功能正确性）

| ID | 描述 | 文件 |
|----|------|------|
| BUG-24 | partial_transfer 操作顺序与设计文档相反 | pipeline.rs:130, helpers.rs:63 |
| BUG-26 | split_from_rest `-` vs checked_sub | helpers.rs:81 |
| BUG-27 | routing fee 计算溢出 | routing.rs:187 |
| BUG-28 | 流动性检查未包含手续费 | routing.rs:194 |
| BUG-29 | compliance slot=0 窗口计算错误 | channel.rs:409 |
| BUG-30 | compliance 阈值语义不一致 | compliance.rs:188 |
| BUG-31 | htlc timelock 计算溢出 | htlc.rs:133 |
| BUG-32 | Challenged 下 settle_deadline 未设置 | channel.rs:1190 |
| BUG-39 | verify_htlc 缺少 HTLC 类型验证 | verify_htlc.rs |
| BUG-40 | verify_htlc/htlc_refund 参数未验证 | verify_htlc.rs, htlc_refund.rs |
| BUG-41 | Challenged 状态 settle_deadline 可能为 None | settle_after_timeout.rs |
| DEV-12 | merge target==source 覆盖 | helpers.rs:157 |
| DEV-14 | score_route fee 溢出 | routing.rs:238 |
| DEV-15 | 签名消息格式与设计文档不一致 | signing.rs:28 |
| DEV-18 | tree_depth 验证顺序 | open_channel.rs |
| DEV-19 | fund_channel 无 SPL Token 存款 | fund_channel.rs |
| DEV-20 | settle_deadline <= vs < | verify_htlc.rs:52 |
| PROG-13 | claim leaf_data amount 未验证 | claim.rs |
| PROG-14 | open_channel 缺少 user 约束 | open_channel.rs |

### P3 级别（代码质量/建议）

| ID | 描述 | 文件 |
|----|------|------|
| DEV-13 | select_best_route 假设排序 | routing.rs:285 |
| DEV-16 | 退款精度损失 | channel.rs:1441 |
| DEV-17 | partial_transfer `-` vs checked_sub | pipeline.rs:145 |
| DEV-21 | compliance leaf 复用 hash_lock | compliance.rs:287 |
| PROG-15 | auto_close_slot 链上未处理 | settle_after_timeout.rs |

### TEST 缺失项

| ID | 描述 | 优先级 |
|----|------|--------|
| TEST-13 | merge_spent_leaves 溢出边界测试 | P2 |
| TEST-14 | merge target==source 冲突测试 | P2 |
| TEST-15 | routing fee 溢出测试 | P2 |
| TEST-16 | claim_leaf 拒绝 HTLC 叶子测试 | P1 |
| TEST-17 | trigger_challenge sequence==current 测试 | P2 |
| TEST-18 | compliance slot=0 窗口行为测试 | P2 |

---

## 四、修复优先级建议

### 第一优先级：P0 安全漏洞（3 个）

1. **BUG-33**: `submit_counter_state` 添加双签名验证
2. **BUG-34**: `open_channel` 添加用户签名验证
3. **BUG-35**: claim/verify_htlc/htlc_refund 添加 SPL Token CPI 转账

### 第二优先级：P1 问题（7 个）

4. **BUG-22**: `claim_leaf` 添加 `leaf_type != Standard` 拒绝检查
5. **BUG-23**: `trigger_challenge` 签名改用 `submitted_sequence`
6. **BUG-25**: `merge_spent_leaves` 使用 `saturating_add`
7. **BUG-36**: `open_channel` 添加 SPL Token 存款 CPI
8. **BUG-38**: `finalize_settlement` 使用 `CpiContext::new_with_signer` + PDA seeds
9. **BUG-37**: 确认 cooperative_settle sequence 语义
10. **PROG-12**: 评估 ed25519_dalek vs syscall

### 第三优先级：P2 功能问题（19 个）

按功能模块分组修复：
- **路由模块**: BUG-27, BUG-28, DEV-14
- **合规模块**: BUG-29, BUG-30
- **链上 claim**: BUG-39, BUG-40, BUG-41, PROG-13, PROG-14
- **链上其他**: DEV-18, DEV-19, DEV-20
- **helpers**: BUG-24, DEV-12
- **签名**: DEV-15
- **HTLC**: BUG-31, BUG-32

---

## 五、与设计文档的对齐检查

| 设计文档章节 | 链下实现 | 链上实现 | 差异说明 |
|-------------|---------|---------|---------|
| §3.1 OpenChannel | ✅ 完整 | ⚠️ 缺签名+存款 | BUG-29, BUG-31 |
| §3.2 SplitTree | ✅ 完整 | N/A | |
| §3.3 LeafUpdate | ✅ 完整 | N/A | |
| §3.4.1 CooperativeClose | ✅ 完整 | ⚠️ sequence `==` | BUG-32 |
| §3.4.2 Challenge/Settle | ✅ 完整 | ⚠️ settle_deadline | BUG-36 |
| §3.4.3 AutoClose | ✅ 完整 | ⚠️ 未处理 | PROG-15 |
| §4.2 Claim | ✅ 完整 | ⚠️ 缺 SPL 转账 | BUG-30 |
| §4.2 VerifyHTLC | ✅ 完整 | ⚠️ 缺类型检查+转账 | BUG-34, BUG-35 |
| §4.2 HTLCRefund | ✅ 完整 | ⚠️ 缺转账 | BUG-30 |
| §4.2 SubmitCounter | ✅ 完整 | ❌ 缺签名验证 | BUG-28 |
| §5.4 Finalize | ✅ 完整 | ⚠️ 缺 PDA seeds | BUG-33 |
| §6 Signing | ✅ 完整 | ✅ 完整 | |
| §10.3 Routing | ⚠️ 溢出 | N/A | BUG-24, BUG-25 |
| §10.7 Compliance | ⚠️ slot 问题 | N/A | BUG-26, BUG-27 |

---

## 六、结论

链下代码（`ignite-pay-state-channel`）整体质量良好，核心支付流程（Open → Split → Transfer → HTLC → Close → Claim → Finalize）实现完整。主要问题集中在：
1. 整数溢出保护不够全面（helpers、routing）
2. 合规模块的 slot 传递问题

链上代码（`ignite-pay-program`）存在多个严重安全漏洞：
1. `submit_counter_state` 完全没有签名验证（任何人可提交伪造状态）
2. claim 类指令缺少 SPL Token 转账（资金实际未被转移）
3. `open_channel`/`fund_channel` 缺少 Token 存款
4. `finalize_settlement` 的 CPI 缺少 PDA signing seeds

**建议**: 优先修复 P0 级别的 3 个问题（BUG-28/29/30），然后按优先级逐步修复 P1/P2 问题。链上程序在部署前必须完成所有 P0 和 P1 修复。
