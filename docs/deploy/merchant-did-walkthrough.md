# 商家数字身份上链 — 业务用例

本文档以一个完整的端到端流程，演示商家如何通过 Ignite Pay 平台完成数字身份注册、VC 签发、链上锚定、密钥轮换和恢复。

---

## 用例场景

**商家**: "星火便利店"
**操作者**: 商户管理员
**目标**: 在 Ignite Pay 平台完成数字身份注册，获取平台签发的 Verifiable Credential，并将身份哈希锚定到 Solana 链上。

本文档提供两种上链模式的完整示例：
- **模式 A（Sponsored 平台代付）**：平台签名并发送交易，记录服务费
- **模式 B（SelfOnchain 商户自助）**：平台构建未签名交易，商户自行签名并广播

---

## 前提

- did-registry 服务已部署并运行在 `http://localhost:8081`
- ignite-pay-did-program 已部署到 Solana Devnet
- 商户已在本地生成 Ed25519 密钥对和 `did:ignite` 标识符

---

## 完整流程

### 步骤 1：商户本地生成密钥与 DID

商户客户端在本地生成 Ed25519 密钥对，并通过 multicodec 编码推导出 `did:ignite` 标识符。

```bash
# 生成 32 字节 Ed25519 私钥
openssl rand -out merchant_private.key 32
```

对应的 DID 标识符推导规则：

```
公钥 (32 bytes)
    → 添加 multicodec 前缀: [0xed, 0x01] + pubkey (共 34 bytes)
    → Base58 编码
    → 拼接前缀: "did:ignite:z" + encoded
```

假设生成的 DID 为：
```
did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK
```

对应 Solana 公钥（active_pubkey）为密钥对的 Solana 格式公钥：
```
7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU
```

---

### 步骤 2：获取服务器 Nonce

```bash
curl -s http://localhost:8081/v1/auth/nonce | jq
```

**响应**:
```json
{
  "nonce": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
  "expires_in": 300
}
```

> Nonce 有效期 5 分钟，单次使用。

---

### 步骤 3：平台签发 Verifiable Credential

使用上一步的 nonce 请求平台签发 VC。

```bash
curl -s -X POST http://localhost:8081/v1/vc/issue \
  -H "Content-Type: application/json" \
  -d '{
    "merchant_did": "did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK",
    "merchant_name": "星火便利店",
    "category": "retail",
    "validity_hours": 8760,
    "nonce": "a1b2c3d4-e5f6-7890-abcd-ef1234567890"
  }' | jq
```

**响应**:
```json
{
  "verifiable_credential": {
    "@context": [
      "https://www.w3.org/2018/credentials/v1",
      "https://ignite-pay.com/credentials/v1"
    ],
    "id": "urn:uuid:f47ac10b-58cc-4372-a567-0e02b2c3d479",
    "type": ["VerifiableCredential", "MerchantAttestation"],
    "issuer": "did:ignite:z6MkplatformPublicKeyEncoded...",
    "issuanceDate": "2025-06-15T08:30:00Z",
    "expirationDate": "2026-06-15T08:30:00Z",
    "credentialSubject": {
      "id": "did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK",
      "name": "星火便利店",
      "category": "retail"
    },
    "proof": {
      "type": "Ed25519Signature2020",
      "created": "2025-06-15T08:30:00Z",
      "proofPurpose": "assertionMethod",
      "verificationMethod": "did:ignite:z6MkplatformPublicKeyEncoded...#key-signing-1",
      "proofValue": "UXJhvK3n2pR8wN7eQm..."
    }
  },
  "vc_hash": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
}
```

**关键信息提取**：
- `vc_hash`: `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`
- VC 已被平台 Ed25519 私钥签名
- VC 已持久化到 sled 数据库（`vc:{vc_hash_hex}` 键）

---

### 步骤 4：获取新 Nonce（用于注册）

```bash
curl -s http://localhost:8081/v1/auth/nonce | jq
```

**响应**:
```json
{
  "nonce": "b2c3d4e5-f6a7-8901-bcde-f12345678901",
  "expires_in": 300
}
```

---

### 步骤 5：商户签名注册消息

商户使用本地 Ed25519 私钥，对以下结构化消息签名：

```
register:did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK:7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855:b2c3d4e5-f6a7-8901-bcde-f12345678901
```

格式: `register:{merchant_did}:{active_pubkey}:{platform_vc_hash}:{nonce}`

签名结果（示例）：
```
did_signature = "j7Kd8xR2mN3pQ5vW9yA0bC4fG6hIjLkMnOpQrStUvWxYzA1B2C3D4E5F6G7H8I9J0K1L2M3N4O5P6Q7R8S9T0U1V2W3X4Y5Z6=="
```

