# 状态通道商户端部署配置文档

## 1. 概述

商户端（Party B，收款方）是状态通道的服务提供者。商户需要：

1. 生成并注册 `did:ignite` 去中心化身份
2. 获取平台签发的 Verifiable Credential (VC)
3. 将 DID 注册到链上 Concurrent Merkle Tree
4. 接收用户通过通道发起的支付，管理 HTLC 原像
5. 在结算时认领属于自己的 UTXO 叶子

商户端通过 `ignite-pay-core` 和 `ignite-pay-state-channel` 离链库作为 Rust 库集成。

---

## 2. DID 数字身份

### 2.1 生成 DID 密钥对

商户使用 `ignite-pay-core` 的 `identity` 模块生成 `did:ignite` 去中心化身份：

```toml
[dependencies]
ignite-pay-core = { path = "../ignite-pay-core" }
ignite-pay-state-channel = { path = "../ignite-pay-state-channel" }
solana-pubkey = "2"
solana-program = "2"
ed25519-dalek = "1"
```

```rust
use ignite_pay_core::identity::{generate_ignite_did, build_did_document, save_identity, load_did};

let db = sled::open("./merchant_data")?;

// 检查是否已有身份
let existing_did = load_did(&db)?;

let (identity, merchant_did) = match existing_did {
    Some(did) => {
        // 已有 DID，用相同的 DID 重新生成身份
        // 注意：密钥会不同，但 DID 标识符保持一致
        let identity = PrivateIdentity::generate(&did);
        (identity, did)
    }
    None => {
        // 首次生成
        let (identity, did) = generate_ignite_did();
        save_identity(&db, &identity, &did)?;
        (identity, did)
    }
};

// 构建 W3C DID Document
let did_doc = build_did_document(&merchant_did, &identity);

println!("商户 DID: {}", merchant_did);
// 输出类似: did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK
```

**DID 编码规则**：

`did:ignite:z` + Base58(`0xed 0x01` + Ed25519 公钥)

其中 `0xed 0x01` 是 multicodec 中 Ed25519 公钥的标识前缀。

**生成后获得**：
- **DID 标识符**: `did:ignite:z6Mk...`
- **Ed25519 签名私钥**: 用于签署支付请求和 DIDComm 消息（安全存储）
- **X25519 密钥协商密钥**: 用于 DIDComm 加密通信（由 Ed25519 派生）
- **Solana 收款密钥对**: 独立的 Solana 密钥对，用于接收支付

> **重要**：DID 签名密钥和 Solana 收款密钥是分离的。DID 密钥用于身份认证，Solana 收款密钥用于接收资金。

### 2.2 DID Document 结构

```json
{
  "@context": ["https://www.w3.org/ns/did/v1"],
  "id": "did:ignite:z6MkhaXgBZ...",
  "verificationMethod": [{
    "id": "did:ignite:z6MkhaXgBZ...#key-signing-1",
    "type": "Ed25519VerificationKey2020",
    "controller": "did:ignite:z6MkhaXgBZ...",
    "publicKeyMultibase": "z6MkhaXgBZ..."
  }],
  "keyAgreement": [{
    "id": "did:ignite:z6MkhaXgBZ...#key-agreement-1",
    "type": "X25519KeyAgreementKey2020",
    "controller": "did:ignite:z6MkhaXgBZ...",
    "publicKeyBase64": "base64-encoded-x25519-public-key"
  }]
}
```

### 2.3 申请平台背书 (VC)

向 Ignite Pay 平台提交以下信息：

| 字段 | 说明 |
|:-----|:-----|
| `merchant_did` | 商户 DID 标识符 |
| `name` | 商户名称 |
| `category` | 商户类别（如 SaaS, API, Content） |
| `service_endpoint` | 商户服务 URL |
| `solana_pubkey` | Solana 收款公钥 |

平台审核后使用 `ignite-pay-core` 的 `vc` 模块签发 Verifiable Credential：

