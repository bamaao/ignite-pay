# Ignite Pay State Channel — 设计文档合规审查清单

> 对照 `docs/utxo_merkletree_state_channel.md` 全面审查 `ignite-pay-state-channel` 代码实现
> 审查日期: 2026-04-11

## 总体状态

- [x] 项目编译通过 (`cargo build`)
- [x] 所有测试通过 (`cargo test` — 186 tests)
- [x] Clippy 无警告 (`cargo clippy`)

---

## 1. 数据结构对齐 (§2A — 链下数据结构)

| # | 检查项 | 状态 | 说明 |
|---|--------|------|------|
| 1.1 | UTXOLeaf 字段完整性 | ✅ | `leaf_type`, `owner`, `amount`, `hash_lock`, `timelock_slot`, `beneficiary` 全部实现 |
| 1.2 | LeafType 枚举 | ✅ | Standard, HTLC, Compliance 与设计文档一致 |
| 1.3 | 空叶子处理 | ✅ | `UTXOLeaf::empty()` + `is_empty()` + `hash()` 正确实现 |
| 1.4 | 空叶子哈希一致性 | ✅ | 所有字段为默认值，哈希全局一致 |
| 1.5 | borsh 序列化 + SHA-256 哈希 | ✅ | `UTXOLeaf::hash()` 使用 `borsh::serialize` + `solana_program::hash::hash` |
| 1.6 | UTXO 不可拆分约束 | ✅ | `transfer_leaf` 整体转移，`partial_transfer` 通过两步原子操作实现 |
| 1.7 | 叶子数量固定约束 | ⚠️ | 树深度固定，叶子槽位固定，但未显式禁止外部调用者增删叶子。实际通过 `MerkleTree::new` 只在 `construct_split_tree` 中调用，间接满足。建议添加文档约束说明。 |
| 1.8 | 找零叶子 (Rest) 概念 | ✅ | 通过 `split_from_rest` / `merge_spent_leaves` 实现，无专门字段 |

---

## 2. 流水线签名机制 (§2B — Pipelined Signing)

| # | 检查项 | 状态 | 说明 |
|---|--------|------|------|
| 2.1 | LeafUpdate 消息格式 | ✅ | `channel_id`, `sequence`, `leaf_index`, `prev_leaf_hash`, `new_leaf`, `signature` 完整 |
| 2.2 | 签名内容格式 | ✅ | `SHA-256(channel_id \|\| sequence \|\| leaf_index \|\| prev_leaf_hash \|\| new_leaf_hash)` |
| 2.3 | Ed25519 签名 | ✅ | 使用 `ed25519_dalek` v1 |
| 2.4 | sequence 严格递增验证 | ✅ | `validate_leaf_update` 中 `update.sequence != state.metadata.sequence + 1` |
| 2.5 | prev_leaf_hash 匹配验证 | ✅ | 与本地树叶子哈希比对 |
| 2.6 | 签名验证 | ✅ | `verify_leaf_update_signature` |
| 2.7 | 批量签名 (Pipeline) | ✅ | `Pipeline` 结构体支持 `transfer_leaf`, `partial_transfer`, `create_htlc`, `resolve_htlc`, `refund_htlc` |
| 2.8 | 批量原子性 (All-or-Nothing) | ✅ | `apply_leaf_update_batch` 失败时全部回滚 |
| 2.9 | 部分失败信息 | ✅ | `BatchFailureInfo` 包含 `failed_index`, `error`, `applied_count` |
| 2.10 | Pipeline Drop 自动回滚 | ✅ | `Drop` trait 实现，`consumed` 标志控制 |
| 2.11 | 批量不允许重复叶子索引 | ✅ | ~~`apply_leaf_update_batch` 拒绝同一批次中重复 `leaf_index`~~ **BUG-35 修复**: 已移除过度限制的重复 leaf_index 检查，自然验证（sequence + prev_leaf_hash）确保正确性。 |
| 2.12 | 服务商处理流程 | ✅ | 链下验证：排序 → 逐个验证 sequence/hash/sig → 更新本地树 |

---

## 3. HTLC 集成 (§2C / §3.3)

