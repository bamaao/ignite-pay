# 商户 App 业务测试用例

## 文档说明

<!-- State Channel: 探索阶段，暂不启用 - 原覆盖范围含 ChannelService、ChannelScreen/Detail -->
覆盖范围：MerchantService、MerchantPushService、VoiceService、FCM、MediatorApi、Onboarding、QR 生成、Settings、Dashboard、PaymentList/Detail、Rust Bridge。

标记说明：
- [E2E] 端到端，需完整环境
- [Unit] 纯 Dart/Rust 单元测试
- [Widget] Flutter Widget 测试
- [Mock] 需要 mock Rust bridge 或外部服务

---

## 1. 商户身份与初始化 (Merchant Identity)

### TC-ID-01: 首次初始化生成 DID

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | 空 storage 目录 |
| 步骤 | 1. 调用 `MerchantService.initialize()` |
| 预期 | `_did` 非空，格式为 `did:ignite:<base58>`，`_didDocJson` 为合法 JSON，包含 `verificationMethod` 和 `authentication` 字段 |
| 验证 | `expect(svc.did, startsWith('did:ignite:'))` |

### TC-ID-02: 重复初始化复用已有 DID

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | storage 中已有 keypair |
| 步骤 | 1. 第一次 `initialize()` 获取 didA<br>2. 第二次 `initialize()` 获取 didB |
| 预期 | didA == didB，身份持久化生效 |

### TC-ID-03: generateIdentity 覆盖旧身份

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | 已有身份 |
| 步骤 | 1. 调用 `generateIdentity()` |
| 预期 | `did` 与之前不同，`notifyListeners` 被调用 |

### TC-ID-04: Rust 端 keypair 持久化

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] |
| 前置 | 无 |
| 步骤 | 1. `generate_merchant_keypair(path)`<br>2. `get_merchant_pubkey(path)` |
| 预期 | 返回的 pubkey 与 keypair 对应的公钥一致，base58 格式 |

### TC-ID-05: Rust 端空 storage 报错

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] |
| 前置 | 空 storage，未生成 keypair |
| 步骤 | 1. 调用 `get_merchant_did(path)` |
| 预期 | 返回 Err（"No merchant keypair found"） |

---

## 2. Onboarding 引导流程

### TC-OB-01: 完整引导流程

| 项目 | 内容 |
|------|------|
| 类型 | [Widget] [Mock] |
| 前置 | SharedPreferences 为空（未引导） |
| 步骤 | 1. App 启动，显示 OnboardingScreen<br>2. 输入 Hub Endpoint `https://hub.example.com`<br>3. 输入 Mediator WS `wss://mediator.example.com`<br>4. 点击"生成商户身份"<br>5. DID 显示<br>6. 点击"开始使用" |
| 预期 | 跳转到主界面，SharedPreferences 中 `hub_endpoint` 和 `mediator_ws_url` 已保存 |

### TC-OB-02: Hub 为空时身份按钮禁用

| 项目 | 内容 |
|------|------|
| 类型 | [Widget] |
| 前置 | 无 |
| 步骤 | 1. Hub 字段留空<br>2. 观察按钮状态 |
| 预期 | "生成商户身份"按钮呈禁用态（灰色），点击无响应 |

### TC-OB-03: 身份未生成时开始按钮禁用

| 项目 | 内容 |
|------|------|
| 类型 | [Widget] |
| 前置 | Hub 已填写但未生成身份 |
| 步骤 | 1. 填写 Hub Endpoint<br>2. 不点击"生成商户身份"<br>3. 观察"开始使用"按钮 |
| 预期 | "开始使用"按钮禁用 |

### TC-OB-04: 引导时初始化推送服务

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | Mediator WS URL 非空 |
| 步骤 | 1. `_start()` 被调用<br>2. MerchantPushService.initialize() 被调用<br>3. connectToMediator(wsUrl) 被调用 |
| 预期 | pushSvc.isConnected == true，pushSvc.commDid 非空 |

