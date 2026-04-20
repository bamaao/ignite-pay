# Ignite Pay App — 页面与交互规格文档

> 基于 ignite-pay 项目文档、Rust API 层和现有 Flutter 代码的完整梳理。
> 版本：V1.0 → V2.0 功能覆盖

---

## 1. 应用概述

Ignite Pay 手机端（代号 Sentinel）是 AI Agent 支付网关的移动授权终端。核心职责：

1. **DID 身份管理** — 生成/导入/备份 `did:ignite` 去中心化身份
2. **MCP 配对** — 通过 QR 码与 AI Agent（MCP Server）建立 DIDComm P2P 连接
3. **支付授权** — 接收 X402 支付挑战，用户审批（含会话密钥创建）
4. **风控策略** — 白名单/黑名单管理、商户限额、自动支付规则
5. **消息通信** — 通过 Mediator 中继的 DIDComm 加密消息收发
6. **审计日志** — 本地交易记录 + IPFS 加密同步

### 1.1 核心架构流

```
AI Agent ──X402──> MCP Server ──DIDComm JWE──> Mediator ──push──> Phone App
                                                                  │
                                                    FCM (海外) / WebSocket (国内)
                                                                  │
                                                         HTTPS Pull (消息收取)
```

### 1.2 推送通道策略

| 用户区域 | 推送方式 | 说明 |
|:---------|:---------|:-----|
| 海外 | FCM (Firebase Cloud Messaging) | 标准推送 |
| 中国大陆 | WebSocket 长连接 | 直连 Mediator WS |
| 通用回退 | HTTPS 轮询拉取 | 任何连接恢复后触发 |

---

## 2. 页面地图

```
┌─────────────────────────────────────────────────────────┐
│                    Sentinel Dashboard                     │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐   │
│  │  身份卡片  │ │ 消息中心  │ │ 扫码配对  │ │ 消费仪表  │   │
│  └──────────┘ └──────────┘ └──────────┘ └──────────┘   │
├──────────┬──────────┬──────────┬──────────┬─────────────┤
│  Vault   │ Policy   │ 消息列表  │  连接管理  │   设置     │
│  & 身份   │ 策略中心  │          │          │            │
├──────────┼──────────┼──────────┼──────────┼─────────────┤
│ ·DID详情  │ ·商户列表  │ ·消息流   │ ·MCP连接  │ ·Solana RPC│
│ ·助记词   │ ·限额规则  │ ·支付详情  │ ·Mediator │ ·树地址    │
│ ·密钥导出  │ ·自动支付  │ ·列表同步  │ ·推送通道  │ ·程序ID    │
│ ·审计日志  │ ·黑白名单  │          │ ·FCM配置  │ ·网络切换   │
│ ·危险操作  │          │          │          │            │
├──────────┴──────────┴──────────┴──────────┴─────────────┤
│                  X402 Challenge (全屏模态)                │
│              ·商户信息 ·金额 ·滑块授权 ·列表操作            │
├─────────────────────────────────────────────────────────┤
│                  QR Scanner (全屏模态)                    │
│              ·扫码配对 ·手动输入邀请URL                     │
└─────────────────────────────────────────────────────────┘
```

---

## 3. 页面详细规格

### 3.1 Dashboard（主页）

**路由**：`/` (home)
**优先级**：P0 — 必须实现

#### 3.1.1 页面结构

| 区域 | 组件 | 数据源 | 状态 |
|:-----|:-----|:-------|:-----|
| 顶部栏 | Sentinel Logo + 网络状态 + 设置入口 | `DidcommService.isConnected` | 已实现，网络状态需对接 |
| DID 身份卡片 | DID 文本 + 复制 + 连接状态动画点 + 待处理消息数 | `DidcommService.did`, `_isConnected` | 已实现 |
| 快捷导航 | Vault / Policy / Messages / Settings 四宫格 | 静态 | 部分实现（缺 Messages/Settings） |
| 扫码配对按钮 | "Scan MCP QR Code" 大按钮 | 触发 QR Scanner | 已实现 |
| 消费仪表盘 | 径向进度条（已消费/限额 SOL） | `LocalLogStore` 聚合 | UI 存在，数据需对接 |
| 最近活动 | 交易列表（商户、金额、时间、状态） | `LocalLogStore.recent_transactions()` | 硬编码，需对接 |
| 授权入口 | 待处理授权通知横幅 + "Authorize Payment" 按钮 | `DidcommService._pendingAuth` stream | 已实现 |