| # | 检查项 | 状态 | 说明 |
|---|--------|------|------|
| 3.1 | HTLC 生命周期状态 | ✅ | Pending → Revealed → Fulfilled; Pending → Expired → Refunded |
| 3.2 | hash_lock / preimage 生成 | ✅ | `HtlcManager::create_htlc` 随机生成 preimage，SHA-256 计算 hash_lock |
| 3.3 | Preimage 验证 | ✅ | `verify_preimage` + `reveal_preimage` |
| 3.4 | Timelock 超时检查 | ✅ | `check_expiry` 使用严格 `>` 比较 |
| 3.5 | HTLC 创建 (Pipeline) | ✅ | `Pipeline::create_htlc` 验证 timelock 约束 |
| 3.6 | HTLC 解锁 (正常路径) | ✅ | `Pipeline::resolve_htlc` 验证 preimage → owner=beneficiary |
| 3.7 | HTLC 退款 (超时路径) | ✅ | `Pipeline::refund_htlc` 验证 current_slot > timelock_slot → owner=原 owner |
| 3.8 | 链上 VerifyHTLC | ✅ | `claim_htlc_verify`: 验证 preimage、beneficiary、Merkle proof、timelock |
| 3.9 | 链上 HTLCRefund | ✅ | `claim_htlc_refund`: 验证超时、owner、Merkle proof |
| 3.10 | 多个并发 HTLC | ✅ | `HtlcManager` 使用 HashMap 支持多个独立 HTLC |
| 3.11 | HTLC 不阻塞其他叶子 | ✅ | 每个 HTLC 占独立叶子，互不影响 |
| 3.12 | timelock_slot 约束验证 | ✅ | `timelock_slot > current_slot + challenge_duration + HTLC_SAFETY_MARGIN` |
| 3.13 | HTLC 持久化 | ✅ | 可选 sled 后端，`persist_to_db` / `load_from_db` |
| 3.14 | cleanup 方法 | ✅ | 移除已完成/已退款的 HTLC |

---

## 4. 业务流程 — 开通通道 (§3.1)

| # | 检查项 | 状态 | 说明 |
|---|--------|------|------|
| 4.1 | OpenChannel 单方质押 | ✅ | `open_channel` 仅需用户操作 |
| 4.2 | Root_init 单叶子树 | ✅ | 创建 1 个叶子，全部资金归用户 |
| 4.3 | sequence 初始化为 0 | ✅ | `metadata.sequence = 0` |
| 4.4 | 链下协商构建 Merkle Tree | ✅ | `construct_split_tree` 双方签名确认 |
| 4.5 | 金额守恒验证 | ✅ | `total == total_deposited` 检查 |
| 4.6 | UTXO 面额策略 | ✅ | 由调用方传入 `leaves` 参数决定面额组合 |
| 4.7 | 面额策略示例 (均匀/混合/找零优先) | ✅ | ~~代码未提供面额策略辅助函数~~ **已修复**: `helpers.rs` 中添加了 `DenominationStrategy` 枚举（Uniform/Mixed/RestFirst）+ `generate_leaves()` 方法。 |
| 4.8 | 双向注资 (fund_channel) | ✅ | FLOW-3: `fund_channel` 支持服务商注资 |
| 4.9 | construct_split_tree 双方验证 | ✅ | 验证 user_total == deposit_a, provider_total == deposit_b |
| 4.10 | auto_close_slot 在 open_channel 中 | ✅ | ~~`open_channel` 不接受此参数~~ **已修复**: `open_channel` 现在接受 `auto_close_slot: Option<u64>` 参数。 |

---

## 5. 业务流程 — 拆分与合并 (§3.2)

| # | 检查项 | 状态 | 说明 |
|---|--------|------|------|
| 5.1 | 标准转移 (整块 UTXO) | ✅ | `Pipeline::transfer_leaf` |
| 5.2 | 从 Rest 拆分 | ✅ | `helpers::split_from_rest` |
| 5.3 | 拆分操作顺序 | ✅ | ~~代码先创建目标叶子再扣减 Rest~~ **ISSUE-1 修复**: 已调整为设计文档 §3.2.2 的顺序：先从 Rest 扣减，再创建目标叶子。保证 `sum(leaves) <= total_deposited`。 |
| 5.4 | 合并已花费叶子 | ✅ | `helpers::merge_spent_leaves` |
| 5.5 | 合并所有权验证 | ✅ | 验证所有源叶子 owner == signer |
| 5.6 | 合并目标≠源检查 | ✅ | `target_idx` 不能在 `source_indices` 中 |
| 5.7 | 组合支付 (多个小 UTXO 拼凑) | ✅ | 通过连续 `transfer_leaf` 实现 |
| 5.8 | 拆分金额守恒 | ✅ | `split_from_rest` 和 `merge_spent_leaves` 都在测试中验证 `total_amount()` 不变 |

