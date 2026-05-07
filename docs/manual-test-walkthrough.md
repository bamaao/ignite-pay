# 手动测试演练手册

按业务流程顺序编排的分步可执行演练手册。
测试人员可以从头到尾按顺序执行，验证每个流程正确后再进入下一个。

**参考文档：**
- [业务流程](business-flows.md) — 流程图和代码位置
- [业务场景](business-scenarios.md) — 详细步骤描述和异常处理
- [App 测试计划](ignite-pay-app-test-plan.md) — 手机端 UI 测试用例 (TC-M0x-xx)
- [E2E Demo](ecom-demo-end-to-end.md) — 电商演示流程

---

## 环境准备

### 前置条件

- Solana CLI 已配置 devnet (`solana config set --url devnet`)
- 已安装 Docker & Docker Compose
- Flutter SDK（用于构建移动应用）
- Python 3.10+（用于电商 Demo）
- WSL 或 Linux 环境（用于 `cargo build-sbf`）

### 启动服务

```bash
# 1. 复制并配置环境变量
make init
# 编辑 .env 填入真实值

# 2. 启动所有后端服务
make build
make up

# 3. 验证健康状态
make health
```

所有服务应报告 OK：
- PostgreSQL、<!-- State Channel: 探索阶段，暂不启用 - 原含 Hub Registry、-->DIDComm Router（用户端 :8080，商户端 :4000）
- DID Registry (:8081)<!-- State Channel: 探索阶段，暂不启用 - 原含 Channel User (:3001)、Channel Provider (:3002)、Channel Hub (:3003) -->

### 密钥生成与资金

```bash
# 生成测试用 Solana 密钥对
solana-keygen new --outfile test-user.json --no-bip39-passphrase
solana-keygen new --outfile test-merchant.json --no-bip39-passphrase

# 在 devnet 上领取测试币
solana airdrop 2 test-user.json --url devnet
solana airdrop 2 test-merchant.json --url devnet
```

---

## 阶段一：身份与配对（基础）

### T1.1 用户 App — 首次启动与 DID 创建

**流程**：F1 前置条件
**App 测试用例**：TC-M01-01, TC-M01-03
**步骤**：
  1. 在 Android 设备或模拟器上安装并启动用户 App (ignite_pay_app)
  2. 验证：显示欢迎页面，包含"创建身份"按钮
  3. 点击"创建身份"
  4. 验证：出现加载动画，随后 DID 生成完成
  5. 验证：DID 以 `did:ignite:z6Mk...` 格式显示（32+ 字符）
  6. 验证：出现 Mediator 配置页面
  7. 输入 Mediator URL（默认或自定义）
  8. 验证：显示"已连接到 Mediator"状态 (TC-M01-04)
  9. 验证：Dashboard 显示 DID 身份
  10. 杀掉并重新启动 App
  11. 验证：跳过向导，直接加载 Dashboard (TC-M01-02)

**通过标准**：
  - [ ] DID 以正确格式生成
  - [ ] Mediator 连接建立
  - [ ] DID 在重启后持久化

**失败排查**：检查 Mediator 服务是否运行 (`make health`)，检查网络连接

### T1.2 商户 App — 首次启动与 DID 创建

**流程**：F17 前置条件
**业务场景**：事件 1，用例 1.2
**步骤**：
  1. 在 Android 设备上安装并启动商户 App (ignite_pay_merchant_app)
  2. 完成商户入驻流程
  3. 验证：<!-- State Channel: 探索阶段，暂不启用 - 原文含"双 DID 生成（状态通道 DID + DIDComm 通信 DID）" -->DID 生成（DIDComm 通信 DID）
  4. 验证：显示商户 ID
  5. 配置商户 MCP 连接

**通过标准**：
  - [ ] 商户 DID 创建成功
  - [ ] 商户 MCP 连接已配置

**失败排查**：检查商户 MCP 服务、DID Registry 服务 (:8081)

### T1.3 手机 <-> MCP 配对（DIDComm 握手）

