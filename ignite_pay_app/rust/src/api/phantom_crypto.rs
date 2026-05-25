// Phantom deep link NaCl crypto operations.
//
// Provides X25519 keypair generation, X25519 key exchange,
// and XSalsa20-Poly1305 encryption for the Phantom deep link protocol.
//
// Phantom uses TweetNaCl.js which expects X25519 (Curve25519) keys,
// NOT Ed25519 keys. See: https://docs.phantom.com/phantom-deeplinks/encryption

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Keypair for Phantom dApp encryption.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhantomKeypair {
    /// Base64url-encoded (no padding) X25519 public key (32 bytes).
    pub public_key_b64: String,
    /// Base64url-encoded (no padding) X25519 secret key (32 bytes).
    pub secret_key_b64: String,
}

/// Generate an X25519 keypair for the dApp side of Phantom deep link encryption.
pub fn phantom_generate_keypair() -> Result<PhantomKeypair> {
    let mut csprng = rand::rngs::OsRng;
    let secret = x25519_dalek::StaticSecret::random_from_rng(&mut csprng);
    let public = x25519_dalek::PublicKey::from(&secret);

    let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;

    Ok(PhantomKeypair {
        public_key_b64: base64::Engine::encode(&engine, public.as_bytes()),
        secret_key_b64: base64::Engine::encode(&engine, secret.as_bytes()),
    })
}

/// Compute the X25519 shared secret.
/// All keys are raw X25519 (Curve25519) — no Ed25519 conversion needed.
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

    let secret_array: [u8; 32] = my_secret_bytes.try_into().unwrap();
    let x25519_secret = x25519_dalek::StaticSecret::from(secret_array);

    let public_array: [u8; 32] = their_public_bytes.try_into().unwrap();
    let x25519_public = x25519_dalek::PublicKey::from(public_array);

    let shared = x25519_secret.diffie_hellman(&x25519_public);
    Ok(base64::Engine::encode(&engine, shared.as_bytes()))
}

/// NaCl box encrypt using XSalsa20-Poly1305.
/// Output format: MAC (16 bytes) + ciphertext (TweetNaCl.js convention).
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

    // Output: tag (16 bytes) + ciphertext (TweetNaCl.js / NaCl crypto_box format)
    let mut output = Vec::with_capacity(16 + buf.len());
    output.extend_from_slice(&tag);
    output.extend_from_slice(&buf);

    Ok(base64::Engine::encode(&engine, &output))
}

/// NaCl box decrypt using XSalsa20-Poly1305.
/// Input format: MAC (16 bytes) + ciphertext (TweetNaCl.js convention).
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

    // Split tag (first 16 bytes) and ciphertext — TweetNaCl.js / NaCl crypto_box format
    let tag = Tag::from_slice(&ciphertext_bytes[..16]);
    let mut buf = ciphertext_bytes[16..].to_vec();

    cipher
        .decrypt_in_place_detached(nonce, b"", &mut buf, tag)
        .map_err(|e| anyhow::anyhow!("Decryption failed: {}", e))?;

    Ok(base64::Engine::encode(&engine, &buf))
}