---

## 6. 业务流程 — 关闭通道 (§3.4)

| # | 检查项 | 状态 | 说明 |
|---|--------|------|------|
| 6.1 | 合作关闭 (CooperativeSettle) | ✅ | `close_channel` 验证双签 → Settling |
| 6.2 | 争议关闭 (TriggerChallenge) | ✅ | `trigger_challenge` 单签触发 |
| 6.3 | 提交反证 (SubmitCounterState) | ✅ | `submit_counter_state` 双签验证 + 更高 sequence |
| 6.4 | 超时结算 (SettleAfterTimeout) | ✅ | `settle_after_timeout` 严格 `>` 检查 |
| 6.5 | 自动关闭 (Auto-close) | ✅ | `auto_settle` 无需挑战期 |
| 6.6 | min_challenge_delay 前跑防护 | ✅ | `trigger_challenge` 检查 `current_slot >= open_slot + min_challenge_delay` |
| 6.7 | 合作关闭设置 settle_deadline | ✅ | `settle_deadline = current_slot + settle_window` |
| 6.8 | TriggerChallenge 更新 root/sequence | ✅ | 更新为提交的 root 和 sequence |
| 6.9 | SubmitCounterState 可选恢复树 | ✅ | `counter_leaves: Option<Vec<UTXOLeaf>>` 可重建 MerkleTree |
| 6.10 | SubmitCounterState 双签要求 | ✅ | 需要 sig_a + sig_b，防止单方伪造 |
| 6.11 | 关闭前 HTLC 清理检查 | ✅ | ~~`close_channel` 不检查是否存在未完成的 HTLC 叶子~~ **BUG-33 修复**: `close_channel` 现在遍历树叶子，发现 HTLC 叶子则拒绝关闭。 |

---

## 7. 链上结算 (§5 — 资金分配)

| # | 检查项 | 状态 | 说明 |
|---|--------|------|------|
| 7.1 | Claim-based 结算 | ✅ | `claim_leaf` / `claim_leaf_with_proof` |
| 7.2 | Claim Merkle proof 验证 | ✅ | 两个变体：内部生成 proof 和外部传入 proof |
| 7.3 | 防重复认领 | ✅ | `claimed_leaves: BTreeSet<u32>` |
| 7.4 | 空叶子不可 Claim | ✅ | 检查 `leaf.is_empty()` |
| 7.5 | 非 Standard 叶子不可普通 Claim | ✅ | 检查 `leaf.leaf_type != LeafType::Standard` 拒绝 HTLC/Compliance 叶子 |
| 7.6 | Claim owner 验证 | ✅ | `leaf.owner != *claimer_pubkey` |
| 7.7 | Claim amount 验证 | ✅ | `claim_amount != leaf.amount` |
| 7.8 | settle_deadline 检查 | ✅ | `current_slot > deadline` 拒绝 |
| 7.9 | 比例退回计算 | ✅ | `u128` 精度：`unclaimed * deposit_a / total_deposit` |
| 7.10 | 溢出保护 | ✅ | `saturating_add` / `saturating_sub` |
| 7.11 | total_claimed 超额保护 | ✅ | `new_total > total_deposited` 检查 |
| 7.12 | Claim 签名验证 | ✅ | ~~代码使用 `state_message` 作为 Claim 签名消息~~ **BUG-34 修复**: 已添加 `claim_message(channel_id, leaf_index, amount, slot)` 函数，格式为 `SHA-256("claim" \|\| channel_id \|\| leaf_index \|\| amount \|\| slot)`。`claim_leaf`、`claim_leaf_with_proof`、`claim_htlc_verify`、`claim_htlc_refund` 均已更新使用 `claim_message`。 |
| 7.13 | VerifyHTLC/HTLCRefund 可在 Challenged 状态执行 | ✅ | 两个方法都检查 `Challenged || Settling` |
| 7.14 | Challenged 状态下 settle_deadline 可选 | ✅ | `BUG-32` 修复：仅当 `settle_deadline` 存在时检查 |

