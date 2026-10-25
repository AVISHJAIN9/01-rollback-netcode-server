//! Cryptographic subsystem for Mini-QUIC — RFC 9001.
//!
//! Modules:
//! - `kex`  — X25519 key agreement (Feature 15)
//! - `aead` — ChaCha20-Poly1305 AEAD and header protection (Feature 16)
//! - `keys` — HKDF key derivation and key-phase tracking (Feature 17)

pub mod aead;
pub mod kex;
pub mod keys;

use core::fmt;

/// Errors produced by the cryptographic subsystem.
#[derive(Debug)]
pub enum CryptoError {
    /// An AEAD seal or open operation failed (authentication tag mismatch or
    /// malformed ciphertext).
    AeadFailure,
    /// The supplied public key bytes are not a valid X25519 point.
    InvalidPublicKey,
    /// The HKDF output length was too long (exceeds 255 * hash_len).
    HkdfOutputTooLong,
    /// The plaintext or ciphertext slice is too short to contain the
    /// required AEAD tag.
    CiphertextTooShort,
    /// The nonce supplied to a seal/open operation was the wrong length
    /// (must be 12 bytes for ChaCha20-Poly1305).
    InvalidNonceLength { expected: usize, provided: usize },
    /// The key slice was not the required length.
    InvalidKeyLength { expected: usize, provided: usize },
    /// A header-protection sample could not be extracted because the
    /// ciphertext was too short (needs at least 4 + 16 bytes).
    SampleTooShort,
}

impl fmt::Display for CryptoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CryptoError::AeadFailure => write!(f, "AEAD authentication failure"),
            CryptoError::InvalidPublicKey => write!(f, "invalid X25519 public key"),
            CryptoError::HkdfOutputTooLong => write!(f, "HKDF output length too large"),
            CryptoError::CiphertextTooShort => {
                write!(f, "ciphertext too short to contain AEAD tag")
            }
            CryptoError::InvalidNonceLength { expected, provided } => {
                write!(f, "invalid nonce length: expected {expected}, got {provided}")
            }
            CryptoError::InvalidKeyLength { expected, provided } => {
                write!(f, "invalid key length: expected {expected}, got {provided}")
            }
            CryptoError::SampleTooShort => {
                write!(f, "ciphertext too short to extract header-protection sample")
            }
        }
    }
}