**流程**：F1
**App 测试用例**：TC-M03-01, TC-M03-02, TC-M03-05
**步骤**：
  1. 在 MCP 服务器上生成配对二维码
  2. 在用户 App 中，点击"扫描二维码"或"配对 MCP"
  3. 扫描 MCP 配对二维码
  4. 验证：DIDComm 三步握手启动：
     - 手机发送 connection-request
     - 从 MCP 收到 connection-response
     - 手机发送 connection-confirm
     - 从 MCP 收到 connection-confirm-response
  5. 验证：SnackBar 显示"已连接到 MCP: did:ignite:..." (TC-M03-01)
  6. 验证：Dashboard 上 MCP 状态显示"已连接"

**异常场景验证**：
  7. 扫描无效二维码（随机二维码）→ 验证错误提示 (TC-M03-02)
  8. 未连接 Mediator 时尝试扫描 → 验证相应提示 (TC-M03-05)

**通过标准**：
  - [ ] 有效二维码配对成功
  - [ ] 无效二维码被拒绝并给出清晰错误提示
  - [ ] Dashboard 显示连接状态

**失败排查**：检查 DIDComm Router (:8080)、Mediator WebSocket 连接

### T1.4 商户 App <-> 商户 MCP 配对

**流程**：F1（商户变体）
**业务场景**：事件 2，用例 2.2
**步骤**：
  1. 在商户 MCP 服务器上生成配对二维码
  2. 在商户 App 中，扫描商户 MCP 二维码
  3. 验证：使用 DIDComm 通信 DID 完成 DIDComm 握手
  4. 验证：商户 App 显示"已连接到商户 MCP"

**通过标准**：
  - [ ] 商户配对完成
  - [ ] 商户 MCP 连接活跃

**失败排查**：检查商户 DIDComm Router (:4000)、商户 MCP 日志

---

## 阶段二：支付授权与执行（核心）

### T2.1 x402 挑战 — 首次支付、Session Key 创建

**流程**：F2 + F4
**App 测试用例**：TC-M04-01
**前置条件**：T1.3 已完成（手机已配对 MCP）

**步骤**：
  1. 启动电商 Demo：`cd ignite-pay-ecom-demo && python server.py`
  2. Agent 发送：`GET /products`
  3. 验证：返回产品列表，价格以 lamports 为单位
  4. Agent 发送：`POST /orders {"product_id": "coffee"}`
  5. 验证：HTTP 402 响应，`PAYMENT-REQUIRED` 头包含 x402 `PaymentRequirements`
  6. 验证：响应头包含 `x402-merchant-did`、`x402-payment-address`、`x402-order-id`
  7. MCP 处理挑战 → 解析 PaymentRequirements → 执行风控检查
  8. MCP 通过 DIDComm (mediator) 向手机发送 `payment-auth-request`
  9. 验证：手机显示支付授权页面，包含支付详情
  10. 验证：页面显示可用支付方式
  11. 用户批准支付（滑动确认）
  12. 手机创建临时 Session Key + 链上注册
  13. 验证：Session Key 已注册（查看 Solana devnet 浏览器）
  14. 如需为 Session Key 充值：`python fund_session.py <session_key_pubkey>`
  15. 手机向 MCP 发送包含 Session Key 的 `payment-auth-response`
  16. MCP 通过 Session Key 在链上执行 SOL 转账
  17. 验证：支付在 Solana devnet 上确认（tx signature）
  18. Agent 重发：`POST /orders {"product_id": "coffee"}`，附带 `X-Payment-Proof` 头
  19. 验证：HTTP 200，订单状态 = "paid"

**通过标准**：
  - [ ] 收到正确格式的 402 挑战
  - [ ] 手机显示授权请求
  - [ ] Session Key 在链上注册
  - [ ] 支付执行成功
  - [ ] 订单确认为已支付（状态从 pending_payment → paid）

**失败排查**：检查 MCP 日志 (`make logs S=ignite-pay-mcp`)、手机 Mediator 连接、devnet RPC、Session Key 充值

### T2.2 Session Key — 后续支付（复用）

**流程**：F5
**App 测试用例**：TC-M04-02
**前置条件**：T2.1 已完成（存在活跃 Session Key）

**步骤**：
  1. Agent 再次发送 `POST /orders {"product_id": "tea"}`
  2. MCP 收到 402 挑战
  3. 验证：MCP 检测到现有活跃 Session Key，不再请求手机授权
  4. MCP 直接通过现有 Session Key 执行支付
  5. 验证：支付在 Solana devnet 上确认
  6. 验证：订单状态 = "paid"

