# 状态通道业务场景总览

## 场景分类

| # | 业务场景 | 文档 | 涉及角色 |
|:--|:---------|:-----|:---------|
| 1 | 通道开通与注资 | [01-channel-open.md](01-channel-open.md) | User, Provider |
| 2 | 离链支付与拆分 | [02-offchain-payment.md](02-offchain-payment.md) | User, Provider |
| 3 | 批量支付与原子性操作 | [03-batch-pipeline.md](03-batch-pipeline.md) | User |
| 4 | HTLC 条件支付 | [04-htlc-payment.md](04-htlc-payment.md) | User, Provider |
| 5 | 协作关闭通道 | [05-cooperative-close.md](05-cooperative-close.md) | User, Provider |
| 6 | 争议解决 | [06-dispute-resolution.md](06-dispute-resolution.md) | User, Provider |
| 7 | HTLC 结算与退款 | [07-htlc-settlement.md](07-htlc-settlement.md) | User, Provider |
| 8 | Hub 路由网络 | [08-hub-routing.md](08-hub-routing.md) | Hub |
| 9 | 多跳支付 | [09-multihop-payment.md](09-multihop-payment.md) | User, Hub, Provider |
| 10 | 自动关闭与 Watchtower | [10-auto-close.md](10-auto-close.md) | User, Provider |
| 11 | 合规管理与审计 | [11-compliance-audit.md](11-compliance-audit.md) | User, Provider |
| 12 | WebSocket 实时通信 | [12-websocket.md](12-websocket.md) | User, Provider, Hub |

## 角色说明

| 角色 | 二进制 | 说明 |
|:-----|:-------|:-----|
| **User** (用户) | `channel-user` | 支付发起方，管理自己的通道和 UTXO |
| **Provider** (商户) | `channel-provider` | 支付接收方，配签并接受支付 |
| **Hub** (路由中继) | `channel-hub` | 继承 Provider 所有功能，额外提供路由发现和多跳中继 |

## 全局约定

- 所有 `{id}` 为 64 位十六进制字符串（32 字节 channel_id）
- 金额单位为 SPL Token 最小单位（如 USDC 6 位小数时，1000000 = 1 USDC）
- slot 时间：1 slot ≈ 400ms（Solana Mainnet），Devnet 可能更慢
- 签名算法：Ed25519（ed25519-dalek v1）
- 离链消息格式：`SHA-256(channel_id || sequence || ...)` 各场景具体说明
- HTTP 接口统一返回 JSON，错误格式：`{"error": "<code>", "message": "<描述>"}`
