# Ignite Pay 产品演示脚本

> 录制时长目标：8-10 分钟
> 风格：技术演示 + 产品讲解，边操作边解说
> 设备：一台运行 Docker 服务的开发机 + 一部 Android 手机（或模拟器）

---

## 开场（0:00 - 1:00）

### 画面：PPT 封面

**解说词：**

> 大家好，我是 ___，今天给大家演示 Ignite Pay —— 一个为 Agent 经济构建的去中心化支付基础设施。
>
> 核心场景很简单：AI Agent 在执行任务时遇到付费墙，自动发起支付请求，用户在手机上滑动确认，支付在 Solana 链上完成。整个过程端到端加密，隐私安全。
>
> 接下来我将用 8 分钟展示两个核心流程：Agent 自动支付、手机授权支付。
> 另外还会介绍多路径支付引擎和风控体系的设计。

---

## 第一部分：产品架构速览（1:00 - 2:00）

### 画面：PPT 架构页

**解说词：**

> 先快速过一下架构。Ignite Pay 分四层：
>
> - **应用层**：AI Agent 通过 MCP 协议接入，消费者使用手机 App，商户使用商户 App
> - **服务层**：买家 MCP 提供 23 个工具，商户 MCP 提供 16 个工具，处理支付编排
> - **通信层**：DIDComm v2 端到端加密，中继服务器无法读取明文
> - **链上层**：Solana 结算 + DID 链上身份
>
> 支持五条支付路径：Session Key 链上支付、外部钱包 Deep Link、Relayer 代付、MagicBlock 链下 Voucher、CCTP 跨链 USDC 充值。ZK Compression DID 和状态通道（State Channel, UTXO 模式）为未来规划。
>
> 好了，我们直接进入实机演示。

---

## 第二部分：环境启动（2:00 - 2:30）

### 画面：终端

**操作：** 依次执行以下命令

```bash
# 显示服务健康状态
make health
```

**解说词：**

> 所有后端服务已启动。PostgreSQL、两个 DIDComm Router、DID Registry 全部在线。
>
> 买家 MCP 和电商 Demo 服务器也已就绪。

**操作：** 快速验证电商服务器

```bash
curl -s http://localhost:9090/products | python -m json.tool
```

**画面显示：** JSON 产品列表（coffee 0.0001 SOL、sandwich 0.00025 SOL、juice 0.00015 SOL）

**解说词：**

> 这里有一个模拟电商服务器，实现了 Coinbase x402 协议。商品价格以 lamports 为单位。

---

## 第三部分：演示一 — Agent 自动支付（2:30 - 4:30）

> 场景：金额低于全局自动通过阈值，Agent 无需打扰用户

### 画面：终端

**操作：**

```bash
# Agent 请求付费资源，收到 HTTP 402
curl -v -X POST http://localhost:9090/orders \
  -H "Content-Type: application/json" \
  -d '{"product_id": "coffee"}'
```

**解说词：**

> Agent 尝试购买一杯咖啡。服务器返回 HTTP 402，响应头包含 `PAYMENT-REQUIRED` 和 `x402-merchant-did`。
>
> 这就是 x402 协议的标准格式。Agent 拿到这个支付挑战后，会调用 MCP 处理。

**操作：** 展示 MCP 处理后的日志（可提前准备好）

**画面：** MCP 日志终端窗口

**解说词：**

> MCP 收到挑战后做了几件事：
> 1. 解析 x402 格式，提取金额、收款地址、商户 DID
> 2. 验证商户 Verifiable Credential
> 3. 链上 DID 验证 — 确认商户身份真实存在
> 4. 风控检查 — 金额低于自动通过阈值，直接批准
>
> 因为金额很小，无需用户确认，MCP 直接执行了链上支付。

**操作：** 展示支付结果

```bash
# Agent 携带支付凭证重试请求
curl -s -X POST http://localhost:9090/orders \
  -H "Content-Type: application/json" \
  -H "X-Payment-Proof: <tx_signature>" \
  -d '{"product_id": "coffee"}'
```

**画面显示：** HTTP 200，`{"status": "paid", "order_id": "..."}`

**解说词：**

> Agent 携带链上交易签名重试请求，服务器验证后返回 200，订单确认已支付。整个过程对用户完全透明。

---

## 第四部分：演示二 — 手机授权支付（4:30 - 7:00）

