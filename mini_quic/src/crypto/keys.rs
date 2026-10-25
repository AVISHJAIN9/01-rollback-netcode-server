//! HKDF key derivation and key-phase management — RFC 9001 §5.1, §5.2, §6.
//!
//! # QUIC HKDF-Expand-Label (RFC 9001 §5.1)
//!
//! The label function follows TLS 1.3's `HKDF-Expand-Label` with the
//! QUIC-specific prefix `"quic "`:
//!
//! ```text
//! HKDF-Expand-Label(secret, label, context, length)
//!   HkdfLabel = length(2) || "quic " || label || context_len(1) || context
//!   HKDF-Expand(secret, HkdfLabel, length)
//! ```
//!
//! # Initial Secrets (RFC 9001 §5.2)
//!
//! Derived from the client's Destination Connection ID using:
//! - QUIC v1 salt: `0x38762cf7f55934b34d179ae6a4c80cadccbb7f0a`
//! - Labels: `"client in"` and `"server in"` for client/server initial secrets.
//!
//! # Key Phase (RFC 9001 §6)
//!
//! The `KeyPhaseManager` tracks the current key phase bit and performs key
//! updates using the `"quic ku"` label when the phase toggles.

use alloc::vec::Vec;
use alloc::vec;

use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::Zeroizing;

use super::CryptoError;

// ─── QUIC v1 Initial Salt ─────────────────────────────────────────────────────

/// RFC 9001 §5.2 — QUIC version-1 initial salt.
pub const INITIAL_SALT: [u8; 20] = [
    0x38, 0x76, 0x2c, 0xf7, 0xf5, 0x59, 0x34, 0xb3, 0x4d, 0x17,
    0x9a, 0xe6, 0xa4, 0xc8, 0x0c, 0xad, 0xcc, 0xbb, 0x7f, 0x0a,
];

// ─── HKDF-Expand-Label ───────────────────────────────────────────────────────

/// QUIC-specific `HKDF-Expand-Label` (RFC 9001 §5.1).
///
/// Constructs the HkdfLabel:
/// ```text
/// struct {
///   uint16 length;
///   opaque label<7..255> = "quic " + label;
///   opaque context<0..255> = context;
/// } HkdfLabel;
/// ```
/// Then calls `HKDF-Expand(secret, HkdfLabel, length)`.
///
/// # Errors
/// Returns [`CryptoError::HkdfOutputTooLong`] when `length > 255 * 32`
/// (the maximum for HKDF-SHA256).
pub fn hkdf_expand_label(
    secret: &[u8],
    label: &[u8],
    context: &[u8],
    length: usize,
) -> Result<Vec<u8>, CryptoError> {
    // Build the HkdfLabel structure.
    let quic_label = {
        let mut l = Vec::with_capacity(5 + label.len());
        l.extend_from_slice(b"quic ");
        l.extend_from_slice(label);
        l
    };

    // HkdfLabel wire encoding:
    //   [0..1] = length as big-endian u16
    //   [2]    = quic_label length (u8)
    //   [3..]  = quic_label bytes
    //   next   = context length (u8)
    //   next   = context bytes
    let mut hkdf_label = Vec::with_capacity(3 + quic_label.len() + 1 + context.len());
    hkdf_label.push(((length >> 8) & 0xFF) as u8);
    hkdf_label.push((length & 0xFF) as u8);
    hkdf_label.push(quic_label.len() as u8);
    hkdf_label.extend_from_slice(&quic_label);
    hkdf_label.push(context.len() as u8);
    hkdf_label.extend_from_slice(context);

    // HKDF-Expand with SHA-256.
    let hk = Hkdf::<Sha256>::from_prk(secret)
        .map_err(|_| CryptoError::HkdfOutputTooLong)?;

    let mut out = vec![0u8; length];
    hk.expand(&hkdf_label, &mut out)
        .map_err(|_| CryptoError::HkdfOutputTooLong)?;

    Ok(out)
}

// ─── Initial Secret Derivation ────────────────────────────────────────────────

/// Key material for one side of the Initial packet space.
#[derive(Debug)]
pub struct InitialKeys {
    /// 32-byte AEAD write key.
    pub key: Zeroizing<Vec<u8>>,
    /// 12-byte AEAD IV.
    pub iv: Zeroizing<Vec<u8>>,
    /// 32-byte header-protection key.
    pub hp_key: Zeroizing<Vec<u8>>,
}

/// Derive the client and server Initial secrets from the Destination
/// Connection ID.
///
/// Per RFC 9001 §5.2:
/// ```text
/// initial_secret = HKDF-Extract(INITIAL_SALT, client_dst_connection_id)
/// client_initial_secret = HKDF-Expand-Label(initial_secret, "client in", "", 32)
/// server_initial_secret = HKDF-Expand-Label(initial_secret, "server in", "", 32)
/// ```
///
/// Returns `(client_keys, server_keys)`.
pub fn derive_initial_keys(
    client_dst_connection_id: &[u8],
) -> Result<(InitialKeys, InitialKeys), CryptoError> {
    // HKDF-Extract: salt=INITIAL_SALT, IKM=DCID
    let (prk, _) = Hkdf::<Sha256>::extract(Some(&INITIAL_SALT), client_dst_connection_id);

    // Client initial secret
    let client_secret = hkdf_expand_label(prk.as_ref(), b"client in", b"", 32)?;
    // Server initial secret
    let server_secret = hkdf_expand_label(prk.as_ref(), b"server in", b"", 32)?;

    let client_keys = derive_packet_keys(&client_secret)?;
    let server_keys = derive_packet_keys(&server_secret)?;

    Ok((client_keys, server_keys))
}

