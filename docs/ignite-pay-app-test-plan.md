# Ignite Pay App — 功能测试文档

> 版本：V2.0 | 覆盖范围：身份管理、MCP 配对、支付授权、会话密钥、消息通信、风控策略、设置管理
> 测试环境：Android 模拟器 (API 36, x86_64) + Solana Devnet

---

## 1. 测试总览

### 1.1 业务模块划分

| 编号 | 模块 | 关联屏幕 | 优先级 |
|:-----|:-----|:---------|:-------|
| M01 | 首次启动与身份创建 | OnboardingScreen | P0 |
| M02 | DID 身份管理 | VaultScreen | P1 |
| M03 | MCP 配对连接 | QrScannerScreen, ConnectionScreen | P0 |
| M04 | 支付授权（X402 挑战） | ChallengeScreen | P0 |
| M05 | 会话密钥管理 | SessionKeysScreen, ChallengeScreen | P0 |
| M06 | 消息通信 | MessagesScreen | P1 |
| M07 | 风控策略 | PolicyScreen | P2 |
| M08 | 应用设置 | SettingsScreen | P2 |
| M09 | 推送通道 | FcmService / WebSocket | P1 |
| M10 | Deep Link 回调 | AndroidManifest, MainNavigator | P1 |

### 1.2 前置条件

| 条件 | 说明 |
|:-----|:-----|
| Solana Devnet 可达 | RPC URL `https://api.devnet.solana.com` 可正常请求 |
| Mediator 服务运行 | 本地或远程 Mediator WebSocket 服务可连接 |
| MCP Server 运行 | 能生成 `didcomm://?_oob=...` 二维码的 MCP 服务 |
| Phantom/Solflare | 安装至少一个 Solana 钱包（测试 Deep Link 时） |
| FCM 可用 | Google Play Services 正常（测试 FCM 推送时） |

---

## 2. M01 — 首次启动与身份创建

### 2.1 业务规则

| 规则编号 | 约束 |
|:---------|:-----|
| R01-01 | 首次启动（DID 为空）→ 显示 Onboarding 向导，不显示 Dashboard |
| R01-02 | 非首次启动（DID 已存在）→ 直接进入 Dashboard |
| R01-03 | DID 格式必须为 `did:ignite:z6Mk...`（Ed25519 多基数编码） |
| R01-04 | 身份创建不可跳过；Mediator 连接可跳过 |
| R01-05 | Mediator 连接失败不阻塞向导完成 |

### 2.2 用例

#### TC-M01-01 首次启动完整向导流程

```
前置：清除应用数据（Settings → Clear Cache 或卸载重装）

步骤：
  1. 启动应用
  2. 验证：显示 WelcomeStep（"Sentinel — Your AI Payment Guardian"）
  3. 点击 "Get Started"
  4. 验证：进入 CreateIdentityStep，显示 "Generate DID" 按钮
  5. 点击 "Generate DID"
  6. 验证：按钮变为 loading spinner
  7. 等待 DID 生成完成
  8. 验证：显示 DID 卡片，DID 格式为 "did:ignite:z6Mk..."，32字符以上
  9. 点击 "Continue"
  10. 验证：进入 MediatorConfigStep，WS URL 默认为 "wss://relay.ignite.did"
  11. 点击 "Skip"
  12. 验证：显示 "You're all set!" 完成页
  13. 点击 "Enter Sentinel"
  14. 验证：进入 Dashboard 主页，DID 卡片显示已生成的 DID

预期结果：DID 成功创建并持久化，再次启动直接进入 Dashboard
```

#### TC-M01-02 重复启动跳过向导

```
前置：已完成 TC-M01-01

步骤：
  1. 完全退出应用
  2. 重新启动应用

预期结果：直接显示 Dashboard，不出现 Onboarding 向导，DID 卡片显示已创建的身份
```

#### TC-M01-03 DID 创建失败处理

```
前置：清除应用数据；制造存储异常（如磁盘满）

步骤：
  1. 启动应用 → Get Started → Generate DID

预期结果：显示红色 SnackBar 错误提示，可重试
```

#### TC-M01-04 Mediator 连接成功

```
前置：已完成身份创建；Mediator 服务可达

步骤：
  1. 在 MediatorConfigStep 输入正确的 WS URL
  2. 点击 "Connect & Continue"
  3. 验证：按钮显示 loading spinner
  4. 等待连接成功

预期结果：显示 "You're all set!"，Dashboard 连接状态为绿色
```

#### TC-M01-05 Mediator 连接失败可跳过