**通过标准**：
  - [ ] 复用现有 Session Key（无需新的手机授权）
  - [ ] 支付执行成功

**失败排查**：检查 Session Key 是否过期、消费额度是否用尽

### T2.3 Session Key — 余额不足充值

**流程**：F3 + F7
**前置条件**：T2.1 已完成，Session Key 余额不足

**步骤**：
  1. 消耗 Session Key 余额（发送多笔支付直到接近零）
  2. Agent 发送 `POST /orders {"product_id": "premium_coffee"}`
  3. MCP 检测到余额不足
  4. MCP 通过 DIDComm 向手机发送 `session-fund-request`
  5. 验证：手机显示充值请求页面
  6. 用户批准充值
  7. 手机在链上为 Session Key 充值
  8. 验证：Session Key 余额增加
  9. MCP 执行支付
  10. 验证：支付确认

**通过标准**：
  - [ ] 正确检测到低余额
  - [ ] 充值请求发送到手机
  - [ ] Session Key 充值后支付继续进行

**失败排查**：检查 devnet 水龙头可用性、Session Key 有效性

### T2.4 Session Key — 续期 / 替换

**流程**：F14
**App 测试用例**：TC-M05-02, TC-M05-03, TC-M05-06
**前置条件**：T2.1 已完成，Session Key 即将过期

**步骤**：
  1. 等待 Session Key 接近过期（或模拟时间）
  2. MCP 检测到即将过期的 Key，创建新的临时密钥对
  3. MCP 向手机发送续期请求
  4. 手机为新的 Session Key 注资
  5. 验证：新 Session Key 在链上注册 (TC-M05-02)
  6. 验证：App 中出现新 Key 卡片，带有"active"绿色徽章
  7. 在链上吊销旧 Session Key (TC-M05-03)
  8. 验证：绿色 SnackBar 显示"Revoked on-chain: <tx_sig>"
  9. 验证：旧 Key 显示"expired"状态 (TC-M05-06)

**通过标准**：
  - [ ] 新 Session Key 创建并注册
  - [ ] 旧 Key 在链上吊销
  - [ ] 后续支付使用新 Key

**失败排查**：检查 Key 过期逻辑、链上吊销交易

---

## 阶段三：商户与二维码支付

### T3.1 二维码 — 商户生成收款码

**流程**：F17 前置条件
**业务场景**：事件 8，用例 8.1
**前置条件**：T1.2 + T1.4 已完成

**步骤**：
  1. 在商户 App 中，点击"生成收款二维码"
  2. 输入金额（如 5.00 USDC）
  3. 验证：屏幕显示二维码
  4. 验证：二维码包含支付详情（商户 DID、金额、订单 ID）
  5. 验证：商户 App 进入双通道等待（WebSocket + FCM）

**通过标准**：
  - [ ] 二维码生成，包含正确的支付详情
  - [ ] 商户 App 正在等待支付

**失败排查**：检查商户 MCP 连接、二维码生成服务

### T3.2 二维码支付 — 用户扫码支付

**流程**：F17
**业务场景**：事件 8，用例 8.2
**前置条件**：T1.3 + T3.1 已完成

**步骤**：
  1. 在用户 App 中，点击"扫描二维码"（支付扫描器，非配对扫描器）
  2. 扫描商户二维码
  3. 验证：显示支付详情（商户名称、金额、订单 ID）
  4. 验证：显示可用支付方式（Session Key、MagicBlock、Relayer）
  5. 用户选择支付方式（如 Session Key）
  6. 用户确认支付（滑动确认）
  7. App 通过 DIDComm 向 MCP 发送 `qr-payment-request`
  8. MCP 在链上执行支付
  9. MCP 向手机返回 `qr-payment-response`
  10. 验证：用户 App 显示"支付成功"

**通过标准**：
  - [ ] 二维码正确扫描和解析
  - [ ] 支付方式选择正常
  - [ ] 支付执行并确认
  - [ ] 显示成功页面

**失败排查**：检查二维码格式、DIDComm 消息路由、链上支付执行

