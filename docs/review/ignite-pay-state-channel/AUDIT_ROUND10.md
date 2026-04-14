# Ignite Pay State Channel 代码审计报告 (第十轮)

> 审计依据: `docs/utxo_merkletree_state_channel.md` (设计指导书 v2.0)
> 审计范围: `ignite-pay-state-channel/` 全部源码 + `ignite-pay-program/` 链上程序
> 审计日期: 2026-04-11 (第十轮)

---

## 审计总览

逐条对照设计文档，检查业务流程实现完整性、规则一致性、测试覆盖和潜在 bug。共发现 **8 个 BUG**、**6 个业务规则偏差**、**7 个测试覆盖缺失**。

---

## 一、BUG 清单

### BUG-10: `fund_channel` 签名被创建但从未验证

**严重度**: 高
**文件**: `src/channel.rs:204`
**设计文档**: §2.B LeafUpdate 签名 = `SHA-256(channel_id || sequence || leaf_index || prev_leaf_hash || new_leaf_hash)`

```rust
let _update = crate::signing::sign_leaf_update(
    &state.metadata.channel_id,
    new_sequence,
    target_index as u32,
    &prev_leaf,
    new_leaf.clone(),
    provider_keypair,
);
// _update 被创建但丢弃，没有验证签名是否正确
```

**问题**: 签名被创建后直接丢弃（`_update`），没有持久化也没有回传给调用者。这导致:
1. 审计链断裂：这个 LeafUpdate 没有被记录到 sled，后续无法回溯
2. 若签名计算出错不会被发现

**建议**: 将 `_update` 改为 `update`，并持久化到 sled 或至少返回给调用者。

---

### BUG-11: `construct_split_tree` 单方注资通道的回归 BUG

**严重度**: 中
**文件**: `src/channel.rs:271-282`
**设计文档**: §3.1.3 "所有叶子 owner = user"

FLOW-3 修改后，当 `deposit_b == 0`（单方注资）时，`provider_total` 必须等于 0。但若 leaves 中不包含任何 provider 叶子，`provider_total` 为 0 是正确的。然而原来的逻辑要求"所有叶子 owner 必须是 user"，新逻辑只是不报错——这本身不是 bug，但 **如果叶子中有空叶子**（amount=0, owner=Pubkey::default()），空叶子的 owner 既不是 user 也不是 provider，会被拒绝。

```rust
for (i, leaf) in leaves.iter().enumerate() {
    if !leaf.is_empty() {
        if leaf.owner == user_pubkey { ... }
        else if leaf.owner == provider_pubkey { ... }
        else { return Err(...) }  // 空叶子的 owner=Pubkey::default() 不匹配
    }
}
```

**分析**: `if !leaf.is_empty()` 检查了 `amount == 0`，所以空叶子会被跳过。**实际不会触发**。但语义上不够清晰——建议增加注释说明空叶子被跳过的原因。

**结论**: 非阻塞问题，代码逻辑正确但可读性可改善。

---

### BUG-12: `MIN_TIMELOCK_BASE` 硬编码 `500` 而非使用 `challenge_duration`

**严重度**: 中
**文件**: `src/channel.rs:17`
**设计文档**: §10.4.2 `MIN_TIMELOCK = CHALLENGE_DURATION + 3 * HOP_MARGIN`

```rust
pub const MIN_TIMELOCK_BASE: u64 = 500 + 3 * HOP_MARGIN;
```

设计文档说 `MIN_TIMELOCK = CHALLENGE_DURATION + 3 * HOP_MARGIN`，其中 `CHALLENGE_DURATION` 是通道参数。但代码将 `500` 硬编码，不随通道的 `challenge_duration` 变化。

**影响**: 若通道的 `challenge_duration` 大于 500，多跳 HTLC 的时间锁可能不够长，导致服务商来不及在链上提交原像。

**建议**: 将 `MIN_TIMELOCK_BASE` 改为函数，接收 `challenge_duration` 参数：
```rust
pub fn min_timelock(challenge_duration: u64) -> u64 {
    challenge_duration + 3 * HOP_MARGIN
}
```

---

### BUG-13: `split_from_rest` 操作顺序与设计文档不一致

**严重度**: 低
**文件**: `src/helpers.rs:56-88`
**设计文档**: §3.2.2 "必须**先从 Rest 扣减**，再在空闲槽位创建新 UTXO"