```
前置：已完成身份创建；Mediator 服务不可达

步骤：
  1. 在 MediatorConfigStep 输入错误的 WS URL
  2. 点击 "Connect & Continue"
  3. 等待连接超时/失败

预期结果：显示错误提示，可重试或点击 "Skip" 继续完成向导
```

### 2.3 交互流程图

```
App Launch
    │
    ├─ DID 存在? ──Yes──> Dashboard
    │
    └─ No
        │
        ▼
    WelcomeStep ──Get Started──> CreateIdentityStep
                                      │
                              Generate DID
                                      │
                            ┌──Success──┴──Error──> SnackBar + 重试
                            │
                            ▼
                     MediatorConfigStep
                      │              │
              Connect & Continue    Skip
                      │              │
               ┌──Success──┐        │
               │           │        │
             Error      OnboardingComplete <───┘
               │           │
            SnackBar    Enter Sentinel
               │           │
            重试/跳过    Dashboard
```

---

## 3. M02 — DID 身份管理

### 3.1 业务规则

| 规则编号 | 约束 |
|:---------|:-----|
| R02-01 | DID 可复制到剪贴板 |
| R02-02 | 助记词默认隐藏，点击显示，再次点击隐藏 |
| R02-03 | "Erase Key Material" 为演示功能，不实际删除 |
| R02-04 | 审计日志展示本地操作记录（签名、密钥派生等） |

### 3.2 用例

#### TC-M02-01 查看 DID 身份

```
前置：已完成身份创建

步骤：
  1. Dashboard → "Vault" 快捷入口，或 Settings → "Vault & Identity"
  2. 验证：显示 Vault 页面
  3. 验证：Identity Hero Card 显示完整 DID，格式 "did:ignite:z6Mk..."
  4. 点击 DID 文本
  5. 验证：显示 "Copied to clipboard" SnackBar

预期结果：DID 正确展示，可复制
```

#### TC-M02-02 查看助记词

```
前置：进入 Vault 页面

步骤：
  1. 找到 "Secret Phrase" 磁贴
  2. 验证：内容被遮罩（显示 "••••••••..." 或类似）
  3. 点击眼睛图标
  4. 验证：显示 12 个单词（orbit, glacier, velvet, phoenix, tundra, mirror, beacon, labyrinth, cascade, ember, zenith, prism）
  5. 再次点击眼睛图标
  6. 验证：内容重新被遮罩

预期结果：助记词可切换显示/隐藏
```

#### TC-M02-03 查看审计日志

```
前置：进入 Vault 页面

步骤：
  1. 点击 "Audit Logs" 磁贴
  2. 验证：进入审计日志页面
  3. 验证：显示操作记录列表（sign_payment, key_derive 等类型）
  4. 验证：每条记录包含时间戳和操作类型

预期结果：审计日志正确展示
```

---

## 4. M03 — MCP 配对连接

### 4.1 业务规则

| 规则编号 | 约束 |
|:---------|:-----|
| R03-01 | QR 码必须以 `didcomm://` 开头，否则提示格式错误 |
| R03-02 | 扫码成功后自动解析 OOB invitation 并发送 connection-request |
| R03-03 | 连接请求通过 WS 通道发送（已连接时）或 HTTPS（未连接时） |
| R03-04 | Mediator 连接状态实时反映在 Connection 页面 |
| R03-05 | MCP 列表从已有消息中提取去重的 merchant DID |

### 4.2 用例

#### TC-M03-01 扫码配对成功

```
前置：Mediator 已连接；MCP Server 运行并展示 QR 码

步骤：
  1. Dashboard → 点击 "Scan MCP QR Code" / "Pair New MCP"
  2. 验证：QR 扫描界面打开，显示 260×260 扫描框和四角指示器
  3. 对准 MCP QR 码（格式 didcomm://?_oob=<base64url>）
  4. 验证：扫码成功，界面自动关闭
  5. 验证：返回 Dashboard，SnackBar 提示 "Connected to MCP: did:ignite:..."

预期结果：MCP 成功配对，Connection 页面可见该 MCP DID
```

#### TC-M03-02 无效 QR 码

```
前置：QR 扫描界面打开

步骤：
  1. 扫描非 didcomm:// 开头的 QR 码（如 URL、文本）

预期结果：显示红色错误提示 "Invalid invitation URL" 或不响应
```

#### TC-M03-03 Mediator 连接管理

```
前置：进入 Settings → Connections

步骤：
  1. 验证：Mediator Card 显示当前连接状态（Connected/Disconnected）
  2. 输入新的 WS URL
  3. 点击 Connect
  4. 验证：按钮显示 loading，连接成功后状态变为 Connected（绿色圆点）
  5. 点击 Disconnect
  6. 验证：状态变为 Disconnected（红色圆点）

预期结果：Mediator 连接可正常建立和断开
```