```rust
use ignite_pay_core::vc::VerifiableCredential;
use ed25519_dalek::SigningKey;

// 平台侧签发 VC
let vc = VerifiableCredential::sign(
    vec!["https://www.w3.org/2018/credentials/v1".to_string()],
    "vc:ignite:merchant:001".to_string(),
    vec!["VerifiableCredential".to_string(), "MerchantAttestation".to_string()],
    platform_did.clone(),                          // 签发者：平台 DID
    chrono::Utc::now() - chrono::Duration::hours(1),
    chrono::Utc::now() + chrono::Duration::days(365),
    merchant_did.clone(),                          // 主体：商户 DID
    "Example Merchant".to_string(),
    Some("SaaS".to_string()),
    &platform_signing_key,                         // 平台签名私钥
    &format!("{}#key-signing-1", platform_did),
);
```

生成的 VC 格式：

```json
{
  "@context": ["https://www.w3.org/2018/credentials/v1"],
  "type": ["VerifiableCredential", "MerchantAttestation"],
  "issuer": "did:ignite:zPlatformDID...",
  "issuanceDate": "2025-01-01T00:00:00Z",
  "expirationDate": "2026-01-01T00:00:00Z",
  "credentialSubject": {
    "id": "did:ignite:z6MkMerchant...",
    "name": "Example Merchant",
    "category": "SaaS"
  },
  "proof": {
    "type": "Ed25519Signature2020",
    "verificationMethod": "did:ignite:zPlatformDID...#key-signing-1",
    "proofValue": "base64-signature..."
  }
}
```

### 2.4 链上注册（SPL Account Compression）

平台将商户信息压缩上链，使用 `ignite-pay-core` 的 `SolanaDidBridge`（需启用 `solana` feature）：

```rust
use ignite_pay_core::solana_did::SolanaDidBridge;

// 平台侧：将商户注册到链上 Merkle Tree
// MerchantLeaf {
//     merchant_did_hash: SHA-256(商户 DID),
//     active_pubkey: Solana 收款地址,
//     platform_vc_hash: SHA-256(canonical_json(VC)),
//     status: 0,  // 0=active
//     slot_updated: current_slot
// }
```

链上参数（由平台一次性部署）：

| 参数 | 值 | 说明 |
|:-----|:---|:-----|
| Merkle Tree 地址 | Solana Pubkey | ConcurrentMerkleTree 账户 |
| Tree Authority | Solana Pubkey | 平台控制密钥 |
| maxDepth | 14 | 支持 ~16K 商家 |
| maxBufferSize | 64 | 并发更新缓冲区 |
| DAS API | Helius 端点 | 用于查询 Merkle Proof |

### 2.5 DID 持久化

```rust
use ignite_pay_core::identity::{save_identity, load_did};

// 保存到 sled
save_identity(&db, &identity, &merchant_did)?;

// 重启后加载
let did = load_did(&db)?;
```

> **注意**：当前 `PrivateIdentity` 的种子无法直接提取，因此 `load_did` 仅恢复 DID 字符串。密钥会在重启时重新生成（DID 不变）。生产环境需额外实现密钥的安全持久化。

---

## 3. 角色与职责

| 职责 | 说明 |
|:-----|:-----|
| DID 身份管理 | 生成/持久化 did:ignite 身份，维护 DID Document |
| 接收支付 | 接收用户签名的 LeafUpdate，配签确认 |
| HTLC 管理 | 生成原像、在服务交付后揭示原像 |
| Provider 配签 | 对用户的 LeafUpdate 和 SignedState 进行 Ed25519 签名 |
| 结算认领 | 在结算窗口内提交 Merkle Proof 认领自己的叶子 |
| 争议响应 | 收到 challenge 后在窗口内提交 counter_state |
| VC 续签 | 定期检查 VC 有效期，到期前向平台申请续签 |

---

## 4. 通道集成

### 4.1 初始化 ChannelManager

```rust
use ignite_pay_state_channel::channel::ChannelManager;
use ignite_pay_state_channel::signing::{generate_keypair, to_pubkey};

let db = sled::open("./merchant_channel_data")?;
let manager = ChannelManager::new(db)?;

// 商户通道密钥对（独立于 DID 密钥）
let provider_keypair = generate_keypair();
let provider_pubkey = to_pubkey(&provider_keypair);
```

### 4.2 加载通道

当用户开通通道后，商户从链上事件或通信协议获取 `channel_id`，加载通道状态：

