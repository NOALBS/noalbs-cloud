//! Encryption for secrets we must be able to read back, such as the OBS
//! websocket password (unlike tokens, which are only hashed).
//!
//! AES-256-GCM with a random 96-bit nonce per message. The stored form is
//! `nonce || ciphertext` (the ciphertext includes the 16-byte tag).

use aes_gcm::{
    Aes256Gcm,
    aead::{Aead, Generate, Key, KeyInit, Nonce},
};
use base64::{Engine, engine::general_purpose::STANDARD};

const NONCE_LEN: usize = 12;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CryptoError {
    #[error("the data encryption key must be exactly 64 hexadecimal characters")]
    InvalidKey,
    #[error("the encrypted value is malformed or was encrypted with another key")]
    Decrypt,
}

/// The key that encrypts stored secrets, from the `data_encryption_key` setting.
#[derive(Clone)]
pub struct DataKey(Aes256Gcm);

impl std::fmt::Debug for DataKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DataKey(..)")
    }
}

impl DataKey {
    /// Parses a hex-encoded 32-byte key (64 hex characters, e.g. from
    /// `openssl rand -hex 32`).
    pub fn from_hex(hex: &str) -> Result<Self, CryptoError> {
        let bytes = decode_hex::<32>(hex).ok_or(CryptoError::InvalidKey)?;
        let key = Key::<Aes256Gcm>::from(bytes);

        Ok(Self(Aes256Gcm::new(&key)))
    }

    pub fn encrypt(&self, plaintext: &[u8]) -> Vec<u8> {
        let nonce = Nonce::<Aes256Gcm>::generate();
        let ciphertext = self
            .0
            .encrypt(&nonce, plaintext)
            .expect("AES-GCM encryption of an in-memory buffer can't fail");

        let mut stored = nonce.to_vec();
        stored.extend_from_slice(&ciphertext);
        stored
    }

    pub fn decrypt(&self, stored: &[u8]) -> Result<Vec<u8>, CryptoError> {
        if stored.len() < NONCE_LEN {
            return Err(CryptoError::Decrypt);
        }
        let (nonce, ciphertext) = stored.split_at(NONCE_LEN);
        let nonce = Nonce::<Aes256Gcm>::try_from(nonce).map_err(|_| CryptoError::Decrypt)?;

        self.0
            .decrypt(&nonce, ciphertext)
            .map_err(|_| CryptoError::Decrypt)
    }

    /// Encrypts a secret for storing as text (e.g. inside JSON): base64 of
    /// `nonce || ciphertext`.
    pub fn encrypt_to_string(&self, plaintext: &str) -> String {
        STANDARD.encode(self.encrypt(plaintext.as_bytes()))
    }

    /// Reverses [`DataKey::encrypt_to_string`].
    pub fn decrypt_string(&self, stored: &str) -> Result<String, CryptoError> {
        let bytes = STANDARD.decode(stored).map_err(|_| CryptoError::Decrypt)?;
        String::from_utf8(self.decrypt(&bytes)?).map_err(|_| CryptoError::Decrypt)
    }
}

fn decode_hex<const N: usize>(hex: &str) -> Option<[u8; N]> {
    let hex = hex.as_bytes();
    if hex.len() != N * 2 {
        return None;
    }

    let mut bytes = [0u8; N];
    for (byte, pair) in bytes.iter_mut().zip(hex.chunks_exact(2)) {
        let high = (pair[0] as char).to_digit(16)?;
        let low = (pair[1] as char).to_digit(16)?;
        *byte = (high * 16 + low) as u8;
    }

    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::{CryptoError, DataKey};

    const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn string_form_round_trips() {
        let key = DataKey::from_hex(KEY).unwrap();
        let stored = key.encrypt_to_string("hunter2");
        assert_ne!(stored, "hunter2");
        assert_eq!(key.decrypt_string(&stored).unwrap(), "hunter2");
        assert_eq!(key.decrypt_string("not base64!"), Err(CryptoError::Decrypt));
    }

    #[test]
    fn round_trips_and_uses_a_fresh_nonce() {
        let key = DataKey::from_hex(KEY).unwrap();
        let a = key.encrypt(b"obs-password");
        let b = key.encrypt(b"obs-password");

        assert_ne!(a, b);
        assert_eq!(key.decrypt(&a).unwrap(), b"obs-password");
        assert_eq!(key.decrypt(&b).unwrap(), b"obs-password");
    }

    #[test]
    fn rejects_bad_keys_and_tampered_data() {
        assert_eq!(
            DataKey::from_hex("abc").unwrap_err(),
            CryptoError::InvalidKey
        );
        assert!(DataKey::from_hex(&"zz".repeat(32)).is_err());

        let key = DataKey::from_hex(KEY).unwrap();
        let mut stored = key.encrypt(b"secret");
        *stored.last_mut().unwrap() ^= 1;
        assert_eq!(key.decrypt(&stored), Err(CryptoError::Decrypt));
        assert_eq!(key.decrypt(b"short"), Err(CryptoError::Decrypt));

        let other = DataKey::from_hex(&"f".repeat(64)).unwrap();
        assert_eq!(
            other.decrypt(&key.encrypt(b"secret")),
            Err(CryptoError::Decrypt)
        );
    }
}