### T3.3 语音播报 — 商户收到通知

**流程**：F18
**业务场景**：事件 8，用例 8.3
**前置条件**：T3.2 已完成

**步骤**：
  1. T3.2 支付成功后，买方 MCP 向商户 MCP 发送 `qr-payment-notify`
  2. 商户 MCP 收到通知
  3. 商户 MCP 转发到商户 App
  4. 验证：商户 App 播放语音播报（如"收款 5.00 USDC"）
  5. 验证：商户 App 显示支付确认，包含 tx 签名

**通过标准**：
  - [ ] 支付通知到达商户 App
  - [ ] 语音播报正确播放
  - [ ] 支付详情与原订单匹配

**失败排查**：检查商户 MCP 日志、推送通道（FCM/WebSocket）、TTS 引擎

---

## 阶段四：风控与商户管理

### T4.1 白名单 — 添加商户并验证自动批准

**流程**：F9
**App 测试用例**：TC-M04-05
**前置条件**：T1.3 已完成

**步骤**：
  1. 在手机上收到来自新商户的 payment-auth-request
  2. 在授权页面上，点击"加入白名单"操作 (TC-M04-05)
  3. 验证：商户已加入白名单
  4. 触发同一商户的另一笔支付（低于自动批准阈值）
  5. 验证：MCP 自动批准支付（无需通知手机）
  6. 验证：支付直接执行

**通过标准**：
  - [ ] 白名单添加成功
  - [ ] 后续支付在阈值内自动批准

**失败排查**：检查 risk_check() 逻辑、白名单存储

### T4.2 黑名单 — 屏蔽商户并验证拒绝

**流程**：F9
**App 测试用例**：TC-M04-06
**前置条件**：T1.3 已完成

**步骤**：
  1. 在手机上收到来自某商户的 payment-auth-request
  2. 在授权页面上，点击"加入黑名单"操作 (TC-M04-06)
  3. 验证：商户已加入黑名单
  4. 触发同一商户的另一笔支付
  5. 验证：支付自动被拒绝（无需通知手机）

**通过标准**：
  - [ ] 黑名单添加成功
  - [ ] 后续支付自动拒绝

**失败排查**：检查 risk_check() 中的黑名单查询

### T4.3 新商户授权 — 首次商户认证

**流程**：F8
**业务场景**：事件 4，用例 4.2
**前置条件**：T1.3 已完成，商户不在白名单中

**步骤**：
  1. Agent 发送来自新商户（从未见过）的支付请求
  2. MCP 检测到商户不在白名单中 → 触发 F8 流程
  3. MCP 向手机发送带额外商户信息的 `payment-auth-request`
  4. 验证：手机显示带"新商户"标识的授权页面
  5. 验证：页面显示商户详情和支付金额
  6. 用户批准
  7. 验证：支付执行，商户被记录

**通过标准**：
  - [ ] 正确检测到新商户
  - [ ] 手机显示新商户授权流程
  - [ ] 授权后支付完成

**失败排查**：检查商户注册表、风控评估逻辑

### T4.4 授权超额 — 额度提升请求

**流程**：F8（额度超限变体）
**前置条件**：T1.3 已完成，现有商户消费额度已用尽

**步骤**：
  1. 向某商户连续支付直到消费额度用尽
  2. 触发同一商户的另一笔支付
  3. MCP 检测到消费额度超限 → 向手机发送重新授权请求
  4. 验证：手机显示"授权超额"并提供提升额度选项
  5. 用户提升额度并批准
  6. 验证：使用新额度执行支付

**通过标准**：
  - [ ] 消费额度超限被检测到
  - [ ] 重新授权流程被触发
  - [ ] 额度提升后支付成功

**失败排查**：检查消费额度追踪、额度更新持久化

---

## 阶段五：MagicBlock 与高级支付

### T5.1 MagicBlock 存款 — 存入全局金库

**流程**：F10
**前置条件**：T1.3 已完成，MagicBlock 已配置

**步骤**：
  1. 在用户 App 中选择"存款"或通过 MCP 触发
  2. 输入存款金额（如 1 SOL）
  3. MCP 调用存入全局买家金库
  4. 验证：Solana devnet 上存款交易确认
  5. 验证：用户金库余额更新