/// Derive the AEAD key, IV, and header-protection key from a packet-space
/// secret.
///
/// Per RFC 9001 §5.1:
/// ```text
/// key    = HKDF-Expand-Label(secret, "quic key", "", 32)
/// iv     = HKDF-Expand-Label(secret, "quic iv",  "", 12)
/// hp_key = HKDF-Expand-Label(secret, "quic hp",  "", 32)
/// ```
pub fn derive_packet_keys(secret: &[u8]) -> Result<InitialKeys, CryptoError> {
    let key    = hkdf_expand_label(secret, b"quic key", b"", 32)?;
    let iv     = hkdf_expand_label(secret, b"quic iv",  b"", 12)?;
    let hp_key = hkdf_expand_label(secret, b"quic hp",  b"", 32)?;

    Ok(InitialKeys {
        key:    Zeroizing::new(key),
        iv:     Zeroizing::new(iv),
        hp_key: Zeroizing::new(hp_key),
    })
}

// ─── 1-RTT Key Update ─────────────────────────────────────────────────────────

/// Derive the next-generation 1-RTT secret from the current one.
///
/// RFC 9001 §6.1:
/// ```text
/// new_secret = HKDF-Expand-Label(current_secret, "quic ku", "", 32)
/// ```
pub fn update_1rtt_secret(current_secret: &[u8]) -> Result<Vec<u8>, CryptoError> {
    hkdf_expand_label(current_secret, b"quic ku", b"", 32)
}

// ─── KeyPhaseManager ─────────────────────────────────────────────────────────

/// Manages the current key phase bit and the keying material for both the
/// current and next key generations.
///
/// RFC 9001 §6 requires that the receiver be able to decrypt packets from
/// both the current and the previous key generation during a key-update
/// transition window.
#[derive(Debug)]
pub struct KeyPhaseManager {
    /// The current key phase bit (0 or 1) as it appears on the wire.
    current_phase_bit: u8,
    /// Secret for the current key phase.
    current_secret: Zeroizing<Vec<u8>>,
    /// Derived packet keys for the current phase.
    current_keys: InitialKeys,
    /// Secret for the next (prepared) key phase (pre-computed on demand).
    next_secret: Option<Zeroizing<Vec<u8>>>,
    /// Derived packet keys for the next phase (pre-computed on demand).
    next_keys: Option<InitialKeys>,
}

impl KeyPhaseManager {
    /// Construct a `KeyPhaseManager` starting at phase bit 0 from the given
    /// initial 1-RTT secret.
    pub fn new(initial_secret: Vec<u8>) -> Result<Self, CryptoError> {
        let current_keys = derive_packet_keys(&initial_secret)?;
        Ok(KeyPhaseManager {
            current_phase_bit: 0,
            current_secret: Zeroizing::new(initial_secret),
            current_keys,
            next_secret: None,
            next_keys: None,
        })
    }

    /// Return the current key phase bit (0 or 1).
    pub fn current_phase_bit(&self) -> u8 {
        self.current_phase_bit
    }

    /// Return the current AEAD key.
    pub fn current_key(&self) -> &[u8] {
        &self.current_keys.key
    }

    /// Return the current AEAD IV.
    pub fn current_iv(&self) -> &[u8] {
        &self.current_keys.iv
    }

    /// Return the current header-protection key.
    pub fn current_hp_key(&self) -> &[u8] {
        &self.current_keys.hp_key
    }

    /// Pre-compute the next key generation without activating it.
    ///
    /// This allows the sender to prepare the next keys before transmitting
    /// a key-phase-1 packet, and allows the receiver to check incoming
    /// packets with the next-generation keys.
    pub fn prepare_next_keys(&mut self) -> Result<(), CryptoError> {
        let next_secret = update_1rtt_secret(&self.current_secret)?;
        let next_keys = derive_packet_keys(&next_secret)?;
        self.next_secret = Some(Zeroizing::new(next_secret));
        self.next_keys = Some(next_keys);
        Ok(())
    }

    /// Commit the pre-computed next keys as current, toggling the phase bit.
    ///
    /// If `prepare_next_keys` was not called first, this method calls it
    /// automatically.
    ///
    /// # Errors
    /// Propagates [`CryptoError`] from key derivation if `prepare_next_keys`
    /// was not previously called.
    pub fn update_keys(&mut self) -> Result<(), CryptoError> {
        if self.next_secret.is_none() {
            self.prepare_next_keys()?;
        }
        let next_secret = self.next_secret.take().unwrap();
        let next_keys = self.next_keys.take().unwrap();

        self.current_secret = next_secret;
        self.current_keys = next_keys;
        self.current_phase_bit ^= 1;
        Ok(())
    }

