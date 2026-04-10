这份设计指导文案旨在为开发团队提供一套基于 **Solana 账户模型** 之上构建 **链下 UTXO + Merkle Tree** 状态通道的标准化架构方案。该方案核心解决高频 AI 流支付中的**并发性、低延迟与合规性**问题。

---

# AI 流支付状态通道设计开发指导书 (UTXO + Merkle Tree 方案)

## 1. 核心设计哲学

传统账户模型通道只有单一余额字段，每次支付都更新同一个余额，后一笔交易必须等前一笔签名完成后才能基于新状态签名——这就是"队头阻塞"。

本方案的解法是 **Merkle 单根 + UTXO 预分配**：

```
┌─────────────────────────────────────────────────────────────┐
│  Solana 链上 (Channel Account)                               │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │  current_root: Root_init (初始全部归用户)                  │ │
│  │  sequence: 0  (链下协商后状态版本从 1 开始)                 │ │
│  │  deposit_a / deposit_b / status / challenge_slot ...    │ │
│  └─────────────────────────────────────────────────────────┘ │
│                          ↕ 结算时提交 Root + Proof            │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │  链下 Merkle Tree (链下协商后构建，双方各持完整副本)        │ │
│  │                                                          │ │
│  │  [UTXO_0] [UTXO_1] [UTXO_2] ... [UTXO_N] [Rest]        │ │
│  │   $0.10    $0.10    $0.10       $0.10    $剩余           │ │
│  │  owner:   owner:   owner:      owner:   owner:           │ │
│  │  user     user     user        user     user             │ │
│  │                                                          │ │
│  │  Root_1 (seq=1, 双方签名) → 支付从此状态开始               │ │
│  └─────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
```

* **Solana 作为结算层**：链上仅存储 Channel 元数据与 Merkle Root，降低链上成本。
* **先链上后链下**：用户先在链上单方面开通通道并锁定资金（初始 Root 为单叶子树），然后链下与服务商协商构建 UTXO 分配的 Merkle Tree，双方签名确认。
* **链下 UTXO 预分配**：链下协商时，将质押的 SPL Token（稳定币）拆分为 N 个等额 UTXO 叶子 + 1 个找零叶子。每个 UTXO 是一棵不可再分的"硬币"。
* **Merkle 单根模型**：同一时刻只有唯一合法 Root。不存在并行分叉。
* **流水线并行**：UTXO 预分配使客户端可以**批量签名**——一次性签好 N 笔叶子更新，服务商按序回放。并行发生在签名准备阶段，而非 Root 产生阶段。

---

## 2. 关键模块实现指南

### A. 链下数据结构 (The "Off-chain Ledger")

每个通道在本地维护一棵 Merkle Tree，双方各持完整副本。

**叶子节点 (Leaf Node) 定义**：

```rust
/// 叶子节点 — 序列化为定长字节后取 SHA-256 作为 Merkle Hash
struct UTXOLeaf {
    /// 叶子类型
    type: LeafType,              // enum { Standard, HTLC, Compliance }

    /// 当前持有者公钥
    owner: Pubkey,

    /// 金额 (token 最小单位，如 USDC 的 micro-USDC)
    amount: u64,

    /// HTLC 条件 (仅 type == HTLC 时有效)
    hash_lock: Option<[u8; 32]>,  // SHA-256(原像 R)
    timelock_slot: Option<u64>,   // Solana slot 绝对高度

    /// HTLC 受益方 (仅 type == HTLC 时有效)
    /// 提供正确原像 R 的一方可获得此 UTXO 的资金
    /// 超时后资金退回 owner
    beneficiary: Option<Pubkey>,
}
```

> **设计说明**：
>
> **防重放**：由全局 `sequence`（在 LeafUpdate 中严格递增）+ `prev_leaf_hash` 联合保证。每次 LeafUpdate 必须指定 `prev_leaf_hash`，服务商比对本地树，确保修改基于最新状态。因此不需要额外的 `leaf_nonce` 字段。
>
> **`beneficiary` 字段的必要性**：HTLC 叶子有两种互斥的结算路径：(1) 提供 R 的受益方认领资金；(2) 超时后 `owner` 退回资金。`owner` 在 HTLC 锁定期间保持不变（代表超时退款的接收方），`beneficiary` 指定谁有权凭 R 认领。这使得链上合约可以明确仲裁：验证 R 时转给 `beneficiary`，超时时转给 `owner`。
>
> 在直连通道中，`owner = user`, `beneficiary = provider`。在多跳场景中，`owner` = 超时退款的 Hub 方，`beneficiary` = 下游节点（提供 R 的一方）。

**关键设计约束**：

1. **金额不可拆分**：一个 UTXO 只能整体转移（从 owner=A 变为 owner=B），不能部分转移。若需精确金额，应在通道开启时预分配合适的面额（如 10 × $0.01 + 5 × $0.05 + 2 × $0.10）。
2. **叶子数量固定**：通道生命周期内叶子总数不变，仅修改叶子内容（owner/type/condition）。
3. **找零叶子 (Rest)**：最后一个叶子持有扣除所有预分配 UTXO 后的剩余资金，用于大额支付或通道关闭时找零。

**状态根 (State Root)**：所有 UTXO 叶子的哈希汇聚值。链上 Channel Account 存储 `current_root`，结算时作为唯一合法凭证。

**空叶子 (Empty Leaf) 处理**：

当 `leaf_count < 2^tree_depth` 时，树中存在未分配的空位。空叶子是一个标准的 UTXOLeaf 结构体实例：

```rust
/// 空叶子常量 — 所有未使用的叶子槽位的初始值
const EMPTY_LEAF: UTXOLeaf = UTXOLeaf {
    type_: LeafType::Standard,
    owner: Pubkey::default(),   // 系统公钥 (全零)
    amount: 0,
    hash_lock: None,
    timelock_slot: None,
    beneficiary: None,
};

/// 空叶子哈希 = borsh_serialize(EMPTY_LEAF) 的 SHA-256
/// 由于所有字段为固定值，此哈希是全局常量
const EMPTY_LEAF_HASH: [u8; 32] = sha256(borsh::serialize(&EMPTY_LEAF));
```

空叶子使用与普通叶子完全相同的序列化+哈希流程，保证哈希计算的一致性。结算时，`owner == Pubkey::default()` 或 `amount == 0` 的叶子自动跳过，不参与资金分配。

### B. 流水线签名机制 (Pipelined Signing)

#### 核心原理：为什么 UTXO 预分配能消除队头阻塞

传统账户模型：
```
余额 $10 → 签"付 $0.01" → 余额 $9.99 (必须等签名完成才能签下一笔)
              阻塞 ↑
```

UTXO 预分配模型：
```
通道开启时预分配:
  UTXO_0($0.01) UTXO_1($0.01) UTXO_2($0.01) ... UTXO_99($0.01) Rest($余额)

支付第 1 笔: 修改 UTXO_0.owner = provider → 计算新 Root_2 (基于 Root_1)
支付第 2 笔: 修改 UTXO_1.owner = provider → 计算新 Root_3 (基于 Root_2)
支付第 3 笔: 修改 UTXO_2.owner = provider → 计算新 Root_4 (基于 Root_3)
```

**Root 是严格线性的**：Root_1 → Root_2 → Root_3 → ...，没有分叉。（Root_init 为链上初始状态 seq=0，Root_1 为链下协商状态 seq=1，支付从 Root_2 开始）

**"不用等"的真正含义**：

并行不在 Root 层面，而在签名准备层面。客户端可以一次性签好所有支付：

```
客户端离线批量签名:
  1. 基于 Root_1, 修改 UTXO_0 → 得到 Root_2, 签名 Sig_2
  2. 基于 Root_2, 修改 UTXO_1 → 得到 Root_3, 签名 Sig_3
  3. 基于 Root_3, 修改 UTXO_2 → 得到 Root_4, 签名 Sig_4
  ...
  N. 一次性发送全部 (Root_i, Sig_i) 给服务商

服务商收到后按序回放验证即可。
```

因为每个 UTXO 是独立的"硬币"，客户端修改 UTXO_0 时不需要关心 UTXO_1 的状态（两者不在同一叶子路径上）。这使得批量签名的计算可以快速流水线完成。

#### 流水线正确性证明：为什么不等返回就能保证每笔结算正确

**核心问题**：客户端连续签发 N 笔 LeafUpdate 后一次性发给服务商，不等待每笔的确认。如何保证每笔交易都正确结算？

**定理：流水线原子性** — 若客户端按 sequence 严格递增签发 LeafUpdate 序列 `[LU_2, LU_3, ..., LU_N]`（从 seq=2 开始，seq=1 为链下协商状态），且服务商按序验证，则以下性质成立：

**性质 1：不可部分回滚 (All-or-Nothing)**

```
前提:
  LU_i 的 new_leaf 是 LU_{i+1} 的 prev_leaf_hash 的输入
  (当 LU_i 和 LU_{i+1} 修改同一叶子时)

结论:
  服务商只能接受全部 N 笔，或者拒绝从 LU_k 开始的后续所有笔
  不可能出现"接受了 LU_2 但不接受 LU_3"然后继续接受 LU_4 的情况

证明:
  服务商验证 LU_k 后更新 local_tree，local_sequence = k
  若 LU_{k+1} 被拒绝:
    - 若因 prev_leaf_hash 不匹配: 说明 LU_{k+1} 依赖的状态与当前树不一致
      → 所有 LU_{k+2}~LU_N 的 sequence > k+1，但 local_sequence 停在 k
      → 后续全部被拒绝 (sequence > local_sequence + 1 检查失败)
    - 若因签名无效: 只有这一笔失败
      → LU_{k+2} 的 sequence = k+2 > local_sequence+1 = k+1
      → 后续也全部被拒绝

  因此，一旦某笔失败，后续全部失败。已成功的 LU_1~LU_k 构成有效状态。
```

**性质 2：不同叶子的独立性 (Independent Commit)**

```
前提:
  LU_i 修改 leaf_a，LU_j 修改 leaf_b，且 leaf_a ≠ leaf_b

结论:
  LU_i 和 LU_j 在 Merkle Tree 中无依赖关系，可独立验证

证明:
  Merkle Tree 中，leaf_a 和 leaf_b 仅在最近公共祖先节点汇聚
  修改 leaf_a 改变 Root_a→Root_a'，但 leaf_b 的 hash 不变
  修改 leaf_b 改变 Root_a'→Root_a''，但这是新的 Root

  关键: LU_j 的 prev_leaf_hash 是 leaf_b 在 Root_{j-1} 下的值
  由于 leaf_a ≠ leaf_b，leaf_b 在 Root_{i-1} 和 Root_i 下完全相同
  因此 LU_j.prev_leaf_hash 始终正确，无论 LU_i 是否已被服务商处理
```

**性质 3：客户端本地计算的确定性 (Deterministic Local State)**

```
客户端签发流水线时，本地状态演进是确定性的:

  State_1 (Root_1, 链下协商后的初始状态)
    ├── LU_2: leaf_0 改变 → State_2 (Root_2)
    ├── LU_3: leaf_1 改变 → State_3 (Root_3)
    ├── LU_4: leaf_2 → HTLC → State_4 (Root_4)
    └── LU_5: leaf_3 改变 → State_5 (Root_5)

  每一步的输入 (当前树状态) 完全由前一步决定
  没有外部输入，没有随机性
  → 客户端计算的 Root_2~Root_5 序列是唯一确定的

  服务商按序验证后，其本地树演进与客户端完全一致
  → 双方在相同 sequence 下持有相同的 Root
```

**流水线失败的恢复策略**：