#### 3.1.2 交互流程

```
[首次启动] → 自动生成 DID → Dashboard 展示
[点击扫码] → QR Scanner 模态 → 扫描 didcomm:// URL → 连接 MCP → 返回 Dashboard
[收到推送] → 拉取消息 → 解密 → 如果是 payment-auth-request → 显示授权横幅
[点击授权] → X402 Challenge 模态 → 滑块确认 → 创建 Session Key → 加密响应 → 关闭
[点击设置] → Settings 页面
```

#### 3.1.3 需修复/对接

- [ ] 消费仪表盘对接 `LocalLogStore` 实际交易数据
- [ ] 活动流对接 `LocalLogStore.recent_transactions()`
- [ ] 网络状态（Mainnet/Devnet）从配置读取
- [ ] 待处理消息数从 `DidcommService._messages` 计算

---

### 3.2 Vault & Identity（保险库与身份）

**路由**：push slide-right
**优先级**：P0 — 必须实现

#### 3.2.1 页面结构

| 区域 | 组件 | 数据源 | 状态 |
|:-----|:-----|:-------|:-----|
| DID 身份卡 | 网格渐变背景 + DID + 密钥类型标签 + 复制 | `RustLib.api.getDid()` | 已实现 |
| 助记词 | 点击揭示/隐藏 12 词 + 警告横幅 | `IdentityManager` 密钥派生 | **硬编码，需对接** |
| Mediator 端点 | WebSocket URL 编辑框 + 连接/断开按钮 | `DidcommService._mediatorWsUrl` | 部分实现，缺连接按钮 |
| 审计日志入口 | 事件计数徽章 + 跳转 | `LocalLogStore` 计数 | **硬编码，需对接** |
| 密钥导出 | 导出 Ed25519 私钥（加密） | `IdentityManager` | **缺失** |
| 危险区域 | "擦除密钥材料" 按钮 + 确认对话框 | `IdentityManager` 销毁 | **仅 UI，需对接** |

#### 3.2.2 交互流程

```
[点击助记词] → 弹出确认对话框 → 揭示 12 词 → 再次点击隐藏
[编辑 Mediator URL] → 输入新 URL → 点击"连接" → WS 连接 + 认证 + 握手
[点击擦除] → 二次确认对话框 → 调用 Rust 清理 → 回到引导页
```

#### 3.2.3 需修复/对接

- [ ] 助记词从 Rust `IdentityManager` 获取实际密钥
- [ ] Mediator 连接按钮（当前只有 URL 编辑框，缺连接动作）
- [ ] 审计日志计数对接 `LocalLogStore`
- [ ] 密钥导出功能
- [ ] 擦除密钥对接 Rust 层销毁

---

### 3.3 Connection Management（连接管理）— 新页面

**路由**：push slide-right（从 Dashboard 或 Settings 进入）
**优先级**：P0 — 必须实现

#### 3.3.1 功能说明

管理手机与 MCP Server 和 Mediator 的连接关系。这是当前缺失的关键页面。

#### 3.3.2 页面结构

