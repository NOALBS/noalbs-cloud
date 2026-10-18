//! Secret tokens handed out once (invite links, OBS connect tokens) and only
//! stored as a SHA-256 hash.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

/// Random bytes in a token (256 bits).
const TOKEN_BYTES: usize = 32;

/// Generates a URL-safe random token (43 characters).
///
/// # Panics
///
/// If the operating system's random number generator is unavailable.
pub fn generate() -> String {
    let mut bytes = [0u8; TOKEN_BYTES];
    getrandom::fill(&mut bytes).expect("the OS random number generator is available");
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Characters of [`password`]: letters and digits, so it's easy to type on a
/// phone.
const PASSWORD_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
/// Length of [`password`] (about 119 bits).
const PASSWORD_LENGTH: usize = 20;

/// Generates a random password of letters and digits, for secrets people type
/// into another app (Moblin's remote control).
///
/// # Panics
///
/// If the operating system's random number generator is unavailable.
pub fn password() -> String {
    // 248 is the largest multiple of 62 below 256: rejecting bytes above it keeps
    // every character equally likely.
    let limit = (256 / PASSWORD_CHARS.len() * PASSWORD_CHARS.len()) as u8;
    let mut password = String::with_capacity(PASSWORD_LENGTH);
    let mut bytes = [0u8; 32];
    while password.len() < PASSWORD_LENGTH {
        getrandom::fill(&mut bytes).expect("the OS random number generator is available");
        for &byte in bytes.iter().filter(|&&byte| byte < limit) {
            if password.len() == PASSWORD_LENGTH {
                break;
            }
            password.push(PASSWORD_CHARS[usize::from(byte) % PASSWORD_CHARS.len()] as char);
        }
    }
    password
}

/// The SHA-256 of a token, which is what gets stored and looked up.
pub fn hash(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

#[cfg(test)]
mod tests {
    use super::{generate, hash, password};

    #[test]
    fn passwords_are_alphanumeric_and_unique() {
        let a = password();
        assert_eq!(a.len(), 20);
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric()));
        assert_ne!(a, password());
    }

    #[test]
    fn tokens_are_url_safe_and_unique() {
        let a = generate();
        let b = generate();
        assert_eq!(a.len(), 43);
        assert_ne!(a, b);
        assert!(
            a.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        );
    }

    #[test]
    fn hash_is_stable_and_distinguishes_tokens() {
        assert_eq!(hash("abc"), hash("abc"));
        assert_ne!(hash("abc"), hash("abd"));
    }
}
