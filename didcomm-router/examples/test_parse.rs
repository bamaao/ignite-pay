use affinidi_messaging_didcomm::Message;

fn main() {
    let jwe = r#"{"protected":"test","recipients":[{"header":{"kid":"test"},"encrypted_key":"abc"}],"iv":"iv123","ciphertext":"cipher123","tag":"tag123"}"#;

    let forward = serde_json::json!({
        "type": "https://didcomm.org/routing/2.0/forward",
        "id": "fwd-test",
        "body": { "next": "did:test:phone" },
        "attachments": [{
            "data": { "json": serde_json::from_str::<serde_json::Value>(jwe).unwrap() }
        }]
    });

    let forward_str = serde_json::to_string(&forward).unwrap();
    println!("Forward JSON: {}", forward_str);

    let msg: Message = serde_json::from_str(&forward_str).unwrap();
    println!("Parsed type: {}", msg.typ);
    println!("Attachments: {:?}", msg.attachments);
    println!("Extra keys: {:?}", msg.extra.keys().collect::<Vec<_>>());

    // Extract inner
    if let Some(attachments) = &msg.attachments {
        if let Some(first) = attachments.first() {
            if let Some(json) = &first.data.json {
                let inner = serde_json::to_string(json).unwrap();
                println!("Extracted inner: {}", inner);

                // Check if it looks like JWE
                let v: serde_json::Value = serde_json::from_str(&inner).unwrap();
                println!("Has ciphertext: {}", v.get("ciphertext").is_some());
                println!("Has recipients: {}", v.get("recipients").is_some());
            }
        }
    }
}
