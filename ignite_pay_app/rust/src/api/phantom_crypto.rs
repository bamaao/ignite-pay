// Phantom deep link NaCl crypto operations.
//
// Provides Ed25519 keypair generation, X25519 key exchange,
// and XSalsa20-Poly1305 encryption for the Phantom deep link protocol.

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Keypair for Phantom dApp encryption.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhantomKeypair {
    /// Base64url-encoded (no padding) Ed25519 public key (32 bytes).
    pub public_key_b64: String,
    /// Base64url-encoded (no padding) Ed25519 secret key (32 bytes seed).
    pub secret_key_b64: String,
}

/// Generate an Ed25519 keypair for the dApp side of Phantom deep link encryption.
pub fn phantom_generate_keypair() -> Result<PhantomKeypair> {
    let mut csprng = rand::rngs::OsRng;
    let signing = ed25519_dalek::SigningKey::generate(&mut csprng);
    let pubkey_bytes = signing.verifying_key().to_bytes();
    let secret_bytes = signing.to_bytes(); // 32-byte seed

    let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;

    Ok(PhantomKeypair {
        public_key_b64: base64::Engine::encode(&engine, &pubkey_bytes),
        secret_key_b64: base64::Engine::encode(&engine, &secret_bytes),
    })
}

/// Compute the X25519 shared secret.
/// Ed25519 keys are converted to X25519 internally.
pub fn phantom_shared_secret(
    my_secret_key_b64: String,
    their_public_key_b64: String,
) -> Result<String> {
    let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;

    let my_secret_bytes = base64::Engine::decode(&engine, &my_secret_key_b64)?;
    let their_public_bytes = base64::Engine::decode(&engine, &their_public_key_b64)?;

    if my_secret_bytes.len() != 32 || their_public_bytes.len() != 32 {
        return Err(anyhow::anyhow!("Invalid key length"));
    }

    // Ed25519 secret -> X25519 secret via libsodium convention:
    // SHA-512 hash the 32-byte seed, clamp the first 32 bytes of the output.
    let my_secret_array: [u8; 32] = my_secret_bytes.try_into().unwrap();

    let mut x25519_secret_bytes = <sha2::Sha512 as sha2::Digest>::digest(my_secret_array);
    // Clamp first 32 bytes
    x25519_secret_bytes[0] &= 248;
    x25519_secret_bytes[31] &= 127;
    x25519_secret_bytes[31] |= 64;
    let secret_array: [u8; 32] = x25519_secret_bytes[..32].try_into().unwrap();
    let x25519_secret = x25519_dalek::StaticSecret::from(secret_array);

    // Their public key: try Ed25519 -> X25519 conversion, else use directly
    let their_public_array: [u8; 32] = their_public_bytes.try_into().unwrap();
    let x25519_public = match ed25519_dalek::VerifyingKey::from_bytes(&their_public_array) {
        Ok(vk) => x25519_dalek::PublicKey::from(vk.to_montgomery().to_bytes()),
        Err(_) => x25519_dalek::PublicKey::from(their_public_array),
    };

    let shared = x25519_secret.diffie_hellman(&x25519_public);
    Ok(base64::Engine::encode(&engine, shared.as_bytes()))
}

/// NaCl box encrypt using XSalsa20-Poly1305.
pub fn phantom_encrypt(
    shared_secret_b64: String,
    nonce_b64: String,
    plaintext_b64: String,
) -> Result<String> {
    let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;

    let shared_bytes = base64::Engine::decode(&engine, &shared_secret_b64)?;
    let nonce_bytes = base64::Engine::decode(&engine, &nonce_b64)?;
    let plaintext_bytes = base64::Engine::decode(&engine, &plaintext_b64)?;

    if shared_bytes.len() != 32 {
        return Err(anyhow::anyhow!("Invalid shared secret length"));
    }
    if nonce_bytes.len() != 24 {
        return Err(anyhow::anyhow!(
            "Invalid nonce length: expected 24, got {}",
            nonce_bytes.len()
        ));
    }

    use xsalsa20poly1305::{AeadInPlace, KeyInit, Nonce, XSalsa20Poly1305};

    let key = xsalsa20poly1305::Key::from_slice(&shared_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let cipher = XSalsa20Poly1305::new(key);

    let mut buf = plaintext_bytes;
    let tag = cipher
        .encrypt_in_place_detached(nonce, b"", &mut buf)
        .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?;

    // Output: ciphertext + tag (NaCl crypto_box_easy format: MAC at end)
    let mut output = Vec::with_capacity(buf.len() + 16);
    output.extend_from_slice(&buf);
    output.extend_from_slice(&tag);

    Ok(base64::Engine::encode(&engine, &output))
}

/// NaCl box decrypt using XSalsa20-Poly1305.
pub fn phantom_decrypt(
    shared_secret_b64: String,
    nonce_b64: String,
    ciphertext_b64: String,
) -> Result<String> {
    let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;

    let shared_bytes = base64::Engine::decode(&engine, &shared_secret_b64)?;
    let nonce_bytes = base64::Engine::decode(&engine, &nonce_b64)?;
    let ciphertext_bytes = base64::Engine::decode(&engine, &ciphertext_b64)?;

    if shared_bytes.len() != 32 {
        return Err(anyhow::anyhow!("Invalid shared secret length"));
    }
    if nonce_bytes.len() != 24 {
        return Err(anyhow::anyhow!("Invalid nonce length"));
    }
    if ciphertext_bytes.len() < 16 {
        return Err(anyhow::anyhow!("Ciphertext too short"));
    }

    use xsalsa20poly1305::{AeadInPlace, KeyInit, Nonce, Tag, XSalsa20Poly1305};

    let key = xsalsa20poly1305::Key::from_slice(&shared_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let cipher = XSalsa20Poly1305::new(key);

    // Split ciphertext and tag (last 16 bytes) — NaCl crypto_box_easy format
    let ct_len = ciphertext_bytes.len() - 16;
    let tag = Tag::from_slice(&ciphertext_bytes[ct_len..]);
    let mut buf = ciphertext_bytes[..ct_len].to_vec();

    cipher
        .decrypt_in_place_detached(nonce, b"", &mut buf, tag)
        .map_err(|e| anyhow::anyhow!("Decryption failed: {}", e))?;

    Ok(base64::Engine::encode(&engine, &buf))
}