代码注释标明 "BUG-6 fix: Creates target FIRST"，即**先创建目标叶子再扣减 Rest**。但设计文档 §3.2.2 明确要求先扣 Rest：

> "拆分本质上是两笔原子 LeafUpdate——必须先从 Rest 扣减，再在空闲槽位创建新 UTXO。这个顺序保证任何中间状态下资金守恒不变量不被打破"

代码注释解释说先创建目标（总金额增加）再扣减 Rest（总金额恢复），保证 `sum(leaves) >= total_deposited`。但设计文档的守恒不变量是 `sum(leaves) == total_deposited`（精确相等）。

**分析**: 代码实现的是"中间态总金额 >= 存款总额"的宽松守恒，而非设计文档要求的"每步精确守恒"。两种方案都安全（不会凭空创造资金），但与设计文档表述不一致。

**建议**: 在代码注释中明确说明与设计文档的偏差及其合理性。

---

### BUG-14: `claim_leaf` 缺少 Merkle proof 参数

**严重度**: 中
**文件**: `src/channel.rs:831`
**设计文档**: §5.2 Claim 流程 "提交 (leaf_index, leaf_data, merkle_proof)"

设计文档要求 claim 时由调用者提供 Merkle proof（因为链上合约需要）。但链下实现中，`claim_leaf` 直接从 `state.tree` 内部生成 proof（`state.tree.get_proof(leaf_index)`），不接受外部 proof 参数。

```rust
// BUG: 代码自己生成 proof，而不是验证调用者提交的 proof
let proof = state.tree.get_proof(leaf_index as usize)?;
```

**影响**: 链下 API 与链上 Claim 指令的接口不一致。链上需要 caller 提供 proof，链下自动生成——这在集成时可能导致混淆。

**建议**: 增加 `claim_leaf_with_proof` 变体，接受外部 proof 参数，与链上接口对齐。

---

### BUG-15: `claim_htlc_verify` 和 `claim_htlc_refund` 仅在 Settling 状态可用

**严重度**: 低
**文件**: `src/channel.rs:952, 1040`
**设计文档**: §4.2 VerifyHTLC/HTLCRefund "**Challenged or Settling status**"

设计文档明确说 VerifyHTLC 和 HTLCRefund 在 **Challenged 或 Settling** 状态下都可以使用。但代码只检查 `ChannelStatus::Settling`：

```rust
if state.metadata.status != ChannelStatus::Settling {
    return Err(...);
}
```

**影响**: 在挑战期间，服务商无法通过 VerifyHTLC 提交原像来证明自己已完成服务。

**建议**: 改为 `status == Challenged || status == Settling`。

---

### BUG-16: `close_channel` 检查 `signed_state.sequence == state.metadata.sequence` 而非 `>=`

**严重度**: 低
**文件**: `src/channel.rs:599`
**设计文档**: §4.2 CooperativeSettle "sequence > on_chain.sequence"

设计文档的 CooperativeSettle 要求 `sequence > on_chain.sequence`，但代码检查严格相等：

```rust
if signed_state.sequence != state.metadata.sequence {
    return Err(...);
}
```

**分析**: 链下实现中，`state` 已由双方持有最新 sequence，严格相等检查是合理的（与 `submit_counter_state` 的 `>` 检查互补）。但链上程序 `cooperative_settle.rs` 也检查 `==`。这与设计文档表述有出入。

**建议**: 在代码注释中说明为何使用 `==` 而非 `>`。

---

### BUG-17: `routing.rs` DFS 路由发现可能产生重复路由

**严重度**: 低
**文件**: `src/routing.rs:107-138`

`dfs_routes` 使用 `visited: Vec<[u8; 32]>` 进行已访问检查，用的是线性搜索 `visited.iter().any(|v| v == neighbor)`。对于 `[u8; 32]` 类型的线性搜索效率低且在大规模网络中可能产生性能问题。

此外，`discover_routes` 的路径空间会随 hub 数量指数增长，没有剪枝策略，可能在大规模网络中搜索超时。

---

## 二、业务规则偏差

### DEV-1: `auto_close_slot` 未实现触发逻辑

**设计文档**: §3.4.3 "current_slot >= auto_close_slot 时，任何人可以触发结算"
**文件**: `src/channel.rs`

