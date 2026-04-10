**AI Agent + 分布式身份 (DID) + 零知识压缩 (ZK Compression)** 的支付网关设计方案。通过将支付流程与身份认证（DID）深度耦合，并利用 ZK Compression 降低链上数据成本，构建高效、隐私且具备精细化权限管理的系统。

---

## 1. 核心流程架构图

```
Agent → 外部服务商 (402) → MCP Server → Mediator → 手机 App
                                 ↑                        ↓
                          支付决策引擎                用户授权/拒绝
                    (VC验证+链上DID验证+名单+额度)        ↓
                                 ↑              DIDComm Auth Response
                          IPFS 名单同步 ←—————————————┘
                                 ↓
                    Session Key 链上支付 (SOL/SPL Token)
```

---

## 2. 关键环节技术解析

### A. 服务商发现与 X402 协议
X402 协议在此处扮演了"价值交换握手"的角色。
* **触发机制**：当 Agent 请求外部服务商资源而未提供有效凭证时，外部服务商返回 `402 Payment Required` 的扩展版（X402）。MCP Server 解析该响应并启动支付流程。
* **元数据分离**：返回的信息流包含：
  * `accepts[].recipient`：**钱包地址**，用于支付路由（不是 DID）
  * `provider_did`：**商家的 `did:ignite`**（独立字段），用于信誉溯源与黑白名单匹配
  * `accepts[].amount/token/network`：支付金额、代币类型、网络
* **VC 附加**：402 响应可附带平台签发的 Verifiable Credential，用于商家身份背书验证。

### B. 基于 SPL Account Compression 的 DID 管理（V2.0）

V2.0 使用 Solana 链上的 **SPL Account Compression (Concurrent Merkle Tree)** 存储 DID 文档哈希，实现链上可验证的商家身份管理。

* **架构**：
  * **链上数据**：`MerchantLeaf` 叶子节点存储在 Concurrent Merkle Tree 中（maxDepth=14, maxBufferSize=64，支持 ~16K 商家）
  * **叶子字段**：`merchant_did` (SHA-256), `active_pubkey` (收款公钥), `platform_vc_hash`, `slot_updated`
  * **信任链**：商家 → 平台 VC 背书 → 链上 Merkle Proof 验证
* **两层验证**：
  1. **链下快速过滤**：通过 Helius DAS API 获取 Merkle Proof，本地 `verify_proof_locally()` 验证
  2. **链上强制验证**：提交 `verify_leaf` 指令到 Solana，由链上程序验证 proof
* **操作**：
  * 商家入驻：平台调用 `append` 指令添加叶子
  * 密钥轮换：平台调用 `replace_leaf` 更新叶子
  * 验证：通过 IndexerClient 获取 proof + CompressionService 本地验证

### C. Session Keys（V2.0）

临时密钥系统，用于安全执行链上支付：

* **自付模式 (SelfFunded)**：用户预充值 SOL 到临时密钥，临时密钥直接支付
  * 流程：创建 Session → 预充值 SOL → 构建 SOL/SPL 转账 → 签名发送 → 记录花费
* **代付模式 (Sponsored)**：项目方 Relayer 代付 gas（V2.x 后续版本）
  * 流程：构建交易 → 临时密钥部分签名 → 发送到 Relayer → Relayer 追加签名广播
* **风控**：
  * 过期时间检查（`expires_at`）
  * 单次花费额度限制（`spending_limit`）
  * 权限范围限定（`scopes`: `["sol:transfer", "spl:transfer"]`）
* **持久化**：Session 数据通过 borsh 序列化存储在 sled 数据库

### D. 支付决策流程

| 优先级 | 场景 | 判断条件 | 处理动作 |
| :--- | :--- | :--- | :--- |
| 1 | **VC 验证失败** | 附带 VC 签名无效/过期/签发者不匹配 | 拒绝支付，返回验证失败原因 |
| 2 | **链上 DID 验证失败** (V2.0) | 商家 DID 未在 Merkle Tree 注册 | 拒绝支付，返回"merchant not found on-chain" |
| 3 | **黑名单阻断** | `provider_did` 在黑名单 | 立即中断，返回 `Security Risk: Provider Blocked` |
| 4 | **白名单自动批准** | `provider_did` 在白名单 && 金额 ≤ `max_amount` | 直接执行链上支付 |
| 5 | **全局阈值自动批准** | 金额 ≤ `auto_approve_max` | 自动执行链上支付，无需手机授权 |
| 6 | **交互式授权** | 以上均不满足 | 触发 DIDComm V2 协议，推送授权请求至用户手机端 |

**支付执行 (V2.0)**：
* 若 Solana 已配置：通过 Session Key 执行真实 SOL/SPL Token 转账
* 若 Solana 未配置：使用 mock payment 生成模拟签名（开发模式）

---

## 3. 授权路由：DIDComm V2 与中继器

在这种长链路（Agent → MCP → Mediator → Mobile App）中，**中继器 (Mediator)** 的角色至关重要：

1. **异步处理**：Agent 无法长时间等待用户点击手机。MCP Server 使用 oneshot channel + timeout 机制实现异步等待。
2. **DIDComm V2 协议**：确保了跨端消息的端到端加密。关键区分：
   * **平台 VC**：由平台 DID 签发的商家身份背书凭证，用于验证商家合法性。手机端不签发 VC。
   * **授权响应**：手机端签署的是 `payment-auth-response` 消息（包含 payment_id、authorized、list_action），不是 VC。