```rust
let channel_id: [u8; 32] = /* 从链上或通信获取 */;
let state = manager.load_state(&channel_id)?;

println!("通道状态: {:?}", state.metadata.status);
println!("用户存款: {}", state.metadata.deposit_a);
```

### 4.3 Provider 配资（可选）

如果通道需要双端注资，商户可以注入资金：

```rust
let update = manager.fund_channel(
    &mut state,
    &provider_keypair,
    500_000,        // 商户注资金额
    None,           // 自动选择空槽位
)?;

// update 是签名的 LeafUpdate，需提交到链上 fund_channel 指令
```

---

## 5. 处理用户支付

### 5.1 接收 LeafUpdate

商户接收用户发送的 LeafUpdate，验证签名后应用：

```rust
use ignite_pay_state_channel::signing::verify_leaf_update_signature;

// 验证签名
if !verify_leaf_update_signature(&leaf_update, &state.metadata.user_pubkey) {
    return Err("Invalid user signature");
}

// 应用更新
manager.apply_leaf_update(&mut state, &leaf_update, &state.metadata.user_pubkey)?;
```

### 5.2 Provider 配签

商户对更新后的状态进行配签，表示同意新状态：

```rust
let cosignature = manager.provider_cosign_state(
    &mut state,
    &provider_keypair,
)?;

// cosignature 是商户的 Ed25519 签名
// 返回给用户作为确认
```

### 5.3 批量更新处理

```rust
// 用户可能一次发送多个 LeafUpdate
let updates: Vec<LeafUpdate> = /* 从通信获取 */;

let result = manager.apply_leaf_update_batch(
    &mut state,
    &updates,
    &state.metadata.user_pubkey,
)?;

// 如果批量中间某条失败，result 为 Err(BatchFailureInfo)
// 已经应用的更新不会自动回滚（需要协作处理）
```

---

## 6. HTLC 管理（商户侧）

### 6.1 服务完成后揭示原像

当商户提供完服务后，需要揭示 HTLC 原像来完成支付：

```rust
use ignite_pay_state_channel::htlc::HtlcManager;

let mut htlc_mgr = HtlcManager::with_db(db.clone(), channel_id);

// 商户持有原像（从用户处获取 hash_lock，服务完成后用户揭示原像）
// 或者：商户生成原像

// 方式 A：用户创建 HTLC，商户等待原像
// 用户发送 hash_lock → 商户验证 → 服务完成后用户揭示 preimage

// 方式 B：商户生成原像
let (hash_lock, preimage) = htlc_mgr.create_htlc(
    100_000,           // 金额
    leaf_index,        // 叶子索引
    user_pubkey,       // 所有者（用户锁定资金）
    provider_pubkey,   // 受益人（商户）
    current_slot,
    500,               // 持续时间
);

// 将 hash_lock 发送给用户（用户创建 HTLC 叶子）
// 服务完成后揭示原像
htlc_mgr.reveal_preimage(&hash_lock, &preimage)?;
```

### 6.2 检查过期

```rust
// 定期检查 HTLC 是否过期
let expired = htlc_mgr.check_expiry(current_slot);
for hash_lock in &expired {
    htlc_mgr.mark_refunded(hash_lock)?;
}
```

### 6.3 HTLC 生命周期

```
Pending → (原像揭示) → Revealed → (链上解决) → Fulfilled
Pending → (超时) → Expired → (退款) → Refunded
```

---

## 7. 结算操作

### 7.1 认领叶子

在结算窗口内，商户提交 Merkle Proof 认领属于自己的 UTXO：

```rust
use ignite_pay_state_channel::signing::claim_message;

// 获取商户拥有的叶子
let leaf_index = 1;  // 商户拥有的叶子索引
let leaf = state.tree.get_leaf(leaf_index)?;

// 生成 Merkle Proof
let proof = state.tree.proof(leaf_index)?;

// 构造链上 claim 调用参数
let claim_amount = leaf.amount;
let leaf_hash = leaf.hash();
let leaf_data = borsh::to_vec(leaf)?;
let leaf_owner = leaf.owner;  // 应为 provider_pubkey

// 签名
let claim_msg = claim_message(&channel_id, leaf_index as u32, claim_amount, current_slot);
let signature = provider_keypair.sign(&claim_msg);
```

### 7.2 HTLC 认领

如果叶子是 HTLC 类型，使用链上 `verify_htlc` 指令：