```
场景: 客户端发送 [LU_2, LU_3, LU_4, LU_5]，服务商接受 LU_2~LU_3 但拒绝 LU_4

原因分析:
  a) LU_4.prev_leaf_hash 不匹配
     → 客户端和服务商的树状态在 sequence=3 处分叉
     → 不应该发生（性质 3 保证确定性），除非:
       - 服务商在 LU_3 之前收到了其他人(不可能，双端通道)的更新
       - 客户端签发时基于了错误的状态

  b) LU_4.signature 无效
     → 客户端签名计算错误，重新签发即可

恢复:
  1. 服务商返回错误信息: { failed_at: 4, reason: "prev_hash_mismatch" | "invalid_sig" }
  2. 客户端从 sequence=3 的状态重新签发 LU_4', LU_5'
  3. 若 prev_hash_mismatch: 客户端请求服务商发送当前树状态，同步后重新签发

安全保证:
  已被服务商接受的 LU_2, LU_3 不会被回滚
  客户端和服务商的 sequence ≥ 3 的状态始终一致
```

#### 协议消息格式

```rust
/// 叶子更新指令 — 客户端发给服务商的最小签名单元
struct LeafUpdate {
    /// 通道 ID
    channel_id: [u8; 32],

    /// 此更新将产生的全局状态版本号 (严格递增)
    sequence: u64,

    /// 被修改的叶子索引
    leaf_index: u32,

    /// 修改前的叶子哈希 (服务商可据此验证一致性)
    prev_leaf_hash: [u8; 32],

    /// 修改后的叶子明文
    new_leaf: UTXOLeaf,

    /// 付款方对此更新的签名
    /// 签名内容 = SHA-256(channel_id || sequence || leaf_index || prev_leaf_hash || new_leaf_hash)
    signature: Signature,
}
```

#### 服务商处理流程

```
服务商收到 [LeafUpdate_1, LeafUpdate_2, ...] 后:

1. 按 sequence 排序
2. 逐个验证:
   a. sequence == local_sequence + 1  (严格递增)
   b. prev_leaf_hash == local_tree.get_leaf(leaf_index).hash()  (基于已知状态)
   c. signature 验证通过
3. 通过后更新本地 Merkle Tree，local_sequence += 1
4. (可选) 回签确认给客户端
```

**注意**：服务商验证时**不需要 Merkle Proof**。服务商持有完整树副本，可以直接比对 `prev_leaf_hash` 与本地叶子哈希。Merkle Proof 仅在**链上结算**时由服务商从本地树生成。

#### HTLC 叶子的流水线

若 `UTXO_5` 是 HTLC 类型，正在等待原像 R，不影响其他叶子：

```
UTXO_0 ~ UTXO_4:  正常流水线签名，即时支付
UTXO_5:            HTLC 状态，等待服务商提交 R 或超时
UTXO_6 ~ UTXO_99: 继续正常流水线签名

UTXO_5 不阻塞任何其他叶子的签名流水线。
```

### C. 哈希时间锁定 (HTLC) 的集成

在按 Token 计费场景中，利用 HTLC 确保服务商必须提供"模型输出凭证"（原像 R）才能最终获得资金。

#### HTLC 生命周期

```
阶段 1: 锁定 (Lock)
┌──────────────────────────────────────────────────────┐
│  服务商生成随机数 R，计算 H = SHA-256(R)              │
│  服务商 → 用户: 发送 H + 服务描述                      │
│  用户签发 LeafUpdate:                                │
│    UTXO_i.type = HTLC                               │
│    UTXO_i.hash_lock = H                              │
│    UTXO_i.timelock_slot = current_slot + TIMEOUT     │
│    UTXO_i.owner = 用户           (超时退款的接收方)   │
│    UTXO_i.beneficiary = 服务商    (提供 R 后的受益方) │
│  新 Root 中包含此 HTLC 叶子                           │
└──────────────────────────────────────────────────────┘

阶段 2: 解锁 — 两条互斥路径
┌──────────────────────────────────────────────────────┐
│ 路径 A: 服务商提供原像 (正常完成)                      │
│   服务商 → 用户: AI 输出 + R                          │
│   用户验证 H == SHA-256(R) 后签发 LeafUpdate:         │
│     UTXO_i.type = Standard                           │
│     UTXO_i.owner = 服务商 (beneficiary → owner)       │
│     UTXO_i.hash_lock = None                          │
│     UTXO_i.beneficiary = None                        │
│                                                       │
│   【关键】若用户拒绝签名:                              │
│   服务商持有 R，可在挑战期内直接向链上合约              │
│   提交 VerifyHTLC(R + Merkle Proof)，                 │
│   合约验证 SHA256(R)==hash_lock 后                    │
│   将资金转给 beneficiary (= 服务商)。                  │
│   服务商不依赖用户在线。                               │
├──────────────────────────────────────────────────────┤
│ 路径 B: 超时退款 (服务商未提供 R)                      │
│   Solana slot > timelock_slot 后                      │
│   用户签发 LeafUpdate:                                │
│     UTXO_i.type = Standard                           │
│     UTXO_i.owner = 用户 (资金退回 owner)               │
│     UTXO_i.hash_lock = None                          │
│     UTXO_i.beneficiary = None                        │
│                                                       │
│   【关键】若链上结算时 HTLC 已过期且无 R 提交:        │
│   合约拒绝 beneficiary 认领，资金归 owner (用户)。     │
└──────────────────────────────────────────────────────┘
```

#### 时序约束

```
timelock_slot 必须满足:
    timelock_slot > current_slot + CHALLENGE_DURATION + SAFETY_MARGIN

原因: 挑战期必须足够长，让服务商有时间在链上提交 R。
若挑战期短于 HTLC 超时时间，恶意用户可以在服务商提交 R 之前
通过挑战结算来"跳过"HTLC，窃取资金。
```

---

## 3. 业务流程 (Business Flows)

本章描述状态通道从创建到关闭的完整生命周期，涵盖四个核心业务流程。所有流程均以具体示例说明。

### 3.1 开通状态通道 (Open Channel)

#### 3.1.1 流程概述

> 下图为简化概述。详细步骤和参数说明见 3.1.3。

```
用户                                    服务商                       Solana 链上
 │                                        │                              │
 │  1. 协商通道参数                        │                              │
 │ ──────────────────────────────────────>│                              │
 │  (deposit_amount, denominations,        │                              │
 │   challenge_duration, token_mint)       │                              │
 │                                        │                              │
 │  2. OpenChannel 链上交易                │                              │
 │  (用户单方质押，初始化 Root_init)           │                              │
 │ ──────────────────────────────────────────────────────────────────────>│
 │                                        │                     创建 ChannelAccount
 │                                        │                     质押 SPL Token
 │                                        │                     current_root = Root_init
 │                                        │                              │
 │  3. 链下协商构建 Merkle Tree             │                              │
 │  (用户构造 UTXO 叶子 → 发给服务商        │                              │
 │   → 服务商验证 → 双方签名 Root_1)        │                              │
 │  <──────────────────────────────────────>                              │
 │                                        │                              │
 │  ========== 通道就绪，开始链下支付 ==========                          │
```

#### 3.1.2 UTXO 面额策略

通道开启时，用户根据预期支付模式选择 UTXO 面额组合。面额选择直接影响并行度和找零频率。

**策略 A：均匀拆分（适合单价固定的场景）**

```
场景: AI API 每次调用 $0.01, 用户质押 $10.00 USDC

通道参数:
  leaf_count = 101    // 100 个支付叶子 + 1 个找零叶子
  tree_depth = 16     // 最大 65536 叶子，空间充足

初始叶子:
  UTXO_0 ~ UTXO_99:  each $0.01, owner=user, type=Standard
  UTXO_100 (Rest):   $9.00, owner=user, type=Standard

  总计: 100 × $0.01 + $9.00 = $10.00 ✓
```

**策略 B：多面额混合（适合价格波动场景）**

```
场景: AI API 调用价格 $0.01 ~ $1.00 不等, 用户质押 $10.00 USDC

初始叶子:
  UTXO_0 ~ UTXO_49:  each $0.01, owner=user    // 50 × $0.01 = $0.50
  UTXO_50 ~ UTXO_59: each $0.05, owner=user    // 10 × $0.05 = $0.50
  UTXO_60 ~ UTXO_69: each $0.10, owner=user    // 10 × $0.10 = $1.00
  UTXO_70 ~ UTXO_74: each $0.50, owner=user    //  5 × $0.50 = $2.50
  UTXO_75:           $5.50, owner=user (Rest)   //  找零 = $5.50

  总计: $0.50 + $0.50 + $1.00 + $2.50 + $5.50 = $10.00 ✓
```

**策略 C：找零优先（适合大额低频场景）**

```
场景: 预计少量大额支付, 用户质押 $100.00 USDC

初始叶子:
  UTXO_0 ~ UTXO_9:   each $0.10, owner=user    // 10 × $0.10 = $1.00
  UTXO_10:           $99.00, owner=user (Rest)  // 几乎全部在找零

支付时: 从 Rest 叶子拆分出精确金额 (见 3.2 拆分流程)
```

#### 3.1.3 OpenChannel 完整流程：链上开通 → 链下协商构建 Tree

**核心原则**：用户先在链上单方面开通通道并锁定资金，然后链下与服务商协商构建 Merkle Tree。链上交易仅锚定一个初始 Root（全部资金归用户），链下协商后双方签名产生第一个有效的分裂 Root。

```
阶段 1: 链上开通 (用户单方操作)

  用户                                       Solana 链上
    │                                            │
    │  1. 提交 OpenChannel 交易                    │
    │  { authority_a: user_pubkey,                 │
    │    authority_b: provider_pubkey,              │
    │    token_mint: USDC,                          │
    │    deposit_amount: 10_000_000,                │
    │    root_init: [u8; 32],    ← 初始 Root       │
    │    tree_depth: 16,                            │
    │    leaf_count: 1,           ← 初始仅 1 个叶子 │
    │    challenge_duration: 86400,                 │
    │    min_challenge_delay: 7200,                 │
    │    sig_a: sig_user(Root_init) }               │
    │ ──────────────────────────────────────────>│
    │                                            │
    │                              合约执行:       │
    │                              a. 创建 ChannelAccount
    │                              b. 验证 sig_a (仅需用户签名)
    │                              c. current_root = Root_init
    │                              d. sequence = 0, status = Open
    │                              e. 转移 10 USDC 到 vault_a
    │                              f. 记录 open_slot
    │                                            │
    │  <── 交易确认, channel_id ─────────────────│
    │                                            │
    │  Root_init 内容 (用户本地构造):             │
    │    仅 1 个叶子:                            │
    │    UTXO_0: { owner=user, amount=10 USDC,   │
    │             type=Standard }                 │
    │    → 所有资金归用户，服务商无需预先认可     │

阶段 2: 链下协商构建 Merkle Tree (双方交互)

  用户                                        服务商
    │                                            │
    │  2. 发送通道创建请求 + 面额方案              │
    │  { channel_id, deposit: 10 USDC,            │
    │    denominations: [100×$0.01 + Rest],        │
    │    tree_config: {depth:16, count:101} }      │
    │ ──────────────────────────────────────────>│
    │                                            │
    │  3. 服务商确认并返回自己的公钥                │
    │  <──────────────────────────────────────────│
    │  { authority_b: <pubkey>, accepted: true }   │
    │                                            │
    │  4. 用户在本地构造分裂 Merkle Tree:           │
    │     a. 按 denominations 创建 101 个 UTXOLeaf │
    │     b. 所有叶子 owner = user                  │
    │     c. 构建 Merkle Tree, 计算 Root_1          │
    │     d. 保存所有叶子的明文 + Merkle Proof       │
    │                                            │
    │  5. 用户将全部叶子明文 + Root_1 发给服务商     │
    │  { leaves: [UTXOLeaf; 101],                  │
    │    root_1: [u8; 32],                          │
    │    sequence: 1 }                              │
    │ ──────────────────────────────────────────>│
    │                                            │
    │  6. 服务商本地验证:                           │
    │     a. 用收到的 leaves 本地构建 Merkle Tree    │
    │     b. 比对计算出的 Root == 用户的 Root_1      │
    │     c. 验证所有 leaves 的 owner == user        │
    │     d. 验证 total_amount == 10 USDC            │
    │                                            │
    │  7. 验证通过，服务商对 Root_1 签名              │
    │  <──────────────────────────────────────────│
    │  sig_provider(channel_id, seq=1, Root_1)    │
    │                                            │
    │  8. 用户也对 Root_1 签名                       │
    │  sig_user(channel_id, seq=1, Root_1)        │
    │                                            │
    │  此时双方持有:                                │
    │    - 完整的叶子明文 (101个)                    │
    │    - 完整的 Merkle Tree 副本                  │
    │    - Root_1 的双方签名 (sequence=1)           │
    │    - 链上 Root_init (sequence=0)              │
    │                                            │
    │  ========== 通道就绪，开始链下支付 ===========
```