---

### 步骤 6：提交链上注册

#### 模式 A：Sponsored（平台代付，默认）

```bash
curl -s -X POST http://localhost:8081/v1/merchants/register \
  -H "Content-Type: application/json" \
  -d '{
    "merchant_did": "did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK",
    "active_pubkey": "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU",
    "platform_vc_hash": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    "did_signature": "j7Kd8xR2mN3pQ5vW9yA0bC4fG6hIjLkMnOpQrStUvWxYzA1B2C3D4E5F6G7H8I9J0K1L2M3N4O5P6Q7R8S9T0U1V2W3X4Y5Z6==",
    "nonce": "b2c3d4e5-f6a7-8901-bcde-f12345678901",
    "mode": "sponsored"
  }' | jq
```

> `mode` 字段可选，默认为 `sponsored`（向后兼容），可省略。

**服务端处理流程**:
1. 验证 `merchant_did` 以 `did:ignite:` 开头
2. 消费 nonce（防重放）
3. 验证 DID 签名（从 DID 中提取公钥，验证 Ed25519 签名）
4. 解析 `active_pubkey` 和 `vc_hash`
5. 从 Photon RPC 获取 ZK Compression 有效性证明
6. 推导压缩 PDA 地址: `seeds = [b"merchant-did", active_pubkey]`
7. 调用 `DidService::initialize_did` 发送链上交易（平台 payer 签名）
8. 缓存商户 DID 到 sled
9. 记录服务费到 sled（`fee:register:{ts}:{did_hash_hex}`）

**响应**:
```json
{
  "signature": "5Jj8nL2kP4mN6qR8sT0uV2wX4yZ6aB8cD0eF2gH4iJ6kL8mN0oP2qR4sT6uV8wX0yZ2aB4cD6eF8gH0iJ2kL4mN6oP8qR0sT2uV4wX6yZ8aB0cD2eF4gH6i"
}
```

#### 模式 B：SelfOnchain（商户自助上链）

```bash
curl -s -X POST http://localhost:8081/v1/merchants/register \
  -H "Content-Type: application/json" \
  -d '{
    "merchant_did": "did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK",
    "active_pubkey": "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU",
    "platform_vc_hash": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    "did_signature": "j7Kd8xR2mN3pQ5vW9yA0bC4fG6hIjLkMnOpQrStUvWxYzA1B2C3D4E5F6G7H8I9J0K1L2M3N4O5P6Q7R8S9T0U1V2W3X4Y5Z6==",
    "nonce": "b2c3d4e5-f6a7-8901-bcde-f12345678901",
    "mode": "self_onchain"
  }' | jq
```

**服务端处理流程**:
1. 同上步骤 1-6（验证、nonce、签名、证明）
2. 调用 `DidService::prepare_initialize_did` 构建未签名交易
3. 使用 bincode 序列化，base64 编码返回

**响应**:
```json
{
  "transaction": "AQAAAAAAAAABAAABA4njKdHxaNnCoWmNk9p5WjnUk4KwbbOGrFckyTSDj5k7CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAmr7Vi8Y0JB3X7JYBmSvvyLIj3j8WLXQirU1pImYJE4YBAAAAAAKCAQIDBAUG...",
  "message": "sign and broadcast within 90 seconds; blockhash expires"
}
```

**商户客户端处理**（Rust 示例）：

```rust
use solana_sdk::transaction::Transaction;
use solana_client::rpc_client::RpcClient;

// 1. 解码 base64
let tx_bytes = base64::engine::general_purpose::STANDARD.decode(&tx_b64)?;

// 2. 反序列化 Transaction
let mut tx: Transaction = bincode::deserialize(&tx_bytes)?;

// 3. 用商户 keypair 签名
tx.sign(&[&merchant_keypair], tx.message.recent_blockhash);

// 4. 广播
let rpc_client = RpcClient::new("https://api.devnet.solana.com");
let sig = rpc_client.send_and_confirm_transaction(&tx)?;
println!("Transaction confirmed: {}", sig);
```

> **注意**：SelfOnchain 模式下商户需在 90 秒内完成签名和广播（blockhash 过期限制）。超时需重新请求未签名交易。

此时链上已创建 `MerchantCompressedDid` 压缩账户：
```
original_pk   = 7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU
controller_pk = 7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU  (= original_pk)
recovery_pk   = 11111111111111111111111111111111                 (未设置)
vc_hash       = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
nonce         = 0
```

---

### 步骤 7：验证链上状态

```bash
curl -s http://localhost:8081/v1/merchants/verify/did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK | jq
```

**响应**:
```json
{
  "verified": true,
  "original_pubkey": "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU",
  "controller_pubkey": "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU",
  "last_updated": 1718438400
}
```