#### TC-M03-04 推送通道显示

```
前置：已连接 Mediator

步骤：
  1. 进入 Settings → Connections
  2. 查看 Push Channel Card

预期结果：
  - 中文用户（zh_CN locale）：显示 "WebSocket" 徽章
  - 海外用户：显示 "FCM" 徽章
```

#### TC-M03-05 未连接时扫码

```
前置：Mediator 未连接

步骤：
  1. 点击 "Scan MCP QR Code"
  2. 扫描有效 QR 码

预期结果：系统自动先连接 Mediator，再发送连接请求；或提示先配置 Mediator
```

### 4.3 交互流程图

```
Dashboard ──Scan QR──> QrScannerScreen
                            │
                    检测到 didcomm:// URL
                            │
                    ┌──格式正确──┴──格式错误──> 红色提示
                    │
                    ▼
            parseOobInvitation()
                    │
                    ▼
            sendConnectionRequest()
              │              │
         WS 已连接        WS 未连接
              │              │
         WS 发送 JWE    HTTP POST 发送
              │              │
              └──────┬───────┘
                     │
              ┌─Success──┴──Error──> SnackBar
              │
              ▼
         返回 Dashboard
         SnackBar: "Connected to MCP: ..."
```

---

## 5. M04 — 支付授权（X402 挑战）

### 5.1 业务规则

| 规则编号 | 约束 |
|:---------|:-----|
| R04-01 | 收到 `payment-auth-request` 消息后弹出 Challenge 全屏弹窗 |
| R04-02 | 金额以 SOL 显示（lamports ÷ 10^9），大金额取整，小金额保留 4 位有效小数 |
| R04-03 | 滑动授权需拖拽到 85% 位置才触发，否则弹回 |
| R04-04 | 授权时自动检查是否有活跃会话密钥，有则复用 |
| R04-05 | 无活跃密钥时弹出签名方式选择器（Built-in / Deep Link / Mobile Wallet） |
| R04-06 | Spending Limit 默认设为支付金额的 10 倍 |
| R04-07 | Session Key 有效期默认 3600 秒（1 小时） |
| R04-08 | List Action 默认 "none"（仅本次） |
| R04-09 | 选择 "Whitelist" 或 "Blacklist" 后显示 Label 输入框 |
| R04-10 | 选择 "Whitelist" 后额外显示 Max Amount 输入框 |
| R04-11 | 拒绝操作直接关闭弹窗，返回 "declined" |

### 5.2 用例

#### TC-M04-01 完整授权流程（无活跃密钥 + Built-in 签名）

```
前置：无活跃会话密钥；收到 payment-auth-request

步骤：
  1. Dashboard 出现 amber "Payment authorization requested" 横幅
  2. 点击 "Authorize Payment"
  3. 验证：Challenge 弹窗显示
     - Merchant Card: 商户 DID（截断显示）
     - Amount: 大字号 SOL 金额
     - Reason: 支付描述
     - List Action: 默认选中 "This time only"
     - Slide to Authorize 滑动条
     - Decline & Block 按钮
  4. 验证：顶部显示 loading spinner（检查已有密钥）
  5. 验证：spinner 消失后显示 "No existing session key" 状态
  6. 向右拖拽 "Slide to Authorize" 滑块到 85% 以上
  7. 验证：弹出 Signing Method 选择器底部弹窗
  8. 验证：三个选项可见 — Built-in Key, Phantom/Solflare, Mobile Wallet
  9. 选择 "Built-in Key"
  10. 验证：ResultBanner 显示 "Registering session key on-chain..."
  11. 等待注册完成
  12. 验证：ResultBanner 显示 "Authorized with session key"
  13. 验证：1.2 秒后弹窗关闭，返回 "authorized"

预期结果：支付成功授权，会话密钥注册上链
```

#### TC-M04-02 授权流程（有活跃密钥）

```
前置：已有活跃会话密钥（通过 Session Keys 页面注册）

步骤：
  1. 收到 payment-auth-request
  2. 点击 "Authorize Payment"
  3. 验证：Challenge 弹窗显示，"Using existing session key" banner
  4. 拖拽滑块授权
  5. 验证：不弹出签名方式选择器，直接使用已有密钥
  6. 验证：显示 "Authorized with existing session key"

预期结果：跳过密钥创建，直接使用已有活跃密钥
```

#### TC-M04-03 滑动未达阈值弹回

```
前置：Challenge 弹窗打开

步骤：
  1. 拖拽 "Slide to Authorize" 滑块到约 50% 位置
  2. 松手

预期结果：滑块弹回起点，不触发任何操作
```