**为什么先链上后链下**：
- **用户资金安全**：链上交易确认后，资金已锁定。服务商看到链上资金后才参与链下协商，避免服务商配合后又反悔。
- **无需服务商预先在线**：用户可以先行链上交易，服务商后续上线时再完成链下协商。
- **链上初始 Root 极简**：`Root_init` 仅含 1 个叶子（全部资金归用户），构造和验证零成本。
- **服务商无风险**：服务商在链下验证 `total_amount == 链上 deposit` 后才签名，如果用户篡改金额，服务商拒绝签名。

**链上 Root_init vs 链下 Root_1 的关系**：
- `Root_init` (seq=0): 链上锚定，1 个叶子，全部资金归用户。这是链上"真相"。
- `Root_1` (seq=1): 链下协商，101 个叶子，双签确认。这是双方认可的"工作状态"。
- 支付从 seq=2 开始，基于 Root_1。
- 若链下协商失败（服务商不响应），用户可基于 `Root_init` 直接关闭通道，资金全额退回。

```rust
/// OpenChannel 指令参数
struct OpenChannelParams {
    /// 质押金额 (token 最小单位)
    deposit_amount: u64,

    /// 初始 Merkle Root (全部资金归用户的单叶子树)
    root_init: [u8; 32],

    /// Merkle Tree 深度 (如 16, 后续链下可扩展)
    tree_depth: u32,

    /// 初始叶子数量 (开通时为 1, 链下协商后扩展)
    leaf_count: u32,

    /// 挑战期长度 (slots)
    challenge_duration: u64,

    /// 最小挑战延迟 (slots)
    min_challenge_delay: u64,

    /// 自动关闭 slot (可选)
    auto_close_slot: Option<u64>,

    /// 用户对 Root_init 的签名 (仅需用户单方签名)
    sig_a: Signature,
}
```

#### 3.1.4 双向注资（可选）

若服务商也需要质押资金（如押金、双向支付），使用 `FundChannel` 指令。同样遵循"先链上后链下"：

```
1. 用户先 OpenChannel (链上质押 deposit_a, Root_init 全部归用户)
2. 服务商调用 FundChannel(channel_id, deposit_b):
   a. 转移 deposit_b 到 vault_b
   b. ChannelAccount.deposit_b = deposit_b
3. 链下协商: 双方构造包含双方叶子的 Tree
   a. 用户叶子总额 == deposit_a
   b. 服务商叶子总额 == deposit_b
   c. 双方签名 Root_1 (seq=1)
```

> **V1 简化**：先仅支持单向注资（用户 → 服务商），服务商无需质押。

---

### 3.2 链下 UTXO 拆分与合并 (Off-chain Split & Merge)

虽然叶子数量固定，但通过修改叶子内容（amount/owner/type），可以在链下实现 UTXO 的拆分和合并。关键在于 **找零叶子 (Rest)** 充当"蓄水池"。

#### 3.2.1 支付流程：标准 UTXO 转移

最简单的操作——将一个 UTXO 整体转给服务商：

```
状态:
  UTXO_0: $0.01, owner=user
  UTXO_1: $0.01, owner=user
  UTXO_100(Rest): $9.98, owner=user

用户支付 $0.01 给服务商:
  LeafUpdate {
    sequence: 2,
    leaf_index: 0,
    prev_leaf_hash: hash({owner:user, $0.01, Standard}),
    new_leaf: {owner:provider, $0.01, Standard},
  }

结果:
  UTXO_0: $0.01, owner=provider  ← 已支付
  UTXO_1: $0.01, owner=user
  UTXO_100(Rest): $9.98, owner=user
```

#### 3.2.2 拆分：从 Rest 叶子创建新面额

当预分配的小额 UTXO 耗尽，或需要非标准金额时，从 Rest 叶子拆分：

**场景**：需要支付 $0.37，但没有精确面额的 UTXO。

```
拆分前:
  UTXO_0 ~ UTXO_9:  owner=provider (已花费)
  UTXO_10 ~ UTXO_99: owner=user, $0.01 each
  UTXO_100(Rest): $9.00, owner=user

方案 A: 组合支付（用现有小面额 UTXO 拼凑）

  用 37 个 $0.01 的 UTXO 支付:
    LeafUpdate(seq=11, UTXO_10→provider)
    LeafUpdate(seq=12, UTXO_11→provider)
    ...
    LeafUpdate(seq=47, UTXO_46→provider)
    共 37 笔 LeafUpdate

  优点: 无需拆分
  缺点: 消耗 37 个叶子槽位，签名开销大

方案 B: 从 Rest 拆分（推荐）

  步骤 1: 从 Rest 中扣除 $0.37（必须先扣减，保持资金守恒）

    LeafUpdate(seq=11) {
      leaf_index: 100,                  // Rest 叶子
      prev_leaf_hash: hash(UTXO_100 当前状态),
      new_leaf: {owner:user, $8.63, Standard},  // $9.00 - $0.37
    }

  步骤 2: 在空闲叶子槽位创建 $0.37（资金来自已扣减的 Rest）

    前提: UTXO_0 (已花费, owner=provider, $0.01) 可以被复用

    LeafUpdate(seq=12) {
      leaf_index: 0,                    // 复用已花费的槽位
      prev_leaf_hash: hash(UTXO_0 当前状态),
      new_leaf: {owner:user, $0.37, Standard},  // 从 Rest 拆出的金额
    }

  步骤 3: 支付 $0.37

    LeafUpdate(seq=13) {
      leaf_index: 0,
      new_leaf: {owner:provider, $0.37, Standard},
    }

  结果:
    UTXO_0: $0.37, owner=provider  ← 已支付
    UTXO_10 ~ UTXO_99: 不变
    UTXO_100(Rest): $8.63, owner=user

资金守恒验证:
  拆分前: $0.01×10(provider) + $0.01×90(user) + $9.00(Rest) = $10.00
  拆分后: $0.37×1(provider) + $0.01×9(provider) + $0.01×90(user) + $8.63(Rest) = $10.00 ✓
  每个 sequence 下，所有叶子 amount 之和 == deposit_a + deposit_b ✓
```

> **关键原则**：拆分本质上是**两笔原子 LeafUpdate**——必须**先从 Rest 扣减**，再在空闲槽位创建新 UTXO。这个顺序保证任何中间状态下资金守恒不变量不被打破（每个 sequence 下所有叶子金额之和 == 总质押）。若顺序反转，会导致中间状态凭空多出资金，若此时触发链上挑战，可能被恶意利用。

#### 3.2.3 合并：回收已花费的 UTXO

随着支付进行，多个小额 UTXO 被转给服务商（owner=provider），占用叶子槽位。当空闲槽位不足时，需要合并回收。

**场景**：100 个 $0.01 的 UTXO 全部已花费，但 Rest 还有 $9.00，需要继续支付。

```
合并前:
  UTXO_0 ~ UTXO_99: owner=provider, $0.01 each (全部已花费)
  UTXO_100(Rest): $9.00, owner=user

  问题: 没有空闲叶子可用了！

合并操作: 将服务商的多个小额 UTXO 合并为一个

  LeafUpdate(seq=201) {
    leaf_index: 0,    // 保留第一个作为合并结果
    new_leaf: {owner:provider, $1.00, Standard},  // 合并金额
  }

  LeafUpdate(seq=202) {
    leaf_index: 1,    // 清空第二个
    new_leaf: {owner:Pubkey::default(), $0.00, Standard},  // 回收到空叶子
  }

  ... 对 UTXO_2 ~ UTXO_99 重复 seq=203~299 ...

  LeafUpdate(seq=299) {
    leaf_index: 99,
    new_leaf: {owner:Pubkey::default(), $0.00, Standard},
  }

合并后:
  UTXO_0: $1.00, owner=provider        ← 合并后的服务商余额
  UTXO_1 ~ UTXO_99: 空叶子 (可复用)
  UTXO_100(Rest): $9.00, owner=user

  空闲槽位恢复到 99 个！
```

> **安全约束**：合并操作改变了服务商的 UTXO 总额。服务商必须验证：
> 1. 合并前所有被合并叶子的 amount 总和 == 合并后叶子的 amount
> 2. 服务商只合并自己拥有的叶子（owner=provider 的才能合并）
> 3. 清空的叶子金额必须为 0（防止凭空创造资金）
>
> **签名权说明**：在双向通道中，服务商也可以发起 LeafUpdate（服务商签发，用户验证）。合并服务商自己的叶子时，由服务商签发 LeafUpdate，用户按序验证。这与用户签发给付的 LeafUpdate 对称——双方都可以签发修改自己拥有的叶子的更新。服务商发起的 LeafUpdate 同样遵循 sequence 严格递增、prev_leaf_hash 匹配的规则。

#### 3.2.4 拆分/合并工具函数

```rust
/// 从 Rest 叶子拆分指定金额到目标叶子
fn split_from_rest(
    channel_id: &[u8; 32],
    rest_index: u32,
    target_index: u32,
    amount: u64,
    current_sequence: u64,
    tree: &mut MerkleTree,
    signer: &Keypair,
) -> Result<(LeafUpdate, LeafUpdate)> {
    let rest_leaf = tree.get_leaf(rest_index);
    require!(rest_leaf.amount >= amount, "Rest 余额不足");
    require!(rest_leaf.owner == signer.pubkey(), "非 Rest 持有者");

    // LeafUpdate 1: Rest 扣减
    let new_rest = UTXOLeaf {
        type_: LeafType::Standard,
        owner: rest_leaf.owner,
        amount: rest_leaf.amount - amount,
        hash_lock: None,
        timelock_slot: None,
        beneficiary: None,
    };

    let update_rest = LeafUpdate::sign(
        channel_id, current_sequence, rest_index,
        &tree.get_leaf(rest_index), &new_rest, signer,
    );

    // LeafUpdate 2: 目标叶子赋值
    let new_target = UTXOLeaf {
        type_: LeafType::Standard,
        owner: signer.pubkey(),
        amount,
        hash_lock: None,
        timelock_slot: None,
        beneficiary: None,
    };

    let update_target = LeafUpdate::sign(
        channel_id, current_sequence + 1, target_index,
        &tree.get_leaf(target_index), &new_target, signer,
    );

    Ok((update_rest, update_target))
}

/// 合并多个已花费叶子到一个
/// 注意: signer 是发起合并的一方 (服务商合并自己的叶子时 signer=provider)
fn merge_spent_leaves(
    channel_id: &[u8; 32],
    source_indices: &[u32],
    target_index: u32,
    current_sequence: u64,
    tree: &mut MerkleTree,
    signer: &Keypair,
) -> Result<Vec<LeafUpdate>> {
    let mut total_amount: u64 = 0;
    let mut updates = Vec::with_capacity(source_indices.len() + 1);
    let mut seq = current_sequence;

    // 累加所有源叶子的金额
    for &idx in source_indices {
        let leaf = tree.get_leaf(idx);
        require!(leaf.owner == signer.pubkey(), "只能合并自己的叶子");
        total_amount = total_amount.saturating_add(leaf.amount);
    }

    // LeafUpdate: 合并到目标叶子
    let merged_leaf = UTXOLeaf {
        type_: LeafType::Standard,
        owner: signer.pubkey(),
        amount: total_amount,
        hash_lock: None,
        timelock_slot: None,
        beneficiary: None,
    };
    updates.push(LeafUpdate::sign(
        channel_id, seq, target_index,
        &tree.get_leaf(target_index), &merged_leaf, signer,
    ));
    seq += 1;

    // LeafUpdate: 清空源叶子
    let empty = UTXOLeaf {
        type_: LeafType::Standard,
        owner: Pubkey::default(),
        amount: 0,
        hash_lock: None,
        timelock_slot: None,
        beneficiary: None,
    };
    for &idx in source_indices {
        if idx == target_index { continue; }
        updates.push(LeafUpdate::sign(
            channel_id, seq, idx,
            &tree.get_leaf(idx), &empty, signer,
        ));
        seq += 1;
    }

    Ok(updates)
}
```

