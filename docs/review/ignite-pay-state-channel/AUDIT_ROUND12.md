# Ignite Pay State Channel 第十二轮代码审计报告

> 审计依据: `docs/utxo_merkletree_state_channel.md` (设计指导书)
> 审计范围: `ignite-pay-state-channel/` 项目 + `ignite-pay-program/` 链上程序
> 审计日期: 2026-04-12 (第十二轮)

---

## 审计总览

对全部源文件逐一比对设计文档，检查业务流程、功能规则、测试覆盖及潜在 Bug。本报告按严重程度分级：

- **BUG**: 功能性缺陷，可能导致资金安全问题
- **DEV**: 业务规则与设计文档不一致
- **TEST**: 测试覆盖缺失
- **PROG**: 链上程序问题

| 类别 | 发现数量 |
|------|---------|
| BUG | 4 |
| DEV | 5 |
| TEST | 5 |
| PROG | 6 |
| **合计** | **20** |

---

## 一、BUG（功能性缺陷）

### BUG-18: 链上 `verify_htlc` 缺少 timelock 检查

**严重度**: P1 (资金安全)
**文件**: `ignite-pay-program/src/instructions/verify_htlc.rs`
**设计文档**: §4.2 VerifyHTLC — "timelock not expired (current_slot <= timelock_slot)"
**问题**: 链上 `verify_htlc` 函数接收了 `preimage` 和 `hash_lock` 参数并验证了 hash_lock 匹配，但**完全没有检查 timelock 是否已过期**。攻击者可以在 HTLC 过期后（此时 owner 应可退款）仍用 preimage 领取资金，与 HTLCRefund 形成竞争条件。
**建议**: 添加 `require!(current_slot <= timelock_slot, ChannelError::HtlcExpired)` 或将 `timelock_slot` 作为函数参数传入并验证。

### BUG-19: 链上 `settle_after_timeout` 使用 `>=` 而非严格 `>`

**严重度**: P2
**文件**: `ignite-pay-program/src/instructions/settle_after_timeout.rs:30`
**设计文档**: §4.2 — "current_slot > challenge_slot + challenge_duration"（严格大于）
**问题**: 链上代码使用 `current_slot >= challenge_slot + channel.challenge_duration`（大于等于），而链下代码 `channel.rs:800` 已正确使用严格 `>`（`current_slot <= challenge_slot + ...` 时拒绝）。链上链下行为不一致，链上允许提前一个 slot 进入 Settling。
**建议**: 改为 `require!(current_slot > challenge_slot + channel.challenge_duration, ...)`。

### BUG-20: 链上 `claim` 指令不验证 `claim_amount` 与传入的 `leaf_hash` 一致性

**严重度**: P2
**文件**: `ignite-pay-program/src/instructions/claim.rs`
**问题**: `claim` 指令接收 `claim_amount` 和 `leaf_hash` 参数，验证了 Merkle proof 和 claimer==leaf_owner，但**没有机制确保 `claim_amount` 实际编码在 `leaf_hash` 中**。调用者可以传入 `claim_amount=1` 和合法的 `leaf_hash`（对应 amount=1000000 的叶子），Merkle proof 会通过但只 claim 1 个单位。链下代码在 `channel.rs:977` 有 `claim_amount != leaf.amount` 检查，但链上无法进行此检查（因为没有反序列化叶子的能力）。
**说明**: 这是由 Merkle tree 设计决定的——链上只验证 hash 不验证内容。需要通过其他机制（如要求传入叶子序列化数据，或链下在 `leaf_hash` 中编码 amount）来防止。当前链下 `claim_leaf` 在 `channel.rs:977` 有此检查，但链上缺失。
**建议**: 考虑要求调用者传入完整的叶子序列化数据，链上重新计算 hash 并对比，同时验证 claim_amount。

### BUG-21: `routing.rs` 通道图构建假设全连接拓扑

**严重度**: P3
**文件**: `ignite-pay-state-channel/src/routing.rs:81-97`
**设计文档**: §10.3.1 通道图应基于实际链上通道数据构建
**问题**: `refresh_graph()` 将所有已注册 Hub 视为完全互连（complete graph），忽略实际的通道状态和流动性拓扑。虽然注释中说明了这是简化模型，但可能导致发现不存在的路由（无实际通道的 Hub 对之间），在生产环境中会引发支付失败。
**建议**: 在生产实现中，通道图应从链上 ChannelAccount 数据构建，仅包含实际存在的双向通道。

---

## 二、DEV（业务规则偏差）

### DEV-7: 路由评分使用平均 `success_rate` 而非设计文档的 `min_success_rate`

