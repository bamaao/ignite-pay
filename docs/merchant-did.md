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
2. 根据 `Original Public Key` 派生 DID：`did:ignite:platform:[Original_PK]`。

### 3.2 平台签发 VC (Verifiable Credential)
1. 商家向平台提交身份证明资料及自己的 `Original Public Key`。
2. 平台审核通过后，签发结构化凭证（VC）。
3. **关键安全点**：VC 载荷中必须包含 `subject: [Original_PK]`，将凭证与特定商户地址死锁。
4. **DID 签名验证**：商家必须用 DID 私钥对请求签名（`issue_vc:{did}:{merchant_name}:{nonce}`），平台验证后才签发 VC，确保请求者确实持有该 DID。
5. **更新场景**：若商家已注册，平台还会校验签名者是当前 controller 或 original key，防止未授权者请求新 VC。

### 3.3 商户自主上链 (On-chain Registration)
1. 商家调用指令将 VC 写入其 ZK 压缩账户。
2. **校验逻辑**：
   * 验证交易发起者（Signer）是否为该商户的 `Original Public Key`。
   * 验证 VC 中的 `subject` 是否与该 `Signer` 一致。
   * 验证 VC 是否带有平台的合法数字签名。
3. **两种上链模式**：
   * **Sponsored（平台代付）**：平台签名并发送交易，商户无需 Solana 私钥，平台记录服务费。
   * **SelfOnchain（商户自助）**：商户通过公开 `POST /v1/proof` 端点获取 ZK proof，本地构建交易并签名广播。或直接用 `light-sdk` + 自建 Photon RPC 完全独立。广播后需调用 `POST /v1/merchants/confirm` 通知平台。

---

## 4. 安全防护方案：防止"冒充上链"
针对"别有用心的人使用他人 VC 上链"或"商家用错账户上链"的风险，实施以下校验：

### A. 链上平台签名验证（已实现）

链上程序存储平台 Ed25519 公钥（`PlatformConfig` PDA，seeds: `[b"platform-config"]`），在 `initialize_did` 和 `update_did_with_vc` 指令中验证平台签名：

* **签名消息**：`credential_subject_pk (32 bytes) || vc_hash (32 bytes)` = 64 字节
* **验证逻辑**：`verify(platform_pubkey, credential_subject_pk || vc_hash, platform_signature)`
* **目的**：确保 vc_hash 由平台授权，攻击者无法自行伪造 VC 上链

### B. Subject Binding — 链上强制（已实现）

链上指令额外接收 `credential_subject_pk: Pubkey` 参数，并强制校验：

* **规则**：`credential_subject_pk == signer.key()`
* **目的**：VC 主体必须是交易提交者。攻击者即使拦截了平台签名，也无法用不同账户上链——subject binding 检查会先行拒绝。

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
## 6. 商家建立DID业务流程详细

**商家拥有完全的自主权**，而**平台只负责信用背书**。

通过这种方式，商家不需要向平台交付任何私钥权限，而是通过“证明（Proof）”的方式完成上链。

---

### 1. 业务流程：三步走架构

#### **第一步：商家创建 DID (本地生成)**
商家在本地（如 SDK 或离线环境）生成一对 **Ed25519** 密钥：
* **私钥**：由商家自己妥善保管，绝不外传。
* **公钥**：作为 DID 文档的 `Verification Method`（验证方法）。
* **DID 标识**：商家通过这个公钥派生出自己的 DID 地址（例如 `did:ignite:merchant_abc...`）。

#### **第二步：平台签发 VC (信用授权)**
平台不参与商家的 DID 文档修改，只做一件事——**签发凭证**：
1.  商家将 DID 标识和必要的信息（如实名资料）发给平台。
2.  平台验证通过后，用**平台的私钥**对"商家的 DID"签署一个声明。
3.  平台将这个签好名的 **VC (Verifiable Credential)** 返回给商家。
4.  **身份校验**：商家必须用 DID 私钥签名请求（`issue_vc:{did}:{name}:{nonce}`），平台验证后才签发。更新场景下还会校验签名者是 controller。

