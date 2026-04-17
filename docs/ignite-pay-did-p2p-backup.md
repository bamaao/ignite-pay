要在 MCP Server 和手机端之间建立基于 DID 的互信连接，本质上是实现一套 **“双向异步身份验证与加密通道协商”** 的过程。由于 MCP Server 通常作为常驻服务，而手机端作为控制端，两者的交互可以参考 **DIDComm** 协议的简化版。

**《用户端：多端 DID 身份互信与连接指南》**

---

### 1. 核心逻辑：从“身份绑定”到“安全握手”

不要尝试在多个设备上共用一套私钥。正确做法是：**每个端点（Endpoint）拥有独立的密钥对，通过用户的 Root DID 进行授权关联。**

* **Root DID**: 用户的根身份（通常对应 Solana 钱包）。
* **Mobile DID/Key**: 手机 App 生成的本地临时身份。
* **Server DID/Key**: MCP Server 生成的本地服务身份。

---

### 2. 连接建立流程（三阶段）

#### 第一阶段：发现与握手发起 (Discovery)
通常由 MCP Server 提供连接入口。
1.  **Server 生成邀请**：MCP Server 生成一个包含其 **Server DID** 和 **Service Endpoint**（如 IP 地址或域名）的二维码。
2.  **手机扫码**：用户打开 App 扫描二维码，获取 Server 的身份信息。

#### 第二阶段：身份互验证 (Mutual Authentication)
这是建立信任的关键，双方需要证明“我们服务于同一个主人”。
1.  **凭证交换**：双方交换各自的 **Delegate VC**（由用户 Root DID 签发的授权凭证）。
    * *App 凭证内容*：“Root DID 授权 $PK_{mobile}$ 代表我。”
    * *Server 凭证内容*：“Root DID 授权 $PK_{server}$ 代表我。”
2.  **链上/本地验证**：
    * App 检查 Server 的凭证，确认其 Subject 确实是同一个 Root DID，且在 ZK 压缩状态树中处于“已授权”状态。
    * Server 执行同样的检查。

#### 第三阶段：加密通道建立 (Encryption)
1.  **密钥交换**：双方利用各自的临时公钥通过 **Diffie-Hellman (ECDH)** 算法协商出一个对称加密密钥。
2.  **通道加密**：后续所有 MCP 指令（如读取文件、发送支付请求）均通过此加密通道传输，确保第三方无法监听。

---

### 3. 开发实现指南 (Implementation)

#### 3.1 手机端 (App Side)
* **密钥管理**：使用手机的安全芯片（如 iOS Secure Enclave）存储 $PK_{mobile}$。
* **授权请求**：初次连接时，调用用户的 Solana 钱包（如 Phantom）对 MCP 的公钥进行签名，生成一份授权 VC。

#### 3.2 服务端 (MCP Server Side)
* **身份持久化**：Server 在初始化时生成自己的 DID。
* **权限校验过滤器**：在处理 MCP Protocol 的 `call_tool` 或 `read_resource` 请求前，必须先验证请求来源的 `Signer` 是否在授权列表中。

#### 3.3 存储结构 (ZK Compression 视角)
建议在用户的压缩账户中维护一个 **端点列表**：

```rust
pub struct Endpoint {
    pub pubkey: Pubkey,      // 端点（手机或Server）的公钥
    pub name: String,        // 别名，如 "iPhone 15" 或 "Home_Server"
    pub status: u8,          // 状态：1=已激活, 0=已撤销
}
```

---

### 4. 安全场景应对

| 场景 | 解决方案 |
| :--- | :--- |
| **手机丢失** | 用户使用 Root 钱包在链上发送 `revoke_endpoint` 指令，撤销手机的 $PK_{mobile}$。MCP Server 检测到撤销后，自动断开连接。 |
| **更换 MCP 环境** | 重新进行“扫码-授权-绑定”流程。由于 Root DID 没变，所有历史数据和 VC 依然可以无缝迁移。 |
| **中间人攻击** | 通过双向验证 VC 和 ECDH 密钥交换，黑客即便截获二维码也无法伪造来自 Root DID 的授权签名。 |

---

### 5. 文档摘要（供开发参考）

**连接指令示例 (JSON-RPC 风格):**
```json
{
  "method": "did_connect",
  "params": {
    "requester_did": "did:sol:app_address",
    "authorization_proof": "BASE64_SIGNATURE_FROM_ROOT_DID",
    "ephemeral_key": "ED25519_PUBLIC_KEY"
  }
}
```

**总结：**
这一套方案让 MCP Server 和手机 App 形成了 **“一个核心（Root DID），多个端点（Delegates）”** 的网状互信结构。这种设计既保证了单点泄露不波及全局，又实现了跨设备的无感协同。