#### TC-M04-04 拒绝支付

```
前置：Challenge 弹窗打开

步骤：
  1. 点击 "Decline & Block" 按钮

预期结果：弹窗关闭，返回 "declined"
```

#### TC-M04-05 List Action — 添加白名单

```
前置：Challenge 弹窗打开

步骤：
  1. 点击 "Whitelist" chip
  2. 验证：chip 高亮（绿色边框）
  3. 验证：下方出现 Label 输入框
  4. 验证：下方出现 Max Amount 输入框
  5. 输入 Label: "ShopX Marketplace"
  6. 输入 Max Amount: "1000000000"
  7. 滑动授权

预期结果：授权请求携带 list_action="add_whitelist", label="ShopX Marketplace", max_amount=1000000000
```

#### TC-M04-06 List Action — 添加黑名单

```
前置：Challenge 弹窗打开

步骤：
  1. 点击 "Blacklist" chip
  2. 验证：chip 高亮（红色边框）
  3. 验证：出现 Label 输入框，无 Max Amount 输入框
  4. 输入 Label: "Scam Site"
  5. 滑动授权

预期结果：授权请求携带 list_action="add_blacklist", label="Scam Site"
```

#### TC-M04-07 List Action — 移除操作

```
前置：Challenge 弹窗打开

步骤：
  1. 点击 "Remove WL" chip → 验证高亮
  2. 验证：无额外输入框
  3. 切换到 "Remove BL" → 验证高亮
  4. 滑动授权

预期结果：list_action 分别为 "remove_whitelist" / "remove_blacklist"
```

#### TC-M04-08 签名方式 — Deep Link

```
前置：Challenge 弹窗打开；安装了 Phantom 钱包

步骤：
  1. 拖拽滑块到 85% 以上
  2. 在签名方式选择器中选择 "Phantom / Solflare"
  3. 验证：显示 "Open wallet to sign transaction..." ResultBanner
  4. 验证：尝试打开 Phantom 钱包（如已安装则跳转）

预期结果：生成 unsigned tx 并存储到 pending，构建 Phantom deep link URL
```

#### TC-M04-09 授权中网络错误

```
前置：断开网络连接

步骤：
  1. 触发 Challenge 弹窗
  2. 选择 Built-in Key 签名
  3. 等待超时

预期结果：ResultBanner 显示 "Error: ..." 红色提示，滑块恢复可操作状态
```

### 5.3 交互流程图

```
payment-auth-request 到达
         │
         ▼
Dashboard 显示 amber 横幅
         │
   "Authorize Payment"
         │
         ▼
ChallengeScreen 打开
    │
    ├─ 检查活跃密钥 (loading spinner)
    │       │
    │   ┌─有活跃密钥──────────┐
    │   │                     │
    │   ▼                     ▼
    │ "Using existing     签名方式选择器
    │  session key"      ┌────┼────────┐
    │       │         Built-in  Deep Link  MWA
    │       │            │         │        │
    │       │            ▼         ▼        ▼
    │       │     createWith    build     (stub →
    │       │     BuiltInKey    Unsigned  Built-in)
    │       │            │      Tx
    │       │            │         │
    │       │            ▼         ▼
    │       │     on-chain    打开钱包 app
    │       │     注册        (等待回调)
    │       │            │         │
    │       │            ▼         ▼
    │       └────────► sendAuthResponse
    │                    withSessionKey
    │                         │
    │                    ┌─Success──┴──Error──> ResultBanner 红色
    │                    │
    │                    ▼
    │              "Authorized" banner
    │              1.2s 后 pop('authorized')
    │
    └─ "Decline & Block" → pop('declined')
```

---

## 6. M05 — 会话密钥管理

### 6.1 业务规则

| 规则编号 | 约束 |
|:---------|:-----|
| R05-01 | 会话密钥通过 Solana Session Program 注册上链（Program ID: `6EFvVTh7...`） |
| R05-02 | 本地存储格式：sled key `session:{base58_pubkey}` → `[64B keypair \| 8B expires_at LE \| 8B spending_limit LE]` |
| R05-03 | 密钥状态判定：`expires_at < now` → "expired"，否则 → "active" |
| R05-04 | Revoke 操作在链上执行（提交 revoke_session 指令），Delete 操作仅删除本地记录 |
| R05-05 | 通过 Session Keys 页面注册默认：spending_limit=5 SOL, duration=86400s (24h) |
| R05-06 | 通过 Challenge 授权注册默认：spending_limit=支付金额×10, duration=3600s (1h) |
| R05-07 | Revoke 前不删除本地记录（仍可见但标记为 expired） |
| R05-08 | Delete 需二次确认对话框 |