### TC-OB-05: 推送初始化失败不阻塞引导

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | Mediator WS URL 无效 |
| 步骤 | 1. pushService.connectToMediator() 抛异常 |
| 预期 | 异常被捕获，`widget.onComplete()` 仍被调用 |

---

## 3. QR 生成与订单创建

### TC-QR-01: 正常生成 QR

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | 身份已初始化，hubEndpoint 非空 |
| 步骤 | 1. 调用 `generatePaymentQr(BigInt.from(1_000_000_000), "咖啡")` |
| 预期 | 返回 `ignite://pay?d=` 前缀字符串；base64 解码后 JSON 包含 `merchant_did`, `amount: 1000000000`, `description: "咖啡"`, `order_id`, `hub_endpoint` |

### TC-QR-02: 金额为零时按钮禁用

| 项目 | 内容 |
|------|------|
| 类型 | [Widget] |
| 前置 | QR 生成页面打开 |
| 步骤 | 1. 输入框留空或输入 0 |
| 预期 | "生成收款码"按钮禁用，`_amountLamports == BigInt.zero` |

### TC-QR-03: 无效金额输入

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] |
| 前置 | 无 |
| 步骤 | 1. `_amountText = "abc"`<br>2. 计算 `_amountLamports` |
| 预期 | `_amountLamports == BigInt.zero`，按钮禁用 |

### TC-QR-04: 金额精度（小数转 lamports）

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] |
| 前置 | 无 |
| 步骤 | 输入 `"1.5"` |
| 预期 | `_amountLamports == BigInt.from(1_500_000_000)` |

### TC-QR-05: QR 生成后创建 Pending 订单

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | 身份已初始化 |
| 步骤 | 1. 调用 `generatePaymentQr(...)`<br>2. 调用 `refreshOrders()` |
| 预期 | `orders` 列表中新增一条记录，`status == 'pending'`，`amount` 和 `description` 与输入一致 |

### TC-QR-06: QR 格式可逆解析

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] |
| 前置 | 无 |
| 步骤 | 1. `generate_payment_qr(merchant_did, 100, "test", "https://hub")`<br>2. 提取 `d=` 后的 base64url<br>3. 解码 JSON |
| 预期 | `qr_type == "ignite-pay-request"`, `version == 1`, 所有字段与输入一致 |

---

## 4. 订单确认与状态流转

### TC-ORD-01: confirm_order 将状态改为 confirmed

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] |
| 前置 | storage 中有 pending 订单 |
| 步骤 | 1. `confirm_order(storage_path, order_id, "channel_abc", 2, 100)` |
<!-- State Channel: 探索阶段，暂不启用 - 预期结果含 channel_id, leaf_index, sequence -->
| 预期 | 订单 `status == "confirmed"`，`confirmed_at` 为当前时间戳 |

### TC-ORD-02: confirm 不存在的订单静默成功

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] |
| 前置 | storage 为空 |
| 步骤 | 1. `confirm_order(storage_path, "nonexistent_id", "ch", 0, 0)` |
| 预期 | 不报错，返回 `Ok(())` |

### TC-ORD-03: 订单状态完整枚举

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] |
| 前置 | 无 |
| 步骤 | 验证 Rust `OrderStatus` 的 Display impl |
| 预期 | `Pending -> "pending"`, `Confirmed -> "confirmed"`, `Failed -> "failed"`, `Expired -> "expired"` |

### TC-ORD-04: list_orders 按时间倒序

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] |
| 前置 | 创建多个订单，间隔 > 1ms |
| 步骤 | 1. 创建 orderA, orderB, orderC<br>2. `list_orders(storage_path, 50)` |
| 预期 | 返回顺序为 orderC, orderB, orderA（最新在前） |

### TC-ORD-05: list_orders limit 参数生效

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] |
| 前置 | 创建 10 个订单 |
| 步骤 | 1. `list_orders(storage_path, 3)` |
| 预期 | 返回 3 条（最新的 3 条） |

### TC-ORD-06: get_pending_orders 仅返回 pending

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] |
| 前置 | 创建 2 个 pending + 1 个 confirmed 订单 |
| 步骤 | 1. `get_pending_orders(storage_path)` |
| 预期 | 返回 2 条，均为 pending 状态 |