---

### 步骤 8：解析 DID Document

```bash
curl -s http://localhost:8081/v1/did/resolve/did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK | jq
```

**响应**:
```json
{
  "@context": ["https://www.w3.org/ns/did/v1"],
  "id": "did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK",
  "verificationMethod": [{
    "id": "did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK#key-signing-1",
    "type": "Ed25519VerificationKey2020",
    "controller": "did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK",
    "publicKeyMultibase": "z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK"
  }],
  "controller_pubkey": "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU",
  "original_pubkey": "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU",
  "last_updated": 1718438400
}
```

---

## 后续操作

### 更新 VC 哈希

当 VC 过期续签或内容变更时，需要更新链上的 `vc_hash`。

```bash
# 1. 获取 nonce
NONCE=$(curl -s http://localhost:8081/v1/auth/nonce | jq -r '.nonce')

# 2. 平台签名: "update-vc:{did}:{new_vc_hash}:{nonce}"
#    此处平台使用 platform_signing_key 签名

# 3. 提交（Sponsored 模式）
curl -s -X POST http://localhost:8081/v1/merchants/update-vc \
  -H "Content-Type: application/json" \
  -d '{
    "merchant_did": "did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK",
    "new_vc_hash": "<新的32字节hex>",
    "platform_signature": "<平台base64签名>",
    "nonce": "'"${NONCE}"'",
    "mode": "sponsored"
  }'
```

链上 nonce 从 0 -> 1。Sponsored 模式下，会记录 `update_vc` 费用到 sled。

若商户希望自行签名和广播，设置 `"mode": "self_onchain"`，平台返回未签名交易：

```bash
# SelfOnchain 模式：返回 { "transaction": "<base64>", "message": "..." }
curl -s -X POST http://localhost:8081/v1/merchants/update-vc \
  -H "Content-Type: application/json" \
  -d '{
    "merchant_did": "did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK",
    "new_vc_hash": "<新的32字节hex>",
    "platform_signature": "<平台base64签名>",
    "nonce": "'"${NONCE}"'",
    "mode": "self_onchain"
  }'
```

SelfOnchain 模式下，signer 为当前链上的 `controller_pk`，商户需持有对应私钥。

### 设置恢复密钥

商户应尽快设置恢复密钥，以防控制器密钥丢失。

```bash
NONCE=$(curl -s http://localhost:8081/v1/auth/nonce | jq -r '.nonce')

# 商户签名: "rotate-key:{did}:{recovery_pubkey}:{nonce}"

# Sponsored 模式（默认）
curl -s -X POST http://localhost:8081/v1/merchants/rotate-key \
  -H "Content-Type: application/json" \
  -d '{
    "merchant_did": "did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK",
    "new_active_pubkey": "<RECOVERY_KEY_BASE58>",
    "did_signature": "<商户base64签名>",
    "nonce": "'"${NONCE}"'"
  }'
```

链上 nonce 递增，`recovery_pk` 被设置。Sponsored 模式下记录 `rotate_key` 费用。

### 查询费用记录

查看平台代付产生的服务费：

```bash
# 查询所有操作的费用
curl -s "http://localhost:8081/v1/fees" | jq

# 仅查询注册费用
curl -s "http://localhost:8081/v1/fees?operation=register" | jq

# 查询指定时间之后的费用
curl -s "http://localhost:8081/v1/fees?since=1718438400000&limit=50" | jq

# 查询 VC 更新费用
curl -s "http://localhost:8081/v1/fees?operation=update_vc&limit=20" | jq
```

**响应示例**:
```json
{
  "fees": [
    {
      "merchant_did": "did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK",
      "operation": "register",
      "fee_lamports": 5000,
      "timestamp": 1718438400000,
      "mode": "sponsored"
    },
    {
      "merchant_did": "did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK",
      "operation": "update_vc",
      "fee_lamports": 2000,
      "timestamp": 1718438500000,
      "mode": "sponsored"
    }
  ]
}
```

> SelfOnchain 模式不产生费用记录（商户自行承担链上费用）。

### 灾备恢复

当 controller 密钥丢失时，使用 recovery 密钥恢复：

1. 使用 recovery 密钥签名 `recover_controller` 指令
2. 直接调用 `DidService::recover_controller`（或通过未来的 API 端点）
3. 设置新的 `controller_pk`
4. 链上 nonce 递增

---

## 状态流转图