---

### 3.3 链下 HTLC 结算 (Off-chain HTLC Settlement)

HTLC 用于"先服务后付费"场景：服务商提供 AI 输出后才获得支付。本节详细描述 HTLC 从创建到结算的完整链下交互。

#### 3.3.1 HTLC 创建：锁定阶段

```
场景: 用户请求 AI 服务，服务商报价 $0.05

用户                                    服务商
 │                                        │
 │  1. 请求服务                            │
 │  "请帮我翻译这段文字"                    │
 │ ──────────────────────────────────────>│
 │                                        │
 │  2. 服务商报价 + 哈希承诺                │
 │  <──────────────────────────────────────│
 │  { price: $0.05, hash_lock: H,          │
 │    timelock_slot: current+5000,          │
 │    description: "AI Translation" }       │
 │                                        │
 │  3. 用户锁定 UTXO                       │
 │  选择 UTXO_50 ($0.05) 创建 HTLC:        │
 │                                        │
 │  LeafUpdate(seq=20) {                   │
 │    leaf_index: 50,                      │
 │    new_leaf: {                          │
 │      type: HTLC,                        │
 │      owner: user,                       │
 │      amount: $0.05,                     │
 │      hash_lock: H,                      │
 │      timelock_slot: current_slot+5000,  │
 │      beneficiary: provider,             │
 │    }                                    │
 │  }                                      │
 │ ──────────────────────────────────────>│
 │                                        │
 │                                        │  4. 服务商验证:
 │                                        │     a. seq==local_seq+1 ✓
 │                                        │     b. prev_hash 匹配 ✓
 │                                        │     c. 签名有效 ✓
 │                                        │     d. hash_lock == H ✓
 │                                        │     e. timelock 合理 ✓
 │                                        │
 │                                        │  5. 服务商开始处理请求
 │                                        │  (同时记住 R 的值)
```

#### 3.3.2 HTLC 正常结算：服务商提供原像

```
用户                                    服务商
 │                                        │
 │                                        │  6. AI 处理完成, 获得结果
 │                                        │
 │  7. 返回 AI 结果 + 原像 R               │
 │  <──────────────────────────────────────│
 │  { result: "Translation: Hello World",  │
 │    preimage: R }                         │
 │                                        │
 │  8. 用户验证:                           │
 │     SHA-256(R) == H? ✓                  │
 │     结果质量满意? ✓                      │
 │                                        │
 │  9. 用户释放 HTLC 给服务商               │
 │  LeafUpdate(seq=21) {                   │
 │    leaf_index: 50,                      │
 │    new_leaf: {                          │
 │      type: Standard,                    │
 │      owner: provider,        ← 资金转给服务商
 │      amount: $0.05,                     │
 │      hash_lock: None,                   │
 │      timelock_slot: None,               │
 │      beneficiary: None,               │
 │    }                                    │
 │  }                                      │
 │ ──────────────────────────────────────>│
 │                                        │
 │                                        │  10. 服务商验证并接受
 │                                        │  $0.05 到账！
 │                                        │
 │  ========== HTLC 完成 ==========
```

#### 3.3.3 HTLC 争议路径：用户拒绝释放

```
用户                                    服务商                       Solana 链上
 │                                        │                              │
 │  (用户收到 R 但拒绝签发 seq=21)          │                              │
 │                                        │                              │
 │                                        │  服务商持有 R，不依赖用户     │
 │                                        │                              │
 │                                        │  方案 A: 触发链上验证         │
 │                                        │                              │
 │                                        │  TriggerChallenge            │
 │                                        │  (root, seq=20, sig_provider)│
 │                                        │ ────────────────────────────>│
 │                                        │                              │
 │                                        │  VerifyHTLC                  │
 │                                        │  (leaf=50, proof, R)         │
 │                                        │ ────────────────────────────>│
 │                                        │                     合约验证:
 │                                        │                     proof ✓
 │                                        │                     HTLC type ✓
 │                                        │                     SHA256(R)==H ✓
 │                                        │                     slot≤timelock ✓
 │                                        │                     提交方==beneficiary ✓
 │                                        │                              │
 │                                        │                     $0.05 → beneficiary (provider)
 │                                        │                              │
 │                                        │  ... 挑战期结束后结算 ...     │
```

#### 3.3.4 HTLC 超时路径：服务商未提供 R

```
用户                                    服务商                       Solana 链上
 │                                        │                              │
 │  (服务商未能提供 AI 输出，不发送 R)       │                              │
 │                                        │                              │
 │  等待直到 current_slot > timelock_slot  │                              │
 │                                        │                              │
 │  用户回收 HTLC:                         │                              │
 │  LeafUpdate(seq=21) {                   │                              │
 │    leaf_index: 50,                      │                              │
 │    new_leaf: {                          │                              │
 │      type: Standard,                    │                              │
 │      owner: user,            ← 资金退回 │                              │
 │      amount: $0.05,                     │                              │
 │      hash_lock: None,                   │                              │
 │      timelock_slot: None,               │                              │
 │      beneficiary: None,               │                              │
 │    }                                    │                              │
 │  }                                      │                              │
 │ ──────────────────────────────────────>│                              │
 │                                        │  服务商验证 slot > timelock:  │
 │                                        │  无法提供 R → 接受退款        │
 │                                        │                              │
 │  ========== HTLC 退款完成 ==========
```

若服务商不在线或拒绝接受退款，用户可通过链上 HTLCRefund 指令强制退款。

#### 3.3.5 多个并发 HTLC

多个 HTLC 可同时存在，分别占用不同的 UTXO 叶子，互不阻塞：

```
当前状态:
  UTXO_0: $0.01, owner=provider    (已支付)
  UTXO_1: $0.01, owner=provider    (已支付)
  UTXO_2: HTLC, $0.05, H_1, owner=user, beneficiary=provider    (等待翻译结果)
  UTXO_3: HTLC, $0.10, H_2, owner=user, beneficiary=provider    (等待摘要结果)
  UTXO_4: HTLC, $0.03, H_3, owner=user, beneficiary=provider    (等待代码生成)
  UTXO_5: $0.01, owner=user        (可用)
  ...
  UTXO_100(Rest): $8.80, owner=user

并发处理:
  - UTXO_2 的 R_1 到达 → 用户签 seq=N 释放
  - UTXO_4 的 R_3 到达 → 用户签 seq=N+1 释放
  - 同时 UTXO_5 仍可用于直接支付 → seq=N+2
  - UTXO_3 的 R_2 尚未到达 → 继续等待，不阻塞其他
```

#### 3.3.6 HTLC 原像 (Preimage) 的生成与管理

```rust
/// 服务商管理 HTLC 原像的工具
struct HtlcManager {
    /// 活跃的 HTLC: hash_lock → (preimage, created_at, amount)
    active_htlcs: HashMap<[u8; 32], HtlcRecord>,
}

struct HtlcRecord {
    /// 原像 R (服务商私密保存，直到服务完成)
    preimage: [u8; 32],
    /// 创建时间
    created_slot: u64,
    /// 金额
    amount: u64,
    /// 关联的叶子索引
    leaf_index: u32,
    /// 状态
    state: HtlcState,   // Pending | Fulfilled | Expired
}

impl HtlcManager {
    /// 为一次服务请求创建 HTLC 承诺
    fn create_htlc(
        &mut self,
        amount: u64,
        leaf_index: u32,
        current_slot: u64,
    ) -> ([u8; 32], [u8; 32]) {
        let preimage: [u8; 32] = rand::random();
        let hash_lock = sha256(&preimage);
        self.active_htlcs.insert(hash_lock, HtlcRecord {
            preimage,
            created_slot: current_slot,
            amount,
            leaf_index,
            state: HtlcState::Pending,
        });
        (hash_lock, preimage)
    }

    /// 服务完成后，释放原像给用户
    fn reveal_preimage(&mut self, hash_lock: &[u8; 32]) -> Option<[u8; 32]> {
        self.active_htlcs.get_mut(hash_lock).map(|record| {
            record.state = HtlcState::Fulfilled;
            record.preimage
        })
    }

    /// 清理已过期或已完成的 HTLC
    fn cleanup(&mut self, current_slot: u64, timelock_default: u64) {
        self.active_htlcs.retain(|_, record| {
            match record.state {
                HtlcState::Pending => current_slot < record.created_slot + timelock_default,
                _ => false, // 已完成或已过期的移除
            }
        });
    }
}
```

---

### 3.4 关闭通道 (Close Channel)

通道关闭有三种路径，根据双方合作程度选择：

```
                        通道 Open
                           │
               ┌───────────┼───────────┐
               │           │           │
         合作关闭      争议关闭      自动关闭
    (Cooperative)   (Dispute)    (Auto-close)
               │           │           │
               └───────────┼───────────┘
                           │
                     通道 Closed
```

#### 3.4.1 合作关闭 (Cooperative Close)

双方同意关闭，资金按最新状态分配。这是最高效的关闭方式，只需一次链上交易开启结算窗口。

```
用户                                    服务商                       Solana 链上
 │                                        │                              │
 │  1. 用户请求关闭                        │                              │
 │  "CloseRequest(Root_latest, seq=N)"    │                              │
 │ ──────────────────────────────────────>│                              │
 │                                        │                              │
 │                                        │  2. 服务商验证最新状态         │
 │                                        │     确认 Root_latest 一致     │
 │                                        │                              │
 │  3. 服务商对 (Root_latest, N) 签名      │                              │
 │  <──────────────────────────────────────│                              │
 │  sig_provider                           │                              │
 │                                        │                              │
 │  4. 用户提交 CooperativeSettle          │                              │
 │  (Root_latest, N, sig_user, sig_provider)                             │
 │ ──────────────────────────────────────────────────────────────────────>│
 │                                        │                     验证双签 ✓
 │                                        │                     sequence ✓
 │                                        │                     进入 Settling
 │                                        │                     settle_deadline = slot+CLAIM_WINDOW
 │                                        │                              │
 │  5. 各方提交 Claim                      │                              │
 │                                        │                              │
 │  用户认领自己的叶子:                     │                              │
 │  Claim(Rest: $8.63, proof)              │                              │
 │  Claim(UTXO_10~99: 未花费, proof)       │                              │
 │ ──────────────────────────────────────────────────────────────────────>│
 │                                        │                              │
 │  服务商认领自己的叶子:                   │                              │
 │  Claim(UTXO_0~9: 已支付, proof)         │                              │
 │ ──────────────────────────────────────────────────────────────────────>│
 │                                        │                              │
 │  6. FinalizeSettlement                  │                              │
 │ ──────────────────────────────────────────────────────────────────────>│
 │                                        │                     未认领按比例退回
 │                                        │                     status = Closed
 │ <──────── 资金到各 Token Account ──────────────────────────────────────│
```

> **优化**：合作关闭时，可以将所有 Claim 合并到 CooperativeSettle 交易中（如果叶子数量不多），一步完成。但这会增加单笔交易大小，受 Solana 交易 1232 字节限制。

