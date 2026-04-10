# 产品技术文档：基于 SPL Account Compression 的 Agent 支付身份 (DID) 体系

## 1. 产品定义
本方案旨在为 AI Agent 构建一套高性能、低成本的去中心化身份验证与支付网关。通过 **SPL Account Compression** 技术，将商家信用与支付授权链上化，确保 Agent 在处理 X402 待支付请求时，具备实时风控与自动化决策能力。

## 2. 核心架构设计

### 2.1 存储模型：压缩状态树 (Compressed State)
不同于传统的链上账户存储，本系统利用 Solana 的 `ConcurrentMerkleTree` 存储商家身份。
* **Leaf (叶子节点)**：存储商家 DID 的核心元数据哈希。
    * `Leaf Data = Hash(Merchant_DID, Current_Pubkey, Platform_VC_Hash, Nonce)`
* **Tree Authority**：平台方（如 Ignite-Pay 官方）作为树的管理者，负责签署新商家入驻及更新商家状态。

### 2.2 信任链 (Chain of Trust)
1.  **商家层**：持有 DID 对应的私钥，用于签署支付请求。
2.  **平台层**：对商家进行审核，并在其叶子节点中附加 **Platform Attestation (平台背书)**。
3.  **协议层 (Ignite-Pay)**：在链上校验 Merkle Proof，确保支付只流向受信任的商家。

---

## 3. 业务流程规范

### 3.1 商家准入与状态压缩上链
1.  **申请**：商家生成密钥对，向平台提交 DID 信息。
2.  **背书**：平台审核后，生成一个 VC (Verifiable Credential)。
3.  **上链**：平台调用 `spl-account-compression` 将商家信息作为新叶子节点插入 Merkle Tree。
4.  **索引**：索引器（Indexer）捕获该交易，生成可供查询的 **Merkle Proof**。

### 3.2 密钥轮换 (Key Rotation)
当商家需要更新公钥时：
* 商家需提交由**旧私钥**签署的新公钥声明。
* 平台验证后，调用 `update_leaf` 指令，在原位置更新叶子节点数据。
* DID 保持不变，确保业务连续性。

### 3.3 支付发现与双层验证 (X402 流程)
当 Agent 接收到服务商返回的 X402 待支付信息时：

#### 第一层：链下快速过滤 (支付 Skill)

* **Action**：支付 Skill 从索引器获取商家叶子节点的 Merkle Proof，并从链上获取当前 Tree Root。
* **Validation**：在本地计算 `Proof + Leaf == Root`，并校验平台签名。
* **Result**：若验证失败或在黑名单，直接阻断支付；若在白名单且额度内，自动放行。

#### 第二层：链上强制校验 (ExecutePayment 合约)
* **Action**：Agent 调用 Ignite-Pay 的结算合约进行转账。
* **Constraint**：合约强制要求传入 `Proof` 和 `Leaf Data`。
* **Logic**：合约内部通过 `spl_account_compression::verify_leaf` 确认该商家确实由平台背书。
* **Safety**：如果验证不通过，Solana 交易直接回滚，资金无法转出。

---

## 4. 数据结构与接口定义

### 4.1 压缩叶子节点定义 (Rust 结构)
```rust
struct MerchantLeaf {
    pub merchant_did: [u8; 32],     // 商家 DID 唯一标识
    pub active_pubkey: Pubkey,      // 商家当前收款/签名公钥
    pub platform_vc_hash: [u8; 32], // 平台签发的凭证哈希 (存放在 IPFS)
    pub slot_updated: u64,          // 最后更新高度
}
```

### 4.2 X402 扩展字段规范
服务商返回的响应头需包含：
* `x402-merchant-did`: 商家的标准 DID。
* `x402-payment-address`: 收款地址（需与链上压缩账户中的 `active_pubkey` 一致）。
* `x402-merkle-context`: (可选) 提示 Agent 应该去哪棵树验证身份。

---

## 5. 产品优势
1.  **极致扩展性**：基于 SPL Account Compression，单棵树可支持数百万商家的准入，而平台只需维护一个固定大小的账户。
2.  **强制合规**：通过链上合约校验 Proof，将“平台背书”变成了支付的物理条件，而非简单的逻辑判断。
3.  **隐私保护**：虽然树在链上，但详细的黑白名单管理文档可存储在 IPFS 中，仅对授权的支付 Skill 可见。
4.  **低延迟**：Agent 侧的链下快速过滤保证了“即时支付”的体验，只有最终结算才产生链上开销。

---

## 6. 后续规划
* **V1.0**：完成基于 SPL Account Compression 的商家入驻与链上支付校验程序。
* **V1.1**：集成 **DID-Common V2**，支持手机端与 Agent 之间的异步授权路由。
* **V2.0**：支持多链 DID 映射，允许 Agent 在多链环境下使用统一的身份凭证进行支付。