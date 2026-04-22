# 场景十：自动关闭与 Watchtower

## 1. 场景描述

通道可设置自动关闭时间 (`auto_close_slot`)，到期后任何第三方（Watchtower）都可以触发结算。这保护了离线用户的资金安全，防止对手方提交过时状态。

## 2. 参与角色

| 角色 | 职责 |
|:-----|:-----|
| User | 开通通道时设置 auto_close_slot |
| Watchtower | 监控通道，到期触发结算（可选） |
| Provider | 正常参与结算流程 |

## 3. 前置条件

- 通道已开通
- 配置了 `auto_close_offset`（在 config.toml 中）或开通时指定了 `auto_close_slot`

## 4. 操作流程

```
User                                   Watchtower                 Solana
 │  1. 开通通道时设置                      │                          │
 │  auto_close_slot = slot + offset       │                          │
 │                                        │                          │
 │  ... User 离线 ...                     │                          │
 │                                        │                          │
 │                                        │  2. 监控 auto_close_slot │
 │                                        │  检测到期                 │
 │                                        │                          │
 │                                        │  3. POST /v1/channels/{id}/settle
 │                                        │──────────────────────────→│
 │                                        │  auto_settle(slot)       │
 │                                        │           通道 → Settling │
 │                                        │                          │
 │  4. User 上线后认领叶子                  │                          │
 │  claim + finalize 流程                  │                          │
```

## 5. HTTP API 调用

### 开通通道时设置自动关闭

开通请求中 `auto_close_slot` 自动计算为 `current_slot + auto_close_offset`（配置文件中的值）。

### 触发自动结算

```bash
curl -X POST http://localhost:3001/v1/channels/{channel_id}/settle \
  -H "Content-Type: application/json" \
  -d '{"settle_window": 10000}'
```

Watchtower 也可调用此端点（使用 User 或 Provider 服务地址）。

## 6. Rust 库调用

```rust
// 开通时设置 auto_close_slot
let state = mgr.open_channel(
    &user_pk, &provider_pk, &token_mint,
    1_000_000, 4, current_slot,
    &vault_a, &vault_b, 5000, 1000,
    Some(current_slot + 500_000),  // auto_close_slot
)?;

// 也可后续设置
mgr.set_auto_close_slot(&mut state, Some(target_slot))?;

// Watchtower 触发结算
mgr.auto_settle(&mut state, current_slot, settle_window)?;
```

## 7. 链上操作

自动结算使用 `settle_after_timeout` 指令，与争议超时结算共用同一条链上指令。

| 指令 | 函数 | 说明 |
|:-----|:-----|:-----|
| `settle_after_timeout` | `build_settle_after_timeout_ix` | 验证 auto_close_slot 已过 |

## 8. 错误处理

| 错误 | 原因 | 处理 |
|:-----|:-----|:-----|
| `AutoCloseNotReached` | auto_close_slot 未到期 | 等待到期后再触发 |
| `ChannelNotOpen` | 通道已不在 Open 状态 | 检查当前状态 |
| `ActiveHtlcsExist` | 存在活跃 HTLC（信息提示） | 非阻塞错误，auto_settle 继续执行；HTLC 资金需在结算窗口内通过 `verify_htlc` / `htlc_refund` 单独处理（→ 场景 07） |

## 9. 注意事项

- `auto_close_offset` 在 config.toml 中配置，单位为 slots（500000 ≈ 55.6 小时）
- 设为 0 表示不自动关闭
- auto_settle 跳过 challenge_duration 等待，直接进入 Settling
- Watchtower 可以是任何持续运行的第三方服务，不需要持有密钥
- 建议 User 定期上线检查通道状态，及时认领叶子

---

## 相关场景

| 场景 | 关系 |
|:-----|:-----|
| [01 开通通道](01-channel-open.md) | 前置：通道需配置 `auto_close_offset` |
| [05 协作关闭](05-cooperative-close.md) | 自动结算后的 claim + finalize 流程同协作关闭 |
| [06 争议解决](06-dispute-resolution.md) | 使用相同的 `settle_after_timeout` 链上指令 |