#### 3.4.2 争议关闭 (Dispute Close)

当双方对最新状态有分歧，或一方不响应时，通过挑战机制关闭。

**场景 A：用户发起挑战（服务商不响应关闭请求）**

```
用户                                    服务商                       Solana 链上
 │                                        │                              │
 │  (用户请求关闭，服务商不响应)             │                              │
 │                                        │                              │
 │  1. 用户单方面提交 TriggerChallenge     │                              │
 │  (Root_latest, seq=50, sig_user)        │                              │
 │ ──────────────────────────────────────────────────────────────────────>│
 │                                        │                     验证 sig_user ✓
 │                                        │                     sequence > on_chain ✓
 │                                        │                     slot >= open_slot + delay ✓
 │                                        │                     status = Challenged
 │                                        │                     challenge_slot = current
 │                                        │                              │
 │                                        │  2a. 服务商提交更优状态       │
 │                                        │  (如果服务商有更高 seq 的签名) │
 │                                        │  SubmitCounterState           │
 │                                        │  (Root, seq=55, sig_user)     │
 │                                        │ ────────────────────────────>│
 │                                        │                     seq 55 > 50 ✓
 │                                        │                     更新 root & sequence
 │                                        │                              │
 │  或者                                   │                              │
 │                                        │                              │
 │                                        │  2b. 服务商无响应             │
 │                                        │  (挑战期无人提交更优状态)     │
 │                                        │                              │
 │  3. 挑战期到期后                        │                              │
 │  SettleAfterTimeout                    │                              │
 │ ──────────────────────────────────────────────────────────────────────>│
 │                                        │                     status = Settling
 │                                        │                              │
 │  4. Claim + FinalizeSettlement          │                              │
 │  (同合作关闭的认领流程)                   │                              │
```

**场景 B：服务商发起挑战（用户恶意不支付）**

```
用户                                    服务商                       Solana 链上
 │                                        │                              │
 │  (用户消费了服务但拒绝签发 LeafUpdate)   │                              │
 │                                        │                              │
 │                                        │  1. 服务商触发挑战            │
 │                                        │  TriggerChallenge             │
 │                                        │  (Root, seq=48, sig_provider) │
 │                                        │ ────────────────────────────>│
 │                                        │                     status = Challenged
 │                                        │                              │
 │                                        │  2. 服务商同时提交 HTLC 原像  │
 │                                        │  VerifyHTLC(leaf=2, proof, R) │
 │                                        │ ────────────────────────────>│
 │                                        │                     SHA256(R)==H ✓
 │                                        │                     \$0.05 → beneficiary (provider)
 │                                        │                              │
 │  3. 用户可以提交更优状态（如果有）       │                              │
 │  SubmitCounterState                     │                              │
 │  (Root, seq=50, sig_provider)           │                              │
 │ ──────────────────────────────────────────────────────────────────────>│
 │                                        │                     更新 root & sequence
 │                                        │                              │
 │  ... 挑战期结束 → SettleAfterTimeout → Claim → FinalizeSettlement ...
```

#### 3.4.3 自动关闭 (Auto-close)

通道在 `auto_close_slot` 到期后自动触发结算，防止资金永久锁定。

```
任何人 (Relayer / Watchtower / 用户 / 服务商)
 │
 │  current_slot >= auto_close_slot ?
 │
 │  SettleAfterTimeout
 │  (无需挑战期，直接进入 Settling)
 │ ──────────────────────────────────>│
 │                                    status = Settling
 │                                    settle_deadline = slot + CLAIM_WINDOW
 │
 │  ... Claim + FinalizeSettlement ...
```

> **Watchtower 模式**：用户可以委托一个 Watchtower 服务监控 `auto_close_slot`。当通道到期时，Watchtower 自动触发结算，用户无需在线。服务商也可以运行自己的 Watchtower。

#### 3.4.4 关闭路径对比

| 特性 | 合作关闭 | 争议关闭 | 自动关闭 |
|:-----|:---------|:---------|:---------|
| 链上交易数 | 1 + N Claims + 1 Finalize | 1~3 + N Claims + 1 Finalize | 1 + N Claims + 1 Finalize |
| 所需时间 | ~1 slot + Claim 窗口 | 挑战期 + Claim 窗口 | ~1 slot + Claim 窗口 |
| 需要对方配合 | 是（双签） | 否（单签即可触发） | 否 |
| 适用场景 | 正常业务结束 | 对方不响应或恶意 | 通道过期 / 双方离线 |
| HTLC 处理 | 链下全部结算后再关 | 链上 VerifyHTLC / HTLCRefund | 同争议关闭 |
| 资金安全性 | 最高（双方同意） | 依赖最新状态提交 | 依赖最后提交的状态 |

#### 3.4.5 关闭前的 HTLC 清理

关闭通道前应尽量清理所有 HTLC 叶子，减少链上操作的复杂度：

```
关闭前检查清单:

1. 遍历所有叶子，找出 type == HTLC 的叶子
2. 对每个 HTLC:
   a. 若服务商持有 R 且用户已验证: 签发 LeafUpdate 释放 (链下)
   b. 若服务商未提供 R 且已超时: 签发 LeafUpdate 退回 (链下)
   c. 若服务商持有 R 但用户拒绝: 标记为需要链上 VerifyHTLC
   d. 若 HTLC 尚未超时且服务商未提供 R: 等待超时后退回
3. 所有 HTLC 清理完成后，再执行关闭
4. 对于无法链下清理的 HTLC (情况 c/d)，在链上结算窗口中处理
```

---

## 4. Solana 链上合约 (Program) 逻辑

使用 Anchor 框架实现。链上合约是**最终仲裁者**——只在通道关闭/争议时介入。

### 4.1 链上账户结构

```rust
#[account]
pub struct ChannelAccount {
    /// 通道唯一标识
    pub channel_id: [u8; 32],

    /// 付款方 (用户)
    pub authority_a: Pubkey,

    /// 收款方 (服务商)
    pub authority_b: Pubkey,

    /// 质押的 SPL Token Mint (如 USDC)
    pub token_mint: Pubkey,

    /// 用户的质押 Token Account
    pub vault_a: Pubkey,

    /// 服务商的质押 Token Account (可选，双向注资时使用)
    pub vault_b: Pubkey,

    /// 当前 Merkle Root (链上唯一真相)
    pub current_root: [u8; 32],

    /// 当前全局状态版本号 (严格递增，挑战时只接受 > 此值的提交)
    pub sequence: u64,

    /// 通道状态
    pub status: ChannelStatus,

    /// 挑战开始时的 Solana slot
    pub challenge_slot: Option<u64>,

    /// 挑战期长度 (Solana slots)
    pub challenge_duration: u64,

    /// Open 后到允许触发 Challenge 之间的最小 slot 间隔
    /// 防止恶意方在通道刚开启时立即触发挑战 (防前跑攻击)
    pub min_challenge_delay: u64,

    /// 通道创建时的 slot (用于计算 min_challenge_delay)
    pub open_slot: u64,

    /// 自动关闭 slot — 到期后任何人都可触发结算
    /// 防止双方同时离线导致资金永久锁定
    pub auto_close_slot: Option<u64>,

    /// Merkle Tree 参数
    pub tree_depth: u32,        // 如 16，决定 Merkle Proof 的层级深度
    pub leaf_count: u32,        // 开通时为 1，链下协商后为实际叶子数（如 101）
                                 // 注意: 此字段在链下协商后不会自动更新，
                                 // 仅作为信息字段。链上验证仅依赖 current_root，
                                 // 不使用 leaf_count。

    /// 用户初始质押金额 (通道开启时记录，用于结算时退回未认领资金)
    pub deposit_a: u64,

    /// 服务商初始质押金额 (双向注资时使用)
    pub deposit_b: u64,

    /// 已认领金额追踪 — 结算窗口中已通过 Claim/VerifyHTLC/HTLCRefund 提取的总额
    /// 用于判断结算是否完成
    pub total_claimed: u64,

    /// 已认领叶子索引集合 — 防止同一叶子被重复认领
    /// 仅在 Settling 状态下使用
    pub claimed_leaves: Vec<u32>,

    /// 结算窗口结束 slot (进入 Settling 状态时设置)
    pub settle_deadline: Option<u64>,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq)]
pub enum ChannelStatus {
    /// 通道正常运作，链下签名中
    Open,
    /// 挑战期中，等待双方提交最新状态
    Challenged,
    /// 正在结算中
    Settling,
    /// 通道已关闭，资金已分配
    Closed,
}
```

### 4.2 指令清单

| 指令 (Instruction) | 触发方 | 功能描述 |
|:---|:---|:---|
| `OpenChannel` | 用户 | 用户单方质押 SPL Token 到 Vault，记录 `deposit_a`，初始化 `current_root = Root_init`（全部资金归用户的单叶子树），设置 `sequence = 0`，记录 `open_slot`。仅需用户签名，无需服务商预先签名 |
| `CooperativeSettle` | 双方 | 双方提交最新 `(root, sequence, sig_a, sig_b)`，合约验证双方签名且 `sequence > on_chain.sequence` 后，将通道状态设为 `Settling`，设置 `settle_deadline = current_slot + CLAIM_WINDOW`。注意：此指令**不直接分配资金**，仅开启结算窗口 |
| `TriggerChallenge` | 单方 | 提交 `(root, sequence, sig)` (己方签名)。前置条件：`current_slot >= open_slot + min_challenge_delay`。合约验证签名有效且 `sequence > on_chain.sequence`，将状态设为 `Challenged`，记录 `challenge_slot` |
| `SubmitCounterState` | 对手方 | 挑战期内提交更新的 `(root, sequence, sig_counterparty)` 且 `sequence > on_chain.sequence`，合约更新 Root 和 Sequence |
| `SettleAfterTimeout` | 任何人 | 挑战期到期 (`current_slot > challenge_slot + challenge_duration`) 后触发，通道进入 `Settling` 状态，设置 `settle_deadline = current_slot + CLAIM_WINDOW` |
| `Claim` | 叶子 owner 或其委托方 | **结算窗口内可用**：提交 `(leaf_index, leaf_data, merkle_proof)`，合约验证：(1) 通道状态为 `Settling`，(2) `current_slot <= settle_deadline`，(3) proof 在 `current_root` 中有效，(4) `leaf_data` 序列化哈希与 proof 中的 leaf_hash 一致，(5) `leaf_data.amount > 0` 且 `leaf_data.owner != Pubkey::default()`。验证通过后将 `leaf_data.amount` 从 Vault 转给 `leaf_data.owner` 的关联 Token Account，累加 `total_claimed`。注意：任何人都可以提交 Claim 交易（代付 gas），但资金只能转给叶子中记录的 `owner` |
| `VerifyHTLC` | beneficiary 方 | **Challenged 或 Settling 状态**：受益方提交 `(leaf_index, merkle_proof, 原像 R)`，合约验证：(1) proof 在当前 Root 中有效，(2) 该叶子为 HTLC 类型，(3) SHA-256(R) == hash_lock，(4) current_slot <= timelock_slot，(5) 提交方 == beneficiary。验证通过后将该叶子金额加入 `total_claimed`，资金转给 `beneficiary`，叶子标记为已认领（通过链下 bitmap 追踪或独立 Claim 记录） |
| `HTLCRefund` | owner 方 | **Challenged 或 Settling 状态**：HTLC 的 owner 提交 proof 证明某 HTLC 叶子的 `current_slot > timelock_slot` 且未被 VerifyHTLC 认领（无 R 提交），合约验证：(1) proof 有效，(2) HTLC 已过期，(3) 提交方 == owner。验证通过后将 `leaf.amount` 转给 `owner`，累加 `total_claimed` |
| `FinalizeSettlement` | 任何人 | `settle_deadline` 到期后触发：将所有未认领的资金（`deposit_a + deposit_b - total_claimed`）按初始质押比例退回 `vault_a` / `vault_b`，通道状态设为 `Closed` |