#### **第三步：商家自主上链 (状态固化)**
这是最关键的一步，商家拿着平台给的 VC，结合自己的 Solana 账户发起交易：
1.  **构造交易**：商家将自己的 DID 文档和平台给的 VC 哈希打包进交易参数。
2.  **双重证明**：
    * **权限证明**：商家用自己的 Solana 账户（作为 `Signer`）支付 Gas 并证明自己是该 DID 的拥有者。
    * **背书证明**：交易中携带的 VC 包含平台的签名，证明该 DID 经过了平台认证。
3.  **ZK 压缩存储**：Solana 合约校验两项证明无误后，将 DID 状态更新到 **ZK Compression** 状态树中。
4.  **上链方式选择**：
    * **Sponsored 模式**：平台用自己的 keypair 签名发送，商户无需 Solana 私钥参与。
    * **SelfOnchain 模式**：商户通过 `POST /v1/proof` 公开端点获取 ZK proof，本地构建并签名交易。如果商户自建 Photon RPC，可完全独立不依赖平台。广播后需调用 `POST /v1/merchants/confirm` 通知平台。

---

### 2. 这种模式的“无敌”之处

你设计的这个流程解决了 Web3 商业中的几个核心痛点：

* **数据确权**：DID 是由商家“上链”的，而不是由平台“分配”的。即使平台倒闭，商家的 DID 依然存在于 Solana 链上，且商家持有唯一的控制私钥。
* **合规背书**：通过 VC 上链，任何第三方（如其他的 AI Agent 或支付网关）在链上查到该商家的 DID 时，都能看到：“这个商户已经通过了平台的风控认证”。
* **隐私与成本**：由于使用了 **ZK Compression**，商家的完整 VC 内容可以存储在链下，链上只存一个哈希。这既保护了商业隐私（不公开敏感数据），又将上链成本降到了几乎为零。

---

### 3. 技术实现细节（针对你的 Ignite-Pay）

在编写合约（Smart Contract）时，`update_did_with_vc` 指令的验证逻辑：

```rust
// 实际链上逻辑（已实现）
pub fn update_did_with_vc(
    ctx: Context<DidWithPlatformAccounts>,
    proof: ValidityProof,
    current_did: MerchantCompressedDid,
    account_meta: CompressedAccountMeta,
    vc_hash: [u8; 32],
    nonce: u64,
    platform_signature: [u8; 64],
    credential_subject_pk: Pubkey,
) -> Result<()> {
    // 1. Controller 授权
    require!(current_did.controller_pk == ctx.accounts.signer.key());

    // 2. 防重放
    require!(current_did.nonce == nonce);

    // 3. Subject Binding: signer 必须是 VC 主体
    require!(credential_subject_pk == ctx.accounts.signer.key());

    // 4. 平台签名验证: sign(credential_subject_pk || vc_hash)
    let message = credential_subject_pk.as_ref() || vc_hash;
    require!(verify_ed25519_signature(&message, &platform_signature, &platform_pubkey));

    // 5. 更新 ZK 压缩树状态
    did.vc_hash = vc_hash;
    did.nonce += 1;
    Ok(())
}
```

### 4. 总结

商家通过自己的 Solana 账户上链，最大的意义在于**建立了一个永久的、不可篡改的“信誉 ID”**。

* **商家**：付出了极微小的 Gas 费，换取了身份的独立性。
* **平台**：作为 Issuer（签发者），通过 VC 建立了生态的信任准入门槛。
* **AI Agent**：直接在链上读取被压缩的 DID 状态，实现毫秒级的信任决策。

**目前商家上链支持两种模式：Sponsored（平台代付 Gas）和 SelfOnchain（商户自付 Gas），通过请求体中的 `mode` 字段选择。**

---

## 7. **“身份唯一性（Sybil Resistance）”** 和 **“账户绑定（Account Binding）”** 的核心矛盾