```
                     ┌───────────────────────┐
                     │  商户生成密钥对        │
                     │  → did:ignite:z...     │
                     └───────────┬───────────┘
                                 │
                     ┌───────────▼───────────┐
                     │  GET /v1/auth/nonce    │
                     │  → nonce (5min TTL)    │
                     └───────────┬───────────┘
                                 │
                     ┌───────────▼───────────┐
                     │  POST /v1/vc/issue     │
                     │  → VC + vc_hash        │
                     │  (平台签名 + 存储)      │
                     └───────────┬───────────┘
                                 │
                     ┌───────────▼───────────┐
                     │  GET /v1/auth/nonce    │
                     │  → 新 nonce            │
                     └───────────┬───────────┘
                                 │
                     ┌───────────▼───────────┐
                     │  商户本地签名           │
                     │  "register:{...}"      │
                     └───────────┬───────────┘
                                 │
              ┌──────────────────▼──────────────────┐
              │  POST /v1/merchants/register         │
              │  mode = "sponsored" | "self_onchain" │
              │  → Photon 证明                       │
              ├─────────────┬────────────────────────┤
              │  Sponsored  │  SelfOnchain            │
              │  平台签名+  │  返回未签名TX            │
              │  发送+记录费│  商户自签+广播            │
              └─────────────┴────────────────────────┘
                                 │
              → MerchantCompressedDid 创建
                original_pk = controller_pk = 签名者
                vc_hash = VC 哈希, nonce = 0
                                 │
           ┌─────────────────────┼─────────────────────┐
           │                     │                     │
  ┌────────▼────────┐  ┌────────▼────────┐  ┌────────▼────────┐
  │ update-vc       │  │ set-recovery    │  │ rotate-key      │
  │ nonce 0→1→...   │  │ nonce 递增      │  │ nonce 递增      │
  │ 更新 vc_hash    │  │ 设置 recovery   │  │ 更新 controller │
  │ (支持双模式)    │  │                 │  │ (支持双模式)    │
  └─────────────────┘  └─────────────────┘  └─────────────────┘

  ┌────────────────────────────────────────────────────────────┐
  │  GET /v1/fees?operation=register&since=ts&limit=100        │
  │  → 查询 Sponsored 模式费用记录（供离线结算）                  │
  └────────────────────────────────────────────────────────────┘
```

---

## 验证清单

部署完成后，依次验证：

| # | 检查项 | 命令 | 预期 |
|---|---|---|---|
| 1 | 服务健康 | `curl localhost:8081/health` | `ok` |
| 2 | Nonce 发放 | `curl localhost:8081/v1/auth/nonce` | 200 + UUID nonce |
| 3 | VC 签发 | `POST /v1/vc/issue` | 200 + VC JSON + vc_hash |
| 4 | 商户注册（Sponsored） | `POST /v1/merchants/register` (mode=sponsored) | 200 + tx signature |
| 5 | 商户注册（SelfOnchain） | `POST /v1/merchants/register` (mode=self_onchain) | 200 + base64 transaction |
| 6 | DID 解析 | `GET /v1/did/resolve/{did}` | 200 + DID Document |
| 7 | 商户验证 | `GET /v1/merchants/verify/{did}` | 200 + verified: true |
| 8 | 状态查询 | `GET /v1/merchants/status/{did}` | 200 + status: active |
| 9 | VC 更新 | `POST /v1/merchants/update-vc` | 200 + tx signature |
| 10 | 密钥轮换 | `POST /v1/merchants/rotate-key` | 200 + tx signature |
| 11 | 费用查询 | `GET /v1/fees` | 200 + fees 数组 |

---

## 常见问题

### Q: 注册时报 "Failed to get validity proof"

Photon RPC 未配置或不可达。确认 `config.toml` 中 `light.photon_url` 已正确设置，且 API Key 有效。

### Q: 注册时报 "On-chain error"

可能是 payer SOL 余额不足。检查：
```bash
solana balance <PAYER_ADDRESS> --url devnet
```
如果余额不足，空投 SOL：
```bash
solana airdrop 2 <PAYER_ADDRESS> --url devnet
```

### Q: VC 签发报 "merchant not registered"

`POST /v1/vc/issue` 要求商户已经注册（sled 中有缓存）。需先完成步骤 6（链上注册），或调整业务流程让注册不依赖已有 VC。

### Q: 签名验证失败

确认签名消息格式完全匹配（包括冒号分隔符），且使用了正确的 nonce。nonce 过期或已使用都会导致失败。

### Q: SelfOnchain 模式下 blockhash 过期怎么办

未签名交易中的 `recent_blockhash` 约 90 秒后过期。如果商户来不及签名或广播失败，需要重新获取 nonce 并重新请求未签名交易。

### Q: 费用记录为空

`GET /v1/fees` 只返回 Sponsored 模式的费用记录。如果所有操作都使用 `self_onchain` 模式，不会有费用记录。