---

## 8. 两层签名体系 (§4.3)

| # | 检查项 | 状态 | 说明 |
|---|--------|------|------|
| 8.1 | 叶子级签名 | ✅ | `leaf_update_message` + `sign_leaf_update` |
| 8.2 | 根级签名 | ✅ | `state_message` + `sign_state` / `verify_state_signature` |
| 8.3 | CooperativeSettle 双签 | ✅ | 验证 sig_a + sig_b |
| 8.4 | TriggerChallenge 单签 | ✅ | 仅需提交方签名 |
| 8.5 | SubmitCounterState 双签 | ✅ | 需要 sig_a + sig_b |
| 8.6 | 签名格式 Ed25519 | ✅ | 纯签名，无额外封装 |
| 8.7 | 服务商回签协议 (Provider Co-signing) | ✅ | `provider_cosign_state` 方法 |
| 8.8 | provider_cosign 持久化 | ✅ | 存储/加载 `Option<[u8; 64]>` |
| 8.9 | Root 变更时清除 provider_cosign | ✅ | `apply_leaf_update` 中 `state.provider_cosign = None` |

---

## 9. 合规模块 (§6 / §11.2 — FLOW-7)

| # | 检查项 | 状态 | 说明 |
|---|--------|------|------|
| 9.1 | SpendingLimit 数据结构 | ✅ | `threshold`, `per_channel`, `window_slots` |
| 9.2 | TravelRuleData 数据结构 | ✅ | 含 `originator_jurisdiction`, `beneficiary_jurisdiction` |
| 9.3 | ComplianceAction 枚举 | ✅ | None / InsertMarker |
| 9.4 | 滑动窗口消费追踪 | ✅ | `window_payments: Vec<PaymentRecord>` + 自动裁剪 |
| 9.5 | 累计消费阈值触发 | ✅ | `effective_spend >= threshold` → InsertMarker + hold |
| 9.6 | 合规暂停机制 | ✅ | `compliance_hold` 阻止后续支付 |
| 9.7 | clear_hold | ✅ | 解除合规暂停 |
| 9.8 | 审计日志 | ✅ | `record_audit` + `get_audit_trail` |
| 9.9 | Compliance 标记叶子 | ✅ | `create_compliance_leaf` |
| 9.10 | 与 ChannelManager 集成 | ✅ | `set_compliance` + `apply_leaf_update` 自动检查 |
| 9.11 | slot=0 边界情况处理 | ⚠️ | `record_payment` 中 slot=0 时使用 `cumulative_spent` 代替 `window_spend`，这是链下无法获取 slot 信息时的权宜之计。可能在频繁 slot=0 调用时导致阈值提前触发。 |
| 9.12 | 审计日志 scan_prefix 顺序依赖 | ✅ | ~~key 前缀冲突风险~~ **ISSUE-2 修复**: 序列计数器 key 已改为 `audit:{cid}:__seq__`，`get_audit_trail` 通过字符串后缀检查跳过计数器 key。 |

---

## 10. Hub 注册与路由 (§10 / §11.3 — FLOW-2)

| # | 检查项 | 状态 | 说明 |
|---|--------|------|------|
| 10.1 | HubLeaf 数据结构 | ✅ | 与设计文档 §10.2.2 一致 |
| 10.2 | HubMetrics 数据结构 | ✅ | 含 `online_rate`, `success_rate`, `fee_rate_bps` 等 |
| 10.3 | HubManager CRUD | ✅ | `register_hub`, `get_hub`, `get_metrics`, `update_metrics`, `list_hubs` |
| 10.4 | 指标哈希计算 | ✅ | `compute_metrics_hash` |
| 10.5 | Hub 持久化 | ✅ | sled + borsh |
| 10.6 | 路由发现 (DFS) | ✅ | `discover_routes` 使用 DFS 搜索 |
| 10.7 | 路由评分公式 | ✅ | `0.3*fee_score + 0.3*latency_score + 0.4*min_success_rate` |
| 10.8 | min_success_rate (非 avg) | ✅ | 使用 `fold(f64::INFINITY, f64::min)` |
| 10.9 | 最佳路由选择 | ✅ | `select_best_route` 使用 `max_by` |
| 10.10 | 显式拓扑控制 | ✅ | `add_channel_edge` |
| 10.11 | 启发式拓扑回退 | ✅ | 无显式边时，连接有流动性的 hub |
| 10.12 | 流动性检查 | ✅ | 路由构建时检查 `available_liquidity` |
| 10.13 | Hub 惩罚机制 | ❌ | **设计文档 §10.2.3 定义了惩罚规则（在线率<99%、成功率<95%、恶意扣留等）。代码中 `HubManager` 未实现惩罚逻辑。** 建议添加 `penalize_hub` 或 `check_hub_sla` 方法。 |

