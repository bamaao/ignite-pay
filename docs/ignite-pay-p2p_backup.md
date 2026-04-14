在 DIDComm V2 的架构中，MCP（Model Context Protocol）服务器或 Agent Skill 与手机端的绑定，本质上是**建立一种“点对点”的受信关系（Connection/Relationship）**。

要实现“DID 绑定”并通过“FCM 通知”，你需要完成：**身份交换、路由映射、凭证上报**这三个核心步骤。

---

## 1. DID 绑定流程：握手与建立连接

由于 DID 是去中心化的，MCP/Skill 无法预先知道手机端的地址。通常采用 **DIDComm Out-of-Band (OOB)** 协议进行绑定：

### 第一步：MCP/Skill 生成邀请
1. MCP 服务端生成一个包含自己 DID 的“连接邀请（Invitation）”。
2. 将邀请转换为一个二维码或深层链接（Deep Link）。

### 第二步：手机端扫码/解析
1. 手机端 Flutter App 扫描二维码，获取 MCP 的 DID。
2. **双向绑定**：手机端生成一条 `Connection Request` 消息，里面包含手机端的 DID。
3. **关键动作**：手机端在消息的 `decorator` 或自定义字段中，附带上自己的 **FCM Token**。

### 第三步：MCP 存储关系映射
MCP 收到请求后，在本地数据库建立映射表：
| 手机端 DID | 手机端 FCM Token | 信任状态 |
| :--- | :--- | :--- |
| `did:peer:user_123` | `fcm_token_abc...` | 已验证 |



---

## 2. 消息通知：如何让 FCM 找到正确的 DID

当 MCP 中的 Skill 产生了需要推送到手机的消息时，流程如下：

### 1. 业务触发
Skill 产生一条加密消息（JWM），目标指向 `did:peer:user_123`。

### 2. 查找路由 (Mediator 逻辑)
服务端逻辑检查该 DID 对应的推送凭证：
* 发现 `did:peer:user_123` 绑定了 `fcm_token_abc`。
* 发现该用户属于“中国区”，自动切换至 **JPush (极光)** 逻辑（如你之前设定的测试方案）。

### 3. 发送信号 (Signal)
服务端向 FCM 发送一个 Data Message。这个消息不含 DIDComm 明文，只含索引：
```json
{
  "to": "fcm_token_abc",
  "data": {
    "type": "DIDCOMM_ARRIVAL",
    "msg_id": "storage_uuid_001", // 服务端暂存这条加密消息的 ID
    "sender_did": "did:peer:mcp_skill_789"
  }
}
```

---

## 3. 技术实现细节 (指导开发)

### A. 手机端：如何上报 FCM Token？
在 Flutter 中，建议将 FCM Token 的更新封装为一种特定的 DIDComm 消息类型（例如 `https://didcomm.org/push-notifications/1.0/set-info`）：

```dart
// 伪代码：向 MCP 发送更新推送信息的 DIDComm 消息
var pushUpdateMsg = Message(
  type: "https://didcomm.org/push-notifications/1.0/set-info",
  from: myDid,
  to: mcpDid,
  body: {
    "fcm_token": currentFcmToken,
    "platform": "android",
    "region": "CN" // 用于国内切换极光测试
  }
);
// 使用 Authcrypt 加密发送给 MCP
```

### B. 服务端 (MCP/Skill)：如何处理？
当 Skill 需要推送时，它不直接调用 FCM，而是调用一个**消息分发组件（Mediator Service）**：

1. **缓存消息**：将加密后的 JWM 存入 Redis，Key 为 `msg_id`。
2. **查询绑定**：通过目标 DID 查出对应的 `fcm_token`。
3. **推送**：调用 Firebase Admin SDK 发送 `data` 类型的推送。

---

## 4. 关键点：国内测试下的“动态绑定”

在国内测试极光（JPush）时，逻辑是一致的：
1. 手机端上报的是极光的 **RegistrationID (RID)** 而不是 FCM Token。
2. 服务端在推送时，检测到该 DID 绑定的是 RID，则调用极光的 REST API 发送自定义消息。

---

## 5. 总结：绑定与通知的闭环



* **绑定（Identity Binding）**：通过 OOB 协议交换 DID，并将 **FCM Token/JPush ID** 作为 DID 的属性（或关联数据）上报给 MCP。
* **通知（Notification）**：MCP 产生消息 -> 查表获取 Token -> 发送 FCM 信号 -> 手机端回拉数据。

**架构建议：**
你可以在 MCP 服务端实现一个简单的 **DID-to-Push-Channel** 的映射服务。这样无论以后你增加多少个 Skill，它们只需要知道目标 DID，剩下的“怎么通知到手机”都由这个映射服务来处理。