3. **名单管理**：用户授权时可选择 `list_action`（whitelist/blacklist/none），授权后自动更新 IPFS 上的名单并同步。

---

## 4. 平台 VC 商家背书流程

```
平台 (Platform DID) → 签发 VC → 附加到 402 响应 → MCP Server 验证
                                          ↑
                                   包含商家 DID、名称、
                                   类别、有效期、Ed25519 签名
```

* **签发者**：平台（使用平台 DID 的 Ed25519 私钥签发）
* **验证内容**：签名有效性、VC 未过期、issuer 匹配配置的平台 DID
* **配置**：MCP Server 的 config.toml 中配置 `[platform]` 节（did + verifying_key_b64）

---

## 5. IPFS 名单管理流程

```
手机授权 → list_action != "none"
         → MCP 更新 sled 本地缓存
         → 上传合并名单到 IPFS → 获取新 CID
         → 发送 list-sync-notification 给手机端
```

* **存储结构**：IPFS 上存储 `MerchantLists`（包含 whitelist + blacklist 数组）
* **本地缓存**：sled 数据库中维护两棵 B-tree（`__whitelist__`、`__blacklist__`）
* **IPFS 客户端**：支持 MockIpfsClient（开发）和 KuboIpfsClient（生产）
* **配置**：config.toml 中配置 `[ipfs]` 节

---

## 6. V2.0 Crate 结构

```
ignite-pay-solana/          # 新增：Solana 链上交互 crate
├── src/
│   ├── lib.rs              # 模块声明 + re-export solana_sdk
│   ├── types.rs            # MerchantLeaf, SessionTokenData, PayMode, PaymentResult
│   ├── error.rs            # SolanaError 统一错误类型
│   ├── compression.rs      # CompressionService: Merkle Tree 操作
│   ├── indexer.rs          # IndexerClient: Helius DAS API 查询
│   ├── session.rs          # SessionManager: 临时密钥创建/持久化/验证
│   └── payment.rs          # IgnitePayClient: SOL/SPL Token 真实转账

ignite-pay-core/            # 修改：添加 solana feature gate
├── src/
│   ├── solana_did.rs       # [新增] SolanaDidBridge: DID 链上验证桥接层
│   └── ...                 # (其他模块不变)

ignite-pay-mcp/             # 修改：集成 Solana 模块
├── config.toml             # [新增] [solana] 配置节
└── src/
    └── main.rs             # [修改] 添加 Solana 客户端 + 链上验证
```

---

## 7. Solana 配置

```toml
# config.toml [solana] 节
[solana]
rpc_url = "https://api.devnet.solana.com"
tree_address = ""          # Concurrent Merkle Tree 地址
tree_authority = ""        # 树管理者公钥
das_endpoint = ""          # Helius DAS API endpoint
pay_mode = "self_funded"   # "self_funded" 或 "sponsored"
```

当 `tree_address` 和 `tree_authority` 为空时，系统回退到 mock payment 模式。

---

## 8. 优化建议与潜在挑战

### 1. 状态同步问题
* **挑战**：IPFS 上的黑白名单更新可能有延迟。
* **建议**：在 MCP Server 本地 sled 缓存确保即时查询，IPFS 仅用于跨设备同步。

### 2. 隐私保护
* **建议**：在向中继器发送支付意图时，可使用隐身地址或对交易金额进行混淆，防止中继器掌握消费画像。

### 3. Agent 的重试逻辑
* **流程**：Agent 拿到支付信息后，在 HTTP Header（如 `Authorization: Bearer <Payment_Proof>`）中带上该信息再次请求。
* **容错**：如果支付成功但服务商未返回资源，系统需要基于 `provider_did` 的仲裁或申诉机制。

### 4. 性能考量
* **链下验证**：Merkle Proof 本地验证为毫秒级，不消耗链上资源
* **链上验证**：仅在争议场景使用 `verify_leaf` 指令
* **Session 管理**：sled 持久化，重启后自动恢复活跃 Session

---

## 9. 阶段规划

| 阶段 | 功能 | 状态 |
| :--- | :--- | :--- |
| **V0.1** | 基础 MCP + DIDComm 加密 + Mediator + Mock 支付 | ✅ 已完成 |
| **V1.0** | 手机端授权闭环（Flutter Rust Bridge + WS 双向通信） | ✅ 已完成 |
| **V1.1** | VC 验证 + IPFS 黑白名单 + 名单同步 | ✅ 已完成 |
| **V2.0** | SPL Account Compression + Session Keys + 链上支付 | 🚧 进行中 |
| **V2.1** | 代付模式 (Sponsored) + Relayer 服务 | 📋 计划中 |

---

## 总结

该方案完美契合了 **"Agent Economy" (智能体经济)** 的需求。通过 X402 实现按需付费，通过 VC 验证和 IPFS 名单管理实现信任体系，通过 DIDComm V2 保证用户的最终控制权（Self-Sovereignty）。V1.x 已实现完整的授权闭环，V2.0 通过 SPL Account Compression 实现链上商家 DID 验证，通过 Session Keys 实现安全便捷的链上支付执行。