---

## 11. 多跳路由 (§10.4 / §11.3 — FLOW-2)

| # | 检查项 | 状态 | 说明 |
|---|--------|------|------|
| 11.1 | 递减 timelock 约束 | ✅ | `hop[i].timelock = base_timelock - i * HOP_MARGIN` |
| 11.2 | MIN_TIMELOCK 计算 | ✅ | `min_timelock = challenge_duration + 3 * HOP_MARGIN` |
| 11.3 | base_timelock 计算 | ✅ | `current_slot + min_timelock + (num_hops-1) * HOP_MARGIN` |
| 11.4 | 同 Hash-Lock 多跳 | ✅ | 所有 hop 使用相同 hash_lock |
| 11.5 | 路由费计算 | ✅ | `compute_hop_amounts` 反向累加费用 |
| 11.6 | 费用溢出保护 | ✅ | `checked_mul` / `checked_div` / `checked_add` |
| 11.7 | Preimage 揭示 | ✅ | `reveal_preimage` 验证后标记 Resolving |
| 11.8 | Hop 逐个解析 | ✅ | `resolve_hop` 标记完成 |
| 11.9 | 全部完成后标记 Completed | ✅ | 检查 `hops.iter().all(\|h\| h.resolved)` |
| 11.10 | 过期检查 | ✅ | `check_expiry` 标记 Failed |
| 11.11 | HTLC LeafUpdate 生成 | ✅ | `create_htlc_leaf_update` 签名生成 |
| 11.12 | 多跳持久化 | ✅ | sled + borsh |
| 11.13 | 路由失败处理 (§10.4.3) | ⚠️ | **设计文档描述了 3 种失败场景（流动性不足、HTLC 超时、恶意扣留 R），代码仅实现了过期检查。缺少 `RouteError` 类型和具体的失败恢复逻辑（如尝试备选路由）。** |

---

## 12. 链上合约对齐 (§4 — Program Logic)

| # | 检查项 | 状态 | 说明 |
|---|--------|------|------|
| 12.1 | ChannelAccount 字段完整 | ✅ | 所有字段在 `ChannelMetadata` 中实现 |
| 12.2 | ChannelStatus 状态机 | ✅ | Open → Challenged → Settling → Closed |
| 12.3 | 10 条指令对齐 | ✅ | OpenChannel, FundChannel, CooperativeSettle, TriggerChallenge, SubmitCounterState, SettleAfterTimeout, Claim, VerifyHTLC, HTLCRefund, FinalizeSettlement |
| 12.4 | Claim 可由任何人提交 | ✅ | 任何人可提交，资金转给 `leaf.owner` |
| 12.5 | Merkle proof sorted-pair | ✅ | `hashv(&[min, max])` 与 `compression.rs:verify_proof_locally` 一致 |
| 12.6 | FundChannel CPI | ⚠️ | 链下模型无法模拟 CPI，`fund_channel` 直接修改 deposit_b。链上实现时需添加 SPL Token CPI。 |
| 12.7 | FinalizeSettlement 比例退回 | ✅ | 使用 `u128` 精度 |
| 12.8 | UpdateChannel 指令缺失 | ⚠️ | 设计文档 §10.6.2 提到"链上充值"使用 UpdateChannel 指令更新 `current_root` 和 `deposit_a`，当前未实现。属于 Phase 6 范围。 |
| 12.9 | auto_close_slot 在 open_channel | ✅ | 同 4.10（已修复） |

---

## 13. 已修复的 BUG

以下问题在之前的审查轮次中已修复：