---

## 5. DIDComm 推送服务 (MerchantPushService)

### TC-PUSH-01: 初始化生成 DIDComm DID

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | 空 storage |
| 步骤 | 1. `pushService.initialize()` |
| 预期 | `commDid` 以 `did:ignite:z` 开头（multicodec 编码格式）<!-- State Channel: 探索阶段，暂不启用 - 原文含"与状态通道 DID 不同" --> |

<!-- State Channel: 探索阶段，暂不启用
### TC-PUSH-02: 双 DID 互不干扰

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | MerchantService 已初始化（状态通道 DID） |
| 步骤 | 1. MerchantPushService.initialize()<br>2. 比较两个 DID |
| 预期 | `merchantService.did != pushService.commDid`，两者格式不同（状态通道是 raw base58，DIDComm 是 multicodec base58） |
-->

### TC-PUSH-03: 重复初始化幂等

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | 无 |
| 步骤 | 1. 第一次 `initialize()`<br>2. 第二次 `initialize()` |
| 预期 | 第二次直接返回，`commDid` 不变，`isInitialized == true` |

### TC-PUSH-04: 连接 mediator 后状态变更

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | 已初始化 |
| 步骤 | 1. `connectToMediator("wss://mediator.test")` |
| 预期 | `isConnected == true`，`pushChannel` 根据用户地区设置为 `'websocket'` 或 `'fcm'` |

### TC-PUSH-05: 中文用户使用 WebSocket 通道

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | locale 为 `zh_CN` |
| 步骤 | 1. 设置 locale 为 `Locale('zh', 'CN')`<br>2. `connectToMediator(...)` |
| 预期 | `_isChineseUser == true`，`pushChannel == 'websocket'`，调用了 `_initWebSocketChannel()` |

### TC-PUSH-06: 非中文用户使用 FCM 通道

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | locale 为 `en_US` |
| 步骤 | 1. 设置 locale 为 `Locale('en', 'US')`<br>2. `connectToMediator(...)` |
| 预期 | `_isChineseUser == false`，`pushChannel == 'fcm'`，调用了 `_initFcm()` |

### TC-PUSH-07: 断开连接后状态重置

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | 已连接 |
| 步骤 | 1. `disconnect()` |
| 预期 | `isConnected == false`，`pushChannel == ''`，WS subscription 已取消 |

---

## 6. 消息解密与确认处理

<!-- State Channel: 探索阶段，暂不启用
### TC-MSG-01: 解密 channel-payment-confirm 消息

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | DIDComm 身份已初始化，有一个 pending 订单 |
| 步骤 | 1. 模拟收到包含 `channel-payment-confirm` 的 JWE<br>2. `_decryptAndProcess(jwe)` |
| 预期 | 订单状态变为 `confirmed`，`confirmations` stream 发出 `PaymentConfirmation`（orderId, channelId, leafIndex, sequence 均正确） |
-->

### TC-MSG-02: 解密 payment-auth-response 消息

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | 同上 |
| 步骤 | 1. 模拟 `msg_type` 含 `payment-auth-response` 的消息 |
| 预期 | 同 TC-MSG-01，触发确认流程 |

### TC-MSG-03: 非支付消息不触发确认

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | 同上 |
| 步骤 | 1. 模拟 `msg_type = "https://didcomm.org/other/1.0/unknown"` 的消息 |
| 预期 | 不调用 `confirmOrder`，`confirmations` stream 不发出事件 |

<!-- State Channel: 探索阶段，暂不启用
### TC-MSG-04: orderId 为空时不调用 confirmOrder

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | 同上 |
| 步骤 | 1. 模拟 `msg_type` 含 `channel-payment-confirm` 但 `order_id == null` 的消息 |
| 预期 | 不调用 `confirmOrder`，但仍发出 `PaymentConfirmation`（orderId 为空字符串） |

### TC-MSG-05: channelId 为空时不调用 confirmOrder

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | 同上 |
| 步骤 | 1. 模拟有 `order_id` 但无 `channel_id` 的消息 |
| 预期 | 不调用 `confirmOrder`，发出 `PaymentConfirmation` |
-->