### 6.2 用例

#### TC-M05-01 查看空密钥列表

```
前置：清除应用数据或无已注册密钥

步骤：
  1. Settings → "Session Keys"
  2. 验证：显示空状态 — Key 图标 + "No session keys registered" + "Register a new key..." 描述
  3. 验证：顶部显示 "Register New Key" 渐变按钮

预期结果：空状态正确展示
```

#### TC-M05-02 注册新密钥（Built-in 方式）

```
前置：进入 Session Keys 页面；网络可达

步骤：
  1. 点击 "Register New Key" 按钮
  2. 验证：按钮变为 "Registering..." 并显示 loading spinner
  3. 等待注册完成
  4. 验证：
     - 按钮恢复 "Register New Key"
     - 列表出现新密钥卡片
     - 卡片显示：缩短的 pubkey, "active" 绿色徽章, 过期时间 "23h 59m left", 限额 "5 SOL"
     - "Revoke" 和 "Delete" 操作按钮可见

预期结果：密钥注册成功，列表正确展示
```

#### TC-M05-03 撤销密钥（On-chain Revoke）

```
前置：已有至少一个活跃密钥

步骤：
  1. 点击某个活跃密钥卡片的 "Revoke" 按钮
  2. 等待链上交易确认
  3. 验证：显示绿色 SnackBar "Revoked on-chain: <tx_sig>..."
  4. 验证：刷新后列表更新

预期结果：链上撤销成功，本地记录仍保留（可手动删除）
```

#### TC-M05-04 删除本地密钥 — 确认

```
前置：已有至少一个密钥

步骤：
  1. 点击 "Delete" 按钮
  2. 验证：弹出确认对话框 "Delete Local Key?"
  3. 验证：对话框说明 "This removes the key from local storage only..."
  4. 点击 "Delete"

预期结果：密钥从列表消失
```

#### TC-M05-05 删除本地密钥 — 取消

```
前置：已有至少一个密钥

步骤：
  1. 点击 "Delete" 按钮
  2. 弹出确认对话框
  3. 点击 "Cancel"

预期结果：密钥仍在列表中，无变化
```

#### TC-M05-06 密钥过期状态

```
前置：等待密钥超过 expires_at 时间

步骤：
  1. 打开 Session Keys 页面
  2. 查看过期密钥卡片

预期结果：
  - 状态徽章显示 "expired"（红色）
  - 过期时间显示 "Expired"
  - Revoke 和 Delete 操作仍可用
```

#### TC-M05-07 Deep Link 回调完成注册

```
前置：通过 Challenge 界面选择 Deep Link 签名方式；外部钱包已签名

步骤：
  1. 外部钱包签名后回调 ignitepay://onchain?signature=<sig>
  2. 验证：应用前台恢复时，MainNavigator 捕获 deep link
  3. 验证：调用 SessionKeyService.completeRegistration(signature)
  4. 验证：Session Keys 列表出现新密钥
  5. 验证：debugPrint 输出 "Session key registered: <pubkey>"

预期结果：Deep Link 回调正确完成密钥注册
```

#### TC-M05-08 注册失败处理

```
前置：网络不可达

步骤：
  1. 点击 "Register New Key"
  2. 等待超时

预期结果：显示红色 SnackBar "Registration failed: ..."
```

### 6.3 交互流程图

```
Settings → Session Keys
        │
        ├─ Register New Key
        │       │
        │   createWithBuiltInKey(5 SOL, 24h)
        │       │
        │   ┌─Success──┴──Error──> 红色 SnackBar
        │   │
        │   ▼
        │ 绿色 SnackBar + 列表刷新
        │
        ├─ Revoke（某密钥）
        │       │
        │   revoke_session_key_onchain()
        │       │
        │   ┌─Success──┴──Error──> 红色 SnackBar
        │   │
        │   ▼
        │ 绿色 SnackBar "Revoked on-chain: ..."
        │
        └─ Delete（某密钥）
                │
            确认对话框
              │       │
           Cancel    Delete
              │       │
            无变化  delete_session_key_local()
                      │
                      ▼
                  列表移除该密钥
```

---

## 7. M06 — 消息通信

### 7.1 业务规则

| 规则编号 | 约束 |
|:---------|:-----|
| R06-01 | 消息列表按时间倒序排列（最新在前） |
| R06-02 | 筛选器支持：All, Payment, List Sync, Connection |
| R06-03 | Payment 类型消息点击 → 打开 ChallengeScreen |
| R06-04 | 非 Payment 类型消息点击 → 打开 Message Detail Dialog |
| R06-05 | 下拉刷新触发 Mediator 重连 + 消息拉取 |
| R06-06 | 空列表显示 "No messages yet" + "Check for messages" 按钮 |

