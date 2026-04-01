//! Cryptographic primitives used across the backend.
//!
//! * Password hashing  — Argon2id (default params: m=19456, t=2, p=1)
//! * AES-256-GCM       — symmetric encryption for PII fields at rest
//! * HMAC-SHA-256      — request signing between UI and server

use anyhow::{Context, Result};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};

// ── Password hashing ──────────────────────────────────────────────────────────

/// Hash a plaintext password with Argon2id.  Returns the PHC string.
pub fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| anyhow::anyhow!("Argon2 hash error: {e}"))
}

/// Verify a plaintext password against a stored PHC string.
pub fn verify_password(password: &str, hash: &str) -> Result<bool> {
    let parsed = PasswordHash::new(hash)
        .map_err(|e| anyhow::anyhow!("Invalid PHC hash: {e}"))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

/// Validate that a password meets minimum policy requirements.
/// Returns an error message if invalid, None if acceptable.
pub fn validate_password(password: &str) -> Option<String> {
    if password.len() < shared::rules::MIN_PASSWORD_LEN {
        return Some(format!(
            "Password must be at least {} characters (got {})",
            shared::rules::MIN_PASSWORD_LEN,
            password.len(),
        ));
    }
    None
}

// ── AES-256-GCM PII encryption ────────────────────────────────────────────────

use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng as AesOsRng},
    Aes256Gcm, Nonce,
};

/// Encrypt a UTF-8 string with AES-256-GCM.
/// Returns `nonce (12 B) || ciphertext` as a `Vec<u8>`.
pub fn encrypt_pii(plaintext: &str, key_bytes: &[u8; 32]) -> Result<Vec<u8>> {
    let cipher = Aes256Gcm::new_from_slice(key_bytes)
        .map_err(|e| anyhow::anyhow!("AES key error: {e}"))?;
    let nonce = Aes256Gcm::generate_nonce(&mut AesOsRng);
    let ciphertext = cipher
        .encrypt(&nonce, plaintext.as_bytes())
        .map_err(|e| anyhow::anyhow!("AES encrypt error: {e}"))?;
    let mut out = nonce.to_vec();
    out.extend(ciphertext);
    Ok(out)
}

/// Decrypt a blob produced by `encrypt_pii`.
pub fn decrypt_pii(blob: &[u8], key_bytes: &[u8; 32]) -> Result<String> {
    if blob.len() < 12 {
        anyhow::bail!("Ciphertext blob too short");
    }
    let (nonce_bytes, ciphertext) = blob.split_at(12);
    let cipher = Aes256Gcm::new_from_slice(key_bytes)
        .map_err(|e| anyhow::anyhow!("AES key error: {e}"))?;
    let nonce = Nonce::from_slice(nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| anyhow::anyhow!("AES decrypt error: {e}"))?;
    String::from_utf8(plaintext).context("Decrypted PII is not valid UTF-8")
}

/// Derive a 32-byte AES key from a string secret using SHA-256.
pub fn derive_aes_key(secret: &str) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    hasher.finalize().into()
}

// ── HMAC-SHA-256 request signing ──────────────────────────────────────────────

use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Compute an HMAC-SHA-256 signature over `message` using `secret`.
pub fn hmac_sign(secret: &str, message: &str) -> String {
    // Qualify the trait explicitly: both `KeyInit` and `hmac::Mac` provide
    // `new_from_slice` in newer digest/hmac versions, causing E0034 ambiguity.
    let mut mac = <HmacSha256 as Mac>::new_from_slice(secret.as_bytes())
        .expect("HMAC accepts any key length");
    mac.update(message.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// SHA-256 hex digest — used to store session token fingerprints.
pub fn sha256_hex(input: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(input.as_bytes()))
}

/// Verify a signature in constant time.
pub fn hmac_verify(secret: &str, message: &str, signature: &str) -> bool {
    let expected = hmac_sign(secret, message);
    // Use `subtle` for constant-time comparison to prevent timing attacks.
    use subtle::ConstantTimeEq;
    expected.as_bytes().ct_eq(signature.as_bytes()).into()
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_round_trip() {
        println!("[crypto] password_round_trip: hashing 'SecureP@ss123'");
        let hash = hash_password("SecureP@ss123").expect("hash succeeded");
        assert!(hash.starts_with("$argon2id$"), "hash should be argon2id PHC format");
        assert!(verify_password("SecureP@ss123", &hash).unwrap(), "correct password verifies");
        assert!(!verify_password("wrong_password", &hash).unwrap(), "wrong password rejected");
        println!("[crypto] password_round_trip: PASS");
    }

    #[test]
    fn aes_gcm_round_trip() {
        println!("[crypto] aes_gcm_round_trip: encrypting PII");
        let key = derive_aes_key("test-secret-key-for-aes");
        let plaintext = "passenger@email.com";
        let blob = encrypt_pii(plaintext, &key).expect("encryption succeeded");
        assert!(blob.len() > 12, "blob contains nonce + ciphertext");
        let recovered = decrypt_pii(&blob, &key).expect("decryption succeeded");
        assert_eq!(recovered, plaintext, "round-trip matches");
        println!("[crypto] aes_gcm_round_trip: PASS");
    }

    #[test]
    fn aes_gcm_wrong_key_fails() {
        println!("[crypto] aes_gcm_wrong_key_fails");
        let key1 = derive_aes_key("secret-key-one");
        let key2 = derive_aes_key("secret-key-two");
        let blob = encrypt_pii("sensitive-data", &key1).expect("encrypt ok");
        assert!(decrypt_pii(&blob, &key2).is_err(), "wrong key should fail to decrypt");
        println!("[crypto] aes_gcm_wrong_key_fails: PASS");
    }

    #[test]
    fn hmac_sign_and_verify() {
        println!("[crypto] hmac_sign_and_verify");
        let secret  = "super-secret";
        let message = "GET /api/v1/rules 1700000000";
        let sig = hmac_sign(secret, message);
        assert_eq!(sig.len(), 64, "HMAC-SHA256 hex is 64 chars");
        assert!(hmac_verify(secret, message, &sig),  "correct sig verifies");
        assert!(!hmac_verify(secret, "tampered",  &sig), "tampered message rejected");
        assert!(!hmac_verify("wrong", message,   &sig), "wrong secret rejected");
        println!("[crypto] hmac_sign_and_verify: PASS");
    }

    #[test]
    fn sha256_hex_deterministic() {
        println!("[crypto] sha256_hex_deterministic");
        let h1 = sha256_hex("hello world");
        let h2 = sha256_hex("hello world");
        let h3 = sha256_hex("hello world!");
        assert_eq!(h1, h2, "same input gives same hash");
        assert_ne!(h1, h3, "different input gives different hash");
        assert_eq!(h1.len(), 64, "SHA-256 hex is 64 chars");
        println!("[crypto] sha256_hex_deterministic: PASS");
    }

    #[test]
    fn derive_aes_key_length() {
        println!("[crypto] derive_aes_key_length");
        let key = derive_aes_key("any-secret");
        assert_eq!(key.len(), 32, "AES key must be exactly 32 bytes");
        println!("[crypto] derive_aes_key_length: PASS");
    }
}