**文件**: `ignite-pay-state-channel/src/routing.rs:253`
**设计文档**: §10.3.2 — reliability_score 应使用 `min(success_rate)` 取整条路径的最低成功率
**问题**: 代码使用 `avg_success = sum(success_rate) / count`（平均成功率），而设计文档明确指定使用 `min`。一条包含低可靠性 Hub 的路由在 `min` 模式下得分会显著低于 `avg` 模式，当前实现可能选择经过不可靠 Hub 的路由。
**建议**: 将 `avg_success` 改为 `min_success`：`let min_success = metrics.iter().map(|m| m.success_rate as f64 / 10000.0).fold(f64::INFINITY, f64::min);`

### DEV-8: 合规模块未与通道操作集成

**文件**: `ignite-pay-state-channel/src/channel.rs` — `apply_leaf_update` 方法
**设计文档**: §11.2.3 — 当 cumulative_spent >= threshold 时应触发 compliance_hold
**问题**: `apply_leaf_update` 在处理叶子更新时没有调用 `ComplianceManager::record_payment()`。合规模块是完全独立的，不会在实际支付流程中自动触发。
**状态**: 前几轮审计已标记为 P2（DEV-2），仍未修复。

### DEV-9: `trigger_challenge` 链下版本缺少 sequence/root 参数

**文件**: `ignite-pay-state-channel/src/channel.rs:730`
**设计文档**: §4.2 TriggerChallenge — 应提交 (submitted_root, submitted_sequence)
**问题**: 链下 `trigger_challenge` 只接受 challenger_pubkey 和 signature，不接受 submitted_root 和 submitted_sequence。而链上版本 `trigger_challenge.rs` 已正确接收这两个参数并更新链上状态。链下版本在触发 challenge 后不会更新 current_root 和 sequence。
**状态**: 前几轮审计已标记为 P2（DEV-4），仍未修复。

### DEV-10: 链上 `ChannelAccount` 缺少 `auto_close_slot` 字段

**文件**: `ignite-pay-program/src/state.rs`
**设计文档**: §4.1 ChannelAccount 布局表中包含 `auto_close_slot: Option<u64>`（§3.4.3 自动关闭功能）
**问题**: 链下 `ChannelMetadata`（types.rs:218）有 `auto_close_slot: Option<u64>` 字段，但链上 `ChannelAccount`（state.rs:21-64）**缺少此字段**。链上没有对应的 auto-close 指令，auto_settle 功能仅在链下实现。
**建议**: 在 `ChannelAccount` 中添加 `pub auto_close_slot: Option<u64>` 字段，并添加对应的链上 auto_settle 指令。

### DEV-11: `window_slots` 滚动窗口未实现

**文件**: `ignite-pay-state-channel/src/compliance.rs`
**设计文档**: §11.2.1 SpendingLimit 包含 `window_slots` 字段用于滚动窗口限流
**问题**: `SpendingLimit` 结构体有 `window_slots` 字段，但 `record_payment()` 只检查 `cumulative_spent >= threshold`，没有使用滚动窗口。`cumulative_spent` 是累计值，永不衰减。这意味着一旦消费累计超过阈值就永久冻结，而非在窗口期后重置。
**建议**: 实现基于 `window_slots` 的滑动窗口逻辑，或者移除 `window_slots` 字段并更新设计文档。

---

## 三、TEST（测试覆盖缺失）

### TEST-8: 缺少链上程序单元/集成测试

**文件**: `ignite-pay-program/`
**问题**: 链上程序（10 个 Anchor 指令）没有任何测试文件。虽然 Anchor 程序通常通过集成测试验证，但核心逻辑（如 Merkle proof 验证、金额溢出检查、状态转换约束）应有独立的单元测试。
**建议**: 至少为以下关键逻辑添加测试：
- Merkle proof 验证（`utils/merkle.rs`）
- Claim 指令的重复 claim 拒绝
- VerifyHTLC 的 hash_lock 验证
- HTLCRefund 的过期检查
- FinalizeSettlement 的按比例退款计算

### TEST-9: 缺少合规模块与通道集成测试

**问题**: 没有测试验证在 `apply_leaf_update` 时合规模块是否被正确调用。当前合规模块完全独立于通道操作，但设计文档要求两者集成。
**状态**: 前几轮审计已标记为 P2（TEST-2），仍未修复。

### TEST-10: 缺少多跳支付端到端测试

**问题**: 没有测试验证跨通道的 HTLC 链式锁定/解锁流程。`multihop.rs` 的测试仅验证了单模块的创建/解析逻辑，未覆盖多通道间的 HTLC 联动。
**状态**: 前几轮审计已标记为 P2（TEST-3），仍未修复。