### 7.2 用例

#### TC-M06-01 查看消息列表

```
前置：已连接 Mediator；有历史消息

步骤：
  1. 切换到 Messages tab
  2. 验证：消息列表显示
  3. 验证：每条消息卡片包含：类型图标、商户 DID（截断）、描述、金额（Payment 类型）

预期结果：消息正确展示，按时间倒序
```

#### TC-M06-02 筛选消息

```
前置：消息列表中有多种类型消息

步骤：
  1. 点击 "Payment" filter chip
  2. 验证：仅显示 payment 类型消息
  3. 点击 "List Sync" chip
  4. 验证：仅显示 list-sync 类型消息
  5. 点击 "All"
  6. 验证：显示所有消息

预期结果：筛选功能正确
```

#### TC-M06-03 点击 Payment 消息

```
前置：消息列表有 Payment 类型消息

步骤：
  1. 点击一条 Payment 消息

预期结果：打开 ChallengeScreen，携带该消息的 paymentId, merchantDid, amount, description
```

#### TC-M06-04 点击非 Payment 消息

```
前置：消息列表有非 Payment 类型消息（如 list-sync-notification）

步骤：
  1. 点击该消息

预期结果：弹出 Message Detail Dialog，显示所有字段（msgType, rawBody 等）
```

#### TC-M06-05 下拉刷新

```
前置：Messages 页面打开

步骤：
  1. 下拉页面
  2. 验证：显示 RefreshIndicator
  3. 等待刷新完成

预期结果：重新连接 Mediator 并拉取最新消息
```

#### TC-M06-06 空消息列表

```
前置：已连接 Mediator 但无消息

步骤：
  1. 切换到 Messages tab
  2. 验证：显示空状态 — 收件箱图标 + "No messages yet"
  3. 点击 "Check for messages" 按钮
  4. 验证：触发消息拉取

预期结果：空状态正确展示，手动拉取功能正常
```

---

## 8. M07 — 风控策略

### 7.1 业务规则

| 规则编号 | 约束 |
|:---------|:-----|
| R07-01 | 策略卡片可展开/折叠 |
| R07-02 | Auto-pay 开关可切换 |
| R07-03 | 限额输入支持 SOL/USD 单位切换 |
| R07-04 | 有效期通过日期选择器设置 |
| R07-05 | 当前数据为 Mock 硬编码（ShopX, DeFi, NFT, RPC） |

### 8.2 用例

#### TC-M07-01 查看策略列表

```
步骤：
  1. Dashboard → "Policies" 或 Settings → "Policy Architect"
  2. 验证：显示 4 个商户策略卡片（ShopX, DeFi, NFT, RPC）
  3. 验证：统计网格显示 Merchants=4, Auto-Pay=2, Weekly Cap=3.00 SOL, Spent=0.47 SOL

预期结果：策略页面正确展示
```

#### TC-M07-02 展开策略详情

```
步骤：
  1. 点击某个商户策略卡片
  2. 验证：卡片展开显示详情
     - Auto-pay 开关
     - Single Limit 输入框 + SOL/USD 切换
     - Weekly Velocity 进度条
     - Expiry 日期选择 + 天数徽章

预期结果：卡片正确展开，所有子组件可见
```

#### TC-M07-03 切换 Auto-pay

```
步骤：
  1. 展开某个商户策略
  2. 切换 Auto-pay 开关

预期结果：开关状态切换（当前为 UI 演示，不影响实际逻辑）
```

---

## 9. M08 — 应用设置

### 9.1 业务规则

| 规则编号 | 约束 |
|:---------|:-----|
| R08-01 | 切换 Network 自动更新 RPC URL（devnet → `https://api.devnet.solana.com`, mainnet-beta → `https://api.mainnet-beta.solana.com`） |
| R08-02 | Program IDs 为只读展示（State Channel, DID ZK, Session Key） |
| R08-03 | Payment Mode 切换：Self-Funded / Sponsored |
| R08-04 | Clear Cache 需二次确认 |

### 9.2 用例

#### TC-M08-01 网络切换

```
步骤：
  1. Settings → Solana Network
  2. 选择 "mainnet-beta"
  3. 验证：RPC URL 自动更新为 https://api.mainnet-beta.solana.com
  4. 选择 "devnet"
  5. 验证：RPC URL 恢复为 https://api.devnet.solana.com

预期结果：网络切换正确联动 RPC URL
```

#### TC-M08-02 自定义 RPC URL

