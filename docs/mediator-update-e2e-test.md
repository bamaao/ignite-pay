# Mediator 地址更新 — 端到端测试指南

本文档描述 DID 配对建立后，App 或 MCP 更换 mediator 地址时的 `mediator-update` 同步机制及可执行测试步骤。

**关联文档：**
- [DIDComm 配对流程](didcomm-pairing-flow.md) — 首次配对与消息路由架构
- [手动测试演练手册](manual-test-walkthrough.md) — T1.5 Mediator 地址更新
- [Agent 支付 E2E 测试](agent-payment-e2e-test-guide.md) — 支付授权链路验证

---

## 1. 背景与动机

配对建立的是 **DID 信任关系**，不是把 mediator 地址写死。配对时双方交换 `mediator_http_url` 用于后续 HTTP forward 路由：

- App 存 `PairedMcp.mediatorHttpUrl`（MCP 的 mediator）
- MCP 存 `phone_mediator_http_url`（App 的 mediator）

若一方更换 mediator 后只改本地配置、不通知对端，旧配对的消息路由会中断。`mediator-update` 消息用于在 **不重配对的条件下** 同步新的 mediator 地址。

---

## 2. 协议说明

### 2.1 消息类型

| 字段 | 值 |
|------|-----|
| 类型 | `https://didcomm.org/ignite-pay/1.0/mediator-update` |
| 方向 | App ↔ MCP（双向） |
| 传输 | JWE 加密，经 forward 包装；mediator 转发时可能为明文 JSON |
| Body | `mediator_http_url`（必填）、`mediator_ws_url`（可选） |

### 2.2 触发时机

| 端 | 何时发送 |
|----|----------|
| App | `connectToMediator` 成功后，向所有已配对 MCP 发送 |
| App | Vault / 设置页保存新 Mediator URL 并重连后 |
| MCP | 每次 WS 连上 mediator、完成 handshake 后，向已配对 App 发送 |

### 2.3 接收处理

| 端 | 行为 |
|----|------|
| App | 更新 `PairedMcp.mediatorHttpUrl`，写入 SharedPreferences |
| MCP | 更新内存中的 `phone_mediator_http_url`，写入 sled（`__phone_mediator_http_url__`） |

仅接受 **已配对 DID** 发来的更新；未配对 DID 的更新会被忽略并打 warn 日志。

### 2.4 时序（App 更换 mediator）

```
App                          App Mediator          MCP                    MCP Mediator
 │                                │                  │                          │
 │── connect 新 mediator ────────>│                  │                          │
 │── mediator-update (JWE) ───────>│── forward ──────>│                          │
 │                                │                  │── 更新 phone_mediator ──│
 │                                │                  │                          │
 │ [后续 MCP→App 消息走新地址]     │                  │                          │
```

### 2.5 已知限制

若 **App 与 MCP 的 mediator 同时更换**，且双方都无法通过旧 mediator 触达对端，会陷入路由失联。此时需至少一端仍可达（旧 mediator 未下线），或通过 QR 重新配对。这是点对点 forward 架构的固有限制。

---

## 3. 前置条件

### 3.1 服务

| 服务 | 默认端口 | 说明 |
|------|----------|------|
| DIDComm Router（用户） | 8080 | 用户 App / ignite-pay-mcp |
| DIDComm Router（商户） | 4000 | 商户 App / merchant-mcp |
| ignite-pay-mcp | — | 用户 Agent MCP |
| ignite-pay-merchant-mcp | — | 商户 Agent MCP |

测试「换 mediator」场景时，建议准备 **两个不同端口的 mediator 实例**（例如 `:8080` 与 `:8081`），模拟迁移前后地址。

```bash
# 示例：docker-compose 中已有 user mediator :8080
# 临时启动第二个实例（按实际部署方式调整）
# 确保 WS 路径为 /ws，HTTP 根路径可 POST forward
```

### 3.2 配置格式

`mediator_http_url` 必须为 **HTTP POST 根路径**，不含 `/ws` 后缀：

| WS URL | 对应 HTTP URL |
|--------|---------------|
| `ws://192.168.0.102:8080/ws` | `http://192.168.0.102:8080/` |
| `wss://mediator.example.com/ws` | `https://mediator.example.com/` |

### 3.3 配对状态

- **用户链路**：T1.3 已完成（用户 App ↔ ignite-pay-mcp）
- **商户链路**：T1.4 已完成（商户 App ↔ merchant-mcp）

---

## 4. 测试场景

### 4.1 场景 A — 用户 App 更换 Mediator

**目标**：App 改 mediator 后，MCP 仍能向 App 发 `payment-auth-request`。

**前置**：T1.3 配对完成；记录当前 App mediator 为 `M1`，备用 mediator 为 `M2`。

| 步骤 | 操作 | 预期 |
|------|------|------|
| A1 | 用户 App → Vault → Mediator Endpoint，改为 `M2` 的 WS URL，保存 | App 重连 `M2` |
| A2 | 查看 App 日志 | `Sent mediator-update to did:ignite:...` |
| A3 | 查看 ignite-pay-mcp 日志 | `Updated phone mediator HTTP URL for ...` |
| A4 | 触发 x402 支付（或 MCP 工具发 auth 请求） | 手机仍收到授权弹窗 |
| A5 | 完成授权 | MCP 收到 `payment-auth-response`，支付继续 |

**通过标准**：
- [ ] MCP sled / 日志中 `phone_mediator_http_url` 指向 `M2` 的 HTTP 地址
- [ ] 支付授权消息往返正常

**日志关键字（MCP）**：
```
Updated phone mediator HTTP URL for did:ignite:...
Sent mediator-update to paired phone ...   # 仅 MCP 重启场景
```