---

## 7. WebSocket 通道

### TC-WS-01: WS 连接建立与 identify 消息

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | 已认证，中文用户 |
| 步骤 | 1. `_initWebSocketChannel()` |
| 预期 | WS 连接到 mediatorWsUrl，发送 `{"from":"<did>","type":"identify"}` |

### TC-WS-02: WS 收到消息触发解密

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | WS 已连接 |
| 步骤 | 1. 模拟 WS stream 收到 JWE 字符串 |
| 预期 | `_decryptAndProcess` 被调用 |

### TC-WS-03: WS 断开后自动重连

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | WS 已连接 |
| 步骤 | 1. 模拟 WS `onDone` 回调 |
| 预期 | 先 `_pullAndDecryptMessages()` 拉取离线消息，等 3 秒后重连 |

### TC-WS-04: WS 错误触发重连

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | WS 已连接 |
| 步骤 | 1. 模拟 WS `onError` |
| 预期 | 同 TC-WS-03 |

### TC-WS-05: disconnect 后不重连

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | WS 已连接 |
| 步骤 | 1. 调用 `disconnect()`<br>2. 模拟 WS close |
| 预期 | `_isConnected == false`，不会触发 `_initWebSocketChannel()` |

---

## 8. FCM 通道

### TC-FCM-01: 前景收到 SIGNAL 触发拉取

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | FCM 已初始化 |
| 步骤 | 1. 收到 `data: {type: 'SIGNAL', msg_id: 'abc123'}` 的 RemoteMessage |
| 预期 | 显示本地通知（标题 "Payment Received"），`_onSignalReceived('abc123')` 被调用，触发 `_pullAndDecryptMessages()` |

### TC-FCM-02: 非 SIGNAL 消息被忽略

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | FCM 已初始化 |
| 步骤 | 1. 收到 `data: {type: 'OTHER'}` 的 RemoteMessage |
| 预期 | 不显示通知，不触发回调 |

### TC-FCM-03: 后台打开通知触发拉取

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | App 在后台 |
| 步骤 | 1. 收到 `onMessageOpenedApp` 消息 `{type: 'SIGNAL', msg_id: 'xyz'}` |
| 预期 | `_onSignalReceived('xyz')` 被调用 |

### TC-FCM-04: FCM token 注册到 mediator

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | FCM 已初始化，已认证 |
| 步骤 | 1. FCM token 非空<br>2. authToken 非空 |
| 预期 | `rust.registerDeviceToken()` 被调用，参数包含 mediatorUrl, authToken, fcmToken |

### TC-FCM-05: FCM 初始化失败不崩溃

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | Firebase 配置缺失 |
| 步骤 | 1. `_initFcm()` 抛异常 |
| 预期 | 异常被 catch，debugPrint 输出错误日志 |

---

## 9. Mediator 认证

### TC-AUTH-01: challenge-response 正常流程

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | mediator 服务可用 |
| 步骤 | 1. `authenticate_with_mediator(mediator_url, did)` |
| 预期 | 发起 GET `/v1/auth/challenge` 获取 nonce → SHA256(did) 派生签名密钥 → Ed25519 签名 nonce → POST `/v1/auth/token` → 返回 JWT 字符串 |

### TC-AUTH-02: challenge 端点失败

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | mediator 不可达 |
| 步骤 | 1. `authenticate_with_mediator("http://invalid", did)` |
| 预期 | 返回 Err，包含 "Challenge request failed" 信息 |

### TC-AUTH-03: token 端点返回无 token

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | mock HTTP 返回 `{}` |
| 步骤 | 1. `authenticate_with_mediator(...)` |
| 预期 | 返回 Err（"No token in auth response"） |

---

## 10. 消息拉取

### TC-PULL-01: 拉取消息并更新游标

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | authToken 有效，mediator 有 3 条消息 |
| 步骤 | 1. `_pullAndDecryptMessages()` |
| 预期 | 3 条消息依次被 `_decryptAndProcess` 处理，`_lastPulledId` 更新为最后一条的 msg_id |