    /// Return the next-generation AEAD key if pre-computed, or `None`.
    pub fn next_key(&self) -> Option<&[u8]> {
        self.next_keys.as_ref().map(|k| k.key.as_slice())
    }
}

// ─── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── RFC 9001 §A.1 test vectors ──────────────────────────────────────────
    //
    // DCID: 0x8394c8f03e515708
    // Expected client_initial_secret (from RFC 9001 Appendix A):
    //   c00cf151ca5be075ed0ebfb5c80323c4 2d6b7db67881289af4008f1f6c357aea
    //
    // These are the authoritative test vectors from the RFC itself.

    const RFC_DCID: [u8; 8] = [0x83, 0x94, 0xc8, 0xf0, 0x3e, 0x51, 0x57, 0x08];

    #[test]
    fn initial_secret_rfc9001_client_key() {
        let (client_keys, _server_keys) = derive_initial_keys(&RFC_DCID).unwrap();

        // RFC 9001 §A.1: client write key
        let expected_key = [
            0x1f, 0x36, 0x96, 0x13, 0xdd, 0x76, 0xd5, 0x46,
            0x77, 0x30, 0xef, 0xcb, 0xe3, 0xb1, 0xa2, 0x2d,
            0x42, 0x27, 0x64, 0x37, 0x2f, 0x8e, 0xcf, 0xe0,
            0x58, 0x85, 0x46, 0x88, 0x09, 0x27, 0x13, 0xb0,
        ];

        assert_eq!(
            client_keys.key.as_slice(),
            &expected_key,
            "Client initial write key does not match RFC 9001 §A.1"
        );
    }

    #[test]
    fn initial_secret_rfc9001_client_iv() {
        let (client_keys, _) = derive_initial_keys(&RFC_DCID).unwrap();

        // RFC 9001 §A.1: client IV
        let expected_iv = [
            0xfa, 0x04, 0x4b, 0x2f, 0x42, 0xa3, 0xfd, 0x3b,
            0x46, 0xfb, 0x25, 0x5c,
        ];

        assert_eq!(
            client_keys.iv.as_slice(),
            &expected_iv,
            "Client initial IV does not match RFC 9001 §A.1"
        );
    }

    #[test]
    fn initial_secret_rfc9001_client_hp_key() {
        let (client_keys, _) = derive_initial_keys(&RFC_DCID).unwrap();

        // RFC 9001 §A.1: client HP key
        let expected_hp = [
            0x9f, 0x50, 0x44, 0x9e, 0x04, 0xa0, 0xe8, 0x10,
            0x28, 0x38, 0x3d, 0x46, 0x93, 0x25, 0x1b, 0x16,
            0x56, 0x0c, 0x53, 0x59, 0xf5, 0x33, 0xc1, 0x1b,
            0x35, 0x6a, 0xbb, 0xe0, 0x0a, 0x5d, 0x08, 0x42,
        ];

        assert_eq!(
            client_keys.hp_key.as_slice(),
            &expected_hp,
            "Client HP key does not match RFC 9001 §A.1"
        );
    }

    #[test]
    fn key_phase_update_toggles_bit() {
        let initial_secret = vec![0xABu8; 32];
        let mut manager = KeyPhaseManager::new(initial_secret).unwrap();

        assert_eq!(manager.current_phase_bit(), 0);
        manager.update_keys().unwrap();
        assert_eq!(manager.current_phase_bit(), 1);
        manager.update_keys().unwrap();
        assert_eq!(manager.current_phase_bit(), 0);
    }

    #[test]
    fn key_phase_update_changes_key_material() {
        let initial_secret = vec![0xCDu8; 32];
        let mut manager = KeyPhaseManager::new(initial_secret).unwrap();

        let key_before: Vec<u8> = manager.current_key().to_vec();
        manager.update_keys().unwrap();
        let key_after: Vec<u8> = manager.current_key().to_vec();

        assert_ne!(key_before, key_after, "key must change after update");
    }

    #[test]
    fn prepare_next_keys_makes_next_key_available() {
        let initial_secret = vec![0x01u8; 32];
        let mut manager = KeyPhaseManager::new(initial_secret).unwrap();

        assert!(manager.next_key().is_none());
        manager.prepare_next_keys().unwrap();
        assert!(manager.next_key().is_some());
    }

    #[test]
    fn hkdf_expand_label_deterministic() {
        let secret = [0x42u8; 32];
        let out1 = hkdf_expand_label(&secret, b"quic key", b"", 32).unwrap();
        let out2 = hkdf_expand_label(&secret, b"quic key", b"", 32).unwrap();
        assert_eq!(out1, out2);
    }

    #[test]
    fn hkdf_expand_label_different_labels_produce_different_output() {
        let secret = [0x55u8; 32];
        let key = hkdf_expand_label(&secret, b"quic key", b"", 32).unwrap();
        let iv  = hkdf_expand_label(&secret, b"quic iv",  b"", 12).unwrap();
        // Different labels (and lengths) must yield different material.
        assert_ne!(key[..12], iv[..12]);
    }
}