### 4.3 两层签名体系

本方案使用两个不同层级的签名方案，分别用于链下支付确认和链上争议仲裁：

| 签名层级 | 签名内容 | 用途 | 验证方 |
|:---------|:---------|:-----|:-------|
| **叶子级签名** (Leaf Signature) | `SHA-256(channel_id \|\| sequence \|\| leaf_index \|\| prev_leaf_hash \|\| new_leaf_hash)` | 链下 LeafUpdate 支付确认 | 服务商（链下验证） |
| **根级签名** (State Signature) | `SHA-256(channel_id \|\| sequence \|\| root)` | 链上 CooperativeSettle / Challenge 争议仲裁 | 合约（链上验证） |

**叶子级签名**：由叶子修改的发起方签发每个 LeafUpdate。在用户付费场景中，通常是用户签发（转移自己的 UTXO 给服务商）；在合并/再平衡场景中，服务商也可以签发（合并自己拥有的叶子）。服务商持有完整树副本，可直接验证。此签名确保每次叶子修改都有合法发起方的授权。

**根级签名**：由双方各自对完整的 (root, sequence) 对签名。此签名用于链上指令，证明双方同意某个全局状态。

#### 根级签名实现

CooperativeSettle 和 TriggerChallenge/SubmitCounterState 需要验证根级链下签名。签名对象和验证方式：

```rust
/// 签名消息内容
fn state_message(channel_id: &[u8; 32], sequence: u64, root: &[u8; 32]) -> [u8; 72] {
    let mut msg = [0u8; 72];
    msg[..32].copy_from_slice(channel_id);
    msg[32..40].copy_from_slice(&sequence.to_le_bytes());
    msg[40..72].copy_from_slice(root);
    msg
}

/// 合约内验证 (Anchor)
/// 使用 Solana 内置的 ed25519 程序验证签名
fn verify_state_signature(
    ctx: &Context<SomeInstruction>,
    channel_id: &[u8; 32],
    sequence: u64,
    root: &[u8; 32],
    signature: &Signature,
    signer_pubkey: &Pubkey,
) -> Result<()> {
    let msg = state_message(channel_id, sequence, root);
    // 通过 Solana ed25519 syscall 验证
    require!(
        signature.verify(signer_pubkey.as_ref(), &msg),
        ErrorCode::InvalidStateSignature
    );
    Ok(())
}
```

**签名格式**：Ed25519 纯签名，签名内容为 `SHA-256(channel_id || sequence || root)`。

**合约验证逻辑**：
- `CooperativeSettle`：需要 `sig_a` + `sig_b` 双签
- `TriggerChallenge` / `SubmitCounterState`：只需提交方单签
- 所有提交必须满足 `sequence > on_chain.sequence`（防回滚攻击）

#### 服务商回签协议 (Provider Co-signing)

CooperativeSettle 需要双方签名，因此服务商必须在正常运营期间对状态根签名。回签协议如下：

```
服务商回签流程:

1. 用户发送一批 LeafUpdate 给服务商
2. 服务商按 sequence 逐个验证通过
3. 服务商更新本地 Merkle Tree，得到最新 Root_latest
4. 服务商对 (channel_id, sequence_latest, Root_latest) 签名
5. 服务商返回 (Root_latest, sequence_latest, sig_provider) 给用户

用户本地保存 sig_provider，用于后续 CooperativeSettle 或作为服务商认可状态的证据。

回签频率策略:
- 实时回签: 每收到一个 LeafUpdate 就回签 (延迟最低，通信开销大)
- 批量回签: 每收到 N 个 LeafUpdate 或每隔 T 秒回签一次 (推荐，平衡延迟与开销)
- 按需回签: 用户在需要关闭通道时请求服务商对最新状态回签
```

**安全属性**：服务商一旦对某个 (root, sequence) 签名，即表示认可该状态。服务商无法在链上提交比此更新的状态（除非用户后来又签发了更高 sequence 的 LeafUpdate）。这确保了 CooperativeSettle 的双签可信性。

---

## 5. 链上结算时的资金分配

链上合约只存储 `current_root`（32字节哈希），无法直接提取叶子数据。因此结算采用**认领制 (Claim-based Settlement)**：各方主动提交叶子数据 + Merkle Proof，合约验证后分配资金。

### 5.1 结算触发方式

两种方式均可触发结算，结果一致——通道进入 `Settling` 状态：

1. **合作结算**：双方提交 `CooperativeSettle(root, seq, sig_a, sig_b)` → 验证双签 → 进入 Settling
2. **争议结算**：`TriggerChallenge` → 挑战期 → `SettleAfterTimeout` → 进入 Settling

### 5.2 认领流程

```
结算窗口 (settle_deadline - current_slot = CLAIM_WINDOW, 如 1000 slots):

┌──────────────────────────────────────────────────────────────┐
│  1. 通道进入 Settling 状态                                     │
│                                                               │
│  2. 各方在窗口内提交 Claim 指令:                                │
│     Claim(leaf_index, leaf_data, merkle_proof)                │
│     → 合约验证 proof 在 current_root 中有效                     │
│     → 验证 leaf_data 序列化哈希与 proof 中的 leaf_hash 一致     │
│     → 从 Vault 转 leaf_data.amount 到 leaf_data.owner          │
│     → total_claimed += leaf_data.amount                        │
│                                                               │
│  3. 特殊处理:                                                  │
│     - VerifyHTLC 已认领的叶子: 资金已转给 beneficiary，         │
│       total_claimed 已累加，该叶子不可再 Claim                  │
│       (合约通过已处理叶子集合追踪，防止重复认领)                 │
│     - HTLC 过期未提供 R 的叶子: HTLCRefund 转给 owner          │
│       total_claimed 累加，该叶子不可再 Claim 或 VerifyHTLC      │
│     - 空叶子 (owner=Pubkey::default() 或 amount==0): 无需 Claim│
│                                                               │
│  4. 防重复认领机制:                                             │
│     合约维护一个已处理叶子集合 (claimed_leaves: Set<u32>):       │
│     - Claim 成功 → claimed_leaves.insert(leaf_index)            │
│     - VerifyHTLC 成功 → claimed_leaves.insert(leaf_index)       │
│     - HTLCRefund 成功 → claimed_leaves.insert(leaf_index)       │
│     - 每次操作前检查 leaf_index ∉ claimed_leaves               │
│                                                               │
│  5. 窗口到期后，任何人可调用 FinalizeSettlement:                 │
│     未认领金额 = deposit_a + deposit_b - total_claimed         │
│     退回比例:                                                   │
│       vault_a 退回 = 未认领金额 × (deposit_a / 总质押)          │
│       vault_b 退回 = 未认领金额 × (deposit_b / 总质押)          │
│     通道状态 → Closed                                          │
└──────────────────────────────────────────────────────────────┘
```

### 5.3 未认领资金的退回规则

`deposit_a` 和 `deposit_b` 记录通道开启时各方注入的资金总额。结算窗口到期后，合约按初始质押比例将未认领资金退回：

```rust
/// FinalizeSettlement 中的退回逻辑
fn finalize(channel: &mut ChannelAccount) {
    let total_deposit = channel.deposit_a + channel.deposit_b;
    let unclaimed = total_deposit.saturating_sub(channel.total_claimed);

    if unclaimed > 0 && total_deposit > 0 {
        // 按初始质押比例退回
        let refund_a = unclaimed * channel.deposit_a / total_deposit;
        let refund_b = unclaimed.saturating_sub(refund_a);

        transfer(channel.vault_a, refund_a);
        transfer(channel.vault_b, refund_b);
    }

    channel.status = ChannelStatus::Closed;
}
```

> **为什么按比例退回**：链上合约无法判断每个未认领叶子最初属于哪一方。按初始质押比例退回是公平的近似方案——假设未认领叶子中各方份额与初始投入成正比。对于争议结算，诚实的参与方会确保认领所有属于自己的叶子，不会留下争议。

---

## 6. 监管与合规实现建议 (非 ZKP 路径)

由于中间节点仅作为路由，合规性通过以下方式静默实现：
* **审计存根**：所有签署的 LeafUpdate 记录在本地保留快照。每个 LeafUpdate 包含 sequence、leaf_index、prev_leaf_hash、new_leaf，形成完整的变更审计链。
* **额度监控**：在链下业务逻辑中增加约束，当累计支付额度触发阈值时，自动在下一个 UTXO 更新中插入"合规标记"或暂停服务等待合规检查。
* **叶子类型扩展**：可在 `LeafType` 中增加 `Compliance` 类型，标记已完成合规审查的 UTXO，结算时合约可验证此标记。

---

## 7. 完整交易时序图

```
用户 (Client)                           服务商 (Provider)                 Solana 链上
    │                                        │                              │
    │  1. OpenChannel(Root_init, sig_a)       │                              │
    │  (用户单方质押, 初始化单叶子 Root)       │                              │
    │ ──────────────────────────────────────────────────────────────────────>│
    │                                        │                     创建 ChannelAccount
    │  <── channel_id, seq=0 ──────────────────────────────── 质押 SPL Token
    │                                        │                              │
    │  2. 链下协商构建 Tree                    │                              │
    │  (发送叶子明文 → 服务商验证 → 双签 Root_1)│                              │
    │  <─────────────────────────────────────>│                              │
    │                                        │                              │
    │  3. 批量签名:                            │                              │
    │  LeafUpdate(seq=2, UTXO_0→provider)    │                              │
    │  LeafUpdate(seq=3, UTXO_1→provider)    │                              │
    │  LeafUpdate(seq=4, UTXO_2→HTLC,H=...) │                              │
    │  LeafUpdate(seq=5, UTXO_3→provider)    │                              │
    │ ──────────────────────────────────────>│                              │
    │                                        │ 按序验证，更新本地 Tree         │
    │                                        │ Root_1→Root_2→Root_3→Root_4→Root_5
    │                                        │                              │
    │                                        │ 提供 AI 输出 + R              │
    │  <──────────────────────────────────────│                              │
    │                                        │                              │
    │  LeafUpdate(seq=6, UTXO_2 HTLC→Std,   │                              │
    │             owner=provider)             │                              │
    │ ──────────────────────────────────────>│                              │
    │                                        │                              │
    │  ··· 继续使用直到通道余额不足 ···        │                              │
    │                                        │                              │
    │  CooperativeSettle(Root_latest, seq,   │                              │
    │                    sig_a, sig_b)        │                              │
    │ ──────────────────────────────────────────────────────────────────────>│
    │                                        │                     合约验证双签
    │                                        │                     进入 Settling
    │                                        │                              │
    │  Claim(leaf_0, proof_0) → provider     │                              │
    │ ──────────────────────────────────────────────────────────────────────>│
    │  Claim(leaf_1, proof_1) → provider     │                              │
    │ ──────────────────────────────────────────────────────────────────────>│
    │  Claim(leaf_rest, proof_rest) → user   │                              │
    │ ──────────────────────────────────────────────────────────────────────>│
    │                                        │                              │
    │  FinalizeSettlement                    │                              │
    │ ──────────────────────────────────────────────────────────────────────>│
    │                                        │                     通道 Closed
    │ <──────── 分配资金到各 Token Account ─────────────────────────────────│
```

---

## 8. 开发路线图 (Milestones)

