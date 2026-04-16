**《基于 ZK Compression 的商户 DID 身份系统开发设计指南》**

这份文档可以直接作为技术规格书（Spec）交付给开发团队。

---

# 商户 DID 身份系统开发设计指南 (V1.0)

## 1. 核心架构概述
本方案采用 **ZK Compression** 技术在 Solana 上实现低成本、高自主权的商户身份管理。
* **DID (Decentralized Identifier)**: 商户的永久身份标识。
* **VC (Verifiable Credential)**: 平台对商户的信用背书。
* **ZK Compression**: 用于将 DID 和 VC 数据压缩存储在状态树中，极大降低存储费用。

---

## 2. 身份标识与密钥体系
为了确保安全与灵活性，系统采用三层密钥结构：

| 密钥类型 | 定义 | 存储位置 | 作用 |
| :--- | :--- | :--- | :--- |
| **Original Public Key (Root)** | 商户注册时的 Solana 地址 | 链上永久 ID | 作为 DID 的锚点，不可更改。 |
| **Controller Key** | 纯 Ed25519 密钥对 | 商户本地/离线 | 拥有 DID 文档的修改权（“锁”的钥匙）。 |
| **Recovery Key** | 备份 Ed25519 密钥对 | 离线冷存储 | 当 Controller Key 丢失时重置权限。 |

---

## 3. 业务流程 (Workflow)

### 3.1 身份初始化
1. 商户在本地生成 `Controller Key` 和 `Recovery Key`。
2. 根据 `Original Public Key` 派生 DID：`did:sol:platform:[Original_PK]`。

### 3.2 平台签发 VC (Verifiable Credential)
1. 商家向平台提交身份证明资料及自己的 `Original Public Key`。
2. 平台审核通过后，签发结构化凭证（VC）。
3. **关键安全点**：VC 载荷中必须包含 `subject: [Original_PK]`，将凭证与特定商户地址死锁。

### 3.3 商户自主上链 (On-chain Registration)
1. 商家调用指令将 VC 写入其 ZK 压缩账户。
2. **校验逻辑**：
   * 验证交易发起者（Signer）是否为该商户的 `Original Public Key`。
   * 验证 VC 中的 `subject` 是否与该 `Signer` 一致。
   * 验证 VC 是否带有平台的合法数字签名。

---

## 4. 安全防护方案：防止“冒充上链”
针对“别有用心的人使用他人 VC 上链”或“商家用错账户上链”的风险，实施以下校验：

### A. 持有者绑定 (Holder Binding)
合约在执行 `update_did` 指令时，必须解析 VC 明文数据：
* **规则**：`Signer.key == VC.payload.subject`。
* **目的**：确保 VC 是“实名制”的，攻击者无法使用偷来的 VC 绑定到自己的账户。

### B. 确定性地址派生 (PDA Derivation)
利用 ZK Compression 的索引特性，将商户数据存储在基于其公钥计算出的固定位置：
* **计算公式**：`Index = Hash(Program_ID + Original_PK)`。
* **目的**：确保每个商户在状态树中只有一个合法的“坑位”，无法抢注他人位置。

---

## 5. ZK 压缩账户结构定义 (Rust 示例)
在合约中，商户的压缩数据定义如下：

```rust
pub struct MerchantCompressedDid {
    pub original_pk: Pubkey,     // 初始锚点公钥
    pub controller_pk: Pubkey,   // 当前控制器公钥 (Ed25519)
    pub recovery_pk: Pubkey,     // 恢复公钥
    pub vc_hash: [u8; 32],       // 平台 VC 的哈希值
    pub last_updated: i64,       // 最后更新时间戳
    pub nonce: u64,              // 防重放计数器
}
```

---

## 6. 开发实施路线图
1. **合约开发 (Anchor)**：
   * 定义 `MerchantCompressedDid` 结构。
   * 实现 `initialize_did` 指令（基于 PDA 派生）。
   * 实现 `update_did_with_vc` 指令（包含签名验证和 Subject 匹配）。
2. **SDK 开发 (Typescript/Rust)**：
   * 提供本地生成 Ed25519 密钥对的工具。
   * 提供构造带有 VC 数据和签名的上链交易函数。
3. **平台后端**：
   * 实现符合 W3C 标准的 VC 签发逻辑。