**通过标准**：
  - [ ] 存款交易在链上确认
  - [ ] 金库余额反映存款

**失败排查**：检查全局金库 PDA、存款指令、devnet RPC

### T5.2 MagicBlock 凭证支付 — 链下凭证

**流程**：F6
**前置条件**：T5.1 已完成（金库中有资金），商户已开通 MagicBlock 通道

**步骤**：
  1. Agent 发送支持 MagicBlock 的商户支付请求
  2. MCP 选择凭证支付方式
  3. MCP 使用递增序列号签名链下凭证
  4. 验证：凭证存储在 VoucherStore
  5. 验证：支付在链下记录

**通过标准**：
  - [ ] 凭证使用正确序列号签名
  - [ ] 支付记录在链下存储中

**失败排查**：检查凭证序列分配、MagicBlock 通道状态

### T5.3 MagicBlock 批量结算

**流程**：F11
**前置条件**：T5.2 已完成（已累积凭证）

**步骤**：
  1. 触发批量结算流程
  2. MCP 从累积凭证重建 Merkle 树
  3. MCP 签名批量结算
  4. 商户在链上提交 `settle_batch` 或 `optimistic_settle`
  5. 验证：Solana devnet 上结算交易确认
  6. 验证：资金转移到商户

**通过标准**：
  - [ ] Merkle 树正确重建
  - [ ] 结算交易确认
  - [ ] 商户收到资金

**失败排查**：检查 Merkle root 计算、结算指令、链上状态

### T5.4 争议与仲裁

**流程**：F12
**前置条件**：T5.2 已完成（存在争议凭证）

**步骤**：
  1. 商户对某笔凭证支付发起争议
  2. MCP 为争议支付提供 Merkle 证明
  3. 在链上提交争议解决
  4. 验证：争议在链上记录
  5. 如判定买方胜诉：验证 `force_release` 已执行
  6. 如判定商户胜诉：验证资金已释放给商户

**通过标准**：
  - [ ] 使用有效 Merkle 证明提交争议
  - [ ] 链上执行争议解决

**失败排查**：检查 Merkle 证明有效性、链上争议指令

### T5.5 支付方式选择 — 多种方式间选择

**流程**：F16
**前置条件**：T1.3 已完成，多种支付方式可用

**步骤**：
  1. 收到多种支付方式可用的支付请求（Session Key、MagicBlock、Relayer）
  2. MCP 判断可用方式并发送到手机
  3. 验证：手机显示支付方式选择页面
  4. 选择每种方式并验证可用：
     - Session Key：链上 SOL/SPL 转账 (F5)
     - MagicBlock：链下凭证 (F6)
     - Relayer：代付支付 (F16 变体)

**通过标准**：
  - [ ] 显示所有可用方式
  - [ ] 选择每种方式后正确执行

**失败排查**：检查方式可用性逻辑、各方式执行路径

### T5.6 Relayer（代付）支付

**流程**：F16 变体
**前置条件**：T1.3 已完成，Relayer 已配置

**步骤**：
  1. 配置支付模式为"代付"(TC-M08-04)
  2. 触发支付请求
  3. MCP 以代付模式创建 Session Key
  4. Relayer 代付链上交易（Gas 费用）
  5. 验证：支付执行时用户无需支付 Gas
  6. 验证：交易在 Solana devnet 上确认

**通过标准**：
  - [ ] 代付 Session Key 创建成功
  - [ ] 支付执行时无需用户 Gas 费用
  - [ ] 交易确认

**失败排查**：检查 Relayer 服务、代付 Session Key 注册

---

<!-- State Channel: 探索阶段，暂不启用
## 阶段六：状态通道操作

### T6.1 开通通道

**状态通道场景**：SC-01
**前置条件**：通道服务运行中 (`make health`)，密钥文件在 `deploy/keys/` 中

**步骤**：
  1. 用户选择 Hub 进行通道创建
  2. POST `/v1/channels/open`，传入通道参数 (user, provider, deposit, tree_depth)
  3. 验证：Channel PDA 在 Solana devnet 上创建
  4. 验证：初始保证金锁定在 Escrow PDA
  5. POST `/v1/channels/fund` 进行追加注资
  6. 验证：Escrow 余额增加
  7. POST `/v1/channels/split` 初始化余额分配
  8. 验证：Merkle 树以正确的叶余额初始化
  9. 验证：金额守恒（叶金额之和 = 总存款）