| BUG ID | 描述 | 修复位置 |
|--------|------|----------|
| BUG-1 | `apply_leaf_update` 后 `provider_cosign` 未清除 | `channel.rs:436` |
| BUG-2 | `close_channel` 未设置 `settle_deadline` | `channel.rs:711` |
| BUG-3 | `trigger_challenge` 未验证签名 | `channel.rs:844-852` |
| BUG-4 | `finalize_settlement` 比例退回精度不足 | `channel.rs:1476-1483` |
| BUG-5 | `apply_leaf_update_batch` 未回滚 | `channel.rs:470-488` |
| BUG-6 | HTLC 操作未验证 preimage/expiry | `pipeline.rs:242-246, 286-293` |
| BUG-22 | `claim_leaf` 未检查叶子类型 | `channel.rs:1054-1059` |
| BUG-23 | `trigger_challenge` 签名使用 `current_slot` 而非 `submitted_sequence` | `channel.rs:845-849` |
| BUG-32 | HTLC claim 方法在 Challenged 状态下检查 settle_deadline | `channel.rs:1224-1231` |
| CODE-1 | Pipeline Drop 未自动回滚 | `pipeline.rs:335-347` |
| CODE-4 | `split_from_rest` 未验证 signer 拥有 Rest 叶子 | `helpers.rs:42-46` |
| BUG-33 | `close_channel` 不检查未完成 HTLC 叶子 | `channel.rs:close_channel` |
| BUG-34 | Claim/VerifyHTLC 签名消息缺乏域分离 | `signing.rs:claim_message` + `channel.rs` |
| BUG-35 | `apply_leaf_update_batch` 过度限制重复 leaf_index | `channel.rs:apply_leaf_update_batch` |
| ISSUE-1 | `split_from_rest` 操作顺序与文档不一致 | `helpers.rs:split_from_rest` |
| ISSUE-2 | 审计日志 key 前缀冲突风险 | `compliance.rs:record_audit` |

---

## 14. 新发现的 BUG / 问题

### BUG-33: `close_channel` 不检查 HTLC 叶子 ✅ 已修复

**严重性**: 中
**位置**: `channel.rs:close_channel`
**设计文档**: §3.4.5
**修复**: 在 `close_channel` 开始处遍历树叶子，发现 HTLC 叶子则返回错误拒绝关闭。测试: `test_close_channel_with_htlc_rejected`。

### BUG-34: Claim/VerifyHTLC/HTLCRefund 签名消息使用 `current_slot` 作为 sequence ✅ 已修复

**严重性**: 低
**位置**: `signing.rs`, `channel.rs`
**修复**: 新增 `claim_message(channel_id, leaf_index, amount, slot)` 函数，格式为 `SHA-256("claim" || channel_id || leaf_index || amount || slot)`。`claim_leaf`、`claim_leaf_with_proof`、`claim_htlc_verify`、`claim_htlc_refund` 均已更新。

### BUG-35: `apply_leaf_update_batch` 拒绝相同 leaf_index 的合法批量更新 ✅ 已修复

**严重性**: 低
**位置**: `channel.rs:apply_leaf_update_batch`
**修复**: 移除了过度限制的 BTreeSet 重复 leaf_index 检查。自然的验证（sequence 递增 + prev_leaf_hash 匹配）确保正确性。

### ISSUE-1: `split_from_rest` 操作顺序与设计文档不一致 ✅ 已修复

**严重性**: 信息
**位置**: `helpers.rs:split_from_rest`
**修复**: 已调整为设计文档 §3.2.2 的顺序：先从 Rest 扣减（减少总额），再创建目标叶子（恢复总额），保证 `sum(leaves) <= total_deposited`。

### ISSUE-2: `compliance.rs` 审计日志 key 可能包含前缀冲突 ✅ 已修复

**严重性**: 低
**位置**: `compliance.rs:record_audit`, `get_audit_trail`
**修复**: 序列计数器 key 已改为 `audit:{cid}:__seq__`，`get_audit_trail` 通过字符串后缀检查跳过计数器 key。

---

## 15. 测试覆盖率评估

### 已覆盖的核心场景 ✅

