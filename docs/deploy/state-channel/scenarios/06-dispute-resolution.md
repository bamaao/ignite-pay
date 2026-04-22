# 场景六：争议解决

## 1. 场景描述

当一方不响应或提交过时状态时，另一方可以在链上发起争议 (challenge)。争议期间，对手方可以提交更新的反状态 (counter-state)。若超时未回应，通道直接进入结算。

## 2. 参与角色

| 角色 | 职责 |
|:-----|:-----|
| Challenger | 提交争议，提供签名状态 |
| Counterparty | 在争议期内提交反状态（可选） |

## 3. 前置条件

- 通道状态为 `Open`
- 距上次状态更新已过 `min_challenge_delay` slots
- Challenger 持有有效的签名状态（sequence 高于当前链上记录）

## 4. 操作流程

```
Challenger                             Counterparty                    Solana
 │                                        │                              │
 │  1. 签名争议消息                        │                              │
 │  sign(channel_id || slot || root)      │                              │
 │                                        │                              │
 │  2. POST /v1/channels/{id}/challenge   │                              │
 │────────────────────────────────────────────────────────────────────────→│
 │  trigger_challenge                     │               通道 → Challenged│
 │  build_trigger_challenge_ix            │                              │
 │                                        │                              │
 │       === challenge_duration 倒计时 === │                              │
 │                                        │                              │
 │                                        │  3a. (可选) 提交反状态        │
 │                                        │  submit_counter_state         │
 │                                        │  build_submit_counter_state_ix│
 │                                        │─────────────────────────────→│
 │                                        │              验证 sig_a+sig_b │
 │                                        │                              │
 │  3b. 超时无反状态                       │                              │
 │  POST /v1/channels/{id}/settle         │                              │
 │  settle_after_timeout                  │                              │
 │────────────────────────────────────────────────────────────────────────→│
 │                                        │               通道 → Settling │
 │                                        │                              │
 │  4. 正常 claim + finalize 流程          │                              │
```

## 5. HTTP API 调用

### 发起争议

```bash
curl -X POST http://localhost:3001/v1/channels/{channel_id}/challenge \
  -H "Content-Type: application/json" \
  -d '{
    "submitted_root": "hex编码的32字节Merkle根",
    "submitted_sequence": 5
  }'
```

### 提交反状态

```bash
curl -X POST http://localhost:3002/v1/channels/{channel_id}/submit-counter \
  -H "Content-Type: application/json" \
  -d '{
    "sig_a": "hex编码的64字节签名A",
    "sig_b": "hex编码的64字节签名B"
  }'
```

### 超时结算

```bash
curl -X POST http://localhost:3001/v1/channels/{channel_id}/settle \
  -H "Content-Type: application/json" \
  -d '{"settle_window": 10000}'
```

## 6. Rust 库调用

```rust
// 发起争议
mgr.trigger_challenge(
    &mut state,
    &challenger_pubkey,
    current_slot,
    &submitted_root,
    submitted_sequence,
    &challenger_signature,
)?;

// 提交反状态
let counter_state = SignedState { channel_id, sequence: higher_seq, root, sig_a, sig_b };
mgr.submit_counter_state(&mut state, &counter_state, None, &user_pk, &provider_pk)?;

// 超时后结算
mgr.settle_after_timeout(&mut state, current_slot, settle_window)?;
```

## 7. 链上操作

| 指令 | 函数 | 说明 |
|:-----|:-----|:-----|
| `trigger_challenge` | `build_trigger_challenge_ix` | 记录争议 slot 和提交的根 |
| `submit_counter_state` | `build_submit_counter_state_ix` | 验证更高 sequence 的双签状态 |
| `settle_after_timeout` | `build_settle_after_timeout_ix` | challenge_duration 过期后进入 Settling |

## 8. 错误处理

| 错误 | 原因 | 处理 |
|:-----|:-----|:-----|
| `ChallengeTooEarly` | 距上次更新未过 min_challenge_delay | 等待更多 slots |
| `InvalidSequence` | submitted_sequence 不高于当前 | 提交更高 sequence 的状态 |
| `InvalidSignature` | 签名验证失败 | 确认签名对应正确的公钥 |
| `NoActiveChallenge` | settle_after_timeout 但无争议 | 先触发 challenge |
| `CounterStateExpired` | 反状态在 challenge_duration 之后提交 | 已进入超时结算 |

## 9. 注意事项

- 争议签名消息格式：`channel_id || current_slot || submitted_root`
- `min_challenge_delay` 防止 front-running 攻击（不可过早发起争议）
- `submit_counter_state` 要求 sig_a + sig_b 两个签名，证明双方同意该状态
- 超时结算后，流程与协作关闭相同：claim 叶子 → finalize
- 在 `Challenged` 状态期间（`challenge_duration` 倒计时内），也可执行 HTLC 认领或退款操作（截止时间为 `challenge_slot + challenge_duration`）
- Challenger 的签名使用 `ed_kp.sign(msg)` 生成，消息中包含 slot 防止重放

---

## 相关场景

| 场景 | 关系 |
|:-----|:-----|
| [01 开通通道](01-channel-open.md) | 前置：需要已开通的通道 |
| [04 HTLC 支付](04-htlc-payment.md) | Challenged 状态下的 HTLC 操作 |
| [05 协作关闭](05-cooperative-close.md) | 争议超时后流程同协作关闭（claim + finalize） |
| [07 HTLC 结算](07-htlc-settlement.md) | 争议窗口内 HTLC 链上结算 |
| [10 自动关闭](10-auto-close.md) | 使用相同的 `settle_after_timeout` 链上指令 |