**通过标准**：
  - [ ] 通道账户在链上创建 (status = Open)
  - [ ] Escrow 正确注资
  - [ ] Merkle root 与叶哈希匹配
  - [ ] 余额守恒成立

**失败排查**：检查 `deploy/keys/` 中的密钥文件、Solana devnet 连接、通道服务日志

### T6.2 链下支付

**状态通道场景**：SC-02
**前置条件**：T6.1 已完成（通道已开通）

**步骤**：
  1. POST `/v1/channels/{id}/pay`，传入支付金额和收款方
  2. 服务构建带有新余额分配的 LeafUpdate
  3. 验证：`sign_leaf_update` 生成有效 Ed25519 签名
  4. POST `/v1/channels/{id}/cosign` 获取 Provider 共签
  5. 验证：`apply_leaf_update` 正确应用
  6. 验证：Merkle root 更新
  7. 验证：序列号连续递增
  8. 重复多笔支付
  9. 验证：所有状态更新一致

**通过标准**：
  - [ ] 叶更新正确签名
  - [ ] Provider 共签成功
  - [ ] 每笔支付后 Merkle root 更新
  - [ ] 序列号连续

**失败排查**：检查签名验证、状态一致性

### T6.3 批量流水线支付

**状态通道场景**：SC-03
**前置条件**：T6.1 已完成

**步骤**：
  1. POST `/v1/channels/{id}/batch`，传入多个操作
  2. 使用 `Pipeline::new()` 创建流水线
  3. 添加操作：`transfer_leaf`、`partial_transfer`、`create_htlc`
  4. 执行流水线：`build()` 应用所有操作
  5. 验证：所有操作原子性成功（全部成功或全部失败）
  6. 验证：Merkle root 反映所有变更
  7. 测试回滚：提交无效操作 → `abort()` 被调用
  8. 验证：回滚后状态不变

**通过标准**：
  - [ ] 批量原子性执行
  - [ ] 所有叶更新正确应用
  - [ ] 失败时回滚正常

**失败排查**：检查流水线原子性、状态快照/恢复逻辑

### T6.4 HTLC 条件支付

**状态通道场景**：SC-04
**前置条件**：T6.1 已完成

**步骤**：
  1. POST `/v1/channels/{id}/htlc/create`，传入 hash_lock 和 timelock
  2. 验证：HTLC 使用正确参数创建
  3. 验证：Timelock > current_slot + challenge_duration + 1000 (HOP_MARGIN)
  4. 揭露原像：POST `/v1/channels/{id}/htlc/resolve`，传入 preimage
  5. 验证：原像与 hash_lock 匹配 (SHA-256)
  6. 验证：HTLC 已解决，资金已转移
  7. 测试退款：创建 HTLC，等待 timelock 过期
  8. POST `/v1/channels/{id}/htlc/refund`
  9. 验证：资金退回原所有者

**通过标准**：
  - [ ] HTLC 使用有效约束创建
  - [ ] 原像正确解决
  - [ ] timelock 过期后退款正常

**失败排查**：检查哈希计算、timelock slot 值、原像验证

### T6.5 协作关闭

**状态通道场景**：SC-05
**前置条件**：T6.1 + T6.2 已完成（通道存在链下状态）

**步骤**：
  1. 验证：通道中无活跃 HTLC
  2. POST `/{id}/close`，附带协作结算条款
  3. 双方签名 `cooperative_settle` 指令
  4. 验证：通道状态 → Settling
  5. POST `/{id}/claim`，为每片叶提供 Merkle 证明
  6. 验证：各所有者认领资金
  7. POST `/{id}/finalize` 完成结算
  8. 验证：通道已关闭，所有资金已分配

**通过标准**：
  - [ ] 获得双方签名
  - [ ] 所有索赔的 Merkle 证明有效
  - [ ] 资金正确分配
  - [ ] 通道状态 = Finalized

**失败排查**：检查是否有活跃 HTLC、签名有效性、Merkle 证明计算

### T6.6 争议解决