```rust
// 需要提供：
// - leaf_index
// - preimage（32 字节）
// - hash_lock
// - leaf_amount
// - beneficiary（应为 provider_pubkey）
// - leaf_hash + Merkle proof
// - timelock_slot（必须 >= current_slot）
// - leaf_data
// - claimer_signature
```

### 7.3 HTLC 退款

如果 HTLC 过期，商户不认领（资金退回用户），或用户使用 `htlc_refund` 指令：

```rust
// 需要：timelock_slot < current_slot
// 用户提交 htlc_refund 指令，资金返回 leaf.owner
```

### 7.4 最终结算

结算窗口结束后，任何一方调用 `finalize_settlement`：

```rust
// 链上操作：
// - 计算未认领余额
// - 按 deposit_a / deposit_b 比例分配
// - 将剩余资金分别转入 vault_a 和 vault_b
// - 关闭通道
```

---

## 8. 合规支持

### 8.1 消费限额

如果通道启用了合规管理：

```rust
use ignite_pay_state_channel::compliance::{ComplianceManager, SpendingLimit};

let compliance = ComplianceManager::new(db.clone())?;

compliance.init_channel_compliance(channel_id, SpendingLimit {
    threshold: 1_000_000,     // 累计消费阈值
    per_channel: 2_000_000,   // 单通道最大支付
    window_slots: 1000,       // 滑动窗口（slots）
})?;

// 每次支付后记录
let action = compliance.record_payment(
    channel_id,
    payment_amount,
    current_slot,
    user_pubkey,
    provider_pubkey,
)?;

match action {
    ComplianceAction::None => { /* 正常 */ }
    ComplianceAction::InsertMarker { compliance_hash, threshold } => {
        // 触发合规审查，需要插入合规标记叶子
    }
}
```

### 8.2 审计追踪

```rust
// 记录每次 LeafUpdate
compliance.record_audit(&leaf_update)?;

// 查询通道完整审计
let trail = compliance.get_audit_trail(channel_id)?;
for update in &trail {
    println!("seq={} leaf_idx={} amount={}",
        update.sequence, update.leaf_index, update.new_leaf.amount);
}
```

---

## 9. 密钥轮换

当商户需要更换 Solana 收款地址时：

1. 使用 DID 签名私钥签署新收款地址声明
2. 提交给平台
3. 平台验证 DID 签名后，调用 `replace_leaf` 更新链上叶子节点
4. `merchant_did_hash` 不变，DID 标识符不变
5. 新的收款地址在下一个 slot 生效

> DID 标识符和签名密钥不变，仅更换收款地址，确保业务连续性。

---

## 10. 多通道管理

商户通常同时维护与多个用户的通道：

```rust
// 使用 sled 前缀管理多通道
let db = sled::open("./merchant_channels")?;
let manager = ChannelManager::new(db)?;

// 加载特定通道
let channel_id = /* ... */;
let state = manager.load_state(&channel_id)?;

// 管理多个 HTLC（每个通道独立）
let htlc_mgr = HtlcManager::with_db(db.clone(), channel_id_1);
let htlc_mgr_2 = HtlcManager::with_db(db.clone(), channel_id_2);
```

---

## 11. 安全检查清单

| 检查项 | 说明 | 状态 |
|:-------|:-----|:-----|
| DID 签名私钥安全存储 | 使用 HSM 或密钥管理服务 | 必须 |
| Solana 收款私钥安全存储 | 定期检查余额并转出到冷钱包 | 建议 |
| VC 有效期检查 | 定期续签平台 VC | 运维注意 |
| 原像管理 | 原像仅在服务确认后揭示 | 必须 |
| LeafUpdate 验证 | 每次配签前验证用户签名 | 必须 |
| 序列号检查 | 只接受 sequence > 当前值的更新 | 必须 |
| 金额守恒 | 验证每次更新后总金额不变 | 必须 |
| 结算窗口监控 | 及时在窗口内认领叶子 | 必须 |
| HTLC 超时处理 | 定期检查过期 HTLC 并退款 | 建议 |
| 审计完整性 | 记录所有 LeafUpdate | 建议 |
| 收款地址监控 | 监控链上收款地址，及时发现异常交易 | 建议 |