| 区域 | 组件 | 数据源 |
|:-----|:-----|:-------|
| Mediator 连接 | 连接状态指示灯 + WS URL + 连接/断开按钮 + 认证状态 | `DidcommService._isConnected`, `_authToken` |
| 推送通道配置 | FCM / WebSocket 切换 + FCM Token 显示 + 注册状态 | `FcmService._token`, `DidcommService._isChineseUser` |
| 已配对 MCP 列表 | MCP DID + 标签 + 连接时间 + 最后活跃 + 删除按钮 | `DidcommService._boundAgents` |
| 添加 MCP | "Scan QR Code" 按钮 + "手动输入 URL" 按钮 | QR Scanner / 文本输入 |

#### 3.3.3 交互流程

```
[打开页面] → 显示当前 Mediator 连接状态 + 已配对 MCP 列表
[连接 Mediator] → 输入 WS URL → 点击连接 → 3 阶段握手 → 状态更新
[配置推送] → 选择 FCM 或 WebSocket → FCM: 请求权限 + 注册 Token → WS: 显示连接状态
[添加 MCP] → 扫码或输入 URL → OOB 解析 → 发送 connection-request → 添加到列表
[删除 MCP] → 确认对话框 → 移除绑定
```

#### 3.3.4 Rust API 依赖

| API | 用途 |
|:----|:-----|
| `connectMediator(storagePath, wsUrl)` | 连接 Mediator |
| `disconnectMediator()` | 断开连接 |
| `authenticateWithMediator(mediatorUrl, did)` | JWT 认证 |
| `registerDeviceToken(mediatorUrl, authToken, fcmToken)` | 注册 FCM Token |
| `parseOobInvitation(invitationUrl)` | 解析邀请 |
| `sendConnectionRequest(...)` | 建立 P2P 连接 |

---

### 3.4 Policy Architect（策略中心）

**路由**：push slide-right
**优先级**：P1 — 核心功能

#### 3.4.1 页面结构

| 区域 | 组件 | 数据源 | 状态 |
|:-----|:-----|:-------|:-----|
| 统计概览 | 2×2 网格：商户数 / 自动支付数 / 周限额 / 已支出 | 聚合计算 | **硬编码** |
| 商户策略列表 | 可展开卡片 + 自动支付开关 | 持久化策略数据 | **硬编码** |
| 策略详情 | DID / 单笔限额 / 周消费进度条 / 过期日期 | 持久化策略数据 | **硬编码** |

#### 3.4.2 交互流程

```
[展开商户卡片] → 显示详情 → 编辑限额 / 切换自动支付 / 设置过期
[修改限额] → 输入 SOL 金额 → 保存到本地策略存储
[切换自动支付] → 开关 → 保存 → 如果开启则设为白名单自动授权
[查看进度条] → 从 LocalLogStore 计算本周该商户消费 → 显示进度
```

#### 3.4.3 需修复/对接

- [ ] 商户列表从实际连接的 MCP 或白名单/黑名单存储获取
- [ ] 限额数据持久化（SharedPreferences 或 SQLite）
- [ ] 消费进度从 `LocalLogStore` 聚合
- [ ] 自动支付开关对接白名单逻辑

---

### 3.5 Messages（消息中心）— 新页面

**路由**：push slide-right
**优先级**：P0 — 必须实现

#### 3.5.1 功能说明

展示所有 DIDComm 解密消息，包括支付请求、列表同步通知、连接请求等。

#### 3.5.2 页面结构

| 区域 | 组件 | 数据源 |
|:-----|:-----|:-------|
| 消息列表 | 按时间倒序的消息流 + 消息类型图标 + 简要信息 | `DidcommService._messages` |
| 消息详情 | 展开或新页面显示完整消息内容 | `DecryptedMessage` 字段 |
| 筛选 | 按类型筛选：全部 / 支付请求 / 列表同步 / 连接 | 消息 `msgType` |
| 空状态 | "暂无消息" + 拉取按钮 | — |

#### 3.5.3 消息类型

| 类型 | 图标 | 详情展示 |
|:-----|:-----|:---------|
| `payment-auth-request` | 💳 | 商户 DID + 金额 + 描述 + "授权"按钮 |
| `list-sync-notification` | 📋 | 列表类型 + CID + "同步"按钮 |
| `connection-request` | 🔗 | 对方 DID + "接受/拒绝" |
| 其他 | 📨 | rawBody JSON |

