这份文档重点梳理了在 **MCP Server 位于内网（保护私钥）**，而 **手机端位于公网** 的环境下，如何通过 **Mediator（中继）** 模式安全地建立点对点（P2P）连接。

---

# DIDComm V2 点对点连接建立技术文档 (内网安全版)

## 1. 核心架构：Mediator 模式
由于 MCP Server 包含私钥且运行在本地内网，无法直接接收公网请求。因此引入 **Mediator (中继器)** 作为公网端点，负责消息的转发与暂存。



* **本地 MCP (Edge Agent)**：持有私钥，处理业务，位于内网。
* **云端 Mediator (Mediator)**：公网可见，无私钥，仅负责密文转发。
* **手机端 (Cloud Client)**：持有私钥，通过公网发起连接。

---

## 2. 准备阶段：建立中继通道
在生成邀请之前，本地 MCP 必须先在云端 Mediator 上“挂号”。

1.  **建立长连接**：本地 MCP 与云端 Mediator 建立 WebSocket (WSS) 持续连接。
2.  **分配路由密钥**：Mediator 为该连接分配一个 `routing_key`（如 `mcp_local_001`）。
3.  **心跳维持**：本地 MCP 保持连接，确保 Mediator 收到发往该路由的消息时能实时推送。

---

## 3. 连接建立流程 (P2P Handshake)

### 第一步：生成带中继信息的邀请 (OOB Invitation)
本地 MCP 生成二维码，其内容遵循 DIDComm Out-of-Band 规范。
* **Endpoint**：填写云端 Mediator 的公网地址。
* **Routing Keys**：包含 Mediator 分配给本地 MCP 的路由标识。



### 第二步：手机端解析与发送请求 (Connection Request)
1.  **扫码**：手机端解析二维码，获取 MCP 的 DID 和 **Mediator 的地址**。
2.  **封包**：手机端生成 `Connection Request`，包含自己的 DID 和 **FCM Token**。
3.  **加密**：使用 MCP 的公钥进行匿名加密 (Anoncrypt)。
4.  **投递**：手机端将加密包 POST 给云端 Mediator。

### 第三步：中继转发与解密
1.  **中继识别**：Mediator 收到请求，根据外层包裹识别出这是给 `mcp_local_001` 的，通过 WSS 转发给内网 MCP。
2.  **解密验签**：本地 MCP 在内网环境下解密消息。
3.  **存储绑定**：本地 MCP 将 `手机 DID` 与其 `FCM Token` 存入数据库，完成绑定。

---

## 4. 建立连接后的反向通知

### 4.1 海外用户 (FCM 通道)

当本地 MCP 需要主动联系手机时，流程如下：

1.  **本地加密**：MCP 使用私钥加密反馈消息 (JWM)。
2.  **上传中继**：MCP 通过 WSS 或 HTTPS 将 JWM 发给云端 Mediator。
3.  **信号触发**：云端 Mediator 存储 JWM，并调用 **FCM** 向手机发送 `msg_id`。
4.  **手机回拉**：手机收到通知，请求云端 Mediator 接口，拉取完整的加密包。

### 4.2 中国用户 (WebSocket 直推通道)

中国用户无法使用 FCM，采用 WebSocket 长连接直推：

1.  **本地加密**：MCP 使用私钥加密反馈消息 (JWM)。
2.  **上传中继**：MCP 通过 WSS 将 JWE 发给云端 Mediator。
3.  **WS 直推**：Mediator 检查用户 WS session 是否在线：
    - **在线**: 直接通过 WS 推送 JWE，手机实时接收。
    - **离线**: JWE 存入 message queue，手机重连后通过 Pickup 协议拉取。
4.  **手机处理**：手机直接解密 WS 收到的 JWE，无需额外拉取步骤。

### 4.3 通道选择

手机在注册时根据 locale/时区判断是否为中国用户：
- **中国用户**: 注册 `push_channel: "websocket"`，维持 WS 长连接
- **海外用户**: 注册 `push_channel: "fcm"` + FCM token

---

## 5. 安全性保障规约

| 维度 | 安全机制 | 说明 |
| :--- | :--- | :--- |
| **私钥安全** | **物理隔离** | 私钥永不离开内网环境，Mediator 接触不到任何密钥。 |
| **内容隐私** | **端到端加密 (E2EE)** | 消息在手机端加密，仅在本地 MCP 解密，中继服务器只看到密文。 |
| **身份防伪** | **DID 签名** | 每一条 Connection 消息都带有发送者的 DID 签名，防止伪造请求。 |
| **抗重放** | **JTI & Exp 校验** | 消息体包含唯一 ID 和过期时间，过期或重复的消息会被本地 MCP 拒绝。 |

---

## 6. 开发环境快速实施建议

### A. 云端 Mediator (极简版实现)
你可以用 Node.js 快速写一个中继转发器：
* **POST /msg**：接收手机端消息，根据 Header 中的路由信息放入队列。
* **WebSocket /ws**：本地 MCP 连接后，将对应队列中的消息推送过去。

### B. 本地 MCP 端
* 使用 `cloudflared` 作为备选穿透方案进行初期调试。
* 集成 `didcomm-rs` 等库处理解密逻辑。

### C. 手机端
* 在 `Connection Request` 的 `body` 中务必包含字段：`{"push_token": "...", "provider": "jpush"}`。

---

## 7. 总结
本方案通过 **“内网 MCP + 公网 Mediator”** 解决了私钥安全与公网可达性的矛盾。
* **建立连接时**：手机根据二维码找 Mediator，Mediator 找本地。
* **日常通信时**：本地找 Mediator，Mediator 通过 FCM “喊”手机，手机找 Mediator 拉取。

这种架构是目前处理 **私有化 Agent 与 移动端交互** 的工业级标准。