//! X25519 Ephemeral Key Agreement — RFC 9001 §5.
//!
//! Generates ephemeral X25519 keypairs and computes Diffie–Hellman shared
//! secrets.  The shared secret material is zeroized on drop via the `Zeroize`
//! trait, preventing secret material from lingering in heap memory after use.
//!
//! # Usage
//! ```rust,ignore
//! let alice = KeyAgreement::generate();
//! let bob   = KeyAgreement::generate();
//!
//! let alice_shared = alice.diffie_hellman(bob.public_key_bytes()).unwrap();
//! let bob_shared   = bob.diffie_hellman(alice.public_key_bytes()).unwrap();
//! assert_eq!(alice_shared.as_bytes(), bob_shared.as_bytes());
//! ```

use x25519_dalek::{EphemeralSecret, PublicKey};
use zeroize::Zeroizing;

// x25519-dalek 2.x uses rand_core::OsRng for ephemeral key generation.
use rand_core::OsRng;

use super::CryptoError;

// ─── SharedSecret ─────────────────────────────────────────────────────────────

/// The 32-byte Diffie–Hellman shared secret produced by an X25519 key exchange.
///
/// Wrapped in [`Zeroizing`] so the underlying bytes are cleared from memory
/// when the value is dropped.
pub struct SharedSecret(Zeroizing<[u8; 32]>);

impl SharedSecret {
    /// Return a reference to the raw 32-byte secret bytes.
    ///
    /// Callers should copy these bytes into an HKDF extract step immediately
    /// and drop this value to ensure timely zeroization.
    #[inline]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl core::fmt::Debug for SharedSecret {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("SharedSecret").field(&"<redacted>").finish()
    }
}

// ─── KeyAgreement ─────────────────────────────────────────────────────────────

/// An ephemeral X25519 keypair used for one Diffie–Hellman operation.
///
/// The private key (`EphemeralSecret`) is consumed on use — it cannot be
/// cloned or copied, which enforces the "ephemeral" property.
pub struct KeyAgreement {
    secret: EphemeralSecret,
    public: PublicKey,
}

impl KeyAgreement {
    /// Generate a fresh ephemeral X25519 keypair using the OS CSPRNG.
    pub fn generate() -> Self {
        let secret = EphemeralSecret::random_from_rng(OsRng);
        let public = PublicKey::from(&secret);
        KeyAgreement { secret, public }
    }

    /// Return the 32-byte public key bytes that should be sent to the peer.
    #[inline]
    pub fn public_key_bytes(&self) -> &[u8; 32] {
        self.public.as_bytes()
    }

    /// Compute the shared secret from this party's private key and the peer's
    /// raw 32-byte public key.
    ///
    /// Consumes `self` because `EphemeralSecret` is single-use: once the DH
    /// operation has been performed the private scalar must not be reused.
    ///
    /// # Errors
    /// Returns [`CryptoError::InvalidPublicKey`] when `peer_public` does not
    /// represent a valid X25519 point (all-zero or low-order point).
    pub fn diffie_hellman(
        self,
        peer_public: &[u8; 32],
    ) -> Result<SharedSecret, CryptoError> {
        // x25519-dalek's EphemeralSecret performs a contributory check
        // (low-order point check) internally; a successful DH always yields
        // a non-zero result.
        let peer_pub = PublicKey::from(*peer_public);

        // EphemeralSecret::diffie_hellman returns x25519_dalek::SharedSecret
        // which implements Zeroize automatically.
        let dh = self.secret.diffie_hellman(&peer_pub);

        // Reject the all-zero output, which indicates a low-order peer key
        // (the x25519-dalek crate does not reject these by default).
        let raw: [u8; 32] = *dh.as_bytes();
        if raw == [0u8; 32] {
            return Err(CryptoError::InvalidPublicKey);
        }

        Ok(SharedSecret(Zeroizing::new(raw)))
    }
}

impl core::fmt::Debug for KeyAgreement {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("KeyAgreement")
            .field("public_key", &hex_encode(self.public_key_bytes()))
            .finish()
    }
}

fn hex_encode(b: &[u8]) -> alloc::string::String {
    use alloc::string::String;
    let mut s = String::with_capacity(b.len() * 2);
    for byte in b {
        alloc::format!("{byte:02x}").chars().for_each(|c| s.push(c));
    }
    s
}

// ─── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_secret_symmetry() {
        // Alice and Bob compute the same shared secret.
        let alice = KeyAgreement::generate();
        let bob = KeyAgreement::generate();

        let alice_pub = *alice.public_key_bytes();
        let bob_pub = *bob.public_key_bytes();

        let alice_shared = alice.diffie_hellman(&bob_pub).unwrap();
        let bob_shared = bob.diffie_hellman(&alice_pub).unwrap();

        assert_eq!(
            alice_shared.as_bytes(),
            bob_shared.as_bytes(),
            "Alice and Bob must compute the same shared secret"
        );
    }

    #[test]
    fn distinct_keypairs_produce_different_public_keys() {
        let a = KeyAgreement::generate();
        let b = KeyAgreement::generate();
        assert_ne!(
            a.public_key_bytes(),
            b.public_key_bytes(),
            "independent ephemeral keys must differ"
        );
    }

    #[test]
    fn shared_secret_is_32_bytes() {
        let alice = KeyAgreement::generate();
        let bob = KeyAgreement::generate();
        let bob_pub = *bob.public_key_bytes();
        let secret = alice.diffie_hellman(&bob_pub).unwrap();
        assert_eq!(secret.as_bytes().len(), 32);
    }
}