#### 3.5.4 交互流程

```
[打开页面] → 显示所有已解密消息
[下拉刷新] → 调用 pullMessages + decryptMessage → 更新列表
[点击支付消息] → 打开 X402 Challenge 模态
[点击列表同步] → 调用 Rust IPFS 同步 → 更新本地列表
[筛选] → 切换消息类型过滤器
```

---

### 3.6 Settings（设置）— 新页面

**路由**：push slide-right
**优先级**：P1 — 核心功能

#### 3.6.1 页面结构

| 区域 | 组件 | 数据源 |
|:-----|:-----|:-------|
| Solana 网络 | RPC URL 输入 + 网络（Mainnet/Devnet）切换 | 配置文件 |
| SPL 压缩配置 | Tree Address + Tree Authority + DAS Endpoint | 配置文件 |
| 程序 ID | 状态通道程序 / DID 程序 / 会话密钥程序 显示 | 硬编码常量 |
| 支付模式 | Self-Funded / Sponsored 切换 | 配置文件 |
| Mediator 配置 | WS URL + HTTP URL + 当前状态 | `DidcommService` |
| 推送通道 | FCM / WebSocket 选择 + 检测结果 | `DidcommService` |
| 存储 | 清除缓存 + 存储占用显示 | — |
| 关于 | 版本号 + 开源许可 | — |

#### 3.6.2 交互流程

```
[切换网络] → 确认对话框 → 更新 RPC URL + 重连
[编辑配置] → 输入 → 保存到 SharedPreferences → 需要时重新连接
[清除缓存] → 确认 → 清理本地存储
```

---

### 3.7 X402 Challenge（支付授权）

**路由**：全屏模态（fade 过渡）
**优先级**：P0 — 已实现，需完善

#### 3.7.1 页面结构

| 区域 | 组件 | 数据源 | 状态 |
|:-----|:-----|:-------|:-----|
| 标题栏 | 盾牌图标 + "X402 Challenge" + 关闭按钮 | 静态 | 已实现 |
| 商户卡片 | 商户 DID + 验证徽章 | `AuthRequest.merchantDid` | 已实现 |
| 金额显示 | 大字 SOL 金额 | `AuthRequest.amount` | 已实现 |
| 支付原因 | 描述文本 | `AuthRequest.description` | 已实现 |
| 列表操作 | 6 种操作选择 + 标签输入 + 最大金额 | `ListAction` 枚举 | 已实现 |
| 滑块授权 | 拖动滑块确认 | 手势 | 已实现 |
| 拒绝按钮 | "Decline & Block" | 手势 | 已实现 |
| 结果横幅 | "创建会话密钥..." / "已授权" / 错误 | 流程状态 | 已实现 |

#### 3.7.2 授权流程（V2.0 — 含会话密钥）

```
[收到 payment-auth-request]
  → 解密消息 → 设置 _pendingAuth → Dashboard 显示通知横幅
  → 用户点击 → 打开 X402 Challenge 模态
  → 显示商户 DID + 金额 + 描述
  → 用户选择列表操作（可选）
  → 用户拖动滑块到 85%
  → 触发 _onAuthorize():
     1. 调用 createSessionKeyForPayment(spendingLimit=amount*10, durationSecs=3600)
     2. 调用 sendAuthResponse(paymentId, authorized=true, listAction, mcpDid, sessionKeyInfo)
     3. 等待加密响应发送
  → 成功: 显示绿色结果横幅 → 1.5s 后关闭
  → 失败: 显示红色错误横幅
```

#### 3.7.3 需完善

- [ ] 商户 DID 链上验证状态展示（VC 验证结果）
- [ ] 会话密钥详情显示（公钥、过期时间、限额）
- [ ] 错误分类提示（网络错误 / 认证失败 / 金额超限）

