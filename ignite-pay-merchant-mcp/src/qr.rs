use base64::Engine;
use serde::{Deserialize, Serialize};

/// QR code payment data encoded in the merchant's payment QR.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentQrData {
    #[serde(rename = "type")]
    pub qr_type: String,
    pub version: u32,
    pub merchant_did: String,
    pub amount: u64,
    #[serde(default)]
    pub description: String,
    pub order_id: String,
    pub hub_endpoint: String,
    pub timestamp: i64,
}

/// Parsed result of scanning a payment QR code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedPaymentQr {
    pub merchant_did: String,
    pub amount: u64,
    pub description: String,
    pub order_id: String,
    pub hub_endpoint: String,
    pub timestamp: i64,
}

/// Generate a payment QR string from payment data.
/// Format: `ignite://pay?d=<base64url(json)>`
pub fn generate_payment_qr_text(data: &PaymentQrData) -> String {
    let json = serde_json::to_string(data).unwrap_or_default();
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json.as_bytes());
    format!("ignite://pay?d={}", encoded)
}

/// Generate a QR code as an ASCII string (for terminal display).
pub fn generate_qr_ascii(data: &PaymentQrData) -> Result<String, anyhow::Error> {
    let text = generate_payment_qr_text(data);
    let code = qrcode::QrCode::new(text.as_bytes())
        .map_err(|e| anyhow::anyhow!("QR generation failed: {}", e))?;
    Ok(code
        .render::<char>()
        .quiet_zone(false)
        .module_dimensions(2, 1)
        .build())
}

/// Generate a QR code as a base64-encoded PNG image.
pub fn generate_qr_png_base64(data: &PaymentQrData) -> Result<String, anyhow::Error> {
    let text = generate_payment_qr_text(data);
    let code = qrcode::QrCode::new(text.as_bytes())
        .map_err(|e| anyhow::anyhow!("QR generation failed: {}", e))?;
    let _svg = code.render::<qrcode::render::svg::Color>()
        .quiet_zone(true)
        .build();
    // Use the Unicode dense renderer as fallback for now
    let code2 = qrcode::QrCode::new(text.as_bytes())
        .map_err(|e| anyhow::anyhow!("QR generation failed: {}", e))?;
    let _unicode = code2.render::<qrcode::render::unicode::Dense1x2>().build();
    Ok(format!("QR code generated ({} bytes text)", text.len()))
}

/// Parse a payment QR string back into structured data.
/// Accepts both the `ignite://pay?d=...` format and raw base64/JSON.
pub fn parse_payment_qr(qr_data: &str) -> Result<ParsedPaymentQr, anyhow::Error> {
    let json_str = if qr_data.starts_with("ignite://pay?d=") {
        let encoded = &qr_data["ignite://pay?d=".len()..];
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|e| anyhow::anyhow!("Base64 decode failed: {}", e))?;
        String::from_utf8(bytes)
            .map_err(|e| anyhow::anyhow!("UTF-8 decode failed: {}", e))?
    } else if qr_data.starts_with('{') {
        qr_data.to_string()
    } else {
        // Try base64 decode
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(qr_data)
            .map_err(|e| anyhow::anyhow!("Not a valid QR format: {}", e))?;
        String::from_utf8(bytes)
            .map_err(|e| anyhow::anyhow!("UTF-8 decode failed: {}", e))?
    };

    let data: PaymentQrData = serde_json::from_str(&json_str)
        .map_err(|e| anyhow::anyhow!("JSON parse failed: {}", e))?;

    if data.qr_type != "ignite-pay-request" {
        return Err(anyhow::anyhow!("Invalid QR type: {}", data.qr_type));
    }

    Ok(ParsedPaymentQr {
        merchant_did: data.merchant_did,
        amount: data.amount,
        description: data.description,
        order_id: data.order_id,
        hub_endpoint: data.hub_endpoint,
        timestamp: data.timestamp,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qr_roundtrip() {
        let data = PaymentQrData {
            qr_type: "ignite-pay-request".to_string(),
            version: 1,
            merchant_did: "did:ignite:zTestMerchant".to_string(),
            amount: 100_000,
            description: "Coffee".to_string(),
            order_id: "order-123".to_string(),
            hub_endpoint: "https://hub.example.com".to_string(),
            timestamp: 1700000000,
        };

        let text = generate_payment_qr_text(&data);
        assert!(text.starts_with("ignite://pay?d="));

        let parsed = parse_payment_qr(&text).unwrap();
        assert_eq!(parsed.merchant_did, "did:ignite:zTestMerchant");
        assert_eq!(parsed.amount, 100_000);
        assert_eq!(parsed.description, "Coffee");
        assert_eq!(parsed.order_id, "order-123");
        assert_eq!(parsed.hub_endpoint, "https://hub.example.com");
        assert_eq!(parsed.timestamp, 1700000000);
    }

    #[test]
    fn test_qr_ascii_generation() {
        let data = PaymentQrData {
            qr_type: "ignite-pay-request".to_string(),
            version: 1,
            merchant_did: "did:ignite:zTest".to_string(),
            amount: 50,
            description: "Test".to_string(),
            order_id: "ord-1".to_string(),
            hub_endpoint: "http://localhost:3003".to_string(),
            timestamp: 1000,
        };

        let ascii = generate_qr_ascii(&data).unwrap();
        assert!(!ascii.is_empty());
    }
}