### TC-PULL-02: 分页拉取（afterId 参数）

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | 已拉取过一次，`_lastPulledId = "msg_003"` |
| 步骤 | 1. 再次调用 `_pullAndDecryptMessages()` |
| 预期 | Rust `pullMessages` 的 `afterId` 参数传入 `"msg_003"`，仅获取此后的新消息 |

### TC-PULL-03: 无消息时不触发处理

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | mediator 返回空消息列表 |
| 步骤 | 1. `_pullAndDecryptMessages()` |
| 预期 | `_decryptAndProcess` 未被调用，`_lastPulledId` 不变 |

### TC-PULL-04: authToken 为空时不拉取

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | `_authToken == null` |
| 步骤 | 1. `_pullAndDecryptMessages()` |
| 预期 | 直接 return，不发起网络请求 |

---

## 11. QR 等待确认（双通道）

### TC-WAIT-01: 推送确认触发 UI 更新

| 项目 | 内容 |
|------|------|
| 类型 | [Widget] [Mock] |
| 前置 | QR 已生成，`_status == 'waiting'` |
| 步骤 | 1. pushService.confirmations 发出匹配当前 orderId 的 PaymentConfirmation |
| 预期 | `_status` 变为 `'confirmed'`，显示绿色对勾，触发语音播报，`_fallbackPollTimer` 被取消 |

### TC-WAIT-02: 不匹配的 orderId 被忽略

| 项目 | 内容 |
|------|------|
| 类型 | [Widget] [Mock] |
| 前置 | QR 已生成，等待 orderId "aaa" |
| 步骤 | 1. confirmations 发出 orderId "bbb" |
| 预期 | `_status` 保持 `'waiting'`，定时器继续运行 |

### TC-WAIT-03: 回退轮询确认

| 项目 | 内容 |
|------|------|
| 类型 | [Widget] [Mock] |
| 前置 | QR 已生成，推送未到达 |
| 步骤 | 1. 模拟 5 秒后 `refreshOrders()` 返回 confirmed 订单 |
| 预期 | `_status` 变为 `'confirmed'`，语音播报触发 |

### TC-WAIT-04: 页面 dispose 清理资源

| 项目 | 内容 |
|------|------|
| 类型 | [Widget] [Mock] |
| 前置 | QR 已生成，正在等待 |
| 步骤 | 1. 返回上一页触发 dispose |
| 预期 | `_fallbackPollTimer` 已 cancel，`_confirmationSub` 已 cancel |

---

<!-- State Channel: 探索阶段，暂不启用
## 12. 通道管理 (ChannelService)

### TC-CH-01: 刷新通道列表

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | Rust 返回 ["channel_abc", "channel_def"] |
| 步骤 | 1. `refreshChannels()` |
| 预期 | `channels` 长度为 2，每个 `ChannelInfo` 含 channelId, status, balance 等字段 |

### TC-CH-02: 单通道状态获取失败

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | Rust 返回 2 个 channel ID，第二个 `merchantGetChannelStatus` 抛异常 |
| 步骤 | 1. `refreshChannels()` |
| 预期 | `channels[0]` 正常，`channels[1].status == 'Unknown'`，其他字段为零值 |

### TC-CH-03: 关闭通道

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | 通道存在 |
| 步骤 | 1. `closeChannel("channel_abc", "https://hub.test")` |
| 预期 | Rust `merchantCloseChannel` 被调用，返回成功消息 |

### TC-CH-04: 结算通道（claim + finalize）

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | 通道有 provider balance |
| 步骤 | 1. `claimLeaf(channelId, hub, leafIndex: 0, amount)`<br>2. `finalize(channelId, hub)` |
| 预期 | 两个 Rust 调用依次执行，均返回成功 |
-->

---

## 13. 语音播报 (VoiceService)

### TC-VOI-01: 中文播报内容

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | `language == 'zh-CN'`, `enabled == true` |
| 步骤 | 1. `announcePayment(BigInt.from(1_500_000_000))` |
| 预期 | TTS 朗读 "收到收款 1.50 USDC" |