`ChannelMetadata` 有 `auto_close_slot: Option<u64>` 字段，`open_channel` 初始化为 `None`，但没有任何方法设置或触发自动关闭。设计文档 §3.4.3 描述了 Auto-close 路径，但代码中缺少：
- `set_auto_close_slot()` 方法
- `auto_settle()` 方法（检查 `current_slot >= auto_close_slot` 后直接进入 Settling）

---

### DEV-2: 合规模块未与通道操作集成

**设计文档**: §6 "在链下业务逻辑中添加约束，当累计支付金额触发阈值时自动插入合规标记"
**文件**: `src/compliance.rs`, `src/channel.rs`

`ComplianceManager` 独立存在，但 `apply_leaf_update` 和 `transfer_leaf`（pipeline.rs）中没有调用 `compliance.record_payment()`。合规检查是独立的，未嵌入支付流程。

---

### DEV-3: 路由评分公式与设计文档不一致

**设计文档**: §10.3.2
```rust
let fee_score = 1.0 / (1.0 + total_fee as f64 / amount as f64);
let latency_score = 1.0 / (1.0 + max_latency as f64 / 1000.0);
```

**代码**: `src/routing.rs:244-268`
```rust
let fee_score = (1.0 - fee_ratio).max(0.0);  // fee_ratio = total_fee / amount
let latency_score = (1.0 - (max_latency_ms as f64 / 10000.0)).max(0.0);
```

两种公式计算结果不同。设计文档用 `1/(1+x)` 反比例，代码用线性截断 `1-x`。当手续费率较高时，代码的评分可能变为负数后被截断为 0，而设计文档的公式始终为正。

---

### DEV-4: 设计文档 §4.2 `TriggerChallenge` 要求 `sequence > on_chain.sequence`

**文件**: `src/channel.rs:665-710`

设计文档 §4.2 表格说 TriggerChallenge 需要"提交 (root, sequence, sig)，验证 sequence > on_chain.sequence"。但代码的 `trigger_challenge` 不接受 sequence/root 参数，只验证签名者和签名。代码不检查提交的 sequence 是否大于链上 sequence。

**分析**: 链下版本中 channel state 已由双方维护，sequence 检查在链上合约中才有意义。但若要与链上逻辑对齐，应增加 sequence 参数。

---

### DEV-5: 设计文档 §10.4.1 路由费用通过 HTLC 金额差实现

**文件**: `src/multihop.rs`

设计文档 §10.5 描述路由费用通过每跳 HTLC 金额递减来隐式实现。但 `MultiHopEntry` 中每跳的 `amount` 是独立指定的，`create_payment` 的 `hops_metadata` 允许调用者自由设置每跳金额——没有自动计算费用递减。

**建议**: 增加辅助函数根据 hub 费率自动计算每跳金额。

---

### DEV-6: `settle_after_timeout` 使用 `>=` 而非设计文档的 `>`

**设计文档**: §4.2 SettleAfterTimeout "`current_slot > challenge_slot + challenge_duration`"
**文件**: `src/channel.rs:734`

```rust
if current_slot < challenge_slot + state.metadata.challenge_duration {
    return Err(...);
}
```

代码在 `current_slot == challenge_slot + challenge_duration` 时允许结算（`>=`），而设计文档要求严格大于（`>`）。这个差异也存在于 `htlc::check_expiry` 中的 `current_slot > record.timelock_slot`（严格大于）——但 settle 用 `>=` 不一致。

---

## 三、测试覆盖缺失

### TEST-1: 缺少 `fund_channel` 后 `construct_split_tree` 的集成测试

**文件**: 无

设计文档 §10.6.2 描述双向注资流程：先 `fund_channel`，再 `construct_split_tree`。当前测试分别测试了两个方法，但没有测试完整的**先注资再分裂**的端到端流程。

---

### TEST-2: 缺少合规模块与通道的集成测试

**文件**: 无

没有测试"通道支付触发合规阈值 → 插入 Compliance 叶子 → 通道暂停 → 清除 hold → 恢复"的完整流程。

---

### TEST-3: 缺少多跳支付端到端测试

**文件**: 无

`multihop.rs` 的测试覆盖了单个 `MultiHopManager` 的操作，但没有测试跨通道的 HTLC 链式锁定/解锁流程。

---

### TEST-4: 缺少 Challenged 状态下 VerifyHTLC/HTLCRefund 的测试

**文件**: `src/channel.rs` tests

