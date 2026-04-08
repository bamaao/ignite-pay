// #[flutter_rust_bridge::frb(sync)] // Synchronous mode for simplicity of the demo
// pub fn greet(name: String) -> String {
//     format!("Hello, {name}!")
// }

// #[flutter_rust_bridge::frb(init)]
// pub fn init_app() {
//     // Default utilities - feel free to customize
//     flutter_rust_bridge::setup_default_user_utils();
// }
use anyhow::Result;

// 定义返回给 Flutter 的结构体
pub struct AuthGrant {
    pub merchant_did: String,
    pub amount: u64,
    pub signature: String,
}

// 这是一个异步函数，会自动映射为 Dart 的 Future
pub async fn sign_payment(merchant_did: String, amount: u64) -> Result<AuthGrant> {
    // 这里未来会调用你存储在手机安全隔层的私钥
    // 暂时先写一个模拟签名
    let mock_signature = format!("sig_of_{}_for_{}", merchant_did, amount);
    
    Ok(AuthGrant {
        merchant_did,
        amount,
        signature: mock_signature,
    })
}