**日志关键字（App）**：
```
Sent mediator-update to did:ignite:...
WsClient connected to ws://...M2.../ws
```

---

### 4.2 场景 B — ignite-pay-mcp 更换 Mediator

**目标**：MCP 改 config 并重启后，App 更新 MCP 的 mediator 地址，App→MCP 消息仍可达。

**前置**：T1.3 配对完成；MCP 当前 mediator 为 `M1`，目标为 `M2`。

| 步骤 | 操作 | 预期 |
|------|------|------|
| B1 | 修改 `ignite-pay-mcp/config.toml` 中 `mediator.ws_url` 为 `M2` | — |
| B2 | 重启 ignite-pay-mcp | MCP 连上 `M2` |
| B3 | 查看 MCP 日志 | `Sent mediator-update to paired phone ...` |
| B4 | 查看 App 日志 | `Updated MCP mediator URL for did:ignite:...` |
| B5 | App 侧发起需 MCP 响应的操作（如 connection 相关或授权回复） | 消息送达 MCP |

**通过标准**：
- [ ] App `PairedMcp.mediatorHttpUrl` 更新为 `M2` 的 HTTP 地址
- [ ] App→MCP forward 不再发往旧 `M1`

**验证 App 持久化**（可选）：
```bash
# Android: adb shell run-as ... 或 SharedPreferences 调试
# 键名：paired_mcps → mediatorHttpUrl 字段
```

---

### 4.3 场景 C — 商户 App 更换 Mediator

**目标**：商户 App 改 mediator 后，merchant-mcp 仍能推送收款确认。

**前置**：T1.4 配对完成。

| 步骤 | 操作 | 预期 |
|------|------|------|
| C1 | 商户 App → 设置 → Mediator WS，改为新地址，保存 | 自动 disconnect + reconnect |
| C2 | 查看商户 App 日志 | `Sent mediator-update to ...` |
| C3 | 查看 merchant-mcp 日志 | `Updated app mediator HTTP URL for ...` |
| C4 | 模拟 buyer 支付 / MCP 发送 `channel-payment-confirm` | 商户 App 收到确认、订单更新或语音播报 |

**通过标准**：
- [ ] merchant-mcp `phone_mediator_http_url` 已更新
- [ ] 收款确认推送正常

---

### 4.4 场景 D — merchant-mcp 更换 Mediator

**目标**：merchant-mcp 重启后，商户 App 自动更新 MCP mediator 地址。

| 步骤 | 操作 | 预期 |
|------|------|------|
| D1 | 修改 `ignite-pay-merchant-mcp/config.toml` 中 `mediator.ws_url` | — |
| D2 | 重启 merchant-mcp | — |
| D3 | 查看 merchant-mcp 日志 | `Sent mediator-update to paired app ...` |
| D4 | 查看商户 App 日志 | `Updated MCP mediator URL for ...` |
| D5 | 商户 App 发送 `connection-request` 类消息或接收 MCP 下行 | 路由正确 |

---

### 4.5 场景 E — 安全：未配对 DID 的更新被拒绝

**目标**：伪造的 `mediator-update` 不能篡改路由。

| 步骤 | 操作 | 预期 |
|------|------|------|
| E1 | 向 MCP mediator POST 伪造 forward，from 为未配对 DID | MCP 日志：`Ignoring mediator-update from unpaired DID` |
| E2 | 检查 sled 中 `phone_mediator_http_url` | 未被修改 |

---

## 5. 故障排查

| 现象 | 可能原因 | 排查 |
|------|----------|------|
| App 发不出 mediator-update | 未连上新 mediator 或 `_pairedMcps` 为空 | 确认 `WsClient connected` / `isConnected=true` |
| MCP 收不到更新 | App 仍走旧 WS，或 forward URL 错误 | 检查 HTTP URL 是否以 `/` 结尾、无 `/ws` |
| MCP 忽略更新 | from DID 与 `__paired_phone__` 不一致 | 对比配对 DID |
| 更新成功但消息仍失败 | 对端 mediator 未启动或防火墙 | `curl -X POST http://host:port/ -d '{}'` 测连通 |
| 双方同时换地址 | 旧 mediator 均已下线 | 需保留一端旧地址或重新扫码配对 |

---

## 6. 关键代码位置

| 组件 | 文件 |
|------|------|
| 协议 | `ignite-pay-core/src/didcomm.rs` — `build_mediator_update` |
| 用户 MCP | `ignite-pay-mcp/src/mediator.rs` |
| 商户 MCP | `ignite-pay-merchant-mcp/src/mediator.rs` |
| 用户 App Rust | `ignite_pay_app/rust/src/api/ws_client.rs`, `simple.rs` |
| 用户 App Dart | `ignite_pay_app/lib/services/didcomm_service.dart`, `vault_screen.dart` |
| 商户 App Rust | `ignite_pay_merchant_app/rust/src/api/merchant_didcomm.rs` |
| 商户 App Dart | `ignite_pay_merchant_app/lib/services/merchant_push_service.dart`, `settings_screen.dart` |

---

## 7. 测试结果跟踪

| 测试 ID | 场景 | 链路 | 结果 | 备注 | 日期 |
|---------|------|------|------|------|------|
| MU-A | 用户 App 换 mediator | App → MCP | | | |
| MU-B | ignite-pay-mcp 换 mediator | MCP → App | | | |
| MU-C | 商户 App 换 mediator | App → MCP | | | |
| MU-D | merchant-mcp 换 mediator | MCP → App | | | |
| MU-E | 未配对 DID 拒绝 | 安全 | | | |