**状态通道场景**：SC-06
**前置条件**：T6.1 + T6.2 已完成，存在争议场景

**步骤**：
  1. POST `/{id}/challenge` — 用签名状态触发挑战
  2. 验证：通道状态 → Challenged
  3. 对手方提交：POST `/{id}/submit-counter`，附带更高序列号的状态
  4. 验证：sig_a + sig_b 均通过验证
  5. 验证：反状态具有更高的序列号
  6. 等待挑战持续时间结束
  7. POST `/{id}/settle` — 超时后结算
  8. 验证：通道状态 → Settling，使用最后有效状态

**通过标准**：
  - [ ] 挑战成功触发
  - [ ] 反状态被接受（更高序列号）
  - [ ] 超时后使用正确状态结算

**失败排查**：检查挑战持续时间、序列号、签名对

### T6.7 Hub 路由

**状态通道场景**：SC-08
**前置条件**：多个 Hub 已注册

**步骤**：
  1. POST `/v1/hub/register` — 注册新 Hub
  2. POST `/hub/metrics` — 更新 Hub 指标（延迟、可靠性）
  3. POST `/routes/add-edge` — 在 Hub 间添加路由边
  4. POST `/routes/find` — 发现从用户到商户的路由
  5. 验证：RouteService 基于指标返回最佳路由
  6. 测试无可用路由 → 验证优雅失败

**通过标准**：
  - [ ] Hub 注册正常
  - [ ] 路由发现找到有效路径
  - [ ] 基于指标选择最佳路由

**失败排查**：检查 Hub 注册表、路由图连通性

---

## 阶段七：E2E Demo

### T7.1 启动电商 Demo 服务器

**前置条件**：所有后端服务运行中，Session Key 已注资

**步骤**：
  1. `cd ignite-pay-ecom-demo`
  2. 安装依赖：`pip install -r requirements.txt`
  3. 启动服务器：`python server.py`
  4. 验证：服务器在 9090 端口监听
  5. 测试：`curl http://localhost:9090/products`
  6. 验证：返回 JSON 格式的产品列表

**通过标准**：
  - [ ] 服务器无错误启动
  - [ ] Products 端点返回有效 JSON

**失败排查**：检查 Python 依赖、端口可用性

### T7.2 运行 Mock 测试

**步骤**：
  1. 运行 Mock 测试脚本：`python test_flow.py`（如有）
  2. 验证：所有 Mock 支付流程完成
  3. 验证：输出无错误

**通过标准**：
  - [ ] Mock 测试端到端通过

**失败排查**：检查 Mock 配置、服务连接

### T7.3 完整 E2E：Agent -> x402 -> MCP -> 手机 -> 支付 -> 订单

**前置条件**：T7.1 已完成，手机已配对 MCP (T1.3)

**步骤**：
  1. Agent 调用 `GET /products` → 收到产品列表
  2. Agent 调用 `POST /orders {"product_id": "coffee"}` → 收到 402 挑战
  3. Agent 调用 MCP `process_x402_challenge()` → MCP 发送到手机
  4. 手机批准 → Session Key 创建 → 链上注册
  5. MCP 执行支付 → Solana 确认
  6. Agent 携带 `X-Payment-Proof` 重发 `POST /orders`
  7. 服务器验证链上交易：
     - 交易存在且已确认
     - 交易无错误
     - 收款方余额增长 >= 预期金额
  8. 验证：订单状态 = "paid"，带有 tx_signature

**通过标准**：
  - [ ] 从产品列表到已支付订单的完整流程
  - [ ] 链上验证通过
  - [ ] 订单以有效交易签名确认

**失败排查**：逐步排查 — 检查 MCP 日志、手机 Mediator、devnet RPC、支付证明格式

---

## 阶段八：App 设置与边界场景

### T8.1 网络切换

**App 测试用例**：TC-M08-01, TC-M08-02
**步骤**：
  1. 进入 设置 → 网络
  2. 从 devnet 切换到 mainnet (TC-M08-01)
  3. 验证：RPC URL 更新，App 重新连接
  4. 设置自定义 RPC URL (TC-M08-02)
  5. 杀掉并重启 App
  6. 验证：自定义 RPC URL 持久化