---

### 3.8 QR Scanner（扫码配对）

**路由**：全屏模态
**优先级**：P0 — 已实现，需增强

#### 3.8.1 页面结构

| 区域 | 组件 | 状态 |
|:-----|:-----|:-----|
| 相机预览 | MobileScanner + 扫描区域叠加层 | 已实现 |
| 手动输入 | "手动输入邀请 URL" 按钮 | **缺失** |
| 扫描结果 | 显示解析的 MCP DID + 标签 + 确认按钮 | **缺失** |

#### 3.8.2 交互流程

```
[扫码成功] → 解析 didcomm:// URL → OobInvitationData
  → [新增] 显示确认对话框：MCP DID + 标签 + Mediator URL
  → 用户确认 → 连接 Mediator → 发送 connection-request
  → 成功: 关闭 Scanner → Dashboard 更新连接状态
  → 失败: 显示错误 + 重试

[手动输入] → 文本输入框 → 粘贴 didcomm:// URL → 同上流程
```

---

### 3.9 Audit Logs（审计日志）

**路由**：push slide-right（从 Vault 进入）
**优先级**：P2 — 增强功能

#### 3.9.1 页面结构

| 区域 | 组件 | 数据源 |
|:-----|:-----|:-------|
| 日志列表 | 按时间倒序的交易记录 | `LocalLogStore.recent_transactions(limit)` |
| 日志条目 | 操作类型 + 商户 + 金额 + 时间 + 状态徽章 | `TransactionLog` |
| IPFS 同步状态 | 已同步/未同步计数 + 手动同步按钮 | `LocalLogStore.unsynced_entries()` |
| 搜索/筛选 | 按操作类型或日期范围筛选 | 客户端筛选 |

#### 3.9.2 交互流程

```
[打开页面] → 从 SQLite 加载最近交易
[点击同步] → 调用 sync_to_ipfs() → 显示进度 → 更新同步状态
[下拉刷新] → 重新加载
```

---

### 3.10 Onboarding（引导页）— 新页面

**路由**：首次启动时显示（检查本地是否有 DID）
**优先级**：P1 — 核心功能

#### 3.10.1 页面结构

| 步骤 | 内容 | 操作 |
|:-----|:-----|:-----|
| 欢迎页 | 应用介绍 + "开始" 按钮 | 下一步 |
| 创建身份 | "生成新 DID" 按钮 + 助记词显示 + 确认备份 | 生成 + 确认 |
| 导入身份 | "导入助记词" 12 词输入框 | 恢复 |
| Mediator 配置 | WS URL 输入（带默认值）+ 连接测试 | 下一步 |
| 完成 | "进入 Sentinel" 按钮 | 跳转 Dashboard |

---

## 4. 数据模型

### 4.1 核心状态（DidcommService 管理的全局状态）

```dart
class DidcommState {
  // 身份
  String did;                    // did:ignite:z...
  String didDocJson;             // DID Document JSON

  // 连接
  bool isConnected;              // Mediator WS 连接状态
  String? authToken;             // JWT Token
  String mediatorWsUrl;          // Mediator WebSocket URL
  String mediatorHttpUrl;        // Mediator HTTP URL

  // 已配对 MCP
  List<McpConnection> boundAgents;  // 已连接的 MCP 列表

  // 消息
  List<DecryptedMessage> messages;  // 解密消息
  AuthRequest? pendingAuth;         // 待处理授权

  // 推送
  PushChannel pushChannel;       // fcm | websocket
  String? fcmToken;              // FCM Token
}
```

### 4.2 McpConnection（MCP 连接记录）

```dart
class McpConnection {
  String mcpDid;                 // MCP Server 的 DID
  String label;                  // 显示名称
  DateTime connectedAt;          // 连接时间
  DateTime? lastActive;          // 最后活跃时间
  String mediatorWsUrl;          // 连接使用的 Mediator
}
```

### 4.3 Policy（商户策略）