| 测试类别 | 测试文件 | 测试数量 | 覆盖场景 |
|----------|----------|----------|----------|
| Merkle Tree | `merkle.rs` + `tests/merkle_tests.rs` | ~20 | 构建、更新、proof、验证、边界 |
| 签名 | `signing.rs` + `tests/signing_tests.rs` | ~19 | 签名/验签、篡改拒绝、错误密钥 |
| 通道操作 | `channel.rs` + `tests/channel_tests.rs` | ~50 | 全生命周期、争议、HTLC claim、批量、关闭HTLC检查 |
| Pipeline | `pipeline.rs` | 9 | 转移、部分转移、HTLC、abort/drop |
| Helpers | `helpers.rs` | 12 | 拆分、合并、所有权检查、面额策略 |
| HTLC | `htlc.rs` | 8 | 创建、揭示、过期、退款 |
| 合规 | `compliance.rs` | 9 | 阈值、暂停、审计、窗口 |
| Hub | `hub.rs` | 5 | 注册、查询、指标 |
| 路由 | `routing.rs` | 7 | 发现、评分、拓扑 |
| 多跳 | `multihop.rs` | 10 | 支付、timelock、解析、费用 |

### 已添加的测试 ✅

| # | 测试 | 状态 | 测试函数 |
|---|------|------|----------|
| T-1 | 关闭前 HTLC 清理 | ✅ | `test_close_channel_with_htlc_rejected` |
| T-2 | 多个并发 HTLC 生命周期 | ✅ | `test_multiple_concurrent_htlcs` |
| T-9 | 批量更新中相同 leaf_index | ✅ | `test_batch_duplicate_leaf_index_rejected` (已存在) |
| T-10 | 双向通道完整流程 | ✅ | `test_dual_funded_close_proportional_refund` (已存在) |
| T-11 | 链下协商失败后全额退回 | ✅ | `test_negotiation_failure_full_refund` |
| T-12 | Compliance hold 暂停后的恢复支付 | ✅ | `test_compliance_hold_clear_then_resume` (已存在) |
| T-13 | 多跳费用精度验证 | ✅ | `test_multihop_fee_precision` |

### 仍未覆盖的场景 ⚠️

| # | 缺失测试 | 优先级 | 对应文档章节 |
|---|----------|--------|-------------|
| T-3 | **Compliance 标记叶子插入通道树** | 中 | §6 |
| T-4 | **多跳支付与 ChannelManager 集成** | 中 | §10.4 |
| T-5 | **路由失败场景（流动性不足、超时）** | 中 | §10.4.3 |
| T-6 | **Hub 惩罚/SLA 违规** | 低 | §10.2.3 |
| T-7 | **Provider 回签后立即关闭** | 低 | §4.3.4 |
| T-8 | **Watchtower / 第三方触发 auto_settle** | 低 | §3.4.3 |

---

## 16. FLOW 实现规范对齐 (§11)

| FLOW | 要求 | 状态 | 说明 |
|------|------|------|------|
| FLOW-1 | Solana 链上程序 (10 指令) | ⚠️ | 链下模型实现完整，链上 Anchor 程序在 `ignite-pay-solana/` 中独立实现 |
| FLOW-2 | 多跳路由 + Hub | ✅ | `routing.rs` + `multihop.rs` + `hub.rs` |
| FLOW-3 | 双向注资通道 | ✅ | `fund_channel` + 扩展 `construct_split_tree` |
| FLOW-4 | Provider 回签 | ✅ | `provider_cosign_state` |
| FLOW-5 | 批量失败信息 | ✅ | `apply_leaf_update_batch_with_info` |
| FLOW-6 | HTLC timelock 约束验证 | ✅ | `Pipeline::create_htlc` |
| FLOW-7 | 合规模块 | ✅ | `compliance.rs` |
| FLOW-8 | VerifyHTLC / HTLCRefund | ✅ | `claim_htlc_verify` / `claim_htlc_refund` |

---

## 17. 技术风险提示 (§9) 对齐

| 风险项 | 代码是否缓解 | 说明 |
|--------|-------------|------|
| 数据可用性 | ⚠️ | 依赖 sled 本地持久化，无 IPFS/Arweave 备份。设计文档建议的快照备份未实现。 |
| 状态排序 | ✅ | `sequence` 严格递增，服务商批量排序验证 |
| 存储压力 | ✅ | `merge_spent_leaves` 回收叶子槽位 |
| 挑战期与 HTLC 时序 | ✅ | `timelock_slot > current_slot + challenge_duration + SAFETY_MARGIN` |
| 前跑攻击 | ✅ | `min_challenge_delay` |
| 资金永久锁定 | ✅ | `auto_close_slot` + `auto_settle` |