### TEST-11: 缺少 `claim_leaf` 对非通道参与者叶子的拒绝测试

**文件**: `ignite-pay-state-channel/tests/channel_tests.rs`
**问题**: 在 `test_full_lifecycle_with_htlc_and_settlement` 测试中（第 506 行注释），merchant 拥有的叶子无法被 user 或 provider claim（因为 claim_leaf 检查 claimer==leaf.owner），但这一重要边界条件没有专门的测试用例验证。
**建议**: 添加测试验证：当叶子 owner 是非通道参与者时，claim_leaf 应该被拒绝。

### TEST-12: 缺少 `compute_hop_amounts` 大额溢出测试

**文件**: `ignite-pay-state-channel/src/multihop.rs`
**问题**: `compute_hop_amounts` 使用 `checked_mul`/`checked_add` 处理溢出，但没有测试验证溢出时的返回 `None` 行为。
**建议**: 添加测试用例：当 destination_amount 接近 u64::MAX 且有多跳费用时，应返回 `None`。

---

## 四、PROG（链上程序问题）

### PROG-6: `FundChannel` 链上指令未更新 `leaf_count`

**文件**: `ignite-pay-program/src/instructions/fund_channel.rs:47-52`
**问题**: 链上 `open_channel` 设置 `leaf_count = 1`，但 `fund_channel` 在更新 `deposit_b` 和 `total_deposited` 后没有更新 `leaf_count`。链下代码在 `fund_channel` 中会更新 `leaf_count`（channel.rs:226），链上不一致。
**建议**: 在链上 `fund_channel` 中添加 `channel.leaf_count += 1;`。

### PROG-7: `FinalizeSettlement` 未执行实际的 SPL Token CPI 转账

**文件**: `ignite-pay-program/src/instructions/finalize_settlement.rs:84-89`
**设计文档**: §11.4.4 — "FinalizeSettlement: SPL Token transfer CPI from escrow -> vault_a / vault_b"
**问题**: 虽然代码正确计算了 `refund_a` 和 `refund_b`，但实际转账逻辑被注释掉了（`let _ = (refund_a, refund_b);`）。用户不会收到退款。
**状态**: 前几轮审计已标记为 P2（PROG-2），仍未修复。

### PROG-8: Ed25519 签名验证使用占位符

**文件**: 多个链上指令（`cooperative_settle.rs`, `trigger_challenge.rs`, `submit_counter_state.rs`, `claim.rs`, `verify_htlc.rs`, `htlc_refund.rs`, `finalize_settlement.rs`）
**设计文档**: §11.4.4 — "Ed25519 签名验证通过 Solana ed25519_program 指令内省"
**问题**: 所有指令中的签名参数均以下划线前缀命名（`_sig_a`, `_sig_b`, `_claimer_signature`, `_caller_signature`, `_challenger_signature`），表示这些参数接收但未使用。签名验证依赖"外部机制"（Ed25519 instruction introspection），但代码中没有实际集成此机制。
**状态**: 前几轮审计已标记为 P2（PROG-5），仍未修复。

### PROG-9: `ChannelAccount::space()` 计算不精确

**文件**: `ignite-pay-program/src/state.rs:68-91`
**问题**: `status` 字段使用 `1 + 32` 估计（1 字节枚举 + 32 padding），实际 Anchor 序列化的 `ChannelStatus` 枚举仅占 1 字节，不应加 32 padding。`claimed_leaves` 使用 `4 + 256 * 4`（最多 256 个条目），但 `open_channel` 中没有初始化上限检查。如果叶子数超过 256（tree_depth > 8 时），claim 操作可能导致账户空间溢出。
**建议**: 修正 space 计算，或在 `open_channel` 中限制 `tree_depth <= 8`。

### PROG-10: `trigger_challenge` 直接接受并存储提交的 root/sequence 而未验证签名

**文件**: `ignite-pay-program/src/instructions/trigger_challenge.rs:53-54`
**问题**: `trigger_challenge` 接受 `submitted_root` 和 `submitted_sequence` 并直接写入链上状态，仅通过注释说明签名验证在"Ed25519 instruction introspection"中完成。如果签名验证机制未正确集成，任何人都可以提交任意 root/sequence 来覆盖通道状态。
**状态**: 与 PROG-8 相关。

### PROG-11: `cooperative_settle` 使用 `==` 比较 sequence，而链下 close 允许当前 sequence