```
步骤：
  1. 在 RPC URL 输入框中输入自定义 URL
  2. 导航到其他页面再返回

预期结果：自定义 URL 被持久化保存
```

#### TC-M08-03 查看 Program IDs

```
步骤：
  1. 滚动到 Program IDs 区块
  2. 验证显示三个只读 ID：
     - State Channel: DJBHr35j...
     - DID ZK Compression: ignDID...
     - Session Key: 6EFvVTh7...

预期结果：Program IDs 正确展示且不可编辑
```

#### TC-M08-04 支付模式切换

```
步骤：
  1. 在 Payment Mode 区块切换 "Sponsored"
  2. 验证：选中状态更新
  3. 重启应用
  4. 验证：设置已持久化

预期结果：支付模式切换并持久化
```

#### TC-M08-05 清除缓存

```
步骤：
  1. 点击 "Clear Cache"
  2. 验证：弹出确认对话框
  3. 点击确认

预期结果：显示操作结果 SnackBar
```

---

## 10. M09 — 推送通道

### 10.1 业务规则

| 规则编号 | 约束 |
|:---------|:-----|
| R09-01 | 海外用户（非 zh_CN locale）使用 FCM 推送 |
| R09-02 | 中国大陆用户（zh_CN / Hans）使用 WebSocket 长连接 |
| R09-03 | FCM 前台通知显示本地通知（标题 "Payment Authorization"） |
| R09-04 | FCM 后台通过 top-level handler 处理 |
| R09-05 | WS 断连后自动重连（3 秒延迟）+ 拉取断连期间消息 |
| R09-06 | 任何通道恢复后触发 HTTPS Pull 兜底拉取 |

### 10.2 用例

#### TC-M09-01 FCM 推送接收（海外用户）

```
前置：海外 locale（en-US）；FCM token 已注册；应用在前台

步骤：
  1. MCP Server 发送支付请求
  2. Mediator 通过 FCM 推送信号
  3. 验证：手机收到本地通知 "Payment Authorization" / "New payment request received"
  4. 验证：DidcommService 触发消息拉取
  5. 验证：Dashboard 显示 pending auth 横幅

预期结果：FCM 推送正确触发消息拉取和 UI 更新
```

#### TC-M09-02 WebSocket 推送接收（中国用户）

```
前置：中文 locale（zh_CN）；Mediator 已连接

步骤：
  1. MCP Server 发送支付请求
  2. Mediator 通过 WS 直接推送 JWE
  3. 验证：_onWsMessage 触发
  4. 验证：消息解密并加入 messages 列表
  5. 验证：Dashboard 显示 pending auth 横幅

预期结果：WS 推送正确接收和处理
```

#### TC-M09-03 WS 断连重连

```
前置：中文用户；WS 连接正常

步骤：
  1. 断开网络连接
  2. 验证：WS 连接断开，触发 onDone 回调
  3. 恢复网络
  4. 等待 3 秒重连
  5. 验证：WS 重新建立连接
  6. 验证：拉取断连期间的消息

预期结果：自动重连并补拉消息
```

---

## 11. M10 — Deep Link 回调

### 11.1 业务规则

| 规则编号 | 约束 |
|:---------|:-----|
| R10-01 | AndroidManifest 注册 `ignitepay://` scheme |
| R10-02 | 回调路径为 `ignitepay://onchain?signature=<base58>` |
| R10-03 | MainNavigator 通过 `app_links` 监听回调 |
| R10-04 | 回调触发 `SessionKeyService.completeRegistration(signature)` |
| R10-05 | 回调成功后 pending unsigned tx 被清除 |

### 11.2 用例

#### TC-M10-01 正常回调流程

```
前置：SessionKeyService 有 pending unsigned tx

步骤：
  1. 外部钱包完成签名
  2. 钱包打开回调 URL: ignitepay://onchain?signature=<valid_base58_signature>
  3. 验证：应用接收到 deep link
  4. 验证：_handleDeepLink 解析 signature 参数
  5. 验证：调用 completeRegistration(signature)
  6. 验证：pendingUnsignedTx 被清除
  7. 验证：debugPrint "Session key registered: <pubkey>"

预期结果：Deep Link 回调正确完成注册
```

#### TC-M10-02 无 Pending 交易时收到回调

```
前置：SessionKeyService 无 pending unsigned tx

步骤：
  1. 打开 ignitepay://onchain?signature=xxx

预期结果：completeRegistration 抛出异常 "No pending unsigned transaction"，被 catchError 捕获并打印日志
```

#### TC-M10-03 无效签名回调

