# Skill: IgnitePay (Agentic Web3 Payment SDK)

## 简介
这是一个基于 **X402** 协议和 **DID (Decentralized Identity)** 的支付技能。它允许 Agent 自动处理支付挑战，支持通过 **DIDComm** 进行离线异步授权，并利用 **Solana ZK Compression** 验证支付边界。

## 核心能力
* **自动支付**: 当遇到商家返回的 402 错误时，自动尝试使用本地白名单策略支付。
* **异步授权**: 如果商家不在白名单或额度不足，自动向用户手机端发起授权请求。
* **白名单管理**: 动态维护商户 DID、支付频次、额度限制及有效期。

## Actions

### pay_merchant
**描述**: 执行 Web3 支付流程。如果支付由于权限问题挂起，该动作会返回 `pending` 状态。
**参数**:
* `merchant_did` (string, **Required**): 目标商家的去中心化身份（例如 `did:solana:6u...`）。
* `amount` (number, **Required**): 拟支付的 SOL 金额。
* `reason` (string, Optional): 支付用途描述（用于在用户手机端显示提醒）。

### check_allowance
**描述**: 查询当前 Agent 对特定商家的剩余支付额度及有效期。
**参数**:
* `merchant_did` (string, **Required**): 商家 DID。

---

## 交互示例 (Examples)

### 场景 1：直接支付（已在白名单）
* **User**: "帮我买一下这个 API 的 10 次调用量，价格是 0.1 SOL，商家的 DID 是 did:solana:abc..."
* **Agent**: *调用 pay_merchant(merchant_did="did:solana:abc", amount=0.1)*
* **Response**: "✅ 支付成功！交易签名：5tYx... 额度已根据本地策略扣除。"

### 场景 2：触发异步授权（不在白名单）
* **User**: "订购这款 AI 插件，支付 0.5 SOL 给 did:solana:xyz。"
* **Agent**: *发现该商家未在白名单，通过 DIDComm 发送消息至手机端。*
* **Response**: "⏳ 商家 did:solana:xyz 尚未授权。我已经向你的手机发送了支付请求，请在手机端确认授权额度后告知我继续。"

### 场景 3：查询额度
* **User**: "我对 did:solana:shop 还有多少支付额度？"
* **Agent**: *调用 check_allowance(merchant_did="did:solana:shop")*
* **Response**: "你目前对该商家的单次限额为 1 SOL，本月剩余可用额度为 4.5 SOL，授权有效期至 2026 年 12 月。"

---

## 开发者说明 (Notes for LLM)
1. **身份锚定**: 所有的支付策略均与用户的 Controller Key 绑定。
2. **异步特性**: `pay_merchant` 可能会进入挂起状态，此时应告知用户检查其绑定的手机移动端。
3. **安全性**: 不要将私钥信息传递给该技能，Rust 内核会安全地通过本地或 Mediator 处理签名。