1.  **Phase 1 (基础架构)**：定义 UTXO 序列化协议 (borsh)，编写 Merkle Tree 库（支持叶子增删改 + proof 生成 + proof 验证），定义 LeafUpdate 消息格式与签名协议。
2.  **Phase 2 (Solana 合约)**：实现 ChannelAccount 结构、OpenChannel、CooperativeSettle、TriggerChallenge、SubmitCounterState、SettleAfterTimeout。完成 Merkle Proof 链上验证。在 devnet 上测试注资与提现。
3.  **Phase 3 (流水线客户端)**：开发支持批量签名的客户端 SDK，实现 UTXO 预分配策略和流水线 LeafUpdate 生成。与服务商对接按序验证。实现 UTXO 拆分/合并操作。
4.  **Phase 4 (HTLC 与链下结算)**：实现 VerifyHTLC、HTLCRefund 指令。对接 AI Gateway，实现从 Token 消耗到哈希锁定的自动化闭环。完善 HTLC 生命周期管理（创建、解锁、超时）。
5.  **Phase 5 (Hub 注册与路由)**：实现 Hub 注册合约（HubLeaf + Merkle Tree）、Hub 指标采集与惩罚机制。开发路由服务（路径发现 + 评分算法）。实现多跳同 Hash-Lock HTLC。
6.  **Phase 6 (优化与生产化)**：实现 Re-compacting（合并已花费 UTXO）、通道自动续费、流动性再平衡。完善 Watchtower 监控、链下数据备份（IPFS/Arweave）。

---

## 9. 技术风险提示

* **数据可用性 (Data Availability)**：双方必须各自保存完整的 Merkle Tree 本地副本。若一方丢失数据且对方恶意提交过时 Root，丢失方无法构造有效反证。建议将 Root 变更快照定期写入 IPFS 或 Arweave 作为备份。
* **状态排序**：LeafUpdate 的 sequence 必须严格递增。服务商应拒绝乱序消息。若网络导致乱序到达，服务商缓冲后按 sequence 重排。
* **存储压力**：持久通道产生大量历史 LeafUpdate。需设计"UTXO 合并"逻辑：将多个已花费 (owner=provider) 的小额 UTXO 合并为一个叶子，释放叶子槽位供后续使用。合并操作需要双方签名确认。
* **挑战期与 HTLC 时序**：挑战期 `challenge_duration` 必须 > 最长 HTLC `timelock_slot - current_slot`，否则服务商来不及在链上提交 R。
* **前跑攻击 (Front-running)**：Solana slot 时间极短 (~400ms)，恶意方可能在前一个交易的同一 slot 内插入 TriggerChallenge。`ChannelAccount.min_challenge_delay` 字段要求 Open → Challenge 之间至少经过 N 个 slot，缓解此攻击。
* **资金永久锁定**：若双方同时离线且通道无自动过期机制，资金将永久锁定。`ChannelAccount.auto_close_slot` 字段提供自动过期机制，到期后任何人都可触发结算。

---

## 10. 多跳路由与 Hub 网络 (Multi-hop Routing)

用户与商家之间通常不存在直连通道，支付需要经过多个 Hub 节点中转。本章描述跨通道的 HTLC 路由机制和 Hub 注册治理体系。

### 10.1 网络拓扑

```
┌─────────────────────────────────────────────────────────────────────────┐
│                                                                         │
│    用户 (Alice)          Hub_A           Hub_B          商家 (Merchant) │
│    did:alice            did:hub_a       did:hub_b      did:merchant    │
│                                                                         │
│    ┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐       │
│    │ Channel  │    │ Channel  │    │ Channel  │    │ Channel  │       │
│    │ Alice-   │────│ Hub_A-   │────│ Hub_B-   │────│ Merchant │       │
│    │ Hub_A    │    │ Hub_B    │    │ Merchant │    │          │       │
│    └──────────┘    └──────────┘    └──────────┘    └──────────┘       │
│                                                                         │
│    支付路径: Alice → Hub_A → Hub_B → Merchant                           │
│    共 3 段通道，需要跨通道传递 HTLC                                       │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

**角色定义**：

| 角色 | 说明 | 要求 |
|:-----|:-----|:-----|
| 用户 (User) | 支付发起方，质押资金到通道 | DID 身份验证 |
| Hub | 中转路由节点，为多条通道提供流动性 | DID + 平台注册 + 抵押金 + SLA |
| 商家 (Merchant) | 服务提供方，接收支付 | DID + 平台注册 |

### 10.2 Hub 注册与治理

Hub 作为中继节点，用户和商家都依赖其在线率和资金充足率。平台必须对 Hub 进行准入审核和持续监管。

#### 10.2.1 Hub 注册流程

```
Hub 运营者                             平台 (Platform)                Solana 链上
    │                                       │                              │
    │  1. 提交注册申请                       │                              │
    │  { did: "did:ignite:hub_xxx",          │                              │
    │    endpoint: "wss://hub.example.com",  │                              │
    │    supported_tokens: [USDC, USDT],     │                              │
    │    max_channel_capacity: $100000 }      │                              │
    │ ──────────────────────────────────────>│                              │
    │                                       │                              │
    │                                       │  2. 平台审核:                 │
    │                                       │     - DID 文档完整性 ✓        │
    │                                       │     - 端点可用性测试 ✓        │
    │                                       │     - KYB (企业认证) ✓        │
    │                                       │                              │
    │  3. Hub 质押抵押金                     │                              │
    │ ──────────────────────────────────────────────────────────────────────>│
    │                                       │              锁定抵押金到 PDA
    │                                       │              记录 HubLeaf 到
    │                                       │              Merkle Tree
    │                                       │                              │
    │  <── 注册成功, hub_id ────────────────│                              │
```

#### 10.2.2 Hub 链上注册数据

Hub 的注册信息存储在平台管理的 Merkle Tree 中（复用 SPL Account Compression 基础设施）：

```rust
/// Hub 注册叶子 — 存储在平台 Merkle Tree 中
struct HubLeaf {
    /// Hub 的 DID 哈希
    hub_did_hash: [u8; 32],

    /// Hub 当前活跃公钥 (用于通信和签名验证)
    active_pubkey: Pubkey,

    /// 通信端点哈希 (SHA-256 of "wss://hub.example.com")
    endpoint_hash: [u8; 32],

    /// 抵押金金额 (token 最小单位)
    collateral: u64,

    /// 平台颁发的 VC 哈希 (证明已通过审核)
    platform_vc_hash: [u8; 32],

    /// 服务指标 (链下更新，哈希锚定上链)
    /// SHA-256 of { online_rate, success_rate, avg_latency, total_routed }
    metrics_hash: [u8; 32],

    /// 最后更新 slot
    slot_updated: u64,
}
```

#### 10.2.3 Hub 指标与评价体系

用户选择 Hub 时，参考以下指标：

```rust
/// Hub 服务指标 — 链下维护，定期哈希上链
struct HubMetrics {
    /// 在线率 (0~10000, 表示 0.00%~100.00%)
    online_rate: u16,

    /// 支付成功率 (0~10000)
    success_rate: u16,

    /// 平均路由延迟 (毫秒)
    avg_latency_ms: u32,

    /// 累计路由金额 (token 最小单位)
    total_routed: u64,

    /// 累计路由笔数
    total_transactions: u64,

    /// 当前活跃通道数
    active_channels: u32,

    /// 可用流动性 (当前可中转的最大金额)
    available_liquidity: u64,

    /// 收费费率 (万分比, 如 10 = 0.1%)
    fee_rate_bps: u16,
}
```

**指标更新流程**：

```
1. 平台 Watchtower 每 T 秒检测 Hub 端点可用性
2. Hub 每次路由完成后上报路由结果 (成功/失败/延迟)
3. 平台聚合指标，计算 metrics_hash
4. 平台调用 update_hub_leaf 更新 Merkle Tree
5. 用户查询 Hub 时，验证 metrics_hash 对应的链上 proof
```

**惩罚机制**：

| 违规行为 | 惩罚 |
|:---------|:-----|
| 在线率 < 99% 连续 7 天 | 扣除 10% 抵押金 |
| 路由成功率 < 95% 连续 3 天 | 扣除 5% 抵押金 |
| 恶意扣留 HTLC 原像 | 扣除全部抵押金 + 永久封禁 |
| 资金不足导致路由失败 | 警告 + 3 次后扣除 5% 抵押金 |

### 10.3 路由发现 (Route Discovery)

用户发起支付前，需要找到一条从自己到商家的可用路由路径。

#### 10.3.1 路由查询流程

```
用户 (Alice)                  路由服务 (Route Server)           链上/索引
    │                               │                              │
    │  1. 路由请求                    │                              │
    │  { from: "did:alice",          │                              │
    │    to: "did:merchant",         │                              │
    │    amount: $0.05,              │                              │
    │    token: USDC }               │                              │
    │ ─────────────────────────────>│                              │
    │                               │                              │
    │                               │  2. 查询通道图:               │
    │                               │  - Alice 的活跃通道            │
    │                               │  - 可达 Hub 的通道             │
    │                               │  - Merchant 的活跃通道         │
    │                               │                              │
    │                               │  3. 查询 Hub 指标             │
    │                               │  (从链上 Merkle Tree 验证)     │
    │                               │ ────────────────────────────>│
    │                               │                              │
    │                               │  4. 计算候选路径:              │
    │                               │  Path_1: Alice→HubA→Merchant  │
    │                               │    fee: 0.05%, latency: 50ms  │
    │                               │    liquidity: ✓               │
    │                               │  Path_2: Alice→HubB→HubC→M    │
    │                               │    fee: 0.08%, latency: 80ms  │
    │                               │    liquidity: ✓               │
    │                               │                              │
    │  5. 返回候选路径                │                              │
    │  <─────────────────────────────│                              │
    │  [Path_1(推荐), Path_2]        │                              │
    │                               │                              │
    │  6. 用户选择 Path_1             │                              │
```

#### 10.3.2 路由选择算法

```rust
/// 路由评分 — 综合考虑费用、延迟、可靠性、流动性
fn score_route(
    path: &[RouteHop],
    amount: u64,
    hub_metrics: &HashMap<Pubkey, HubMetrics>,
) -> f64 {
    let mut total_fee: u64 = 0;
    let mut max_latency: u32 = 0;
    let mut min_success_rate: u16 = 10000;
    let mut min_liquidity: u64 = u64::MAX;

    for hop in path {
        let metrics = hub_metrics.get(&hop.hub_pubkey);
        if let Some(m) = metrics {
            total_fee += amount * m.fee_rate_bps as u64 / 10000;
            max_latency = max_latency.max(m.avg_latency_ms);
            min_success_rate = min_success_rate.min(m.success_rate);
            min_liquidity = min_liquidity.min(m.available_liquidity);
        }
    }

    // 流动性必须足够
    if min_liquidity < amount + total_fee {
        return f64::NEG_INFINITY;
    }

    // 加权评分 (越高越好)
    let fee_score = 1.0 / (1.0 + total_fee as f64 / amount as f64);
    let latency_score = 1.0 / (1.0 + max_latency as f64 / 1000.0);
    let reliability_score = min_success_rate as f64 / 10000.0;

    0.3 * fee_score + 0.3 * latency_score + 0.4 * reliability_score
}
```

### 10.4 跨通道 HTLC 路由 (Multi-hop HTLC)

跨通道支付使用嵌套 HTLC 实现原子性：所有通道使用同一个原像 R，或者使用同一条 hash_lock 链。

#### 10.4.1 同 Hash-Lock 多跳（推荐）

所有通道使用相同的 `hash_lock = SHA-256(R)`，R 由最终商家生成。任何一方获得 R 后，可在所有上游通道解锁。

```
路由: Alice → Hub_A → Hub_B → Merchant

Step 1: 商家生成 R, 计算 H = SHA-256(R)

Step 2: 从商家端往回锁定 HTLC (反向构建)

  Merchant → Hub_B:
    通道 Hub_B-Merchant 中:
    UTXO_i: HTLC, $0.05, hash_lock=H, owner=Hub_B, beneficiary=Merchant
    (Hub_B 锁定资金，Merchant 为受益方。超时退回 Hub_B)

  Hub_B → Hub_A:
    通道 Hub_A-Hub_B 中:
    UTXO_j: HTLC, $0.05+fee_B, hash_lock=H, owner=Hub_A, beneficiary=Hub_B
    (Hub_A 锁定资金，Hub_B 为受益方。超时退回 Hub_A)

  Hub_A → Alice:
    通道 Alice-Hub_A 中:
    UTXO_k: HTLC, $0.05+fee_A+fee_B, hash_lock=H, owner=Alice, beneficiary=Hub_A
    (Alice 锁定资金，Hub_A 为受益方。超时退回 Alice)