```
前置：SessionKeyService 有 pending unsigned tx

步骤：
  1. 打开 ignitepay://onchain?signature=invalid_signature
  2. completeRegistration 尝试使用无效签名

预期结果：Rust 层返回错误（签名验证失败或 RPC 提交失败），被 catchError 捕获
```

---

## 12. 跨模块端到端流程

### E2E-01 完整支付授权端到端

```
1. 首次启动 → 完成向导（TC-M01-01）
2. 配置 Mediator 连接（TC-M03-03）
3. 扫码配对 MCP（TC-M03-01）
4. MCP 发送 payment-auth-request → Dashboard 显示横幅
5. 点击 "Authorize Payment"（TC-M04-01）
6. 选择 Built-in Key 签名 → 等待链上注册
7. 授权成功 → 弹窗关闭
8. 打开 Session Keys → 验证新密钥存在（TC-M05-02）
9. MCP 再次发送支付请求 → 授权时复用已有密钥（TC-M04-02）
```

### E2E-02 Deep Link 端到端

```
1. 配对 MCP 并触发支付请求
2. 在 Challenge 选择 "Phantom/Solflare" 签名
3. 验证：pending unsigned tx 已创建
4. 外部钱包签名并回调（TC-M10-01）
5. 验证：Session Keys 列表新增密钥
6. 后续支付复用该密钥
```

### E2E-03 密钥生命周期

```
1. 注册密钥（TC-M05-02）→ 状态 "active"
2. 等待过期 → 状态变为 "expired"（TC-M05-06）
3. 注册新密钥 → 再次 "active"
4. 撤销旧密钥（TC-M05-03）→ 链上 revoke
5. 删除本地旧密钥记录（TC-M05-04）→ 列表中移除
```

---

## 13. 数据校验规则汇总

| 字段 | 校验规则 | 错误提示 |
|:-----|:---------|:---------|
| DID | 格式 `did:ignite:z6Mk...`，长度 ≥ 32 字符 | 内部错误，不可用户输入 |
| WS URL | 合法 WebSocket URL（`ws://` 或 `wss://`） | "Failed to connect to mediator" |
| QR 内容 | 必须以 `didcomm://` 开头 | "Invalid invitation URL" |
| Spending Limit | 正整数（lamports） | Rust 层参数校验 |
| Duration | 正整数（秒） | Rust 层参数校验 |
| Label | 非空字符串（当 list action 为 add 时） | 未校验，可传空 |
| Max Amount | 正整数（lamports），可选 | int.tryParse → null |
| Owner Signature | Base58 编码的 64 字节 Ed25519 签名 | "Invalid owner signature length" |
| Session Pubkey | Base58 编码的 32 字节 Ed25519 公钥 | sled 查找失败 |

---

## 14. 边界条件与异常场景

| 场景 | 预期行为 |
|:-----|:---------|
| 网络断开时授权 | ResultBanner 显示 "Error: ..." + 具体错误信息 |
| Mediator 地址错误 | 连接超时，可重试或跳过 |
| 重复扫码同一 MCP | 第二次扫码行为同首次（不防重复） |
| 金额为 0 的支付请求 | Amount 显示 "0 SOL" |
| 超大金额（>10,000 SOL） | Amount 正常显示（不截断） |
| 同时收到多个支付请求 | 取最新一个为 pendingAuth |
| 应用在后台收到 FCM | 触发消息拉取，前台恢复后 UI 更新 |
| 多个密钥同时过期 | 列表全部标记 "expired" |
| Revoke 已过期的密钥 | 仍发送链上交易（不阻止） |
| Delete 正在使用的活跃密钥 | 删除本地记录（不检查是否正在使用） |
| Deep Link 回调时应用未运行 | 冷启动后 app_links 可能不触发，需手动操作 |

---

## 15. 测试矩阵

### 15.1 平台覆盖

| 平台 | 版本 | 架构 | 测试要求 |
|:-----|:-----|:-----|:---------|
| Android Emulator | API 36 | x86_64 | 全量测试 |
| Android Physical | API 34+ | arm64-v8a | M05, M10 需真机 |
| iOS | 17+ | arm64 | 未适配（未测试） |

### 15.2 网络环境

| 环境 | 覆盖模块 |
|:-----|:---------|
| 正常网络（Devnet 可达） | 全量 |
| 弱网络（高延迟） | M04, M05 超时行为 |
| 无网络 | M03, M04, M05 错误处理 |
| VPN/代理 | M09 FCM 可达性 |

### 15.3 用户区域

| Locale | 推送通道 | 测试重点 |
|:-------|:---------|:---------|
| en-US | FCM | FCM 注册、前台通知 |
| zh_CN | WebSocket | WS 长连接、断连重连 |
| 其他 | FCM | 回退到 FCM |