```dart
class MerchantPolicy {
  String merchantDid;            // 商户 DID
  String label;                  // 显示名称
  bool autoPay;                  // 自动支付开关
  double singleLimit;            // 单笔限额 (SOL)
  double weeklyCap;              // 周限额 (SOL)
  DateTime? expiryDate;          // 策略过期时间
  ListAction listAction;         // 白名单/黑名单状态
  String? listLabel;             // 列表标签
  double? listMaxAmount;         // 列表最大金额
}
```

### 4.4 TransactionLog（交易日志）

```dart
class TransactionLog {
  String id;                     // 日志 ID
  String action;                 // sign_payment | key_derive | ...
  String? merchantDid;           // 商户 DID
  double? amount;                // 金额 (SOL)
  DateTime timestamp;            // 时间戳
  String status;                 // success | pending | failed
  bool synced;                   // 是否已同步到 IPFS
}
```

---

## 5. Rust API 对接清单

### 5.1 已对接（可用）

| Rust API | Dart 调用位置 | 说明 |
|:---------|:-------------|:-----|
| `initializeIdentity` | `DidcommService.initialize()` | DID 生成/加载 |
| `getDid` | `DidcommService.initialize()` | 获取 DID |
| `connectMediator` | `DidcommService.connectToMediator()` | WS 连接 |
| `disconnectMediator` | `DidcommService.disconnect()` | 断开 |
| `authenticateWithMediator` | `DidcommService.connectToMediator()` | JWT 认证 |
| `pullMessages` | `DidcommService._pullAndDecryptMessages()` | 拉取消息 |
| `decryptMessage` | `DidcommService._pullAndDecryptMessages()` | 解密 |
| `sendAuthResponse` | `DidcommService.sendAuthResponse()` | V1.0 授权 |
| `createSessionKeyForPayment` | `ChallengeScreen._onAuthorize()` | 创建会话密钥 |
| `registerDeviceToken` | `DidcommService.connectToMediator()` | 注册 FCM |
| `parseOobInvitation` | `DidcommService.parseInvitationAndConnect()` | 解析邀请 |
| `sendConnectionRequest` | `DidcommService.parseInvitationAndConnect()` | P2P 连接 |

### 5.2 未对接（需新增）

| Rust API | 目标页面 | 用途 |
|:---------|:---------|:-----|
| `createSessionKey` | Settings / Challenge | 高级会话密钥创建（指定 scopes） |
| `createAndRegisterSessionKey` | Settings / Challenge | 链上注册会话密钥 |
| `signPayment` | Challenge | 真实支付签名（当前为 mock） |
| `LocalLogStore.record_transaction` | Challenge 授权成功后 | 记录交易 |
| `LocalLogStore.recent_transactions` | Dashboard / Audit Logs | 交易历史 |
| `LocalLogStore.unsynced_entries` | Audit Logs | 未同步日志 |
| `LocalLogStore.sync_to_ipfs` | Audit Logs | IPFS 同步 |
| `LocalLogStore.restore_from_ipfs` | Audit Logs | IPFS 恢复 |

### 5.3 需新增的 Rust API

| 需求 | 说明 |
|:-----|:-----|
| `exportMnemonicPhrase(storagePath) -> Vec<String>` | 导出助记词 |
| `importMnemonicPhrase(words) -> DidInfo` | 从助记词恢复 |
| `eraseAllKeyMaterial(storagePath)` | 安全擦除 |
| `getBoundAgents(storagePath) -> Vec<BoundAgent>` | 获取已配对 MCP 列表 |
| `removeBoundAgent(storagePath, agentDid)` | 删除 MCP 绑定 |
| `getSessionKeyInfo(storagePath) -> Vec<SessionKeyInfo>` | 查看活跃会话密钥 |
| `revokeSessionKey(storagePath, sessionPda)` | 撤销会话密钥 |

---

## 6. 关键交互流程

### 6.1 首次启动流程