设计文档 §4.2 和 §3.4.2 Scenario B 描述了在 Challenged 状态下服务商可以提交 VerifyHTLC，但当前测试只在 Settling 状态测试。且由于 BUG-15，Challenged 状态下这些操作会被拒绝。

---

### TEST-5: 缺少 `auto_close_slot` 相关测试

**文件**: 无

设计文档 §3.4.3 描述的自动关闭流程没有任何测试覆盖。

---

### TEST-6: 缺少重复 claim 的边界测试

`claim_leaf` 有 `claimed_leaves` 集合防止重复 claim，但没有测试验证：
- 同一个叶子不能被 claim_htlc_verify 后再 claim_leaf
- claim_leaf 和 claim_htlc_refund 互斥

---

### TEST-7: 缺少多 hop 递减时间锁的边界测试

`multihop.rs` 测试了 3 hop 和 1 hop 的时间锁，但缺少：
- 最大 hop 数（如 10+ hop）的时间锁不会溢出或变成负数
- `HOP_MARGIN = 0` 的边界情况

---

## 四、链上程序 (ignite-pay-program) 审计

### PROG-1: `Claim` 指令未检查 `claimed_leaves` 集合

**文件**: `ignite-pay-program/src/instructions/claim.rs`
**设计文档**: §5.2 "duplicate claim prevention"

设计文档要求链上合约维护 `claimed_leaves: Set<u32>` 防止重复 claim。但 Anchor 程序中 `ChannelAccount` 没有 `claimed_leaves` 字段（设计文档 §4.1 有此字段），Claim 指令也没有检查是否已被 claim。

**原因**: `ChannelAccount` state 中缺少 `claimed_leaves: Vec<u32>` 字段。

---

### PROG-2: `FinalizeSettlement` 实际未执行 SPL Token 转账

**文件**: `ignite-pay-program/src/instructions/finalize_settlement.rs:77`

```rust
let _ = (refund_a, refund_b); // Used in production CPI calls
```

`refund_a` 和 `refund_b` 被计算后丢弃，没有实际执行 CPI 转账。标记为 TODO 状态。

---

### PROG-3: `ChannelAccount` 缺少设计文档要求的字段

**设计文档**: §4.1 ChannelAccount 包含 `claimed_leaves: Vec<u32>`, `leaf_count: u32`
**文件**: `ignite-pay-program/src/state.rs`

链上 `ChannelAccount` 缺少 `claimed_leaves` 字段，这是防重复 claim 的关键数据结构。

---

### PROG-4: `TriggerChallenge` 未验证 `sequence > on_chain.sequence`

**文件**: `ignite-pay-program/src/instructions/trigger_challenge.rs`
**设计文档**: §4.2 TriggerChallenge "验证 sequence > on_chain.sequence"

链上 `trigger_challenge` 指令没有 sequence/root 参数，无法验证提交的 sequence 大于链上记录的 sequence。

---

### PROG-5: Ed25519 签名验证为占位符

**文件**: 所有 instruction 文件

注释说"Ed25519 signature verification is done via Solana instruction introspection"，但未实现实际的 Ed25519 指令验证。需要配合 `solana_sdk::ed25519_instruction` 使用。

---

## 五、总结

| 类别 | 数量 | 严重度分布 |
|------|------|-----------|
| BUG | 8 | 高 1, 中 3, 低 4 |
| 业务规则偏差 | 6 | - |
| 测试覆盖缺失 | 7 | - |
| 链上程序问题 | 5 | - |
| **合计** | **26** | - |

### 优先级建议

**P0 (必须修复)**:
- BUG-10: fund_channel 签名未持久化
- BUG-12: MIN_TIMELOCK_BASE 硬编码
- BUG-15: VerifyHTLC/HTLCRefund 应支持 Challenged 状态
- PROG-1/3: 链上 claimed_leaves 缺失

**P1 (应该修复)**:
- DEV-1: auto_close_slot 触发逻辑
- DEV-6: settle_after_timeout `>` vs `>=` 不一致
- BUG-14: claim_leaf 缺少外部 proof 参数
- TEST-4: Challenged 状态 VerifyHTLC 测试

**P2 (建议改进)**:
- DEV-2: 合规模块集成
- DEV-3: 评分公式对齐
- DEV-4/5: 接口细节对齐
- BUG-13/16: 注释说明设计偏差
- 其余测试覆盖缺失