如果商家可以用 **账户 B** 把原本属于 **账户 A** 的 VC 上链，会导致你的 DID 体系出现逻辑混乱：平台认为这是同一个商家，但链上却存在两个互不关联的地址。

要解决“商家随意更换上链账户”导致的问题，可以从以下三个维度进行防御：

---

### 1. VC 内部的“主体锁定” (Subject Binding)
这是最根本的防御。平台签发的 VC 不应该是“对事不对人”，而必须是**“对人且对地址”**的。

* **实现方式**：在平台签发 VC 的 JSON 载荷中，必须包含一个 `subject` 字段，其值固定为商家创建 DID 时关联的那个 **原始公钥（Original Address）**。
* **校验逻辑**：链上合约在处理上链请求时，会解析 VC 明文：
  > 如果 `交易发起者 (Signer)` **不等于** `VC.subject`，合约直接报错。
* **结果**：商家即使想换个账户上链也不行，因为 VC 是一张“限本人使用”的通行证。

---

### 2. 派生地址的“物理隔离” (PDA Derivation)
在 Solana 中，我们可以利用 **PDA (程序派生地址)** 的特性，让每个商家的 DID 账户在链上有一个“预定好的坑位”。

* **逻辑**：商家的压缩账户地址（Address）不再是随机生成的，而是：
  $$Address = Hash(ProgramID + VC.subject + "DID_SEED")$$
* **意义**：
  * 如果商家用 **账户 A** 注册，他的数据只能存在 **位置 A'**。
  * 如果他想用 **账户 B** 上链，合约会发现他试图写入 **位置 B'**。
  * 即使他强行把 VC 塞进 **位置 B'**，你的支付 SDK (Ignite-Pay) 在查询时依然会根据他的 **原始身份 ID** 去找 **位置 A'**。
* **结论**：商家用错账户上链，会导致他找不到自己的数据，或者数据无效。

---

### 3. 授权令牌 (Delegated Proof/Nonce)
如果你希望允许商家“换个账户上链”（例如商家的主钱包没钱了，用个小号付 Gas），但又怕身份被冒用，可以使用**二次授权签名**：

1. **平台签发 VC** 给商家的 **原始公钥 A**。
2. **商家用私钥 A 签署**一个“授权指令”：*“我授权账户 B 代替我将此 VC 上链”*。
3. **合约校验**：
   * 验证平台签名（VC 真实性）。
   * 验证商家 A 的签名（授权真实性）。
   * 此时，哪怕是**账户 B** 发起的交易，合约也知道它是代表 **账户 A** 在操作。

---

### 总结：你的架构应对方案

为了确保 **Ignite-Pay** 的商户体系不乱，合约层已实施以下检查闭环：

| 防御层级 | 检查项 | 状态 |
| :--- | :--- | :--- |
| **链上签名验证** | `verify(platform_pk, subject_pk \|\| vc_hash, sig)` | 已实现 |
| **Subject Binding** | `credential_subject_pk == signer.key()` | 已实现 |
| **Controller 授权** | `current_did.controller_pk == signer.key()` | 已实现 |
| **防重放 Nonce** | `current_did.nonce == nonce`，每次 +1 | 已实现 |
| **PDA 地址隔离** | `seeds = [b"merchant-did", original_pk]` | 已实现 |
| **VC 吊销** | `RevokedVc` PDA（seeds: `[b"revoked-vc", vc_hash]`），验证方查 PDA 存在即判定已吊销 | 已实现 |



### 为什么商家“乱换账户”对平台有风险？
如果商家今天用 A 账户上链，明天用 B 账户上链，而你没有强制绑定，你的后台索引器（Indexer）会看到两个不同的实体。在支付结算时，AI Agent 可能会因为无法确定“哪个才是真正的收款地址”而导致交易失败。

**当前实现采用"严格绑定"模式（`Signer == VC.subject`），但通过 Controller Key 轮换机制支持密钥更新。**

---
## 8. 安全

### 链上防护机制（已实现）

