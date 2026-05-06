# DID PDA 迁移指南

## 概述

本文档描述从 ZK Compression (Light Protocol) 迁移到标准 Solana PDA 的商家 DID 链上存储方案。

## 架构对比

| 方面 | ZK Compression | PDA |
|------|---------------|-----|
| 账户类型 | CompressedAccount (事件) | 标准 `#[account]` PDA |
| 存储成本 | ~0.0004 SOL/账户 | ~0.0015 SOL/账户 (153 bytes) |
| 查询方式 | Photon RPC (`getCompressedAccount`) | 标准 RPC (`getAccount`) |
| 证明机制 | ValidityProof + AddressTree | 无需证明 |
| 依赖 | Light Protocol, Photon RPC | 无额外依赖 |
| 地址推导 | `light_sdk::derive_address` | `Pubkey::find_program_address` |
| 种子 | `[b"merchant-did", original_pk]` | `[b"merchant-did", original_pk]` |

## Feature Flag 使用

所有 crate 共享 `zk-compression` feature：

```bash
# 默认编译（PDA 方案，无 Light SDK 依赖）
cargo build

# 编译 ZK Compression 方案（保留旧代码）
cargo build --features zk-compression
```

受影响的 crate：
- `ignite-pay-did-program` — 链上程序
- `ignite-pay-solana` — SDK
- `ignite-pay-core` — 共享库
- `did-registry` — REST API 服务
- `ignite-pay-mcp` — MCP 服务器

## PDA 结构

### MerchantDidAccount

```
Seeds: [b"merchant-did", original_pk]
Space: 153 bytes (8 discriminator + 145 data)
```

| 字段 | 类型 | 大小 | 说明 |
|------|------|------|------|
| original_pk | Pubkey | 32 | 初始公钥（不可变） |
| controller_pk | Pubkey | 32 | 当前控制者公钥 |
| recovery_pk | Pubkey | 32 | 恢复公钥 |
| vc_hash | [u8; 32] | 32 | VC 哈希 |
| last_updated | i64 | 8 | 最后更新时间戳 |
| nonce | u64 | 8 | 防重放计数器 |
| bump | u8 | 1 | PDA bump |

## 指令对比

### initialize_did

| 参数 | ZK 版本 | PDA 版本 |
|------|---------|----------|
| proof | ValidityProof | — |
| address_tree_info | PackedAddressTreeInfo | — |
| output_state_tree_index | u8 | — |
| vc_hash | [u8; 32] | [u8; 32] |
| platform_signature | [u8; 64] | [u8; 64] |
| credential_subject_pk | Pubkey | Pubkey |

PDA 版本移除了所有 proof/tree 相关参数。

## 迁移指南

### 部署迁移

从 ZK Compression 迁移到 PDA 需要：

1. **重新部署链上程序**：使用默认 feature 编译 `ignite-pay-did-program`
2. **初始化 PlatformConfig**：执行 `init_platform` 指令
3. **商家重新注册**：PDA 地址与压缩地址不同，需要商家重新执行注册流程
4. **更新 did-registry 配置**：移除 `[light]` 配置节
5. **更新 MCP 配置**：移除 `photon_url`、`address_tree` 等 ZK 字段

### 回滚到 ZK Compression

```bash
# 使用 zk-compression feature 重新编译所有 crate
cargo build --features zk-compression
```

### 不变的部分

以下组件无需任何改动：
- VC (可验证凭证) 体系 — 平台仍然签发 W3C VC
- 平台签名验证 — `sign(credential_subject_pk || vc_hash)` 不变
- 手机 App
- 商家 MCP
- DIDComm 消息协议
- IPFS 存储