> 场景：金额超过阈值，需要用户在手机上滑动确认

### 画面：终端 + 手机屏幕并排（或分屏录制）

**操作（终端）：**

```bash
# 修改 MCP 配置，将 auto_approve_max 设为 0
# 触发更高金额的支付
curl -v -X POST http://localhost:9090/orders \
  -H "Content-Type: application/json" \
  -d '{"product_id": "sandwich"}'
```

**解说词：**

> 这次购买三明治，金额 0.00025 SOL。由于超过了自动通过阈值或者这是一个新商户，MCP 需要用户授权。
>
> 看手机。

### 画面：手机 App

**操作：**

1. 展示手机 Dashboard 上的 amber 横幅："Payment authorization requested"
2. 点击 "Authorize Payment"
3. ChallengeScreen 弹窗出现

**解说词：**

> 手机收到推送通知。注意，这条消息是通过 DIDComm 端到端加密传输的，中继服务器无法读取内容。
>
> 弹窗显示：商户 DID、支付金额 0.00025 SOL、描述信息。还有风控操作选项：本次授权、加入白名单、加入黑名单。

**操作：**

4. 滑动 "Slide to Authorize" 到 85% 以上
5. 弹出签名方式选择器
6. 选择 "Built-in Key"

**解说词：**

> 滑动确认后，App 会创建一个临时 Session Key 并注册到 Solana 链上。这个 Key 有独立的消费限额和有效期，即使泄露也不会影响主钱包。

**操作：**

7. 等待 "Authorized with session key" 显示
8. 弹窗自动关闭

**画面：** 切回终端

**解说词：**

> MCP 收到授权响应后，通过 Session Key 执行链上转账。来看 MCP 日志。

**画面：** MCP 日志

```
Received payment-auth-response: authorized=true, method=session_key
execute_payment: tx=5Kj7...（交易签名）
Payment proof: tx=5Kj7...
```

**操作：** 查询链上交易

```bash
solana confirm <tx_signature> --url devnet
```

**画面显示：** Confirmed

**解说词：**

> 交易已在 Solana Devnet 上确认。整个流程：Agent 遇到付费墙 → MCP 编排 → 手机授权 → 链上支付 → 完成。

---

## 第五部分：亮点总结与展望（7:00 - 8:00）

### 画面：PPT 总结页

**解说词：**

> 回顾一下今天演示的核心能力：
>
> **第一，Agent 自主支付 + 人类授权。** AI Agent 遇到付费墙自动处理，小额自动通过，大额推送到手机让用户一划确认。
>
> **第二，端到端加密。** 所有 Agent 到手机的通信走 DIDComm v2 JWE 加密，中继服务器零知识。
>
> **第三，多路径支付引擎。** Session Key 链上直接转账、外部钱包 Deep Link、Relayer 代付、MagicBlock 链下 Voucher 亚秒级支付、CCTP 跨链 USDC——五条路径根据场景自动选择最优方案。ZK Compression DID 和状态通道（State Channel, UTXO 模式）为未来规划。
>
> **第四，六层风控体系。** 黑名单 → IPFS CID 黑名单 → 单笔限额 → 白名单自动通过 → IPFS CID 白名单 → 默认推手机授权。
>
> **技术栈：** 全 Rust 后端，22 个 crate，3 个 Solana 链上程序，Flutter + Rust Bridge 移动端，39 个 MCP 工具。

### 画面：PPT 结束页

**解说词：**

> Ignite Pay 正在构建 Agent 经济的支付基础设施。如果你们在构建 AI Agent、接入 x402 协议、或者需要去中心化微支付能力，欢迎交流。
>
> 谢谢大家。

---

## 录制注意事项

| 事项 | 说明 |
|------|------|
| 分辨率 | 终端建议 1080p，手机建议 720p（文件不要太大） |
| 分屏方案 | 左侧终端，右侧手机模拟器；或后期拼接 |
| 网络准备 | 提前完成 Devnet 空投，避免录制时等待 |
| 备用方案 | 提前录制好 MCP 日志输出和链上确认截图，防止录制时网络抖动 |
| 节奏控制 | 每个演示环节控制在 2 分钟内，出现等待时加速剪辑 |
| 字幕 | 建议后期添加关键字幕（如 "HTTP 402"、"DIDComm 加密"、"Session Key 注册"） |
