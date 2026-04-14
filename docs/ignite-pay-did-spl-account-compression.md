# 产品技术文档：基于 SPL Account Compression 的 Agent 支付身份 (DID) 体系

## 1. 产品定义
本方案旨在为 AI Agent 构建一套高性能、低成本的去中心化身份验证与支付网关。通过 **SPL Account Compression** 技术，将商家信用与支付授权链上化，确保 Agent 在处理 X402 待支付请求时，具备实时风控与自动化决策能力。

## 2. 核心架构设计

### 2.1 存储模型：压缩状态树 (Compressed State)
不同于传统的链上账户存储，本系统利用 Solana 的 `ConcurrentMerkleTree` 存储商家身份。
* **Leaf (叶子节点)**：存储商家 DID 的核心元数据哈希。
    * `Leaf Data = Hash(Merchant_DID_Hash, Active_Pubkey, Platform_VC_Hash, Slot_Updated)`
    * 其中 `Merchant_DID_Hash` 为商家 DID 公钥的 32 字节 SHA-256 哈希（从 `did:ignite` 字符串提取 Ed25519 公钥后哈希），`Active_Pubkey` 为商家 Solana 收款地址（与 DID 文档中的 Ed25519 签名公钥无关）
* **Tree Authority**：平台方（如 Ignite-Pay 官方）作为树的管理者，负责签署新商家入驻及更新商家状态。

### 2.2 信任链 (Chain of Trust)
1.  **商家层**：持有 DID 对应的私钥，用于签署支付请求。
2.  **平台层**：对商家进行审核，并在其叶子节点中附加 **Platform Attestation (平台背书)**。
3.  **协议层 (Ignite-Pay)**：在链上校验 Merkle Proof，确保支付只流向受信任的商家。

---

## 3. 业务流程规范

### 3.1 商家准入与状态压缩上链
1.  **申请**：商家生成 `did:ignite` 密钥对和独立的 Solana 收款密钥对，向平台提交 DID 信息和服务元数据。
2.  **背书**：平台审核后，签发 Verifiable Credential (VC)，包含商家 DID、有效期、服务类型等声明（VC 结构详见 `ignite-pay-did.md` §4.5）。
3.  **上链**：平台将 `MerchantLeaf { merchant_did_hash, active_pubkey, platform_vc_hash, status, slot_updated }` 作为新叶子节点插入链上 Merkle Tree。
4.  **索引**：索引器（Indexer）捕获该交易，生成可供查询的 **Merkle Proof**。
5.  **交付**：平台将 VC 返回给商家，商家在后续 X402 响应中附带 VC（直接嵌入或 IPFS CID 引用）。

### 3.2 密钥轮换 (Key Rotation)
当商家需要更新 Solana 收款地址时：
* 商家需提交由 **DID 私钥**（即 `did:ignite` 中的 Ed25519 签名密钥）签署的新收款地址声明。
* 平台验证签名后，调用 `update_leaf` 指令，在原位置更新 `active_pubkey` 字段。
* DID 保持不变（`merchant_did_hash` 不变），确保业务连续性。

### 3.3 支付发现与双层验证 (X402 流程)
当 Agent 接收到服务商返回的 X402 待支付信息时：

#### 第一层：链下快速过滤 (支付 Skill)

本层结合 VC 验证（详见 `ignite-pay-did.md` §4.5）与链上 Merkle Proof 验证，确保商家同时具备平台背书和链上记录：

1. **VC 验证**：从 402 响应中提取商家 VC，使用内置平台公钥验证 VC 签名和有效期。
2. **Merkle Proof 验证**：从索引器获取商家叶子节点的 Merkle Proof，从链上获取当前 Tree Root，本地计算 `Proof + Leaf == Root`。
3. **一致性校验**：验证 VC 中的 `credentialSubject.id` 与叶子节点中的 `merchant_did_hash` 对应同一商家。
* **Result**：若任一验证失败或在黑名单，直接阻断支付；若全部通过且在白名单额度内，自动放行。

#### 第二层：链上强制校验 (ExecutePayment 合约)
* **Action**：Agent 调用 Ignite-Pay 的结算合约进行转账。
* **Session Key**：合约要求使用用户在手机端授权时创建的链上 Session Key 签名。Session Key 由手机端通过 DIDComm 授权流程注册到链上（绑定 owner、spending_limit、scopes、expires_at）。MCP/Skill 收到授权响应后，使用该 Session Key 代表用户签名链上交易。
* **Constraint**：合约强制要求传入 `Proof` 和 `Leaf Data`，以及有效的 Session Key 签名。
* **Logic**：合约内部通过 `spl_account_compression::verify_leaf` 确认该商家确实由平台背书，同时验证 Session Key 的有效性（未过期、额度未超限、scope 匹配）。
* **Safety**：如果任一验证不通过（商家验证失败、Session Key 无效/过期/超限），Solana 交易直接回滚，资金无法转出。

---

## 4. 数据结构与接口定义

### 4.1 压缩叶子节点定义 (Rust 结构)
```rust
struct MerchantLeaf {
    pub merchant_did_hash: [u8; 32],   // 商家 DID 公钥的 SHA-256 哈希（从 did:ignite 字符串提取 Ed25519 公钥后哈希）
    pub active_pubkey: Pubkey,          // 商家 Solana 收款地址（非 DID 文档中的 Ed25519 签名公钥，用于接收支付）
    pub platform_vc_hash: [u8; 32],     // 平台签发 VC 的 SHA-256 哈希，计算方式: SHA-256(canonical_json(VC))
    pub status: u8,                     // 商家状态: 0=active, 1=suspended, 2=revoked
    pub slot_updated: u64,              // 最后更新高度
}
```

### 4.2 X402 扩展字段规范
服务商返回的响应头需包含：
* `x402-merchant-did`: 商家的 `did:ignite` 标识符（用于黑白名单匹配，详见 `ignite-pay-did.md` §4.2）。
* `x402-payment-address`: 商家 Solana 收款地址（需与链上压缩账户中的 `active_pubkey` 一致）。
* `x402-merkle-context`: (可选) 链上 Merkle Tree 地址，提示 Agent 应该去哪棵树验证身份。

---

## 5. 产品优势
1.  **极致扩展性**：基于 SPL Account Compression，单棵树可支持数百万商家的准入，而平台只需维护一个固定大小的账户。
2.  **强制合规**：通过链上合约校验 Proof，将“平台背书”变成了支付的物理条件，而非简单的逻辑判断。
3.  **隐私保护**：虽然树在链上，但详细的黑白名单管理文档可存储在 IPFS 中，仅对授权的支付 Skill 可见。
4.  **低延迟**：Agent 侧的链下快速过滤保证了“即时支付”的体验，只有最终结算才产生链上开销。

---

## 6. 后续规划
> 以下版本规划与 `ignite-pay-did.md` §8 保持一致，两份文档共同描述同一系统的演进路线。

* **V0.1** (当前)：`did:ignite` 本地身份 + DIDComm V2 通信 + Mock 支付 + MCP Server。
* **V1.0**：手机端 DIDComm 授权链路 + Session Key 链上注册（手机端授权时创建）+ SPL Account Compression 商家上链 + 链上身份验证程序。
* **V1.1**：VC 商家背书 + IPFS 黑白名单 + 手机端名单管理 + sled 本地缓存风控决策 + 链上支付合约（使用 Session Key 签名）。
* **V2.0**：Solana 链上支付集成（Session Key 驱动） + 多链 DID 映射，允许 Agent 在多链环境下使用统一的身份凭证进行支付。