### TC-VOI-02: 英文播报内容

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | `language == 'en-US'`, `enabled == true` |
| 步骤 | 1. `announcePayment(BigInt.from(500_000_000))` |
| 预期 | TTS 朗读 "Payment received: 0.50 USDC" |

### TC-VOI-03: 禁用时不播报

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | `enabled == false` |
| 步骤 | 1. `announcePayment(BigInt.from(100))` |
| 预期 | TTS `speak` 未被调用 |

### TC-VOI-04: 设置持久化

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | 无 |
| 步骤 | 1. `setEnabled(false)`<br>2. `setLanguage('en-US')`<br>3. `setVolume(0.5)`<br>4. 重新创建 VoiceService 并 `initialize()` |
| 预期 | `enabled == false`, `language == 'en-US'`, `volume == 0.5` |

---

## 14. Settings 页面

### TC-SET-01: 推送服务状态显示

| 项目 | 内容 |
|------|------|
| 类型 | [Widget] [Mock] |
| 前置 | pushService 已连接，WebSocket 通道 |
| 步骤 | 1. 打开 Settings 页面 |
| 预期 | DIDComm DID 显示且可复制，Mediator 连接显示绿色圆点 + "已连接"，推送通道显示 "WebSocket (国内)" |

### TC-SET-02: 未连接时状态显示

| 项目 | 内容 |
|------|------|
| 类型 | [Widget] [Mock] |
| 前置 | pushService 未连接 |
| 步骤 | 1. 打开 Settings 页面 |
| 预期 | DIDComm DID 显示 "未初始化"，Mediator 连接显示红色圆点 + "未连接"，推送通道显示 "未配置" |

### TC-SET-03: 修改 Hub Endpoint

| 项目 | 内容 |
|------|------|
| 类型 | [Widget] [Mock] |
| 前置 | 已有 hub 配置 |
| 步骤 | 1. 点击 Hub Endpoint 行<br>2. 修改 URL<br>3. 点击"保存" |
| 预期 | MerchantService 的 hubEndpoint 更新，SharedPreferences 已保存 |

### TC-SET-04: 语音测试按钮

| 项目 | 内容 |
|------|------|
| 类型 | [Widget] [Mock] |
| 前置 | 无 |
| 步骤 | 1. 点击"测试播报"按钮 |
| 预期 | VoiceService.announcePayment(BigInt.from(100_000_000)) 被调用（0.10 USDC） |

---

## 15. Dashboard

### TC-DASH-01: 今日汇总数据

| 项目 | 内容 |
|------|------|
| 类型 | [Widget] [Mock] |
| 前置 | 今日有 2 笔 confirmed 订单（1.00 + 2.50 USDC），1 笔 pending |
| 步骤 | 1. 查看 Dashboard |
| 预期 | 今日汇总显示 "3.50 USDC" 和 "2 笔"（仅计 confirmed） |

### TC-DASH-02: 无订单空态

| 项目 | 内容 |
|------|------|
| 类型 | [Widget] [Mock] |
| 前置 | orders 为空 |
| 步骤 | 1. 查看 Dashboard |
| 预期 | 今日汇总显示 "0.00 USDC"，最近订单区显示空态提示 |

---

## 16. Payment List & Detail

### TC-PAY-01: 筛选器切换

| 项目 | 内容 |
|------|------|
| 类型 | [Widget] [Mock] |
| 前置 | 2 笔 pending + 3 笔 confirmed |
| 步骤 | 1. 选择 "pending" 筛选<br>2. 选择 "confirmed" 筛选<br>3. 选择 "全部" |
| 预期 | 分别显示 2 条、3 条、5 条记录 |

### TC-PAY-02: 下拉刷新

| 项目 | 内容 |
|------|------|
| 类型 | [Widget] [Mock] |
| 前置 | 无 |
| 步骤 | 1. 触发 RefreshIndicator |
| 预期 | `MerchantService.refreshOrders()` 被调用 |

<!-- State Channel: 探索阶段，暂不启用 - 原前置含 channelId, leafIndex, sequence；原预期含通道信息
### TC-PAY-03: 订单详情显示