```
App 启动
  → 检查本地是否存在 DID（storagePath 下 sled DB）
  → [不存在] → Onboarding 引导页
     → Step 1: 欢迎介绍
     → Step 2: 生成 DID + 显示助记词 + 确认备份
     → Step 3: 配置 Mediator URL（默认 wss://relay.ignite.did）
     → Step 4: 连接测试 → 成功 → 进入 Dashboard
  → [已存在] → 直接加载 DID → Dashboard
```

### 6.2 MCP 配对流程

```
Dashboard 点击 "Scan MCP QR Code"
  → QR Scanner 模态打开
  → 扫描 didcomm://?_oob=<base64url> URL
  → [新增] 显示确认对话框:
     - MCP DID: did:ignite:z...
     - 标签: "My Agent"
     - Mediator: wss://relay.ignite.did
  → 用户确认
  → Rust parseOobInvitation() → OobInvitationData
  → 检查 Mediator 是否已连接，未连接则先连接
  → Rust sendConnectionRequest(storagePath, mcpDid, mcpDidDocJson, mediatorWsUrl, pushChannel, fcmToken?)
  → 等待连接确认
  → 成功: 保存 McpConnection → Dashboard 更新 → 关闭 Scanner
  → 失败: 显示错误信息 + 重试选项
```

### 6.3 支付授权流程（完整）

```
[Mediator 推送 / WS 消息 / HTTPS 拉取]
  → DidcommService 收到 JWE 信封
  → Rust decryptMessage() → DecryptedMessage
  → msgType == "payment-auth-request"
  → 设置 _pendingAuth → Dashboard 显示授权横幅

[用户点击 "Authorize Payment"]
  → X402 Challenge 模态打开
  → 显示: 商户 DID / 金额(SOL) / 描述
  → 用户选择列表操作（可选）:
     - This time only / Whitelist / Blacklist / ...
     - 输入标签（可选）
     - 输入最大金额（可选）
  → 用户拖动滑块到 85% → 触发授权

  _onAuthorize():
    1. Rust createSessionKeyForPayment(spendingLimit, durationSecs)
       → SessionKeyInfo(ephemeralPubkey, ephememalSecretKey, expiresAt, ...)
    2. [新增] Rust LocalLogStore.record_transaction(...)
    3. Rust sendAuthResponse(paymentId, authorized=true, listAction, mcpDid, sessionKeyInfo, ...)
       → 加密为 JWE → 通过 WS/HTTP 发送到 Mediator
    4. 等待发送确认
  → 成功: 绿色结果横幅 → 1.5s 后关闭
  → 失败: 红色错误横幅 → 重试选项
```

### 6.4 Mediator 连接流程

```
用户输入 Mediator WS URL（或使用默认）
  → 点击 "连接"
  → Rust connectMediator(storagePath, wsUrl)
    → Phase 0: 收到 ws-challenge → DID 签名 → 发送 ws-challenge-response → 收到 ws-auth-ok
    → Phase A: 发送 mediate-request → 收到 mediate-grant → 发送 keylist-update → 发送 peer-introduction
    → 进入双向循环
  → Rust authenticateWithMediator(httpUrl, did) → JWT Token
  → 根据推送通道注册:
     - FCM: Rust registerDeviceToken(mediatorUrl, token, fcmToken)
     - WS: DidcommService._initWebSocketChannel()
  → 连接成功 → 状态更新 → 拉取离线消息
```

---

## 7. 导航结构

### 7.1 底部导航栏（建议新增）

```
┌────────────────────────────────────────────┐
│  🏠 Home   │  📨 Messages  │  ⚙️ Settings  │
└────────────────────────────────────────────┘
```

| Tab | 页面 | 说明 |
|:----|:-----|:-----|
| Home | Dashboard | 主仪表盘 |
| Messages | Messages | 消息中心 |
| Settings | Settings | 设置（包含 Vault、Policy 入口） |

### 7.2 页面层级

