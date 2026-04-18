# 场景十一：合规管理与审计

## 1. 场景描述

通道可启用合规管理功能，对支付行为进行实时监控。当累计支付超过阈值时自动触发合规审查（插入合规标记叶子并冻结通道），同时维护完整的审计追踪记录。

## 2. 参与角色

| 角色 | 职责 |
|:-----|:-----|
| User/Provider | 配置合规参数，记录支付和审计 |
| ComplianceManager | 执行限额检查，生成合规叶子 |

## 3. 前置条件

- config.toml 中包含 `[compliance]` 配置段
- 通道开通后初始化合规状态

## 4. 操作流程

```
User                                ComplianceManager
 │  1. 初始化合规                        │
 │  init_channel_compliance(channel_id, │
 │    SpendingLimit{threshold,           │
 │    per_channel, window_slots})        │
 │─────────────────────────────────────→│
 │                                      │
 │  2. 每次支付后记录                     │
 │  record_payment(channel_id, amount,  │
 │    slot, user_pk, provider_pk)        │
 │─────────────────────────────────────→│
 │                                      │  3. 检查滑动窗口
 │                                      │  累计支付 > threshold?
 │                                      │
 │  ← ComplianceAction::None            │  (正常，无动作)
 │  或                                  │
 │  ← ComplianceAction::InsertMarker    │  (触发合规审查)
 │     {compliance_hash, threshold}     │
 │                                      │
 │  4. (如触发) 创建合规叶子              │
 │  create_compliance_leaf(...)         │
 │  → 插入 Merkle 树                    │
 │                                      │
 │  5. (如触发) 通道冻结                  │
 │  后续支付被阻止                        │
 │                                      │
 │  6. 合规审查通过后                     │
 │  clear_hold(channel_id)              │
 │─────────────────────────────────────→│
 │  通道恢复正常                          │
```

### 审计追踪

```
每次 LeafUpdate 后:
  record_audit(&leaf_update)
  → 存储: {sequence, leaf_index, new_leaf, timestamp}

查询:
  get_audit_trail(channel_id)
  → Vec<AuditEntry> (按 sequence 排序)
```

## 5. HTTP API 调用

### 查询合规状态

```bash
curl http://localhost:3001/v1/compliance/{channel_id}
```

响应示例：
```json
{
  "channel_id": "hex...",
  "total_spent": 500000000,
  "threshold": 1000000000,
  "window_spent": 200000000,
  "window_slots": 100000,
  "hold_active": false
}
```

### 支付时的合规检查

支付端点 (`/v1/channels/{id}/pay`) 内部自动调用 `record_payment` 并检查合规。如果触发 hold，返回错误：

```json
{
  "error": "ComplianceHold",
  "message": "Spending threshold exceeded, compliance review required"
}
```

## 6. Rust 库调用

```rust
use ignite_pay_state_channel::compliance::{ComplianceManager, SpendingLimit};

let compliance = ComplianceManager::new(db.clone())?;

// 初始化通道合规
compliance.init_channel_compliance(channel_id, SpendingLimit {
    threshold: 1_000_000_000,   // 累计消费阈值
    per_channel: 100_000_000,   // 单通道最大支付
    window_slots: 100_000,      // 滑动窗口
})?;

// 每次支付后记录
let action = compliance.record_payment(
    channel_id,
    payment_amount,
    current_slot,
    user_pubkey,
    provider_pubkey,
)?;

match action {
    ComplianceAction::None => { /* 正常 */ },
    ComplianceAction::InsertMarker { compliance_hash, threshold } => {
        // 创建合规叶子并插入 Merkle 树
        let leaf = ComplianceManager::create_compliance_leaf(compliance_hash, threshold);
        // 通道进入 hold 状态
    },
}

// 清除 hold
compliance.clear_hold(channel_id)?;

// 审计追踪
compliance.record_audit(&leaf_update)?;
let trail = compliance.get_audit_trail(channel_id)?;
```

## 7. 链上操作

合规管理完全离链。合规叶子插入 Merkle 树后，其存在会在结算时的 Merkle proof 中体现。

## 8. 错误处理

| 错误 | 原因 | 处理 |
|:-----|:-----|:-----|
| `ComplianceHold` | 累计支付超阈值，通道冻结 | 等待合规审查 clear_hold |
| `PerChannelExceeded` | 单次支付超过 per_channel 限额 | 拆分为多笔小额支付 |
| `WindowExceeded` | 滑动窗口内支付超限 | 等待窗口滚动 |

## 9. 注意事项

- `[compliance]` 配置段是可选的，不写则禁用合规功能
- 合规仅对 User 和 Hub 角色生效（Provider 不配置合规）
- 滑动窗口基于 Solana slots，不是时间
- `travel_rule_threshold` 用于标识需要 Travel Rule 报告的大额支付
- 审计记录为 append-only，不可删除或修改
- `record_audit` 应在每次 `apply_leaf_update` 后调用