| 项目 | 内容 |
|------|------|
| 类型 | [Widget] [Mock] |
| 前置 | 一笔 confirmed 订单含 channelId, leafIndex, sequence |
| 步骤 | 1. 点击订单卡片进入详情 |
| 预期 | 显示金额、状态（绿色 "已确认"）、描述、时间戳、通道信息（channelId, leafIndex, sequence）、订单号可复制 |
-->

### TC-PAY-04: 订单详情无通道信息

| 项目 | 内容 |
|------|------|
| 类型 | [Widget] [Mock] |
| 前置 | 一笔 pending 订单，channelId 为 null |
| 步骤 | 1. 点击进入详情 |
| 预期 | 通道信息区域不显示（条件渲染） |

---

## 17. MediatorApi (Dart HTTP 客户端)

### TC-API-01: setBaseUrl 更新基础 URL

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] |
| 前置 | 无 |
| 步骤 | 1. `api.setBaseUrl("https://new-mediator.test")` |
| 预期 | 后续请求发送到新 URL |

### TC-API-02: pullMessages 解析响应

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | mock Dio 返回 `{messages: [{msg_id: "1", jwe_envelope: "...", created_at: 123}]}` |
| 步骤 | 1. `api.pullMessages(token)` |
| 预期 | 返回 `List<DidcommMessage>` 长度为 1，字段映射正确 |

### TC-API-03: registerWebSocketChannel 发送正确 body

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | 无 |
| 步骤 | 1. `api.registerWebSocketChannel(token)` |
| 预期 | POST body 为 `{push_channel: 'websocket'}`，Header 含 `Authorization: Bearer <token>` |

### TC-API-04: registerDeviceToken 发送 FCM token

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | 无 |
| 步骤 | 1. `api.registerDeviceToken(token, "fcm_token_abc")` |
| 预期 | POST body 为 `{fcm_token: 'fcm_token_abc', push_channel: 'fcm'}` |

---

## 18. Rust Bridge: merchant_didcomm

### TC-RUST-DC-01: initialize_merchant_comm 生成 DIDComm DID

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] |
| 前置 | 空 storage |
| 步骤 | 1. `initialize_merchant_comm(path)` |
| 预期 | 返回 `DidInfo`，did 以 `did:ignite:z` 开头（multicodec 编码），全局状态 `GLOBAL_COMM_DID` 非空 |

<!-- State Channel: 探索阶段，暂不启用
### TC-RUST-DC-02: DIDComm DID 与状态通道 DID 不同

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] |
| 前置 | 同一 storage |
| 步骤 | 1. `initialize_merchant(path)` → didA<br>2. `initialize_merchant_comm(path)` → didB |
| 预期 | didA != didB，didA 是 raw base58 格式，didB 是 multicodec base58 格式 |
-->

### TC-RUST-DC-03: decrypt_message 解密失败报错

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] |
| 前置 | 无效 JWE 字符串 |
| 步骤 | 1. `decrypt_message(path, "not_a_jwe")` |
| 预期 | 返回 Err，包含 "Decryption failed" |

### TC-RUST-DC-04: pull_messages HTTP 错误

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | mediator 返回 401 |
| 步骤 | 1. `pull_messages(url, "bad_token", None, 50)` |
| 预期 | 返回 Err，包含 "Pull messages failed: 401" |

### TC-RUST-DC-05: register_device_token HTTP 错误

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | mediator 返回 500 |
| 步骤 | 1. `register_device_token(url, "token", "fcm_token")` |
| 预期 | 返回 Err，包含 "Token registration failed: 500" |

---

## 19. 端到端集成场景

<!-- State Channel: 探索阶段，暂不启用 - 原步骤4引用 channel-payment-confirm
### TC-E2E-01: 完整收款流程（WebSocket 通道）

| 项目 | 内容 |
|------|------|
| 类型 | [E2E] |
| 前置 | 中文用户，mediator 和 hub 可用 |
| 步骤 | 1. App 启动 → 自动初始化身份和推送<br>2. 生成 QR（金额 5.00 USDC）<br>3. 模拟用户扫码支付<br>4. mediator 通过 WS 推送 channel-payment-confirm<br>5. App 收到推送 → 解密 → 确认订单 |
| 预期 | QR 页面显示绿色对勾 + "已收款"，语音播报 "收到收款 5.00 USDC"，Dashboard 今日汇总更新 |