系统通过以下链上机制防止重放攻击和身份冒充：

#### 1. PlatformConfig PDA — 平台公钥链上存储

```
PlatformConfig PDA
seeds: [b"platform-config"]
存储: platform_ed25519_pubkey (32 bytes)
初始化: init_platform 指令（一次性部署调用）
```

`initialize_did` 和 `update_did_with_vc` 读取此 PDA 中的公钥来验证平台签名。未初始化时所有 VC 绑定操作会被拒绝。

#### 2. 平台签名验证 — 防止伪造 VC

平台用 Ed25519 私钥对 `(credential_subject_pk || vc_hash)` 签名。链上验证：
- 签名消息：`credential_subject_pk (32B) || vc_hash (32B)` = 64 字节
- 验证通过才允许写入 `vc_hash`
- 攻击者没有平台私钥，无法伪造签名

#### 3. Subject Binding — 链上强制"实名制"

链上指令额外接收 `credential_subject_pk: Pubkey`，强制校验 `credential_subject_pk == signer.key()`。

攻击向量分析：
- 拦截 `(vc_hash, platform_signature, credential_subject_pk)` 后用自己的 signer 提交 → Subject Binding 检查失败（signer ≠ credential_subject_pk）
- 篡改 `credential_subject_pk` 为自己的公钥 → 平台签名验证失败（签名消息变了）

#### 4. Controller + Nonce — 防止未授权更新

- `update_did_with_vc` 要求 `current_did.controller_pk == signer.key()`
- 链上 nonce 递增，每次 mutation 必须提交正确 nonce

### 仍需实现 → VC 吊销（已实现）

平台可通过 `revoke_vc` 指令吊销已签发的 VC。链上创建 `RevokedVc` PDA（seeds: `[b"revoked-vc", vc_hash]`），验证方检查 PDA 存在即判定已吊销。仅 `PlatformConfig.authority` 有权调用。VC 中包含 `credentialStatus` 字段，指向链上吊销注册表的 `program_id`，供第三方验证方定位检查。

#### 5. VC 吊销（revoke_vc）— 已实现

链上 `RevokedVc` PDA 提供不可篡改的吊销记录：

* **PDA Seeds**: `[b"revoked-vc", vc_hash]` — 每个 VC 对应唯一 PDA
* **权限控制**: 仅 `PlatformConfig.authority` 可调用（链上强制）
* **防重复**: `AlreadyRevoked` 错误防止重复吊销
* **链下缓存**: did-registry 在 sled 中缓存吊销记录（`revoked_vc:{vc_hash_hex}`）
* **credentialStatus**: 每个 VC 包含 `credentialStatus` 字段，第三方验证方通过 `program_id` 定位链上注册表

**验证方检查流程**:
1. 验证 VC 的 Ed25519 签名和有效期
2. 计算 `vc_hash = SHA-256(vc_json)`
3. 推导 PDA: `find_program_address(&[b"revoked-vc", vc_hash], program_id)`
4. 查询 PDA 是否存在 → 存在则已吊销

---

## 9. 开发实施路线图
1. **合约开发 (Anchor)**：
   * 定义 `MerchantCompressedDid` 结构。
   * 定义 `PlatformConfig` PDA 结构（存储平台公钥）。
   * 实现 `init_platform` 指令（一次性部署）。
   * 实现 `initialize_did` 指令（平台签名验证 + Subject Binding）。
   * 实现 `update_did_with_vc` 指令（平台签名验证 + Subject Binding + Nonce）。
2. **SDK 开发 (Typescript/Rust)**：
   * 提供本地生成 Ed25519 密钥对的工具。
   * 提供构造带有 VC 数据、平台签名和 credential_subject_pk 的上链交易函数。
3. **平台后端**：
   * 实现符合 W3C 标准的 VC 签发逻辑。
   * 实现 `sign_vc_binding(credential_subject_pk, vc_hash)` 方法。
   * 实现 `platform_config_address()` PDA 地址推导。