**文件**: `ignite-pay-program/src/instructions/cooperative_settle.rs:37-44`
**问题**: 链上 `cooperative_settle` 要求 `sequence == channel.sequence`，这在链上是正确的（因为链上 sequence 代表最终确认的状态）。但链下 `close_channel` 的 sequence 检查行为（channel.rs:611）使用了严格相等并添加了注释说明偏差。两处行为一致，但链上 `cooperative_settle` 没有对提交的 root 和 signatures 进行实质验证（仅占位符），需要确保集成 Ed25519 验证后才安全。

---

## 五、前几轮遗留 P2 问题追踪

以下是第十一轮审计标记为 P2 延后的问题，本轮仍未修复：

| ID | 描述 | 状态 |
|----|------|------|
| DEV-2 | 合规模块与通道操作集成 | 未修复（本轮 DEV-8 重新描述） |
| DEV-4 | `trigger_challenge` 链下增加 sequence/root 参数 | 未修复（本轮 DEV-9 重新描述） |
| PROG-2 | FinalizeSettlement SPL Token CPI 转账 | 未修复（本轮 PROG-7 重新描述） |
| PROG-5 | Ed25519 签名验证改为 instruction introspection | 未修复（本轮 PROG-8 重新描述） |
| TEST-2 | 合规模块与通道集成测试 | 未修复（本轮 TEST-9 重新描述） |
| TEST-3 | 多跳支付端到端测试 | 未修复（本轮 TEST-10 重新描述） |

---

## 六、本轮新增问题汇总

| ID | 类别 | 严重度 | 描述 | 文件 |
|----|------|--------|------|------|
| BUG-18 | BUG | P1 | 链上 verify_htlc 缺少 timelock 检查 | verify_htlc.rs |
| BUG-19 | BUG | P2 | 链上 settle_after_timeout 使用 >= 而非 > | settle_after_timeout.rs:30 |
| BUG-20 | BUG | P2 | 链上 claim 无法验证 claim_amount 与 leaf_hash 一致 | claim.rs |
| BUG-21 | BUG | P3 | 路由图全连接拓扑假设 | routing.rs:81 |
| DEV-7 | DEV | P2 | 路由评分用 avg 而非 min success_rate | routing.rs:253 |
| DEV-8 | DEV | P2 | 合规模块未集成通道操作 | channel.rs |
| DEV-9 | DEV | P2 | trigger_challenge 链下缺少 sequence/root | channel.rs:730 |
| DEV-10 | DEV | P2 | 链上 ChannelAccount 缺少 auto_close_slot | state.rs |
| DEV-11 | DEV | P3 | compliance window_slots 未实现 | compliance.rs |
| TEST-8 | TEST | P2 | 链上程序无测试 | ignite-pay-program/ |
| TEST-9 | TEST | P2 | 合规+通道集成测试缺失 | — |
| TEST-10 | TEST | P2 | 多跳端到端测试缺失 | — |
| TEST-11 | TEST | P3 | 非 participant 叶子 claim 拒绝测试缺失 | channel_tests.rs |
| TEST-12 | TEST | P3 | compute_hop_amounts 溢出测试缺失 | multihop.rs |
| PROG-6 | PROG | P2 | FundChannel 未更新 leaf_count | fund_channel.rs |
| PROG-7 | PROG | P2 | FinalizeSettlement 未执行 CPI 转账 | finalize_settlement.rs |
| PROG-8 | PROG | P2 | Ed25519 签名验证占位符 | 多个指令 |
| PROG-9 | PROG | P3 | ChannelAccount space() 计算不精确 | state.rs |
| PROG-10 | PROG | P2 | trigger_challenge 未验证签名即更新状态 | trigger_challenge.rs |
| PROG-11 | PROG | P3 | cooperative_settle 签名验证占位符 | cooperative_settle.rs |

---

## 七、建议优先级

### 必须修复 (P1)
1. **BUG-18**: 链上 verify_htlc 添加 timelock 检查 — 直接影响资金安全

### 应尽快修复 (P2)
2. **BUG-19**: 链上 settle_after_timeout 严格 `>` 检查
3. **BUG-20**: 链上 claim amount 验证机制
4. **DEV-7**: 路由评分改为 min success_rate
5. **DEV-10**: 链上 ChannelAccount 添加 auto_close_slot
6. **PROG-6**: FundChannel 更新 leaf_count
7. **PROG-7/8/10**: 链上签名验证和 CPI 转账实现

### 可延后 (P3)
8. **BUG-21**: 路由图拓扑改进（当前简化模型可先用于 MVP）
9. **DEV-11**: compliance window_slots 实现
10. **TEST-11/12**: 补充边界条件测试
11. **PROG-9/11**: space 计算修正和签名验证完善