### TC-E2E-02: 完整收款流程（FCM 通道）

| 项目 | 内容 |
|------|------|
| 类型 | [E2E] |
| 前置 | 英文用户，Firebase 已配置 |
| 步骤 | 1-3 同上<br>4. mediator 发送 FCM SIGNAL<br>5. App 收到 FCM → pull messages → 解密 → 确认订单 |
| 预期 | 同 TC-E2E-01，但播报为英文 "Payment received: 5.00 USDC" |
-->

### TC-E2E-03: 离线消息补拉

| 项目 | 内容 |
|------|------|
| 类型 | [E2E] |
| 前置 | WS 通道，订单已创建 |
| 步骤 | 1. 断开网络<br>2. 模拟支付确认<br>3. 恢复网络，WS 重连 |
| 预期 | 重连时 `_pullAndDecryptMessages()` 拉取离线消息，订单被确认 |

### TC-E2E-04: App 重启后恢复状态

| 项目 | 内容 |
|------|------|
| 类型 | [E2E] |
| 前置 | 上一轮有 3 笔订单（2 confirmed + 1 pending） |
| 步骤 | 1. 杀掉 App<br>2. 重新启动 |
| 预期 | 身份从 storage 恢复，订单列表完整，推送服务自动重连，pending 订单仍为 pending |

---

## 20. 边界与异常场景

### TC-EDGE-01: 极大金额精度

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] |
| 前置 | 无 |
| 步骤 | 1. `generate_payment_qr(did, u64::MAX, "test", hub)` |
| 预期 | 不报错，QR 中 amount 字段为 u64 最大值 |

### TC-EDGE-02: 空描述

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] |
| 前置 | 无 |
| 步骤 | 1. `generate_payment_qr(did, 100, "", hub)` |
| 预期 | 成功，QR JSON 中 description 为空字符串 |

### TC-EDGE-03: mediator URL 格式错误

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | 无 |
| 步骤 | 1. `connectToMediator("not_a_url")` |
| 预期 | 连接失败，`isConnected == false`，不崩溃 |

### TC-EDGE-04: 同时收到多条确认消息

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] [Mock] |
| 前置 | 3 笔 pending 订单 |
| 步骤 | 1. 一次性 pull 回 3 条确认消息 |
| 预期 | 3 笔订单依次被 confirmOrder，confirmations stream 发出 3 个事件 |

### TC-EDGE-05: 重复确认同一订单

| 项目 | 内容 |
|------|------|
| 类型 | [Unit] |
| 前置 | 订单已 confirmed |
| 步骤 | 1. 再次调用 `confirm_order(path, order_id, ...)` |
| 预期 | 不报错，confirmed_at 被更新为最新时间戳，覆盖写入 |

---

## 已知问题跟踪

| ID | 描述 | 相关文件 | 影响测试 |
|----|------|----------|----------|
| BUG-01 | `settings_screen.dart:_getPubkey` 传入 `svc.hubEndpoint` 作为 `storagePath`，而非文件系统路径 | settings_screen.dart:231 | TC-SET-01 |
| BUG-02 | Dashboard "在线"状态始终为绿色，无实际连接检查 | dashboard_screen.dart | TC-DASH-01 |
<!-- State Channel: 探索阶段，暂不启用
| BUG-03 | `channel_detail_screen.dart` 结算固定 claim leaf index 0，多 leaf 通道不适用 | channel_detail_screen.dart | TC-CH-04 |
-->
| BUG-04 | `OrderStatus::Failed/Expired` 无 Rust 函数触发转换 | merchant.rs | TC-ORD-03 |
| BUG-05 | audit 日志功能（append_audit/recent_audit）无 Dart 调用方 | merchant.rs | 无对应测试 |
| BUG-06 | WS 重连无指数退避和最大重试次数 | merchant_push_service.dart | TC-WS-03 |