### T8.2 支付模式切换

**App 测试用例**：TC-M08-04
**步骤**：
  1. 进入 设置 → 支付模式
  2. 从"自付"切换到"代付"
  3. 触发支付
  4. 验证：支付使用代付 (Relayer) 模式
  5. 切换回"自付"
  6. 验证：下一笔支付使用自付 Session Key

### T8.3 Deep Link 回调

**App 测试用例**：TC-M10-01, TC-M10-02, TC-M10-03
**前置条件**：已安装外部钱包 (Phantom/Solflare)

**步骤**：
  1. 配对 MCP，收到支付请求
  2. 选择 Phantom/Solflare 作为签名方式
  3. 钱包打开，用户签名
  4. Deep link 回调：`ignitepay://onchain?signature=...` (TC-M10-01)
  5. 验证：App 收到回调，Key 注册成功
  6. 测试：无待处理交易时的回调 (TC-M10-02) → 验证被忽略
  7. 测试：无效签名的回调 (TC-M10-03) → 验证显示错误

### T8.4 消息列表与筛选

**App 测试用例**：TC-M06-01 至 TC-M06-06
**步骤**：
  1. 打开消息页面 (TC-M06-01)
  2. 验证：消息列表显示最近消息
  3. 按类型筛选：全部 / 支付 / 列表同步 / 连接 (TC-M06-02)
  4. 点击支付消息 → 验证 ChallengeScreen 打开 (TC-M06-03)
  5. 点击非支付消息 → 验证详情弹窗 (TC-M06-04)
  6. 下拉刷新 → 验证列表更新 (TC-M06-05)
  7. 清空消息 → 验证空状态 (TC-M06-06)

### T8.5 风控策略

**App 测试用例**：TC-M07-01, TC-M07-02, TC-M07-03
**步骤**：
  1. 打开风控页面 (TC-M07-01)
  2. 验证：策略列表显示 4 个商户卡片
  3. 点击卡片展开详情 (TC-M07-02)
  4. 切换自动支付开关 (TC-M07-03)
  5. 验证：切换在导航后保持

---

## 测试结果跟踪

复制此表并在测试过程中填写：

| 测试 ID | 测试名称 | 结果 | 备注 | 日期 |
|---------|---------|------|------|------|
| T1.1 | 用户 App 首次启动 | | | |
| T1.2 | 商户 App 首次启动 | | | |
| T1.3 | 手机-MCP 配对 | | | |
| T1.4 | 商户-MCP 配对 | | | |
| T2.1 | x402 首次支付 | | | |
| T2.2 | Session Key 复用 | | | |
| T2.3 | 余额不足充值 | | | |
| T2.4 | Session Key 续期 | | | |
| T3.1 | 二维码生成 | | | |
| T3.2 | 二维码扫码支付 | | | |
| T3.3 | 语音播报 | | | |
| T4.1 | 白名单自动批准 | | | |
| T4.2 | 黑名单拒绝 | | | |
| T4.3 | 新商户授权 | | | |
| T4.4 | 授权超额 | | | |
| T5.1 | MagicBlock 存款 | | | |
| T5.2 | 凭证支付 | | | |
| T5.3 | 批量结算 | | | |
| T5.4 | 争议与仲裁 | | | |
| T5.5 | 支付方式选择 | | | |
| T5.6 | Relayer 代付 | | | |
<!-- State Channel: 探索阶段，暂不启用
| T6.1 | 开通通道 | | | |
| T6.2 | 链下支付 | | | |
| T6.3 | 批量流水线 | | | |
| T6.4 | HTLC 条件支付 | | | |
| T6.5 | 协作关闭 | | | |
| T6.6 | 争议解决 | | | |
| T6.7 | Hub 路由 | | | |
-->
| T7.1 | 电商 Demo 启动 | | | |
| T7.2 | Mock 测试 | | | |
| T7.3 | 完整 E2E 流程 | | | |
| T8.1 | 网络切换 | | | |
| T8.2 | 支付模式切换 | | | |
| T8.3 | Deep Link 回调 | | | |
| T8.4 | 消息列表与筛选 | | | |
| T8.5 | 风控策略 | | | |

**结果取值**：PASS | FAIL | SKIP | N/A
