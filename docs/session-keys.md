一套既能支持“用户零门槛（代付）”又能支持“完全去中心化（自付）”的灵活架构。

以下是一份基于 **Solana** 的 Session Keys 集成落地方案：

---

## 1. 核心架构设计

为了同时兼容两种模式，架构上采用 **插件化 Fee Payer** 策略。

### 逻辑组件
1.  **Session Provider (客户端):** 负责临时密钥（Ephemeral Key）的生成与存储（建议 `sessionStorage`）。
2.  **Auth Contract (链上):** 验证 Session Token 的有效性（Owner、Scope、TTL）。
3.  **Relayer Service (后端 - 可选):** 处理代付模式下的二次签名与 Gas 注入。

---

## 2. 详细技术路径

### A. 账户结构设计 (Anchor PDA)
每个 Session 对应一个链上 PDA 账户，用于存储授权元数据：

```rust
#[account]
pub struct SessionToken {
    pub owner: Pubkey,          // 主钱包地址
    pub ephemeral_pubkey: Pubkey, // 临时密钥公钥
    pub expiry: i64,            // 过期时间戳
    pub scope: Vec<String>,     // 授权的指令列表 (如 ["pay", "transfer"])
    pub spending_limit: u64,    // 该会话最大允许支付额度 (Lamports/USDC)
}
```

---

### B. 两种模式的落地流程

#### 模式一：自付模式 (Self-Funded)
* **适用场景：** 高级用户、开发者、Web3 原生应用。
* **交互逻辑：**
    1.  **初始化：** 用户主钱包调用合约创建 `SessionToken`。
    2.  **打款：** 交易中包含一笔 `system_program::transfer`，将少量 SOL（如 0.02 SOL）从主钱包转入临时密钥地址。
    3.  **执行：** 客户端直接用临时密钥构造交易并广播，`feePayer` 设置为临时密钥公钥。

#### 模式二：代付模式 (Relayer Sponsored)
* **适用场景：** 游戏、小白用户、Agent 自动化高频支付。
* **交互逻辑：**
    1.  **初始化：** 用户主钱包仅签名创建 `SessionToken`，无需给临时密钥转账。
    2.  **构造：** 客户端构造交易，`feePayer` 设置为 **Relayer 钱包**。
    3.  **局部签名：** 临时密钥对交易进行 `partialSign`。
    4.  **中转：** 客户端将 `serializedTransaction` 发送至 Relayer API。
    5.  **最终签名：** Relayer 校验用户权限后，用代付私钥进行二次签名并推送到 RPC。

---

## 3. 核心代码实现 (TypeScript SDK 示例)

```typescript
export class IgnitePaySDK {
  // 模式切换开关
  constructor(private mode: 'SELF' | 'SPONSORED', private relayerUrl?: string) {}

  async sendAgentTransaction(instruction: TransactionInstruction, ephemeralKeypair: Keypair) {
    const transaction = new Transaction();
    transaction.add(instruction);

    if (this.mode === 'SPONSORED') {
      // --- 代付模式 ---
      const { blockhash } = await connection.getLatestBlockhash();
      transaction.recentBlockhash = blockhash;
      transaction.feePayer = RELAYER_PUBKEY; // 从配置获取代付公钥

      // 临时密钥先签名
      transaction.partialSign(ephemeralKeypair);

      // 发送至后端 Relayer 进行二次签名和广播
      return await axios.post(`${this.relayerUrl}/sponsor`, {
        tx: transaction.serialize({ requireAllSignatures: false }).toString('base64')
      });
      
    } else {
      // --- 自付模式 ---
      transaction.feePayer = ephemeralKeypair.publicKey;
      return await sendAndConfirmTransaction(connection, transaction, [ephemeralKeypair]);
    }
  }
}
```

---

## 4. 关键风控建议

### 1. 额度熔断 (Spending Limit)
在合约层面增加 `current_usage` 字段。每次通过 Session Key 支付时，累加金额。一旦超过 `spending_limit`，交易强制失败。这能有效防止临时私钥泄露导致的资产清空风险。

### 2. 指令黑名单
禁止 Session Key 执行 `UpdateState` 或 `CloseAccount` 等涉及账户控制权的指令。Session Key 应仅限执行业务逻辑（如 `ProcessPayment`）。

### 3. 自动回收 (Self-Custody Bonus)
在自付模式下，Session 过期后，可以设计一个 `CloseSession` 指令，将临时密钥中剩余的 Gas 费（SOL）退还给主钱包。

---

## 5. 方案对比总结

| 特性 | 自付模式 (Self-Funded) | 代付模式 (Sponsored) |
| :--- | :--- | :--- |
| **Gas 来源** | 临时密钥账户 (预充值) | 项目方 Relayer 钱包 |
| **用户感知** | 需确认一次“充值”交易 | 零感知，直接进入交互 |
| **中心化程度** | 完全去中心化 | 依赖 Relayer 服务可用性 |
| **适用 Ignite-Pay 功能** | 大额、低频的结算 | 高频、微额的 Agent 自动支付 |

作为 **Ignite-Pay** 的架构师，我建议在 SDK 初始化时提供一个 `gasPolicy` 配置项。对于 Agent 端，默认推行 **代付模式** 以确保自动化流程不因缺 Gas 而中断；对于 Web 端管理后台，采用 **自付模式** 以降低项目方的运营成本。