Step 3: Alice 确认路由和 HTLC 条件后，签发 LeafUpdate 锁定 UTXO_k 为 HTLC
  (此时 Alice 尚不知道 R，但她信任 HTLC 机制：只有 Hub_A 提供 R 才能获得资金)

Step 4: Hub_A 收到 Alice 的 HTLC 确认后，签发 LeafUpdate 锁定 UTXO_j 为 HTLC

Step 5: Hub_B 收到 Hub_A 的 HTLC 确认后，签发 LeafUpdate 锁定 UTXO_i 为 HTLC

Step 6: Merchant 收到支付后，向所有上游节点公开 R
  Merchant → Hub_B → Hub_A → Alice: 广播 R

Step 7: 所有节点用 R 完成各自的 HTLC 解锁
```

**时序图**：

```
Alice          Hub_A          Hub_B          Merchant
  │              │              │              │
  │  1. 路由请求 + H             │              │
  │─────────────>│              │              │
  │              │  2. 转发 + H  │              │
  │              │─────────────>│              │
  │              │              │  3. 转发 + H  │
  │              │              │─────────────>│
  │              │              │              │
  │              │              │  4. Hub_B 通道: HTLC owner=Hub_B, beneficiary=Merchant
  │              │              │<─────────────│
  │              │              │              │  UTXO: HTLC, $0.05, H
  │              │  5. Hub_A 通道: HTLC owner=Hub_A, beneficiary=Hub_B
  │              │<─────────────│              │
  │              │              │              │  UTXO: HTLC, $0.05+fee_B, H
  │  6. Alice 通道: HTLC owner=Alice, beneficiary=Hub_A
  │<─────────────│              │              │
  │              │              │              │  UTXO: HTLC, $0.05+fee_A+B, H
  │              │              │              │
  │  7. Alice 确认支付           │              │
  │─────────────>│              │              │
  │              │  8. Hub_A 转发               │
  │              │─────────────>│              │
  │              │              │  9. Hub_B 转发│
  │              │              │─────────────>│
  │              │              │              │
  │              │              │  10. Merchant 公开 R
  │              │              │<─────────────│
  │              │  11. R 传播  │              │
  │              │<─────────────│              │
  │  12. R 传播  │              │              │
  │<─────────────│              │              │
  │              │              │              │
  │  13. 各方用 R 解锁 HTLC     │              │
  │  ✓           │  ✓           │  ✓           │
```

#### 10.4.2 跨通道 HTLC 的时序约束

多跳 HTLC 涉及多个通道，每个通道的 timelock_slot 必须满足递减约束：

```
约束: 从支付方到收款方，timelock_slot 严格递减

  Alice-Hub_A 通道:     timelock_slot = T
  Hub_A-Hub_B 通道:     timelock_slot = T - Δ
  Hub_B-Merchant 通道:  timelock_slot = T - 2Δ

其中 Δ = SAFETY_MARGIN (如 1000 slots ≈ 6.7 minutes)

原因:
  若 Merchant 不公开 R，Alice 的 HTLC 最先超时，Alice 可以退款
  然后上游通道按顺序超时退款
  中间 Hub 有足够时间在自己下游通道获得 R 后，在上游通道提交

  若时序反序 (Merchant 的 timelock 最先到期):
  Merchant 超时退款 (owner=Hub_B 退回)，但 Hub_B 还持有 R
  Hub_B 作为 beneficiary 在 Hub_A-Hub_B 通道用 R 认领资金
  → Hub_B 同时拿回了 Hub_B-Merchant 通道的退款 (作为 owner) + Hub_A-Hub_B 通道的资金 (作为 beneficiary)
  → Hub_B 双倍获利，资金安全破坏
```

**推荐参数**：

```
HOP_MARGIN = 1000 slots  (约 6.7 分钟, 每跳的安全裕量)
MIN_TIMELOCK = CHALLENGE_DURATION + 3 * HOP_MARGIN  (最少 HTLC 时长)

3 跳路由的 timelock 设置:
  Alice-Hub_A:     current_slot + MIN_TIMELOCK + 2 * HOP_MARGIN
  Hub_A-Hub_B:     current_slot + MIN_TIMELOCK + HOP_MARGIN
  Hub_B-Merchant:  current_slot + MIN_TIMELOCK
```

#### 10.4.3 路由失败处理

```
场景 1: Hub 流动性不足
  Hub_B 无法在 Hub_B-Merchant 通道锁定足够资金
  → Hub_B 向 Hub_A 返回 RouteError("InsufficientLiquidity")
  → Hub_A 尝试其他路径或返回 Alice
  → Alice 选择备选路由

场景 2: HTLC 超时 (某个 Hub 离线)
  Hub_A 在收到 Alice 支付后离线
  → Alice 的 HTLC 在 timelock_slot 到期后自动退款
  → Hub_A-Hub_B 通道的 HTLC 也会超时退款
  → 资金安全，无人损失 (除了时间)

场景 3: 路由中间节点恶意扣留 R
  Hub_B 收到 Merchant 的 R 但不转发给 Hub_A

  分析:
  Hub_B 在 Hub_B-Merchant 通道是 beneficiary，持有 R 后可认领 Merchant 的资金。
  但 Hub_A-Hub_B 通道的 HTLC 中，Hub_B 是 beneficiary，Hub_A 无法获得 R。
  Hub_A 的 HTLC 超时后，Hub_A 作为 owner 退款。

  Hub_B 的结果:
  - Hub_B-Merchant 通道: 用 R 认领了 Merchant 的 $0.05 ✓ (合法受益)
  - Hub_A-Hub_B 通道: 作为 beneficiary 用 R 认领了 Hub_A 锁定的 $0.05+fee_B ✓

  Hub_B 的总收益 = $0.05 (Merchant通道) + $0.05+fee_B (Hub_A通道)
  但 Hub_B 本应在正常流程中:
  - Hub_B-Merchant 通道: $0.05 (路由费已含)
  - Hub_A-Hub_B 通道: $0.05+fee_B (路由费收益)

  结论: Hub_B 的资金收支与正常流程完全一致。Hub_B 扣留 R 不影响资金安全，
  只是让 Hub_A 的退款流程走链上超时路径（而非链下立即解锁），增加延迟。

  防御: 无需防御。资金安全不受影响。
  Hub_B 扣留 R 的唯一后果是增加链上交易（Hub_A 走超时退款），增加成本但无资金损失。
```

### 10.5 路由费结算

Hub 通过路由费盈利。路由费在每个通道的 HTLC 金额差额中隐式结算：

```
用户支付: $0.05
路由费: Hub_A 0.01%, Hub_B 0.02%

各通道锁定金额:
  Alice-Hub_A:     $0.05 × (1 + 0.01% + 0.02%) = $0.050015
  Hub_A-Hub_B:     $0.05 × (1 + 0.02%)          = $0.050010
  Hub_B-Merchant:  $0.05                          = $0.050000

HTLC 解锁后:
  Alice → Hub_A:     $0.050015 (Hub_A 获得 $0.000005 路由费)
  Hub_A → Hub_B:     $0.050010 (Hub_B 获得 $0.000010 路由费)
  Hub_B → Merchant:  $0.050000 (Merchant 收到 $0.05)
```

路由费在通道内部通过正常的 HTLC → Standard LeafUpdate 流程实现，无需额外的链上操作。

### 10.6 Hub 与用户的通道管理

#### 10.6.1 用户-Hub 通道开通过程

```
1. 用户查询候选 Hub 列表 (从路由服务获取)
2. 用户选择 Hub (基于在线率、费率、流动性)
3. 用户验证 Hub 的链上注册信息:
   a. HubLeaf 的 Merkle Proof 验证通过
   b. platform_vc_hash 有效
   c. collateral >= 最低要求
   d. metrics_hash 中的在线率 >= 99%
4. 用户先提交 OpenChannel 链上交易 (单方质押)
5. 用户向 Hub 发起链下协商 (发送 channel_id + 面额方案)
6. 双方链下协商 UTXO 面额 → 构造 Tree → 双签 Root_1
7. 通道就绪，开始支付
```

#### 10.6.2 Hub 之间的通道管理

Hub 之间需要预先建立通道以提供流动性。双向注资通道同样遵循"先链上后链下"：

```
Hub_A                                Hub_B
  │                                     │
  │  1. 协商通道参数                      │
  │  (双向注资, 各出 $5000,              │
  │   面额: 25×$50(A) + 25×$50(B) + 25×$100(A) + 25×$100(B) + Rest_A + Rest_B) │
  │                                     │
  │  2. Hub_A 先提交 OpenChannel 链上交易 │
  │  (单方质押 $5000, Root_init = 全部归 Hub_A)
  │                                     │
  │  3. Hub_B 提交 FundChannel 链上交易  │
  │  (质押 $5000, deposit_b = $5000)    │
  │                                     │
  │  4. 链下协商构建 Tree:                │
  │  Hub_A 在本地构造分裂 Tree:           │
  │     UTXO_0~24:  $50, owner=Hub_A    │
  │     UTXO_25~49: $50, owner=Hub_B    │
  │     UTXO_50~74: $100, owner=Hub_A   │
  │     UTXO_75~99: $100, owner=Hub_B   │
  │     UTXO_100 (Rest_A): $1250, owner=Hub_A
  │     UTXO_101 (Rest_B): $1250, owner=Hub_B
  │     计算 Root_1                      │
  │                                     │
  │  Hub_A 总额: 25×$50 + 25×$100 + $1250 = $5000 ✓
  │  Hub_B 总额: 25×$50 + 25×$100 + $1250 = $5000 ✓
  │  总计: $10000 ✓                      │
  │                                     │
  │  5. Hub_A 发送叶子明文 + Root_1       │
  │ ──────────────────────────────────>│
  │                                     │
  │  6. Hub_B 验证:                      │
  │     a. 本地构建 Tree, 比对 Root_1     │
  │     b. 验证 Hub_A 叶子总额 (含 Rest_A) == $5000  │
  │     c. 验证 Hub_B 叶子总额 (含 Rest_B) == $5000  │
  │     d. 验证总金额 == $10000          │
  │                                     │
  │  7. 双方对 Root_1 签名 (seq=1)        │
  │                                     │
  │  ========== 双向通道就绪 ===========
```

> **注意**：双向通道中，双方各自链上质押后再链下协商分配。每个 Hub 持有一个独立的 Rest 叶子（Rest_A / Rest_B），确保双方的叶子金额总和精确等于各自的质押金额。若链下协商失败，双方可基于链上 Root_init 各自全额退回。

#### 10.6.3 流动性再平衡 (Rebalancing)

通道使用过程中，资金会单向流动（如用户一直付费给 Hub），导致某一方 UTXO 耗尽：

```
场景: Alice-Hub_A 通道中，Alice 的可花费 UTXO 耗尽

检测: Alice 本地发现所有 owner=Alice 且 type=Standard 的叶子 amount=0

方案 1: 链下再平衡 (通过另一个通道)
  Alice 通过 Hub_B 的通道接收一笔退款，再通过 Hub_B→Hub_A 路由
  将资金转回 Hub_A 通道 (需要 Hub_A 在 Hub_B 通道有流动性)

方案 2: 链上充值 (简单但需要链上交易)
  1. Alice 追加质押到现有通道
  2. 双方协商新的 UTXO 分配
  3. 签发一批 LeafUpdate 重新分配叶子
  4. 提交 UpdateChannel 链上交易更新 current_root 和 deposit_a

方案 3: 关闭旧通道，开新通道
  (最简单但成本最高)
```