```
MaterialApp
├── BottomNavigationBar
│   ├── Tab 0: SentinelDashboard (主页)
│   │   ├── → VaultIdentityScreen (push)
│   │   │   └── → AuditLogsPage (push)
│   │   ├── → PolicyArchitectScreen (push)
│   │   ├── → ConnectionManagementScreen (push)
│   │   ├── → showQrScanner (modal)
│   │   └── → showX402Challenge (modal)
│   ├── Tab 1: MessagesScreen (消息)
│   │   └── → MessageDetail (push)
│   └── Tab 2: SettingsScreen (设置)
│       └── → ConnectionManagementScreen (push)
├── OnboardingScreen (首次启动条件显示)
```

---

## 8. 设计风格

### 8.1 主题

- **风格**: 暗色玻璃拟物化 (Dark Glassmorphism)
- **主色调**: 霓虹青 (#00F5FF)
- **警告色**: 琥珀 (#FFB800)
- **成功色**: 翠绿 (#00FF88)
- **错误色**: 玫红 (#FF3366)
- **背景**: 深色渐变 (#0A0E17 → #141B2D)
- **卡片**: 半透明模糊 + 细微边框 (rgba(255,255,255,0.05))

### 8.2 字体

- **UI 文本**: Inter (Google Fonts)
- **等宽数据**: JetBrains Mono (DID、金额、地址)
- **标题**: Inter Bold

### 8.3 图标

- Lucide Icons 贯穿全应用

---

## 9. 实现优先级

### Phase 1 — 核心功能对接（P0）

| 任务 | 页面 | 说明 |
|:-----|:-----|:-----|
| 修复 sled 只读路径 | Dashboard | 使用应用内部存储路径 |
| 新增 Connection Management 页面 | 新页面 | MCP 配对管理 + Mediator 连接 + 推送配置 |
| 新增 Messages 页面 | 新页面 | 消息列表 + 详情 + 筛选 |
| 新增 Settings 页面 | 新页面 | Solana / Mediator / 推送配置 |
| 增强 QR Scanner | QR Scanner | 扫码确认 + 手动输入 |
| 对接审计日志 | Audit Logs | LocalLogStore 对接 |
| 对接交易历史 | Dashboard | 活动流真实数据 |
| 新增 Rust API | Rust | 导出助记词 / 擦除密钥 / 获取绑定列表 |

### Phase 2 — 功能完善（P1）

| 任务 | 页面 | 说明 |
|:-----|:-----|:-----|
| 新增 Onboarding 引导页 | 新页面 | 首次启动流程 |
| 对接策略管理 | Policy | 持久化 + 真实数据 |
| 对接助记词 | Vault | 真实密钥派生 |
| 消费仪表盘对接 | Dashboard | 真实消费数据 |
| 底部导航栏 | 全局 | Home / Messages / Settings |

### Phase 3 — 高级功能（P2）

| 任务 | 页面 | 说明 |
|:-----|:-----|:-----|
| 会话密钥管理 UI | Settings | 查看/撤销活跃会话密钥 |
| IPFS 审计同步 | Audit Logs | 同步/恢复 |
| 多语言支持 | 全局 | 中文/英文 |
| 生物识别认证 | Vault | 本地安全层 |

---

## 10. 已知问题

| 问题 | 影响 | 修复方案 |
|:-----|:-----|:---------|
| sled 路径 `./phone_data` 为只读 | 应用无法启动 | 使用 `path_provider` 获取应用内部目录 |
| FCM 在模拟器不可用 | 推送不工作 | 模拟器仅用 WS 推送，文档说明 |
| DidcommService 重复实现连接逻辑 | 与 Rust 不一致 | 统一走 Rust `sendConnectionRequest` |
| 策略数据全部硬编码 | 无法持久化 | 新增本地 SQLite 策略表 |
| 缺少错误提示 UI | 用户无法感知错误 | 新增全局 SnackBar 错误提示 |
